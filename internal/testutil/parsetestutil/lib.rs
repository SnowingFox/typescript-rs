//! `tsgo_testutil_parsetestutil` — 1:1 Rust port of Go
//! `internal/testutil/parsetestutil`.
//!
//! Test helpers for parsing a source string into a `SourceFile` and asserting
//! the parse produced no diagnostics. Consumed by the printer/emit test suites.
//!
//! # Harness shim (divergence from Go)
//!
//! Go's helpers take a `*testing.T` and report failures via `t.Error`. Rust has
//! no equivalent test handle available to a library, so the check helpers
//! `panic!` on failure instead (a failing assertion in the consuming `#[test]`).
//! The diagnostic text is still rendered through [`tsgo_diagnosticwriter`],
//! exactly as Go does, so the panic message matches the Go output (minus the
//! file-location prefix, which the parser's minimal `Diagnostic` does not yet
//! carry — see `parse_type_script`).

use tsgo_ast::{NodeArena, NodeData, NodeId, VisitOptions};
use tsgo_core::languagevariant::LanguageVariant;
use tsgo_core::text::TextRange;
use tsgo_core::{get_script_kind_from_file_name, if_else};
use tsgo_diagnostics::Category;
use tsgo_diagnosticwriter::{
    write_format_diagnostics, Diagnostic as DiagnosticWriterDiagnostic, FileLike, FormattingOptions,
};
use tsgo_parser::{parse_source_file, Diagnostic, SourceFileParseOptions};
use tsgo_tspath::ComparePathsOptions;

/// A parsed source file: the owning arena, the root `SourceFile` node id, the
/// parse diagnostics, the original source text, and the detected language
/// variant.
///
/// This bundles what Go's `*ast.SourceFile` exposes to these helpers (text +
/// diagnostics + nodes) into one value, because in this port the [`NodeArena`]
/// owns every node rather than the nodes being globally reachable pointers.
///
/// Side effects: none (pure value type).
#[derive(Debug)]
pub struct ParsedSourceFile {
    /// The arena owning every parsed node.
    pub arena: NodeArena,
    /// The id of the root `SourceFile` node.
    pub source_file: NodeId,
    /// The syntactic diagnostics, in source order.
    pub diagnostics: Vec<Diagnostic>,
    /// The original source text that was parsed.
    pub text: String,
    /// The language variant (`Jsx` for `.tsx`, otherwise `Standard`).
    pub language_variant: LanguageVariant,
}

/// Parses `text` into a [`ParsedSourceFile`], naming the file `/main.tsx` when
/// `jsx` is set and `/main.ts` otherwise (so the scanner picks the right
/// language variant).
///
/// # Examples
/// ```
/// use tsgo_testutil_parsetestutil::parse_type_script;
/// let file = parse_type_script("let x = 1;", false);
/// assert!(file.diagnostics.is_empty());
/// ```
///
/// Side effects: allocates a fresh arena for the parse.
// Go: internal/testutil/parsetestutil/parsetestutil.go:ParseTypeScript
pub fn parse_type_script(text: &str, jsx: bool) -> ParsedSourceFile {
    // Go uses `tspath.Path(fileName)` for the parse options' `Path` field; the
    // ported `SourceFileParseOptions` does not yet carry a `Path`, so only the
    // file name is threaded through.
    let file_name = if_else(jsx, "/main.tsx", "/main.ts");
    let result = parse_source_file(
        SourceFileParseOptions {
            file_name: file_name.to_string(),
        },
        text,
        get_script_kind_from_file_name(file_name),
    );
    let language_variant = match result.arena.data(result.source_file) {
        NodeData::SourceFile(d) => d.language_variant,
        _ => LanguageVariant::Standard,
    };
    ParsedSourceFile {
        arena: result.arena,
        source_file: result.source_file,
        diagnostics: result.diagnostics,
        text: text.to_string(),
        language_variant,
    }
}

/// Asserts that `file` has no parse diagnostics; panics with the
/// [`tsgo_diagnosticwriter`]-rendered diagnostics otherwise.
///
/// # Examples
/// ```
/// use tsgo_testutil_parsetestutil::{parse_type_script, check_diagnostics};
/// check_diagnostics(&parse_type_script("let x = 1;", false));
/// ```
///
/// Side effects: panics (fails the calling test) when diagnostics are present.
// Go: internal/testutil/parsetestutil/parsetestutil.go:CheckDiagnostics
pub fn check_diagnostics(file: &ParsedSourceFile) {
    if !file.diagnostics.is_empty() {
        panic!("{}", format_diagnostics(&file.diagnostics));
    }
}

/// Adapts a parser [`Diagnostic`] to the [`tsgo_diagnosticwriter`] `Diagnostic`
/// trait so the helpers can reuse Go's exact rendering.
///
/// `file()` returns `None`: the parser's minimal `Diagnostic` does not yet carry
/// a `FileLike` back-pointer (full `ast.Diagnostic` is a later phase), so the
/// `path(line,col)` prefix Go emits via `FromASTDiagnostics` is omitted.
struct DiagnosticAdapter<'a> {
    diagnostic: &'a Diagnostic,
}

impl DiagnosticWriterDiagnostic for DiagnosticAdapter<'_> {
    fn file(&self) -> Option<&dyn FileLike> {
        None
    }
    fn pos(&self) -> i32 {
        self.diagnostic.loc.pos()
    }
    fn end(&self) -> i32 {
        self.diagnostic.loc.end()
    }
    fn len(&self) -> i32 {
        self.diagnostic.loc.end() - self.diagnostic.loc.pos()
    }
    fn code(&self) -> i32 {
        self.diagnostic.message.code()
    }
    fn category(&self) -> Category {
        self.diagnostic.message.category()
    }
    fn localize(&self, locale: &tsgo_locale::Locale) -> String {
        let args: Vec<&str> = self.diagnostic.args.iter().map(String::as_str).collect();
        self.diagnostic.message.localize(locale, &args)
    }
    fn message_chain(&self) -> Vec<&dyn DiagnosticWriterDiagnostic> {
        Vec::new()
    }
    fn related_information(&self) -> Vec<&dyn DiagnosticWriterDiagnostic> {
        Vec::new()
    }
}

/// Renders `diagnostics` in the compact one-line form, mirroring Go's
/// `diagnosticwriter.WriteFormatDiagnostics(FromASTDiagnostics(...))` with an
/// LF newline.
fn format_diagnostics(diagnostics: &[Diagnostic]) -> String {
    let adapters: Vec<DiagnosticAdapter> = diagnostics
        .iter()
        .map(|diagnostic| DiagnosticAdapter { diagnostic })
        .collect();
    let refs: Vec<&dyn DiagnosticWriterDiagnostic> = adapters
        .iter()
        .map(|adapter| adapter as &dyn DiagnosticWriterDiagnostic)
        .collect();
    let opts = FormattingOptions {
        locale: tsgo_locale::parse("en").expect("en locale"),
        compare_paths_options: ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: String::new(),
        },
        new_line: "\n".to_string(),
    };
    let mut out = String::new();
    write_format_diagnostics(&mut out, &refs, &opts);
    out
}

/// Like [`check_diagnostics`] but prefixes the panic message with `message`.
///
/// Side effects: panics (fails the calling test) when diagnostics are present.
// Go: internal/testutil/parsetestutil/parsetestutil.go:CheckDiagnosticsMessage
pub fn check_diagnostics_message(file: &ParsedSourceFile, message: &str) {
    if !file.diagnostics.is_empty() {
        panic!("{message}{}", format_diagnostics(&file.diagnostics));
    }
}

/// Sets the location of `node` and every node in its subtree to the undefined
/// range `(-1, -1)`, marking the tree as synthetic.
///
/// # Examples
/// ```
/// use tsgo_testutil_parsetestutil::{parse_type_script, mark_synthetic_recursive};
/// let mut file = parse_type_script("let x = 1;", false);
/// mark_synthetic_recursive(&mut file.arena, file.source_file);
/// assert_eq!(file.arena.loc(file.source_file).pos(), -1);
/// ```
///
/// Side effects: mutates the locations of `node` and all of its descendants in
/// the arena.
// Go: internal/testutil/parsetestutil/parsetestutil.go:MarkSyntheticRecursive
pub fn mark_synthetic_recursive(arena: &mut NodeArena, node: NodeId) {
    arena.set_loc(node, TextRange::new(-1, -1));
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    let mut visit = |arena: &mut NodeArena, child: NodeId| -> NodeId {
        mark_synthetic_recursive(arena, child);
        child
    };
    arena.visit_each_child(node, opts, &mut visit);
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
