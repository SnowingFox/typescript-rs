//! [`EmitContext`]: the emit-time side-table store and owner of the node arena.
//!
//! Go keys its side tables on `*ast.Node` pointers; following `tsgo_ast`, the
//! Rust port owns a single [`NodeArena`] and keys side tables on [`NodeId`].
//! This file currently implements the auto-generated-name table and the arena
//! ownership the name generator and factory need; the remaining side tables
//! (original links, emit flags, comment/source-map ranges, environments,
//! helpers) are added alongside the emit loop.

use crate::emithelpers::EmitHelper;
use crate::factory::NodeFactory;
use crate::generatedidentifierflags::GeneratedIdentifierFlags;
use rustc_hash::FxHashMap;
use tsgo_ast::{NodeArena, NodeId, NodeList};

/// Options for creating an auto-generated name.
///
/// Side effects: none (pure value type).
// Go: internal/printer/emitcontext.go:AutoGenerateOptions
#[derive(Clone, Debug, Default)]
pub struct AutoGenerateOptions {
    /// Extra flags (kind bits are supplied by the factory method).
    pub flags: GeneratedIdentifierFlags,
    /// Optional prefix applied to the generated name.
    pub prefix: String,
    /// Optional suffix applied to the generated name.
    pub suffix: String,
}

/// A unique id for an auto-generated name, ensuring distinct names get distinct
/// text while clones share text.
///
/// Side effects: none (pure value type).
// Go: internal/printer/emitcontext.go:AutoGenerateId
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AutoGenerateId(pub u32);

/// The recorded description of an auto-generated name.
///
/// Side effects: none (pure value type).
// Go: internal/printer/emitcontext.go:AutoGenerateInfo
#[derive(Clone, Debug)]
pub struct AutoGenerateInfo {
    /// Whether/how to auto-generate the text for the identifier.
    pub flags: GeneratedIdentifierFlags,
    /// Unique id distinguishing this generated name.
    pub id: AutoGenerateId,
    /// Optional prefix applied to the generated name.
    pub prefix: String,
    /// Optional suffix applied to the generated name.
    pub suffix: String,
    /// For a node-based generated name, the source node to generate from.
    pub node: Option<NodeId>,
}

/// Stores side-table information used during transformation/emit, and owns the
/// [`NodeArena`] every node lives in.
///
/// # Examples
/// ```
/// use tsgo_printer::emitcontext::EmitContext;
/// let mut ec = EmitContext::new();
/// let name = ec.factory().new_temp_variable();
/// assert!(ec.has_auto_generate_info(name));
/// ```
///
/// Side effects: `factory` and the `*_mut` accessors allow mutation of the arena
/// and side tables.
// Go: internal/printer/emitcontext.go:EmitContext
#[derive(Debug, Default)]
pub struct EmitContext {
    arena: NodeArena,
    auto_generate: FxHashMap<NodeId, AutoGenerateInfo>,
    next_auto_generate_id: u32,
    original: FxHashMap<NodeId, NodeId>,
    emit_flags: FxHashMap<NodeId, crate::emitflags::EmitFlags>,
    /// Stack of variable environments; each scope collects the
    /// `VariableDeclaration` node ids hoisted into it via
    /// [`add_variable_declaration`](EmitContext::add_variable_declaration).
    var_environments: Vec<Vec<NodeId>>,
    /// Emit helpers attached to specific nodes (e.g. the source file), read by
    /// the printer when emitting the helper prologue.
    node_helpers: FxHashMap<NodeId, Vec<&'static EmitHelper>>,
    /// Emit helpers requested during the current transform, in insertion order
    /// (dependencies first), drained by [`read_emit_helpers`](EmitContext::read_emit_helpers).
    requested_helpers: Vec<&'static EmitHelper>,
}

impl EmitContext {
    /// Creates an empty emit context with a fresh arena.
    ///
    /// Side effects: allocates a new arena.
    // Go: internal/printer/emitcontext.go:NewEmitContext
    pub fn new() -> EmitContext {
        EmitContext::default()
    }

    /// Creates an emit context that takes ownership of an existing arena (e.g. a
    /// parsed file's arena), so the factory can append synthetic nodes to it.
    ///
    /// Side effects: takes ownership of `arena`.
    pub fn with_arena(arena: NodeArena) -> EmitContext {
        EmitContext {
            arena,
            auto_generate: FxHashMap::default(),
            next_auto_generate_id: 0,
            original: FxHashMap::default(),
            emit_flags: FxHashMap::default(),
            var_environments: Vec::new(),
            node_helpers: FxHashMap::default(),
            requested_helpers: Vec::new(),
        }
    }

    /// Returns the node `name` was synthesized from, if recorded.
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/emitcontext.go:EmitContext.Original
    pub fn original(&self, node: NodeId) -> Option<NodeId> {
        self.original.get(&node).copied()
    }

    /// Walks the `original` chain to the most-original node (identity when no
    /// link is recorded, as for freshly-parsed nodes).
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/emitcontext.go:EmitContext.MostOriginal
    pub fn most_original(&self, node: NodeId) -> NodeId {
        let mut node = node;
        while let Some(orig) = self.original.get(&node).copied() {
            node = orig;
        }
        node
    }

    /// Records that `updated` was synthesized from `original`.
    ///
    /// Side effects: inserts into the original table.
    // Go: internal/printer/emitcontext.go:EmitContext.SetOriginal
    pub fn set_original(&mut self, updated: NodeId, original: NodeId) {
        self.original.insert(updated, original);
    }

    /// Returns the emit flags associated with `node` (empty if none).
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/emitcontext.go:EmitContext.EmitFlags
    pub fn emit_flags(&self, node: NodeId) -> crate::emitflags::EmitFlags {
        self.emit_flags
            .get(&node)
            .copied()
            .unwrap_or(crate::emitflags::EmitFlags::NONE)
    }

    /// Replaces the emit flags associated with `node`.
    ///
    /// Side effects: inserts into the emit-flags table.
    // Go: internal/printer/emitcontext.go:EmitContext.SetEmitFlags
    pub fn set_emit_flags(&mut self, node: NodeId, flags: crate::emitflags::EmitFlags) {
        self.emit_flags.insert(node, flags);
    }

    /// Returns a shared reference to the owned arena.
    ///
    /// Side effects: none (pure).
    pub fn arena(&self) -> &NodeArena {
        &self.arena
    }

    /// Returns a mutable reference to the owned arena.
    ///
    /// Side effects: allows mutation of the arena.
    pub fn arena_mut(&mut self) -> &mut NodeArena {
        &mut self.arena
    }

    /// Returns an emit-aware node factory borrowing this context.
    ///
    /// Side effects: the returned factory mutates this context's arena and
    /// side tables.
    pub fn factory(&mut self) -> NodeFactory<'_> {
        NodeFactory::new(self)
    }

    /// Reports whether `node` has an associated auto-generate entry.
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/emitcontext.go:EmitContext.HasAutoGenerateInfo
    pub fn has_auto_generate_info(&self, node: NodeId) -> bool {
        self.auto_generate.contains_key(&node)
    }

    /// Returns the auto-generate entry for `name`, if any.
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/emitcontext.go:EmitContext.GetAutoGenerateInfo
    pub fn get_auto_generate_info(&self, name: NodeId) -> Option<&AutoGenerateInfo> {
        self.auto_generate.get(&name)
    }

    /// Allocates the next auto-generate id.
    ///
    /// Side effects: increments the internal counter.
    // Go: internal/printer/factory.go:nextAutoGenerateId
    pub(crate) fn alloc_auto_generate_id(&mut self) -> AutoGenerateId {
        self.next_auto_generate_id += 1;
        AutoGenerateId(self.next_auto_generate_id)
    }

    /// Records `info` as the auto-generate entry for `name`.
    ///
    /// Side effects: inserts into the auto-generate table.
    pub(crate) fn set_auto_generate(&mut self, name: NodeId, info: AutoGenerateInfo) {
        self.auto_generate.insert(name, info);
    }

    /// Requests that `helper` (and, first, its dependencies) be emitted for the
    /// current source file. De-duplicates by helper identity. Panics on a scoped
    /// helper (the TS library has none).
    ///
    /// Side effects: appends to the requested-helpers list.
    // Go: internal/printer/emitcontext.go:EmitContext.RequestEmitHelper
    pub fn request_emit_helper(&mut self, helper: &'static EmitHelper) {
        assert!(!helper.scoped, "cannot request a scoped emit helper");
        for dependency in helper.dependencies {
            self.request_emit_helper(dependency);
        }
        if !self.requested_helpers.iter().any(|h| h.is(helper)) {
            self.requested_helpers.push(helper);
        }
    }

    /// Returns and clears the helpers requested since the last read.
    ///
    /// Side effects: drains the requested-helpers list.
    // Go: internal/printer/emitcontext.go:EmitContext.ReadEmitHelpers
    pub fn read_emit_helpers(&mut self) -> Vec<&'static EmitHelper> {
        std::mem::take(&mut self.requested_helpers)
    }

    /// Attaches one or more emit helpers to `node` (typically the source file),
    /// de-duplicating by helper identity. The printer emits these in the module
    /// prologue.
    ///
    /// Side effects: inserts into the per-node helper table.
    // Go: internal/printer/emitcontext.go:EmitContext.AddEmitHelper
    pub fn add_emit_helper(&mut self, node: NodeId, helper: &'static EmitHelper) {
        let helpers = self.node_helpers.entry(node).or_default();
        if !helpers.iter().any(|h| h.is(helper)) {
            helpers.push(helper);
        }
    }

    /// Returns the emit helpers attached to `node` (empty when none).
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/emitcontext.go:EmitContext.GetEmitHelpers
    pub fn get_emit_helpers(&self, node: NodeId) -> &[&'static EmitHelper] {
        self.node_helpers
            .get(&node)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Starts a new variable environment used to collect hoisted `var`
    /// declarations (e.g. temporaries created during a down-level lowering).
    ///
    /// # Examples
    /// ```
    /// use tsgo_printer::emitcontext::EmitContext;
    /// let mut ec = EmitContext::new();
    /// ec.start_variable_environment();
    /// assert!(ec.end_variable_environment().is_empty());
    /// ```
    ///
    /// Side effects: pushes a scope onto the variable-environment stack.
    // Go: internal/printer/emitcontext.go:EmitContext.StartVariableEnvironment
    pub fn start_variable_environment(&mut self) {
        self.var_environments.push(Vec::new());
    }

    /// Hoists a `var <name>;` declaration into the current variable environment.
    ///
    /// Side effects: appends a fresh `VariableDeclaration` node to the arena and
    /// records it in the current scope. No-op if no environment is active.
    // Go: internal/printer/emitcontext.go:EmitContext.AddVariableDeclaration
    pub fn add_variable_declaration(&mut self, name: NodeId) {
        let declaration = self.arena.new_variable_declaration(name, None, None, None);
        if let Some(scope) = self.var_environments.last_mut() {
            scope.push(declaration);
        }
    }

    /// Ends the current variable environment, returning the statements to prepend
    /// at the start of the scope: a single `var <decls>;` statement when any
    /// declarations were hoisted, otherwise an empty list.
    ///
    /// Side effects: pops the variable-environment stack; may append a
    /// declaration-list / statement node to the arena.
    // Go: internal/printer/emitcontext.go:EmitContext.EndVariableEnvironment
    pub fn end_variable_environment(&mut self) -> Vec<NodeId> {
        let scope = self.var_environments.pop().unwrap_or_default();
        if scope.is_empty() {
            return Vec::new();
        }
        let declaration_list = self
            .arena
            .new_variable_declaration_list(NodeList::new(scope));
        let statement = self.arena.new_variable_statement(None, declaration_list);
        vec![statement]
    }
}

#[cfg(test)]
#[path = "emitcontext_test.rs"]
mod tests;
