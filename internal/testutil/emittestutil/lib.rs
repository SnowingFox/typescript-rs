//! `tsgo_testutil_emittestutil` — 1:1 Rust port of Go
//! `internal/testutil/emittestutil`.
//!
//! A single helper, [`check_emit`], that pretty-prints a source file, asserts
//! the output matches an expected string, and re-parses the output to confirm
//! it is still diagnostic-free. Used by the printer/transformer test suites.
//!
//! # Divergences from Go
//!
//! * Go's `CheckEmit` takes a `*testing.T` and reports via `gotest.tools/assert`;
//!   this port `panic!`s on a mismatch (a failing assertion in the caller's
//!   `#[test]`), exactly like the ported [`tsgo_testutil_parsetestutil`] checks.
//! * In this port the [`EmitContext`] owns the [`NodeArena`] holding the source
//!   file, and the printer reads the original source text as an argument, so
//!   [`check_emit`] takes the `EmitContext`, the source-file id, and the source
//!   `text` separately (Go reads the text and arena off the global `*ast.SourceFile`).

use tsgo_ast::{NodeData, NodeId};
use tsgo_core::compileroptions::NewLineKind;
use tsgo_core::languagevariant::LanguageVariant;
use tsgo_printer::{EmitContext, PrintHandlers, Printer, PrinterOptions};
use tsgo_testutil_parsetestutil::{check_diagnostics_message, parse_type_script};

/// Pretty-prints `source_file` (owned by `emit_context`'s arena, with original
/// `text`) with LF newlines, asserts the trimmed output equals `expected`, and
/// re-parses the output asserting it produces no diagnostics.
///
/// Side effects: panics (fails the calling test) when the emitted text differs
/// from `expected` or when the re-parse reports diagnostics.
// Go: internal/testutil/emittestutil/emittestutil.go:CheckEmit
pub fn check_emit(emit_context: &EmitContext, source_file: NodeId, text: &str, expected: &str) {
    let mut printer = Printer::new(
        PrinterOptions {
            new_line: NewLineKind::Lf,
            ..Default::default()
        },
        PrintHandlers::default(),
        emit_context,
    );
    let emitted = printer.emit_source_file(source_file, text);
    let actual = emitted.strip_suffix('\n').unwrap_or(&emitted);
    assert_eq!(actual, expected, "emit mismatch");

    let jsx = matches!(
        emit_context.arena().data(source_file),
        NodeData::SourceFile(d) if d.language_variant == LanguageVariant::Jsx
    );
    let reparsed = parse_type_script(&emitted, jsx);
    check_diagnostics_message(&reparsed, "error on reparse: ");
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
