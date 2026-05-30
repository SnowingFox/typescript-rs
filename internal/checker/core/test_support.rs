//! Test-only stub program: parse a single source string with `tsgo_parser` and
//! bind it with `tsgo_binder`, exposing the result through [`BoundProgram`].
//!
//! This is the minimal in-memory stand-in for the real multi-file
//! `compiler.Program` (Phase 6), used to drive the 4b symbol-query tests
//! (notably the port of Go's `TestGetSymbolAtLocation`). It lives behind
//! `cfg(test)`, so `tsgo_parser`/`tsgo_binder` are dev-dependencies only.

use tsgo_ast::flow::{FlowList, FlowListId, FlowNode, FlowNodeId};
use tsgo_ast::{NodeArena, NodeId, Symbol, SymbolId, SymbolTable};
use tsgo_binder::bind_source_file;
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

use super::program::BoundProgram;

/// A parsed-and-bound single source file.
pub(crate) struct StubProgram {
    arena: NodeArena,
    source_file: NodeId,
    bind: tsgo_binder::BindResult,
}

impl StubProgram {
    /// Parses `text` as a `.ts` file named `file_name` and binds it.
    pub(crate) fn parse_and_bind(file_name: &str, text: &str) -> StubProgram {
        StubProgram::parse_and_bind_with(file_name, text, ScriptKind::Ts)
    }

    /// Parses `text` as a `.tsx` file (JSX enabled) named `file_name` and binds it.
    pub(crate) fn parse_and_bind_tsx(file_name: &str, text: &str) -> StubProgram {
        StubProgram::parse_and_bind_with(file_name, text, ScriptKind::Tsx)
    }

    fn parse_and_bind_with(file_name: &str, text: &str, script_kind: ScriptKind) -> StubProgram {
        let opts = SourceFileParseOptions {
            file_name: file_name.to_string(),
        };
        let mut parsed = parse_source_file(opts, text, script_kind);
        let bind = bind_source_file(&mut parsed.arena, parsed.source_file);
        StubProgram {
            arena: parsed.arena,
            source_file: parsed.source_file,
            bind,
        }
    }
}

impl BoundProgram for StubProgram {
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

    fn flow_node_of(&self, node: NodeId) -> Option<FlowNodeId> {
        self.bind.node_flow.get(&node).copied()
    }

    fn flow_node(&self, id: FlowNodeId) -> FlowNode {
        self.bind.flow_nodes[id.0 as usize]
    }

    fn flow_list(&self, id: FlowListId) -> FlowList {
        self.bind.flow_lists[id.0 as usize]
    }
}
