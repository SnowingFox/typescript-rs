//! Port of Go `internal/transformers/transformer.go`: the shared transform
//! driver that runs a visit callback over a `SourceFile`.
//!
//! # Ownership model (read this first)
//!
//! Go's `Transformer` holds a shared `*printer.EmitContext` pointer, the
//! `*printer.NodeFactory` it owns, and an `*ast.NodeVisitor` wrapping the
//! transform's `visit func(*Node) *Node` callback. In Rust the `EmitContext`
//! owns the single [`NodeArena`](tsgo_ast::NodeArena), so the shared pointer maps
//! to `Rc<RefCell<EmitContext>>` (PORTING.md §3: a shared, mutable Go pointer →
//! `Rc<RefCell<T>>`). The visit callback receives `&mut EmitContext` (giving it
//! the arena + factory + side tables) and returns the replacement node id;
//! transforms recurse into children via
//! [`NodeArena::visit_each_child`](tsgo_ast::NodeArena::visit_each_child) on
//! `ec.arena_mut()`. There is no standalone `NodeVisitor` object (the Rust
//! `ast` port exposes child-visiting directly on the arena), so Go's
//! `Visitor()`/`Factory()` accessors collapse into [`Transformer::emit_context`].

use crate::SharedEmitContext;
use std::cell::RefCell;
use std::rc::Rc;
use tsgo_ast::NodeId;
use tsgo_printer::EmitContext;

/// A boxed transform visit callback.
///
/// Mirrors Go's `visit func(node *ast.Node) *ast.Node`, threaded the owning
/// [`EmitContext`] explicitly because the arena lives there.
///
/// Side effects: invoking the callback may append nodes and write side tables.
pub type VisitFn = Box<dyn FnMut(&mut EmitContext, NodeId) -> NodeId>;

/// The shared transform driver: an [`EmitContext`] plus the visit callback run
/// over a source file.
///
/// # Examples
/// ```
/// use tsgo_transformers::new_transformer;
/// // An identity transformer created with a fresh context.
/// let mut tx = new_transformer(Box::new(|_ec, node| node), None);
/// let _ = tx.emit_context();
/// ```
///
/// Side effects: [`Transformer::transform_source_file`] mutates the shared
/// context's arena.
// Go: internal/transformers/transformer.go:Transformer
pub struct Transformer {
    emit_context: SharedEmitContext,
    visit: VisitFn,
}

/// Creates a transformer wrapping `visit`, defaulting to a fresh shared
/// [`EmitContext`] when `emit_context` is `None`.
///
/// # Examples
/// ```
/// use tsgo_transformers::new_transformer;
/// let tx = new_transformer(Box::new(|_ec, node| node), None);
/// assert!(std::rc::Rc::strong_count(&tx.emit_context()) >= 1);
/// ```
///
/// Side effects: allocates a shared context when none is supplied.
// Go: internal/transformers/transformer.go:Transformer.NewTransformer
pub fn new_transformer(visit: VisitFn, emit_context: Option<SharedEmitContext>) -> Transformer {
    let emit_context = emit_context.unwrap_or_else(|| Rc::new(RefCell::new(EmitContext::new())));
    Transformer {
        emit_context,
        visit,
    }
}

impl Transformer {
    /// Returns a clone of the shared emit context handle.
    ///
    /// Side effects: none (clones an `Rc`).
    // Go: internal/transformers/transformer.go:Transformer.EmitContext
    pub fn emit_context(&self) -> SharedEmitContext {
        Rc::clone(&self.emit_context)
    }

    /// Runs the transform over source file `file`, returning the (possibly new)
    /// source-file id.
    ///
    /// Side effects: mutates the shared context's arena.
    // Go: internal/transformers/transformer.go:Transformer.TransformSourceFile
    pub fn transform_source_file(&mut self, file: NodeId) -> NodeId {
        let ec = Rc::clone(&self.emit_context);
        let mut ec = ec.borrow_mut();
        (self.visit)(&mut ec, file)
    }

    /// Runs the visit callback against an already-borrowed `ec`. Used by
    /// chaining so a composed transform can reuse a single borrow rather than
    /// re-borrowing the shared `RefCell` per component.
    ///
    /// Side effects: see [`VisitFn`].
    pub(crate) fn run_visit(&mut self, ec: &mut EmitContext, node: NodeId) -> NodeId {
        (self.visit)(ec, node)
    }
}

#[cfg(test)]
#[path = "transformer_test.rs"]
mod tests;
