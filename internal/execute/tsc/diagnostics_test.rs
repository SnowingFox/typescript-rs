use super::*;
use std::sync::Arc;
use tsgo_compiler::{new_compiler_host, CompilerHost, ParsedFile};
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::tristate::Tristate;
use tsgo_diagnostics::Category;
use tsgo_diagnosticwriter::{
    write_format_diagnostic, Diagnostic as DwDiagnostic, FormattingOptions,
};
use tsgo_locale::{parse as parse_locale, Locale};
use tsgo_parser::SourceFileParseOptions;
use tsgo_tspath::ComparePathsOptions;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use crate::sys::VfsSystem;

fn make(code: i32, pos: i32, len: i32, file: Option<DiagFile>) -> ReportedDiagnostic {
    ReportedDiagnostic {
        code,
        category: Category::Error,
        message: format!("m{code}"),
        pos,
        len,
        file,
        message_chain: Vec::new(),
        related_information: Vec::new(),
    }
}

fn en() -> Locale {
    parse_locale("en").unwrap()
}

fn vfs_sys() -> VfsSystem {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map([("/p/index.ts", "x")], true));
    VfsSystem::new(fs, "/p", "/lib")
}

fn parsed_file(text: &str) -> ParsedFile {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map([("/p/index.ts", text)], true));
    let host = new_compiler_host("/p", fs, "/lib");
    host.get_source_file(&SourceFileParseOptions {
        file_name: "/p/index.ts".into(),
    })
    .expect("source file")
}

#[test]
fn sort_orders_by_position_then_code() {
    let sorted = sort_and_deduplicate_diagnostics(vec![
        make(2322, 10, 1, None),
        make(2304, 2, 1, None),
        make(1005, 2, 1, None),
    ]);
    let codes: Vec<i32> = sorted.iter().map(|d| d.code).collect();
    // (pos 2, code 1005), (pos 2, code 2304), (pos 10, code 2322).
    assert_eq!(codes, vec![1005, 2304, 2322]);
}

#[test]
fn dedup_removes_exact_duplicates() {
    let sorted =
        sort_and_deduplicate_diagnostics(vec![make(2304, 2, 1, None), make(2304, 2, 1, None)]);
    assert_eq!(sorted.len(), 1);
}

#[test]
fn sort_orders_global_diagnostics_before_file_diagnostics() {
    let file = Some(DiagFile::new("/p/index.ts", "const x = 1;\n"));
    let sorted =
        sort_and_deduplicate_diagnostics(vec![make(2322, 6, 1, file), make(5023, 0, 0, None)]);
    // Global (file name "") sorts before the file diagnostic.
    assert_eq!(sorted[0].code, 5023);
    assert_eq!(sorted[1].code, 2322);
}

#[test]
fn from_options_wraps_as_global_diagnostic() {
    let options_diagnostic = tsgo_compiler::OptionsDiagnostic {
        message:
            &tsgo_diagnostics::OPTION_0_1_HAS_BEEN_REMOVED_PLEASE_REMOVE_IT_FROM_YOUR_CONFIGURATION,
        args: vec!["target".to_string(), "ES5".to_string()],
    };
    let wrapped = ReportedDiagnostic::from_options(&options_diagnostic, &en());
    assert!(wrapped.file.is_none());
    assert_eq!(wrapped.code, 5108);
    assert_eq!(wrapped.category, Category::Error);
    assert_eq!(
        wrapped.message,
        "Option 'target=ES5' has been removed. Please remove it from your configuration."
    );
}

#[test]
fn from_parser_attaches_the_owning_file() {
    let file = parsed_file("const x: number = 1;\n");
    // A synthetic parser diagnostic at offset 6 (the `x`).
    let diagnostic = tsgo_parser::Diagnostic {
        loc: tsgo_core::text::TextRange::new(6, 7),
        message: &tsgo_diagnostics::CANNOT_FIND_NAME_0,
        args: vec!["x".to_string()],
    };
    let wrapped = ReportedDiagnostic::from_parser(&diagnostic, Some(&file), &en());
    assert!(wrapped.file.is_some());
    assert_eq!(wrapped.pos, 6);
    assert_eq!(wrapped.len, 1);
    assert_eq!(wrapped.code, 2304);
}

#[test]
fn from_checker_wraps_file_chain_and_related() {
    let file = parsed_file("const x: number = 1;\n");
    let leaf = tsgo_checker::DiagnosticMessageChain {
        code: 2322,
        category: Category::Error,
        message: "Type 'string' is not assignable to type 'number'.".to_string(),
        next: Vec::new(),
    };
    let related = tsgo_checker::Diagnostic {
        code: 2728,
        category: Category::Error,
        message: "'x' is declared here.".to_string(),
        start: 6,
        length: 1,
        related_information: Vec::new(),
        message_chain: Vec::new(),
    };
    let diagnostic = tsgo_checker::Diagnostic {
        code: 2322,
        category: Category::Error,
        message: "Type 'A' is not assignable to type 'B'.".to_string(),
        start: 0,
        length: 5,
        related_information: vec![related],
        message_chain: vec![leaf],
    };
    let wrapped = ReportedDiagnostic::from_checker(&diagnostic, &file);
    assert!(wrapped.file.is_some());
    assert_eq!(wrapped.code, 2322);
    assert_eq!(wrapped.message_chain.len(), 1);
    assert_eq!(
        wrapped.message_chain[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
    assert_eq!(wrapped.related_information.len(), 1);
    assert_eq!(wrapped.related_information[0].code, 2728);

    // The flattened render includes the indented message chain.
    let opts = FormattingOptions {
        locale: en(),
        compare_paths_options: ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: "/p".into(),
        },
        new_line: "\n".into(),
    };
    let mut out = String::new();
    write_format_diagnostic(&mut out, &wrapped, &opts);
    assert!(
        out.contains("index.ts(1,1): error TS2322: Type 'A' is not assignable to type 'B'."),
        "unexpected render: {out:?}"
    );
    assert!(
        out.contains("\n  Type 'string' is not assignable to type 'number'."),
        "missing indented chain: {out:?}"
    );
}

#[test]
fn dw_diagnostic_accessors_expose_span_file_and_children() {
    let diag = ReportedDiagnostic {
        code: 2322,
        category: Category::Error,
        message: "msg".into(),
        pos: 6,
        len: 1,
        file: Some(DiagFile::new("/p/index.ts", "const x = 1;\n")),
        message_chain: vec![make(2322, 0, 0, None)],
        related_information: vec![make(2728, 0, 0, None)],
    };
    let dyn_diag: &dyn DwDiagnostic = &diag;
    assert_eq!(dyn_diag.code(), 2322);
    assert_eq!(dyn_diag.pos(), 6);
    assert_eq!(dyn_diag.end(), 7);
    assert_eq!(dyn_diag.len(), 1);
    assert_eq!(dyn_diag.category(), Category::Error);
    assert_eq!(dyn_diag.localize(&en()), "msg");
    assert_eq!(dyn_diag.message_chain().len(), 1);
    assert_eq!(dyn_diag.related_information().len(), 1);

    let file_like = dyn_diag.file().expect("file");
    assert_eq!(file_like.file_name(), "/p/index.ts");
    assert_eq!(file_like.text(), "const x = 1;\n");
    assert!(!file_like.ecma_line_map().is_empty());
}

#[test]
fn quiet_reporter_writes_nothing() {
    let options = CompilerOptions {
        quiet: Tristate::True,
        ..Default::default()
    };
    let sys = vfs_sys();
    let reporter = create_diagnostic_reporter(&sys, &en(), &options);
    reporter.report(&sys, &make(2304, 0, 1, None));
    assert_eq!(sys.output(), "");
}

#[test]
fn plain_reporter_writes_compact_one_line_form() {
    let options = CompilerOptions::default();
    let sys = vfs_sys();
    let reporter = create_diagnostic_reporter(&sys, &en(), &options);
    let diag = ReportedDiagnostic {
        code: 2322,
        category: Category::Error,
        message: "Type 'string' is not assignable to type 'number'.".into(),
        pos: 5,
        len: 16,
        file: Some(DiagFile::new("/p/index.ts", "const x: number = \"s\";\n")),
        message_chain: Vec::new(),
        related_information: Vec::new(),
    };
    reporter.report(&sys, &diag);
    assert_eq!(
        sys.output(),
        "index.ts(1,6): error TS2322: Type 'string' is not assignable to type 'number'.\n"
    );
}

#[test]
fn pretty_reporter_renders_ansi_with_color_and_context() {
    let options = CompilerOptions {
        pretty: Tristate::True,
        ..Default::default()
    };
    let sys = vfs_sys();
    let reporter = create_diagnostic_reporter(&sys, &en(), &options);
    let diag = ReportedDiagnostic {
        code: 2322,
        category: Category::Error,
        message: "Type 'string' is not assignable to type 'number'.".into(),
        pos: 5,
        len: 16,
        file: Some(DiagFile::new("/p/index.ts", "const x: number = \"s\";\n")),
        message_chain: Vec::new(),
        related_information: Vec::new(),
    };
    reporter.report(&sys, &diag);
    let output = sys.output();
    assert!(
        output.contains('\u{1b}'),
        "expected ANSI escapes: {output:?}"
    );
    assert!(output.contains("TS2322"));
}

#[test]
fn error_summary_is_noop_in_plain_mode_and_written_in_pretty_mode() {
    let diag = ReportedDiagnostic {
        code: 2322,
        category: Category::Error,
        message: "Type 'string' is not assignable to type 'number'.".into(),
        pos: 5,
        len: 16,
        file: Some(DiagFile::new("/p/index.ts", "const x: number = \"s\";\n")),
        message_chain: Vec::new(),
        related_information: Vec::new(),
    };

    let plain_options = CompilerOptions::default();
    let plain_sys = vfs_sys();
    let plain = create_report_error_summary(&plain_sys, &en(), &plain_options);
    plain.report(&plain_sys, std::slice::from_ref(&diag));
    assert_eq!(plain_sys.output(), "");

    let pretty_options = CompilerOptions {
        pretty: Tristate::True,
        ..Default::default()
    };
    let pretty_sys = vfs_sys();
    let pretty = create_report_error_summary(&pretty_sys, &en(), &pretty_options);
    pretty.report(&pretty_sys, std::slice::from_ref(&diag));
    assert!(
        pretty_sys.output().contains("Found 1 error in index.ts"),
        "missing summary: {:?}",
        pretty_sys.output()
    );
}
