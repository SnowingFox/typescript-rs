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
use tsgo_core::compileroptions::CompilerOptions;

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
    /// This file's source text, so the checker can reproduce Go's trivia-skipped
    /// diagnostic spans (see [`BoundProgram::source_text`]).
    text: Rc<str>,
    root: NodeId,
    bind: Rc<BindResult>,
    /// The program's real compiler options (see
    /// [`BoundProgram::compiler_options`]).
    options: Rc<CompilerOptions>,
    /// True when the parse pass recorded syntactic diagnostics for this file.
    has_parse_diagnostics: bool,
}

impl BoundFile {
    /// Builds a [`BoundFile`] over `file` with all-defaults compiler options, or
    /// `None` if `file` has not been bound yet (see
    /// [`ParsedFile::bind`](crate::ParsedFile::bind)).
    ///
    /// This is the additive, options-free overload; the checker then sees
    /// all-defaults options. To surface a program's REAL options to the
    /// checker's option-gated diagnostics, use [`Self::for_file_with_options`].
    ///
    /// Side effects: none (clones the file's `Rc` arena/bind handles).
    pub fn for_file(file: &ParsedFile) -> Option<BoundFile> {
        BoundFile::for_file_with_options(file, Rc::new(CompilerOptions::default()))
    }

    /// Builds a [`BoundFile`] over `file` carrying the program's real `options`,
    /// or `None` if `file` has not been bound yet.
    ///
    /// The shared `options` are returned by
    /// [`BoundProgram::compiler_options`], so a checker built over this view
    /// reads the program's actual `--target`/`--downlevelIteration`/`--strict`
    /// config (overriding the trait's all-defaults default).
    ///
    /// Side effects: none (clones the file's `Rc` arena/bind handles and shares
    /// the options via `Rc`).
    // Go: internal/compiler/program.go:Program.Options
    pub fn for_file_with_options(
        file: &ParsedFile,
        options: Rc<CompilerOptions>,
    ) -> Option<BoundFile> {
        let bind = file.bind_rc()?;
        Some(BoundFile {
            arena: file.arena_rc(),
            text: Rc::from(file.text()),
            root: file.node(),
            bind,
            options,
            has_parse_diagnostics: !file.diagnostics().is_empty(),
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

    fn source_text(&self) -> Option<&str> {
        Some(&self.text)
    }

    fn globals(&self) -> Option<&SymbolTable> {
        // Go's `Checker.globals` is the merge of every global (script / lib,
        // i.e. non-module) source file's top-level `Locals`. For a single bound
        // file, its root `SourceFile` node's `locals` table is that file's
        // contribution — and, for a `lib.*.d.ts`, the table holding the real
        // `Array` / `String` / `Object` global declarations the checker's 4z
        // `get_global_symbol` / `get_global_type` resolve against.
        //
        // DEFER(P6): the cross-file MERGE (combining the lib file's globals with
        // the source files' top-level declarations, and across multiple libs)
        // needs a multi-file program view; a single `BoundFile` exposes one
        // file's table.
        // blocked-by: multi-file `compiler.Program` `BoundProgram` view + the
        // `/// <reference lib>` graph (P6-8).
        self.bind.locals.get(&self.root)
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

    fn compiler_options(&self) -> &CompilerOptions {
        &self.options
    }

    fn has_parse_diagnostics(&self) -> bool {
        self.has_parse_diagnostics
    }
}

#[cfg(test)]
#[path = "boundfile_test.rs"]
mod tests;
