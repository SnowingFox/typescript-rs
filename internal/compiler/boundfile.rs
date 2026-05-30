//! A per-file [`BoundProgram`] view the checker consumes.
//!
//! The checker's `Checker::new_checker` takes an `Rc<dyn BoundProgram>` (round
//! 4l): the minimal read-only surface over one bound source file (its arena,
//! root node, and the binder's symbol/flow side maps), *retained* by the
//! checker. This module bridges a bound [`ParsedFile`] to that trait so
//! `tsgo_checker` can be constructed from compiler-owned data.
//!
//! # Ownership divergence (P6-4)
//!
//! Because the checker stores `Rc<dyn BoundProgram + 'static>`, the program it
//! retains must own / `'static`-share its data — a borrowing `BoundFile<'a>`
//! (the P6-2 shape) no longer compiles. Per PORTING §3 (a shared, non-owning
//! pointer maps to `Rc<T>`) and §5 (the arena owns the nodes), [`BoundFile`] now
//! *owns* shared `Rc` handles to the bound file's arena and bind result (cloned
//! from the [`ParsedFile`] — see [`ParsedFile`](crate::ParsedFile)'s own
//! ownership note). It is therefore `'static`, so `Rc<BoundFile>` coerces to
//! `Rc<dyn BoundProgram>` and K checkers can share one program by `Rc::clone`.
//!
//! DIVERGENCE(port): Go's checker is constructed once over the whole
//! `compiler.Program` and checks many files; the ported `BoundProgram` is
//! single-file. Until a multi-file program view is exposed by the checker's
//! public API, each pool is seeded from one file's [`BoundFile`].
//! blocked-by: multi-file `compiler.Program` view (P6 program).

use std::rc::Rc;

use tsgo_ast::flow::{FlowList, FlowListId, FlowNode, FlowNodeId};
use tsgo_ast::{NodeArena, NodeId, Symbol, SymbolId, SymbolTable};
use tsgo_binder::BindResult;
use tsgo_checker::BoundProgram;

use crate::host::ParsedFile;

/// An owned, shareable view of one bound source file, implementing
/// [`BoundProgram`].
///
/// Owns `Rc` handles to the file's arena and bind result, so it is `'static`
/// and can be placed in `Rc<dyn BoundProgram>` for the checker to retain (see
/// the module-level ownership divergence note).
///
/// Side effects: none (shares the file's arena and bind result via `Rc`).
// Go: internal/compiler/program.go:Program (the bound-file query surface)
pub struct BoundFile {
    arena: Rc<NodeArena>,
    root: NodeId,
    bind: Rc<BindResult>,
}

impl BoundFile {
    /// Builds a [`BoundFile`] over `file`, or `None` if `file` has not been bound
    /// yet (see [`ParsedFile::bind`](crate::ParsedFile::bind)).
    ///
    /// Side effects: none (clones the file's `Rc` arena/bind handles).
    pub fn for_file(file: &ParsedFile) -> Option<BoundFile> {
        let bind = file.bind_rc()?;
        Some(BoundFile {
            arena: file.arena_rc(),
            root: file.node(),
            bind,
        })
    }
}

impl BoundProgram for BoundFile {
    fn arena(&self) -> &NodeArena {
        &self.arena
    }

    fn root(&self) -> NodeId {
        self.root
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

#[cfg(test)]
#[path = "boundfile_test.rs"]
mod tests;
