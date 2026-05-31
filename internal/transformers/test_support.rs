//! Shared test harness for the transformer infrastructure.
//!
//! Parses a tiny TypeScript source into an [`EmitContext`]-owned arena and emits
//! a (possibly transformed) node back to text, mirroring the Go transformer
//! tests' `ParseTypeScript` + `CheckEmit` round-trip.

use std::cell::RefCell;
use std::rc::Rc;
use tsgo_ast::flow::{FlowList, FlowListId, FlowNode, FlowNodeId, FlowSwitchClauseData};
use tsgo_ast::{Kind, NodeArena, NodeId, Symbol, SymbolId, SymbolTable, VisitOptions};
use tsgo_binder::{bind_source_file, BindResult};
use tsgo_checker::{BoundProgram, Checker};
use tsgo_core::compileroptions::NewLineKind;
use tsgo_core::get_script_kind_from_file_name;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};
use tsgo_printer::printer::{PrintHandlers, Printer, PrinterOptions};
use tsgo_printer::EmitContext;

use crate::EmitReferenceResolver;

/// Parses `input` as `/main.ts` and returns a shared emit context owning the
/// parsed arena plus the root source-file id. Panics on parse diagnostics so a
/// malformed fixture fails loudly.
pub(crate) fn parse_shared(input: &str) -> (Rc<RefCell<EmitContext>>, NodeId) {
    parse_shared_named(input, "/main.ts")
}

/// Like [`parse_shared`] but parses as `/main.tsx`, enabling JSX syntax.
pub(crate) fn parse_shared_tsx(input: &str) -> (Rc<RefCell<EmitContext>>, NodeId) {
    parse_shared_named(input, "/main.tsx")
}

/// Parses `input` under the script kind implied by `file_name`.
pub(crate) fn parse_shared_named(
    input: &str,
    file_name: &str,
) -> (Rc<RefCell<EmitContext>>, NodeId) {
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

/// Builds a scope-correct [`EmitReferenceResolver`] over `input` (parsed as
/// `/main.ts`), for driving the import-elision transform.
///
/// Parses and binds `input` into its own bound program, then wraps the
/// checker's [`EmitResolver`](tsgo_checker::EmitResolver). Because parsing is
/// deterministic, this bound program's node ids match the (separately parsed)
/// [`parse_shared`] arena the transform reads from, so a declaration node id
/// from the transform resolves to the same syntactic node here.
pub(crate) fn build_reference_resolver(input: &str) -> EmitReferenceResolver {
    let program = BoundFile::parse_and_bind("/main.ts", input);
    let resolver = Checker::new().get_emit_resolver();
    EmitReferenceResolver::new(Rc::new(program), resolver)
}

/// A parsed-and-bound single source file exposed through [`BoundProgram`], the
/// transformer-crate test stand-in for the real multi-file program (mirrors the
/// checker crate's own `StubProgram`, which is not exported across crates).
struct BoundFile {
    arena: NodeArena,
    source_file: NodeId,
    bind: BindResult,
}

impl BoundFile {
    fn parse_and_bind(file_name: &str, text: &str) -> BoundFile {
        let opts = SourceFileParseOptions {
            file_name: file_name.to_string(),
        };
        let mut parsed = parse_source_file(opts, text, ScriptKind::Ts);
        assert!(
            parsed.diagnostics.is_empty(),
            "parse error for {text:?}: {:?}",
            parsed.diagnostics
        );
        let bind = bind_source_file(&mut parsed.arena, parsed.source_file);
        BoundFile {
            arena: parsed.arena,
            source_file: parsed.source_file,
            bind,
        }
    }
}

impl BoundProgram for BoundFile {
    fn arena(&self) -> &NodeArena {
        &self.arena
    }

    fn root(&self) -> NodeId {
        self.source_file
    }

    fn symbol_of_node(&self, node: NodeId) -> Option<SymbolId> {
        self.bind.node_symbol.get(&node).copied()
    }

    fn symbol(&self, id: SymbolId) -> &Symbol {
        &self.bind.symbols[id.index()]
    }

    fn locals(&self, container: NodeId) -> Option<&SymbolTable> {
        self.bind.locals.get(&container)
    }

    fn globals(&self) -> Option<&SymbolTable> {
        // For a script (non-module) source file the top-level `locals` are the
        // program's globals (the same synthetic-globals stand-in the checker's
        // `StubProgram` uses).
        self.bind.locals.get(&self.source_file)
    }

    fn flow_node_of(&self, node: NodeId) -> Option<FlowNodeId> {
        self.bind.node_flow.get(&node).copied()
    }

    fn flow_node(&self, id: FlowNodeId) -> FlowNode {
        self.bind.flow_nodes[id.0 as usize]
    }

    fn flow_list(&self, id: FlowListId) -> FlowList {
        self.bind.flow_lists[id.0 as usize]
    }

    fn flow_switch_clause_data(&self, id: FlowNodeId) -> Option<FlowSwitchClauseData> {
        self.bind.flow_switch_data.get(&id).copied()
    }
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
