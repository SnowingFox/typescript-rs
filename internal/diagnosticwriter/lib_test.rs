use super::*;
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::compute_ecma_line_starts;
use tsgo_core::text::TextPos;
use tsgo_core::tristate::Tristate;
use tsgo_locale::Locale;
use tsgo_tspath::ComparePathsOptions;

// A minimal in-memory `FileLike` whose ECMA line map is derived from its text,
// mirroring how a real source file computes line starts.
struct FakeFile {
    name: String,
    text: String,
    line_map: Vec<TextPos>,
}

impl FakeFile {
    fn new(name: &str, text: &str) -> FakeFile {
        FakeFile {
            name: name.to_string(),
            text: text.to_string(),
            line_map: compute_ecma_line_starts(text),
        }
    }
}

impl FileLike for FakeFile {
    fn file_name(&self) -> &str {
        &self.name
    }
    fn text(&self) -> &str {
        &self.text
    }
    fn ecma_line_map(&self) -> &[TextPos] {
        &self.line_map
    }
}

// A minimal `Diagnostic` whose `localize` returns a fixed message verbatim
// (locale-independent), with optional file/position, message chain, and related
// information for exercising the renderers.
struct FakeDiagnostic {
    file: Option<FakeFile>,
    pos: i32,
    len: i32,
    code: i32,
    category: Category,
    message: String,
    message_chain: Vec<FakeDiagnostic>,
    related: Vec<FakeDiagnostic>,
}

impl FakeDiagnostic {
    fn new(category: Category, code: i32, message: &str) -> FakeDiagnostic {
        FakeDiagnostic {
            file: None,
            pos: 0,
            len: 0,
            code,
            category,
            message: message.to_string(),
            message_chain: Vec::new(),
            related: Vec::new(),
        }
    }

    fn with_file(mut self, file: FakeFile, pos: i32, len: i32) -> FakeDiagnostic {
        self.file = Some(file);
        self.pos = pos;
        self.len = len;
        self
    }

    fn with_chain(mut self, chain: Vec<FakeDiagnostic>) -> FakeDiagnostic {
        self.message_chain = chain;
        self
    }

    fn with_related(mut self, related: Vec<FakeDiagnostic>) -> FakeDiagnostic {
        self.related = related;
        self
    }
}

impl Diagnostic for FakeDiagnostic {
    fn file(&self) -> Option<&dyn FileLike> {
        self.file.as_ref().map(|f| f as &dyn FileLike)
    }
    fn pos(&self) -> i32 {
        self.pos
    }
    fn end(&self) -> i32 {
        self.pos + self.len
    }
    fn len(&self) -> i32 {
        self.len
    }
    fn code(&self) -> i32 {
        self.code
    }
    fn category(&self) -> Category {
        self.category
    }
    fn localize(&self, _locale: &Locale) -> String {
        self.message.clone()
    }
    fn message_chain(&self) -> Vec<&dyn Diagnostic> {
        self.message_chain
            .iter()
            .map(|d| d as &dyn Diagnostic)
            .collect()
    }
    fn related_information(&self) -> Vec<&dyn Diagnostic> {
        self.related.iter().map(|d| d as &dyn Diagnostic).collect()
    }
}

fn en() -> Locale {
    tsgo_locale::parse("en").unwrap()
}

// A colorless `FormattedWriter`: emits the text only, dropping the style and the
// reset sequence, so tests can assert the plain skeleton.
fn plain_writer(out: &mut dyn std::fmt::Write, text: &str, _style: &str) {
    let _ = out.write_str(text);
}

fn opts(current_directory: &str) -> FormattingOptions {
    FormattingOptions {
        locale: en(),
        compare_paths_options: ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: current_directory.to_string(),
        },
        new_line: "\n".to_string(),
    }
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:getCategoryFormat
#[test]
fn category_format_colors() {
    assert_eq!(get_category_format(Category::Error), "\u{1b}[91m");
    assert_eq!(get_category_format(Category::Warning), "\u{1b}[93m");
    assert_eq!(get_category_format(Category::Suggestion), "\u{1b}[90m");
    assert_eq!(get_category_format(Category::Message), "\u{1b}[94m");
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:WriteFlattenedDiagnosticMessage
#[test]
fn flatten_single_message() {
    let d = FakeDiagnostic::new(Category::Error, 2304, "Cannot find name 'x'.");
    assert_eq!(
        flatten_diagnostic_message(&d, "\n", &en()),
        "Cannot find name 'x'."
    );
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:flattenDiagnosticMessageChain
#[test]
fn flatten_message_chain_indent() {
    let grandchild = FakeDiagnostic::new(Category::Error, 0, "grandchild");
    let child = FakeDiagnostic::new(Category::Error, 0, "child").with_chain(vec![grandchild]);
    let root = FakeDiagnostic::new(Category::Error, 0, "root").with_chain(vec![child]);
    assert_eq!(
        flatten_diagnostic_message(&root, "\n", &en()),
        "root\n  child\n    grandchild"
    );
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:WriteLocation
#[test]
fn write_location_basic() {
    let file = FakeFile::new("a.ts", "x\nabc");
    let o = opts("");
    let mut out = String::new();
    // pos 4 lands at line index 1 (display 2), UTF-16 char 2 (display 3).
    write_location(&mut out, &file, 4, &o, plain_writer);
    assert_eq!(out, "a.ts:2:3");
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:WriteFormatDiagnostic
#[test]
fn format_compact_with_file() {
    let file = FakeFile::new("a.ts", "x\nabc");
    let d =
        FakeDiagnostic::new(Category::Error, 2304, "Cannot find name 'x'.").with_file(file, 4, 0);
    let o = opts("");
    let mut out = String::new();
    write_format_diagnostic(&mut out, &d, &o);
    assert_eq!(out, "a.ts(2,3): error TS2304: Cannot find name 'x'.\n");
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:WriteFormatDiagnostic
#[test]
fn format_compact_no_file() {
    let d = FakeDiagnostic::new(Category::Error, 2304, "Cannot find name 'x'.");
    let o = opts("");
    let mut out = String::new();
    write_format_diagnostic(&mut out, &d, &o);
    assert_eq!(out, "error TS2304: Cannot find name 'x'.\n");
}

const RED: &str = "\u{1b}[91m";

// Go: internal/diagnosticwriter/diagnosticwriter.go:writeCodeSnippet
#[test]
fn code_snippet_single_line() {
    let file = FakeFile::new("a.ts", "const a = 1;\nlet x = y;\n");
    let o = opts("");
    let mut out = String::new();
    // The `y` token sits at byte 21, column 8 (UTF-16) on display line 2.
    write_code_snippet(&mut out, &file, 21, 1, RED, "", &o);
    assert_eq!(
        out,
        "\n\u{1b}[7m2\u{1b}[0m let x = y;\n\u{1b}[7m \u{1b}[0m \u{1b}[91m        ~\u{1b}[0m"
    );
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:writeCodeSnippet
#[test]
fn code_snippet_zero_length() {
    let file = FakeFile::new("a.ts", "const a = 1;\nlet x = y;\n");
    let o = opts("");
    let mut out = String::new();
    // length 0: squiggle the single character right after the start position.
    write_code_snippet(&mut out, &file, 21, 0, RED, "", &o);
    assert_eq!(out.matches('~').count(), 1);
    assert!(out.ends_with("~\u{1b}[0m"));
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:writeCodeSnippet
#[test]
fn code_snippet_tabs_to_spaces() {
    let file = FakeFile::new("a.ts", "\tabc\n");
    let o = opts("");
    let mut out = String::new();
    write_code_snippet(&mut out, &file, 1, 3, RED, "", &o);
    // The leading tab is rendered as a single space, and no raw tab survives.
    assert!(out.contains(" abc"));
    assert!(!out.contains('\t'));
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:FormatDiagnosticWithColorAndContext
#[test]
fn related_information_rendered() {
    let main_file = FakeFile::new("a.ts", "let x = y;\n");
    let related_file = FakeFile::new("b.ts", "var y;\n");
    let related = FakeDiagnostic::new(Category::Message, 0, "'y' is declared here.").with_file(
        related_file,
        4,
        1,
    );
    let d = FakeDiagnostic::new(Category::Error, 2304, "Cannot find name 'y'.")
        .with_file(main_file, 8, 1)
        .with_related(vec![related]);
    let o = opts("");
    let mut out = String::new();
    format_diagnostic_with_color_and_context(&mut out, &d, &o);

    // Colored category + code header and both messages are present.
    assert!(out.contains("\u{1b}[91merror\u{1b}[0m"));
    assert!(out.contains(" TS2304: "));
    assert!(out.contains("Cannot find name 'y'."));
    assert!(out.contains("'y' is declared here."));
    // A gutter-styled source snippet is emitted for the primary diagnostic.
    assert!(out.contains("\u{1b}[7m"));
    // Related information is rendered after the primary message and indented.
    let main_at = out.find("Cannot find name 'y'.").unwrap();
    let related_at = out.find("'y' is declared here.").unwrap();
    assert!(main_at < related_at);
    assert!(out.contains("\n  "));
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:writeCodeSnippet
#[test]
fn code_snippet_fold_over_5_lines() {
    let file = FakeFile::new("a.ts", "l0\nl1\nl2\nl3\nl4\nl5\nl6\n");
    let o = opts("");
    let mut out = String::new();
    // Span lines 1..6 (>= 5 lines): only the first two and last two are shown.
    write_code_snippet(&mut out, &file, 3, 15, RED, "", &o);
    assert!(out.contains("..."));
    assert!(out.contains("l1"));
    assert!(out.contains("l2"));
    assert!(out.contains("l5"));
    assert!(out.contains("l6"));
    assert!(!out.contains("l3"));
    assert!(!out.contains("l4"));
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:WriteErrorSummaryText
#[test]
fn summary_zero_errors_empty() {
    let o = opts("");
    let mut out = String::new();
    write_error_summary_text(&mut out, &[], &o);
    assert_eq!(out, "");
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:WriteErrorSummaryText
#[test]
fn summary_single_error_in_file() {
    let d = FakeDiagnostic::new(Category::Error, 2304, "boom").with_file(
        FakeFile::new("a.ts", "let a;\n"),
        0,
        0,
    );
    let o = opts("");
    let mut out = String::new();
    write_error_summary_text(&mut out, &[&d], &o);
    assert_eq!(out, "\nFound 1 error in a.ts\u{1b}[90m:1\u{1b}[0m\n\n");
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:WriteErrorSummaryText
#[test]
fn summary_single_global_error() {
    let d = FakeDiagnostic::new(Category::Error, 2304, "boom");
    let o = opts("");
    let mut out = String::new();
    write_error_summary_text(&mut out, &[&d], &o);
    assert_eq!(out, "\nFound 1 error.\n\n");
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:WriteErrorSummaryText
#[test]
fn summary_multi_files_table() {
    let d1 = FakeDiagnostic::new(Category::Error, 1, "e1").with_file(
        FakeFile::new("a.ts", "let a;\n"),
        0,
        0,
    );
    let d2 = FakeDiagnostic::new(Category::Error, 2, "e2").with_file(
        FakeFile::new("a.ts", "let a;\n"),
        0,
        0,
    );
    let d3 = FakeDiagnostic::new(Category::Error, 3, "e3").with_file(
        FakeFile::new("b.ts", "let b;\n"),
        0,
        0,
    );
    let diags: Vec<&dyn Diagnostic> = vec![&d1, &d2, &d3];
    let o = opts("");
    let mut out = String::new();
    write_error_summary_text(&mut out, &diags, &o);
    assert_eq!(
        out,
        "\nFound 3 errors in 2 files.\n\nErrors  Files\n     2  a.ts\u{1b}[90m:1\u{1b}[0m\n     1  b.ts\u{1b}[90m:1\u{1b}[0m\n\n"
    );
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:WriteErrorSummaryText
#[test]
fn summary_single_file_multi_errors() {
    let d1 = FakeDiagnostic::new(Category::Error, 1, "e1").with_file(
        FakeFile::new("a.ts", "let a;\n"),
        0,
        0,
    );
    let d2 = FakeDiagnostic::new(Category::Error, 2, "e2").with_file(
        FakeFile::new("a.ts", "let a;\n"),
        0,
        0,
    );
    let d3 = FakeDiagnostic::new(Category::Error, 3, "e3").with_file(
        FakeFile::new("a.ts", "let a;\n"),
        0,
        0,
    );
    let diags: Vec<&dyn Diagnostic> = vec![&d1, &d2, &d3];
    let o = opts("");
    let mut out = String::new();
    write_error_summary_text(&mut out, &diags, &o);
    // totalErrorCount > 1 with a single erroring file: no table is emitted.
    assert_eq!(
        out,
        "\nFound 3 errors in the same file, starting at: a.ts\u{1b}[90m:1\u{1b}[0m\n\n"
    );
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:getErrorSummary
#[test]
fn summary_sorted_by_filename() {
    // Inserted b.ts before a.ts; the table must still list a.ts first.
    let db = FakeDiagnostic::new(Category::Error, 1, "e").with_file(
        FakeFile::new("b.ts", "let b;\n"),
        0,
        0,
    );
    let da = FakeDiagnostic::new(Category::Error, 2, "e").with_file(
        FakeFile::new("a.ts", "let a;\n"),
        0,
        0,
    );
    let diags: Vec<&dyn Diagnostic> = vec![&db, &da];
    let o = opts("");
    let mut out = String::new();
    write_error_summary_text(&mut out, &diags, &o);
    assert!(out.find("a.ts").unwrap() < out.find("b.ts").unwrap());
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:ToDiagnostics
#[test]
fn to_diagnostics_widens_to_trait_objects() {
    let diags = vec![
        FakeDiagnostic::new(Category::Error, 1, "a"),
        FakeDiagnostic::new(Category::Warning, 2, "b"),
    ];
    let widened = to_diagnostics(diags);
    assert_eq!(widened.len(), 2);
    assert_eq!(widened[0].code(), 1);
    assert_eq!(widened[1].category(), Category::Warning);
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:FormatDiagnosticsStatusWithColorAndTime
#[test]
fn status_with_color_and_time() {
    let d = FakeDiagnostic::new(Category::Message, 0, "hi");
    let o = opts("");
    let mut out = String::new();
    format_diagnostics_status_with_color_and_time(&mut out, "12:00:00", &d, &o);
    assert_eq!(out, "[\u{1b}[90m12:00:00\u{1b}[0m] hi");
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:FormatDiagnosticsStatusAndTime
#[test]
fn status_and_time_plain() {
    let d = FakeDiagnostic::new(Category::Message, 0, "hi");
    let o = opts("");
    let mut out = String::new();
    format_diagnostics_status_and_time(&mut out, "12:00:00", &d, &o);
    assert_eq!(out, "12:00:00 - hi");
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:TryClearScreen
#[test]
fn try_clear_screen_watch() {
    // 6031 = Starting_compilation_in_watch_mode.
    let d = FakeDiagnostic::new(
        Category::Message,
        6031,
        "Starting compilation in watch mode...",
    );
    let options = CompilerOptions::default();
    let mut out = String::new();
    assert!(try_clear_screen(&mut out, &d, &options));
    assert_eq!(out, "\u{1b}[2J\u{1b}[3J\u{1b}[H");
}

// Go: internal/diagnosticwriter/diagnosticwriter.go:TryClearScreen
#[test]
fn try_clear_screen_suppressed() {
    let d = FakeDiagnostic::new(
        Category::Message,
        6031,
        "Starting compilation in watch mode...",
    );
    let options = CompilerOptions {
        preserve_watch_output: Tristate::True,
        ..CompilerOptions::default()
    };
    let mut out = String::new();
    assert!(!try_clear_screen(&mut out, &d, &options));
    assert_eq!(out, "");
}
