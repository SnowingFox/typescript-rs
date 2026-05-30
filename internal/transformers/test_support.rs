//! Shared test harness for the transformer infrastructure.
//!
//! Parses a tiny TypeScript source into an [`EmitContext`]-owned arena and emits
//! a (possibly transformed) node back to text, mirroring the Go transformer
//! tests' `ParseTypeScript` + `CheckEmit` round-trip.

use std::cell::RefCell;
use std::rc::Rc;
use tsgo_ast::{Kind, NodeArena, NodeId, VisitOptions};
use tsgo_core::compileroptions::NewLineKind;
use tsgo_core::get_script_kind_from_file_name;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};
use tsgo_printer::printer::{PrintHandlers, Printer, PrinterOptions};
use tsgo_printer::EmitContext;

/// Parses `input` as `/main.ts` and returns a shared emit context owning the
/// parsed arena plus the root source-file id. Panics on parse diagnostics so a
/// malformed fixture fails loudly.
pub(crate) fn parse_shared(input: &str) -> (Rc<RefCell<EmitContext>>, NodeId) {
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
    (
        Rc::new(RefCell::new(EmitContext::with_arena(parse.arena))),
        parse.source_file,
    )
}

/// Emits source-file `node` from the shared context, returning the produced text
/// with its trailing newline trimmed. `source_text` is the original input text
/// the printer reads for non-synthetic literal nodes and line starts.
pub(crate) fn emit(ec: &Rc<RefCell<EmitContext>>, node: NodeId, source_text: &str) -> String {
    let ec_ref = ec.borrow();
    let mut printer = Printer::new(
        PrinterOptions {
            new_line: NewLineKind::Lf,
            ..Default::default()
        },
        PrintHandlers::default(),
        &ec_ref,
    );
    let text = printer.emit_source_file(node, source_text);
    text.strip_suffix('\n').unwrap_or(&text).to_string()
}

/// Recursively rewrites every identifier whose text is `from` into a fresh
/// identifier `to`, rebuilding interior nodes through `visit_each_child`. A
/// reusable transform body for the driver/chain tests.
pub(crate) fn rename_ident(arena: &mut NodeArena, node: NodeId, from: &str, to: &str) -> NodeId {
    if arena.kind(node) == Kind::Identifier && arena.text(node) == from {
        return arena.new_identifier(to);
    }
    let opts = VisitOptions {
        synthetic_location: false,
        clone_lists: false,
    };
    arena.visit_each_child(node, opts, &mut |a, c| rename_ident(a, c, from, to))
}
