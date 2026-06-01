//! Indentation-string rendering (reachable subset of `span.go`).
//!
//! Ports the pure helper `getIndentationString` from Go
//! `internal/format/span.go`. The rest of `span.go` — the `formatSpanWorker`
//! that walks the AST, applies rules, and emits `core.TextChange` edits — is
//! deferred to a later P7 `format` round.
//!
//! # Deferred (blocked-by)
//!
//! `formatSpanWorker` (`findEnclosingNode`, `processNode`/`processChildNode`/
//! `processPair`, `applyRuleEdits`, the trailing-whitespace trimming, the
//! `dynamicIndenter`, ...) requires: the `tsgo_astnav` `&mut SourceFile`
//! navigation context threaded through a recursive `ast::NodeVisitor` walk while
//! a `tsgo_scanner` scanner runs over the same text; SourceFile-aware scanner
//! line/position helpers (`GetECMALineOfPosition`/`GetECMALineStarts`/
//! `GetTokenPosOfNode`); and the `formattingScanner` (`scanner.go`). These are
//! the subject of a follow-up round. See the phase-7 worklog.

use crate::format_code_settings::FormatCodeSettings;

/// Renders the indentation prefix string for `indentation` columns under
/// `options`.
///
/// When `convert_tabs_to_spaces` is true, emits that many spaces. Otherwise it
/// emits as many tabs as fit (`indentation / tab_size`) followed by the
/// remainder in spaces; a `tab_size` of 0 yields the empty string. Mirrors Go's
/// `getIndentationString`.
///
/// # Examples
/// ```
/// use tsgo_format::format_code_settings::get_default_format_code_settings;
/// use tsgo_format::span::get_indentation_string;
/// let opts = get_default_format_code_settings(); // spaces
/// assert_eq!(get_indentation_string(4, &opts), "    ");
/// ```
///
/// Side effects: none (pure).
// Go: internal/format/span.go:getIndentationString
pub fn get_indentation_string(indentation: i32, options: &FormatCodeSettings) -> String {
    if !options.editor.convert_tabs_to_spaces.is_true() {
        if options.editor.tab_size == 0 {
            return String::new();
        }
        let indentation = indentation.max(0);
        let tabs = indentation / options.editor.tab_size;
        let spaces = indentation - (tabs * options.editor.tab_size);
        let mut res = "\t".repeat(tabs as usize);
        if spaces > 0 {
            res.push_str(&" ".repeat(spaces as usize));
        }
        res
    } else {
        " ".repeat(indentation.max(0) as usize)
    }
}

#[cfg(test)]
#[path = "span_test.rs"]
mod tests;
