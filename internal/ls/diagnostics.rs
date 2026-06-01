//! Port of Go `internal/ls/diagnostics.go`: the per-file diagnostics feature.
//!
//! Go's `ProvideDiagnostics` gathers a file's syntactic, semantic, suggestion,
//! and (when declarations are emitted) declaration diagnostics, then maps each
//! `ast.Diagnostic` to an `lsproto.Diagnostic` via `lsconv.DiagnosticToLSPPull`.
//!
//! # Reachable subset
//!
//! This round ports the syntactic and semantic halves as two methods returning
//! [`lsproto::Diagnostic`]s directly:
//! - [`LanguageService::get_syntactic_diagnostics`] reads the file's parser
//!   diagnostics (`file.Diagnostics()`).
//! - [`LanguageService::get_semantic_diagnostics`] drives the program's checker
//!   pool (`program.GetSemanticDiagnostics`).
//!
//! The diagnostic→LSP mapping is the reachable subset of Go's `diagnosticToLSP`:
//! category→severity, the integer `code`, the `"ts"` source, the localized
//! message, and the UTF-16 range (via the project [`Converters`]). The
//! client-capability-gated facets (related information, tags, the Visual Studio
//! `TS<code>` string code) and the message-chain flattening are deferred.
//!
//! DEFER(phase-7-ls): suggestion + declaration diagnostics (`GetSuggestionDiagnostics`/
//! `GetDeclarationDiagnostics`), per-file filtering of a multi-file program's
//! semantic diagnostics, related-information / tags / message-chain rendering,
//! and the push (`publishDiagnostics`) path.
//! blocked-by: the program's per-file semantic-diagnostic partition + suggestion/
//! declaration diagnostics, `tsgo_diagnosticwriter` message-chain flattening, and
//! the client-capability surface.

use tsgo_checker::{Category, Diagnostic as CheckerDiagnostic};
use tsgo_core::text::TextRange;
use tsgo_locale::Locale;
use tsgo_ls_lsconv::{Converters, Script};
use tsgo_lsproto::{Diagnostic, DiagnosticSeverity, IntegerOrString};

use crate::languageservice::LanguageService;

impl LanguageService {
    /// Returns the syntactic (parser) diagnostics for `file_name` as
    /// [`lsproto::Diagnostic`]s, or an empty list if the program has no such
    /// file.
    ///
    /// Mirrors Go's `program.GetSyntacticDiagnostics(file)` half of
    /// `getAllDiagnostics`: the file's parse diagnostics, mapped to LSP shape
    /// (UTF-16 range, integer `code`, `"ts"` source, localized message).
    ///
    /// Side effects: none (reads the already-parsed file).
    // Go: internal/ls/diagnostics.go:getAllDiagnostics (GetSyntacticDiagnostics)
    pub fn get_syntactic_diagnostics(&self, file_name: &str) -> Vec<Diagnostic> {
        let Some(script) = self.document_script(file_name) else {
            return Vec::new();
        };
        let Some(file) = self.program().get_source_file(file_name) else {
            return Vec::new();
        };
        let converters = self.converters();
        let locale = Locale::default();
        file.diagnostics()
            .iter()
            .map(|diag| {
                let args: Vec<&str> = diag.args.iter().map(String::as_str).collect();
                to_lsp_diagnostic(
                    converters,
                    &script,
                    diag.message.code(),
                    diag.message.category(),
                    diag.message.localize(&locale, &args),
                    diag.loc,
                )
            })
            .collect()
    }

    /// Returns the semantic (type-checker) diagnostics for `file_name` as
    /// [`lsproto::Diagnostic`]s, or an empty list if the program has no such
    /// file.
    ///
    /// Mirrors Go's `program.GetSemanticDiagnostics(file)` half of
    /// `getAllDiagnostics`: binds the program and drives its checker pool, then
    /// maps each diagnostic to LSP shape.
    ///
    /// DEFER(phase-7-ls): per-file filtering. The compiler's
    /// `Program::semantic_diagnostics` returns every non-lib file's diagnostics
    /// (it has no per-file partition yet); the reachable subset is a
    /// single-user-file program, where they all belong to `file_name`.
    /// blocked-by: a per-file semantic-diagnostic partition on `tsgo_compiler`.
    ///
    /// Side effects: binds every program file and allocates the checker pool
    /// (idempotent; mutates per-file arenas).
    // Go: internal/ls/diagnostics.go:getAllDiagnostics (GetSemanticDiagnostics)
    pub fn get_semantic_diagnostics(&mut self, file_name: &str) -> Vec<Diagnostic> {
        let Some(script) = self.document_script(file_name) else {
            return Vec::new();
        };
        let diagnostics = self.program_mut().semantic_diagnostics();
        let converters = self.converters();
        diagnostics
            .iter()
            .map(|diag| to_lsp_from_checker(converters, &script, diag))
            .collect()
    }
}

/// Maps a [`tsgo_checker::Diagnostic`] to an [`lsproto::Diagnostic`] over
/// `script`.
///
/// Side effects: none (pure).
// Go: internal/ls/lsconv/converters.go:diagnosticToLSP
fn to_lsp_from_checker(
    converters: &Converters,
    script: &dyn Script,
    diagnostic: &CheckerDiagnostic,
) -> Diagnostic {
    to_lsp_diagnostic(
        converters,
        script,
        diagnostic.code,
        diagnostic.category,
        diagnostic.message.clone(),
        TextRange::new(diagnostic.start, diagnostic.start + diagnostic.length),
    )
}

/// Builds an [`lsproto::Diagnostic`] from a diagnostic's parts, converting its
/// internal byte range to a UTF-16 LSP range over `script`.
///
/// This is the reachable subset of Go's `diagnosticToLSP`: severity from the
/// category, the integer `code`, the `"ts"` source, the localized `message`,
/// and the converted `range`. Related information, tags, and the Visual Studio
/// `TS<code>` string code are deferred (see the module note).
///
/// Side effects: none (pure).
// Go: internal/ls/lsconv/converters.go:diagnosticToLSP
fn to_lsp_diagnostic(
    converters: &Converters,
    script: &dyn Script,
    code: i32,
    category: Category,
    message: String,
    text_range: TextRange,
) -> Diagnostic {
    Diagnostic {
        range: converters.to_lsp_range(script, text_range),
        severity: Some(category_to_severity(category)),
        code: Some(IntegerOrString {
            integer: Some(code),
            string: None,
        }),
        code_description: None,
        source: Some("ts".to_string()),
        message,
        tags: None,
        related_information: None,
        data: None,
    }
}

/// Maps a diagnostic [`Category`] to its LSP [`DiagnosticSeverity`].
///
/// Mirrors Go's `diagnosticToLSP` switch: suggestion→hint, message→information,
/// warning→warning, and everything else (errors) →error.
///
/// Side effects: none (pure).
// Go: internal/ls/lsconv/converters.go:diagnosticToLSP (severity switch)
fn category_to_severity(category: Category) -> DiagnosticSeverity {
    match category {
        Category::Suggestion => DiagnosticSeverity::HINT,
        Category::Message => DiagnosticSeverity::INFORMATION,
        Category::Warning => DiagnosticSeverity::WARNING,
        Category::Error => DiagnosticSeverity::ERROR,
    }
}

#[cfg(test)]
#[path = "diagnostics_test.rs"]
mod tests;
