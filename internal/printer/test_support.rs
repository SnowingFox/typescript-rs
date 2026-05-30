//! Shared test harness mirroring Go `emittestutil.CheckEmit` + `parsetestutil`.

use crate::emitcontext::EmitContext;
use crate::printer::{PrintHandlers, Printer, PrinterOptions};
use tsgo_ast::{NodeArena, NodeId};
use tsgo_core::compileroptions::NewLineKind;
use tsgo_core::get_script_kind_from_file_name;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

/// Emits a synthetic (factory-built) source file, mirroring Go's
/// `MarkSyntheticRecursive` + `CheckEmit(nil, file, ...)` flow. The arena's nodes
/// carry undefined positions (synthetic), so no source text is available.
pub(crate) fn check_synthetic(mut arena: NodeArena, source_file: NodeId, expected: &str) {
    arena.set_parent_in_children(source_file);
    let ec = EmitContext::with_arena(arena);
    let mut printer = Printer::new(
        PrinterOptions {
            new_line: NewLineKind::Lf,
            ..Default::default()
        },
        PrintHandlers::default(),
        &ec,
    );
    let text = printer.emit_source_file(source_file, "");
    let actual = text.strip_suffix('\n').unwrap_or(&text);
    assert_eq!(actual, expected, "synthetic emit mismatch");
}

/// Parses `input`, lets `setup` mutate the emit context (e.g. attach emit
/// helpers to the source file) before emitting, and returns the produced text.
pub(crate) fn emit_after<F>(input: &str, setup: F) -> String
where
    F: FnOnce(&mut EmitContext, NodeId),
{
    let file_name = "/main.ts";
    let script_kind = get_script_kind_from_file_name(file_name);
    let parse = parse_source_file(
        SourceFileParseOptions {
            file_name: file_name.to_string(),
        },
        input,
        script_kind,
    );
    assert!(
        parse.diagnostics.is_empty(),
        "parse error for {input:?}: {:?}",
        parse.diagnostics
    );
    let source_file = parse.source_file;
    let mut ec = EmitContext::with_arena(parse.arena);
    setup(&mut ec, source_file);
    let mut printer = Printer::new(
        PrinterOptions {
            new_line: NewLineKind::Lf,
            ..Default::default()
        },
        PrintHandlers::default(),
        &ec,
    );
    printer.emit_source_file(source_file, input)
}

/// Parses `input`, emits the whole source file, and returns the produced text
/// (including the trailing newline the emitter writes).
pub(crate) fn emit(input: &str, jsx: bool) -> String {
    let file_name = if jsx { "/main.tsx" } else { "/main.ts" };
    let script_kind = get_script_kind_from_file_name(file_name);
    let parse = parse_source_file(
        SourceFileParseOptions {
            file_name: file_name.to_string(),
        },
        input,
        script_kind,
    );
    assert!(
        parse.diagnostics.is_empty(),
        "parse error for {input:?}: {:?}",
        parse.diagnostics
    );
    let ec = EmitContext::with_arena(parse.arena);
    let mut printer = Printer::new(
        PrinterOptions {
            new_line: NewLineKind::Lf,
            ..Default::default()
        },
        PrintHandlers::default(),
        &ec,
    );
    printer.emit_source_file(parse.source_file, input)
}

/// Asserts that emitting `input` yields `expected` (after trimming the trailing
/// newline), and that the output re-parses without diagnostics. Mirrors the Go
/// `emittestutil.CheckEmit` helper.
pub(crate) fn check_emit(input: &str, expected: &str, jsx: bool) {
    let text = emit(input, jsx);
    let actual = text.strip_suffix('\n').unwrap_or(&text);
    assert_eq!(actual, expected, "emit mismatch for input {input:?}");

    let file_name = if jsx { "/main.tsx" } else { "/main.ts" };
    let script_kind = get_script_kind_from_file_name(file_name);
    let reparse = parse_source_file(
        SourceFileParseOptions {
            file_name: file_name.to_string(),
        },
        &text,
        script_kind,
    );
    assert!(
        reparse.diagnostics.is_empty(),
        "reparse error for {input:?} (emitted {text:?}): {:?}",
        reparse.diagnostics
    );
}
