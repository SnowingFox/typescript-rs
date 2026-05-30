//! A thin abstraction over a bound source program.
//!
//! Symbol queries (`resolve_name`, `get_symbol_at_location`, ...) need the
//! parsed node arena plus the binder's per-node symbol map and per-container
//! `locals` tables. Rather than depend on a concrete program/host (the real one
//! is `compiler.Program`, ported in Phase 6) or on `tsgo_binder`'s `BindResult`
//! type directly, the query layer talks to this trait, which exposes only
//! `tsgo_ast` types.
//!
//! In tests a minimal stub (parse via `tsgo_parser` + bind via `tsgo_binder`)
//! implements it; later phases plug in the real program.

use tsgo_ast::flow::{FlowList, FlowListId, FlowNode, FlowNodeId};
use tsgo_ast::{NodeArena, NodeId, Symbol, SymbolId, SymbolTable};

/// A bound source file the checker can answer symbol queries against.
///
/// This is the minimal surface the 4b symbol queries require; it grows as later
/// sub-phases need more (declared types, flow, diagnostics, multi-file lookup).
///
/// # Examples
/// ```
/// use tsgo_checker::BoundProgram;
/// // The query layer is generic over any bound program.
/// fn statement_count<P: BoundProgram>(p: &P) -> usize {
///     p.arena().node_count() // total nodes parsed
/// }
/// ```
///
/// Side effects: implementations are read-only views over already-bound data.
// Go: internal/compiler/program.go:Program (the subset the checker queries)
pub trait BoundProgram {
    /// The node arena owning every parsed node.
    fn arena(&self) -> &NodeArena;

    /// The root `SourceFile` node id.
    fn root(&self) -> NodeId;

    /// The declaration symbol bound to `node` (Go's `node.Symbol()`), if any.
    fn symbol_of_node(&self, node: NodeId) -> Option<SymbolId>;

    /// The symbol record for `id`.
    fn symbol(&self, id: SymbolId) -> &Symbol;

    /// The `locals` table of a locals-bearing container node, if it has one.
    fn locals(&self, container: NodeId) -> Option<&SymbolTable>;

    /// The control-flow node attached to `node` (Go's `node.FlowNode`), if any.
    ///
    /// The binder sets this on narrowable references (identifiers, `this`,
    /// property/element access) and removes it for unreachable code.
    fn flow_node_of(&self, node: NodeId) -> Option<FlowNodeId>;

    /// The flow node for `id` in the binder's control-flow graph.
    fn flow_node(&self, id: FlowNodeId) -> FlowNode;

    /// The flow-list cell for `id` (a cons cell of label antecedents).
    fn flow_list(&self, id: FlowListId) -> FlowList;
}

#[cfg(test)]
#[path = "program_test.rs"]
mod tests;
