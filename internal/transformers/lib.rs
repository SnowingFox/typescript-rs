//! Port of Go package `internal/transformers`.
//!
//! The transform pipeline lowers a checked TypeScript AST toward emittable
//! JavaScript: type erasure, downleveling (ES decorators, class fields, async),
//! module transforms, JSX, and `.d.ts` declaration emit. Each Go subpackage
//! (`tstransforms`, `estransforms`, `moduletransforms`, `jsxtransforms`,
//! `declarations`) is ported as a submodule of this crate, with `.rs` files
//! living next to their `.go` counterparts.
//!
//! Ported incrementally under strict TDD (see `docs/rust-rewrite/references/tdd.md`
//! and the `/tdd` skill). Round 6a establishes the shared transform
//! infrastructure (the `Transformer` driver, chaining, modifier visiting, and
//! shared utilities); later rounds fill in the individual transforms.

use std::cell::RefCell;
use std::rc::Rc;
use tsgo_ast::{NodeId, SymbolId};
use tsgo_checker::{
    BoundProgram, Checker, EmitResolver, SerializedTypeNode, TypeReferenceSerializationKind,
};
use tsgo_printer::EmitContext;

pub mod chain;
pub mod destructuring;
pub mod estransforms;
pub mod jsxtransforms;
pub mod modifiervisitor;
pub mod moduletransforms;
pub mod transformer;
pub mod tstransforms;
pub mod utilities;

pub use chain::{chain, TransformOptions, TransformerFactory};
pub use modifiervisitor::extract_modifiers;
pub use transformer::{new_transformer, Transformer, VisitFn};

/// A shared, mutable [`EmitContext`] handle.
///
/// Go threads a single `*printer.EmitContext` pointer through every transformer
/// in a pipeline; the Rust port shares it as `Rc<RefCell<EmitContext>>` so the
/// chained transformers all append to one arena (PORTING.md §3).
///
/// Side effects: none (a type alias).
pub type SharedEmitContext = Rc<RefCell<EmitContext>>;

/// A scope-correct reference query the import-elision transform consults to drop
/// unused imports, the Rust adaptation of Go's `opt.EmitResolver` handle.
///
/// It bundles the checker's [`EmitResolver`] with the [`BoundProgram`] it
/// queries (Go threads the program implicitly through the resolver's checker
/// back-pointer; this port passes the program explicitly per the crate's
/// ownership model). [`is_referenced`](Self::is_referenced) is the real,
/// scope-aware replacement for a textual name-match: a use shadowed by an inner
/// binding of the same name is correctly *not* counted as a reference to an
/// outer import.
///
/// It is threaded as an *additive* parameter to
/// [`new_import_elision_transformer`](tstransforms::importelision::new_import_elision_transformer)
/// rather than as a [`TransformOptions`] field, because the compiler crate
/// constructs `TransformOptions` with an exhaustive struct literal that adding a
/// field would break (and the compiler crate is out of this port's edit scope).
///
/// Some queries (e.g.
/// [`get_type_reference_serialization_kind`](Self::get_type_reference_serialization_kind))
/// need a mutable [`Checker`] to build declared types; the resolver owns one
/// (built from the same source as `program`) behind an
/// `Rc<RefCell<..>>` and borrows it mutably internally, so the passthrough
/// methods keep their `&self`, read-only-looking surface. Go threads the program
/// implicitly through the checker's emit-resolver back-pointer; this port bundles
/// the checker here instead.
///
/// # Examples
/// ```
/// use tsgo_transformers::EmitReferenceResolver;
/// # fn demo(r: &EmitReferenceResolver, decl: tsgo_ast::NodeId) -> bool {
/// r.is_referenced(decl)
/// # }
/// ```
///
/// Side effects: none (a read-only view over a bound program).
#[derive(Clone)]
pub struct EmitReferenceResolver {
    program: Rc<dyn BoundProgram>,
    resolver: EmitResolver,
    /// A checker over `program`, needed by the type-backed queries that build
    /// declared types (Go's emit resolver reaches the checker through a
    /// back-pointer). Shared (`Rc`) so [`Clone`] copies share one cache.
    checker: Rc<RefCell<Checker>>,
}

impl EmitReferenceResolver {
    /// Bundles `resolver` with the `program` it queries.
    ///
    /// `program`'s node arena must share node ids with the arena the transform
    /// reads original (pre-transform) declaration nodes from, so a declaration
    /// node id from the transform resolves to the same syntactic node in the
    /// bound program.
    ///
    /// # Examples
    /// ```
    /// use tsgo_transformers::EmitReferenceResolver;
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// use std::rc::Rc;
    /// # fn demo(program: Rc<dyn BoundProgram>, resolver: EmitResolver) -> EmitReferenceResolver {
    /// EmitReferenceResolver::new(program, resolver)
    /// # }
    /// ```
    ///
    /// Side effects: none (stores the handles; builds an empty [`Checker`]).
    pub fn new(program: Rc<dyn BoundProgram>, resolver: EmitResolver) -> EmitReferenceResolver {
        EmitReferenceResolver {
            program,
            resolver,
            checker: Rc::new(RefCell::new(Checker::new())),
        }
    }

    /// Reports whether the declaration `node` (an import clause / namespace
    /// import / import specifier) introduces a binding referenced anywhere in
    /// the file by a value-position use that resolves to it.
    ///
    /// Delegates to [`EmitResolver::is_referenced`] against the bound program
    /// (Go's `emitResolver.IsReferencedAliasDeclaration`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_transformers::EmitReferenceResolver;
    /// # fn demo(r: &EmitReferenceResolver, decl: tsgo_ast::NodeId) -> bool {
    /// r.is_referenced(decl)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/checker/checker.go:Checker.isReferenced (via EmitResolver.IsReferencedAliasDeclaration)
    pub fn is_referenced(&self, node: NodeId) -> bool {
        self.resolver.is_referenced(self.program.as_ref(), node)
    }

    /// Resolves an identifier *use* (`node`, in value position) to the
    /// declaration symbol it references, walking the scope chain outward so the
    /// innermost binding of the name wins (shadowing).
    ///
    /// The CommonJS module transform consults this to rewrite a use of an
    /// imported binding into a qualified member access on the require-alias
    /// (Go's `visitExpressionIdentifier` -> `GetReferencedImportDeclaration`,
    /// which is itself resolved over the symbol the use refers to). A use
    /// shadowed by a local of the same name resolves to the local, so it is
    /// correctly *not* rewritten. Delegates to
    /// [`EmitResolver::resolve_reference`] against the bound program.
    ///
    /// # Examples
    /// ```
    /// use tsgo_transformers::EmitReferenceResolver;
    /// # fn demo(r: &EmitReferenceResolver, use_node: tsgo_ast::NodeId) -> Option<tsgo_ast::SymbolId> {
    /// r.resolve_reference(use_node)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/checker/checker.go:Checker.resolveName / getResolvedSymbol
    pub fn resolve_reference(&self, node: NodeId) -> Option<SymbolId> {
        self.resolver.resolve_reference(self.program.as_ref(), node)
    }

    /// Returns the symbol declared by `node` (e.g. an import specifier, import
    /// clause, or namespace import), or `None` if the node binds no symbol.
    ///
    /// The CommonJS transform uses this to map each collected import binding to
    /// its declaration symbol, then matches a use's
    /// [`resolve_reference`](Self::resolve_reference) result against that symbol
    /// (Go's `GetReferencedImportDeclaration` returns the declaration node; the
    /// port compares declaration symbols instead).
    ///
    /// # Examples
    /// ```
    /// use tsgo_transformers::EmitReferenceResolver;
    /// # fn demo(r: &EmitReferenceResolver, decl: tsgo_ast::NodeId) -> Option<tsgo_ast::SymbolId> {
    /// r.symbol_of_declaration(decl)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/checker/checker.go:Checker.getSymbolOfDeclaration
    pub fn symbol_of_declaration(&self, node: NodeId) -> Option<SymbolId> {
        self.program.symbol_of_node(node)
    }

    /// Returns the *export container* a value-position identifier `node`
    /// resolves to: the `SourceFile` node when the use refers to a top-level
    /// *exported variable* of the current module (which the CommonJS transform
    /// rewrites into an `exports.<name>` access), else `None`.
    ///
    /// The CommonJS module transform consults this to rewrite a use of a local
    /// export into a qualified `exports.<name>` access (Go's
    /// `visitExpressionIdentifier` -> `GetReferencedExportContainer`). A use of
    /// a non-exported local, or of an inner binding that shadows an export,
    /// resolves to a non-exported symbol and yields `None`; an exported
    /// function/class (a non-variable local) is referenced unqualified and so
    /// also yields `None` when `prefix_locals` is false. Delegates to
    /// [`EmitResolver::get_referenced_export_container`] against the bound
    /// program.
    ///
    /// # Examples
    /// ```
    /// use tsgo_transformers::EmitReferenceResolver;
    /// # fn demo(r: &EmitReferenceResolver, use_node: tsgo_ast::NodeId) -> Option<tsgo_ast::NodeId> {
    /// r.get_referenced_export_container(use_node, false)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/binder/referenceresolver.go:referenceResolver.GetReferencedExportContainer
    pub fn get_referenced_export_container(
        &self,
        node: NodeId,
        prefix_locals: bool,
    ) -> Option<NodeId> {
        self.resolver
            .get_referenced_export_container(self.program.as_ref(), node, prefix_locals)
    }

    /// Reports whether the alias declaration `node` (e.g. an `export { x }`
    /// specifier) aliases something that is, transitively, a *value* — the query
    /// the export-side elision asks to keep value re-exports while dropping
    /// type-only ones.
    ///
    /// Delegates to [`EmitResolver::is_value_alias_declaration`] against the
    /// bound program (Go's `emitResolver.IsValueAliasDeclaration`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_transformers::EmitReferenceResolver;
    /// # fn demo(r: &EmitReferenceResolver, spec: tsgo_ast::NodeId) -> bool {
    /// r.is_value_alias_declaration(spec)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/transformers/tstransforms/importelision.go:ImportElisionTransformer.isValueAliasDeclaration
    pub fn is_value_alias_declaration(&self, node: NodeId) -> bool {
        self.resolver
            .is_value_alias_declaration(self.program.as_ref(), node)
    }

    /// Reports whether the alias declaration `node` (e.g. `import x =
    /// require("m")`) is *referenced* and so must be kept by emit.
    ///
    /// Delegates to [`EmitResolver::is_referenced_alias_declaration`] against the
    /// bound program (Go's `emitResolver.IsReferencedAliasDeclaration`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_transformers::EmitReferenceResolver;
    /// # fn demo(r: &EmitReferenceResolver, decl: tsgo_ast::NodeId) -> bool {
    /// r.is_referenced_alias_declaration(decl)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/transformers/tstransforms/importelision.go:ImportElisionTransformer.isReferencedAliasDeclaration
    pub fn is_referenced_alias_declaration(&self, node: NodeId) -> bool {
        self.resolver
            .is_referenced_alias_declaration(self.program.as_ref(), node)
    }

    /// Serializes the type-annotation node `type_node` to the runtime-constructor
    /// descriptor the legacy-decorator transform turns into the second argument
    /// of `__metadata("design:type", <Ctor>)` (e.g. `: number` →
    /// [`SerializedTypeNode::Number`]).
    ///
    /// Delegates to [`EmitResolver::serialize_type_node_for_metadata`] against
    /// the bound program (Go's
    /// `tstransforms/typeserializer.go:metadataSerializer.serializeTypeNode`,
    /// driven by the metadata transform). `type_node`'s id must be the original
    /// (pre-transform) annotation node so it resolves to the same syntactic node
    /// in the bound program.
    ///
    /// # Examples
    /// ```
    /// use tsgo_transformers::EmitReferenceResolver;
    /// use tsgo_checker::SerializedTypeNode;
    /// # fn demo(r: &EmitReferenceResolver, ty: tsgo_ast::NodeId) -> SerializedTypeNode {
    /// r.serialize_type_node_for_metadata(ty)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure read over the bound program).
    // Go: internal/transformers/tstransforms/typeserializer.go:metadataSerializer.serializeTypeNode
    pub fn serialize_type_node_for_metadata(&self, type_node: NodeId) -> SerializedTypeNode {
        self.resolver
            .serialize_type_node_for_metadata(self.program.as_ref(), type_node)
    }

    /// Classifies a `TypeReference` type node for legacy-decorator `design:type`
    /// emit: whether the referenced entity is reachable at runtime as a value
    /// (`: SomeClass` → a runtime constructor) or only as a type
    /// (`: SomeInterface` → the `Object` fallback).
    ///
    /// Delegates to [`EmitResolver::get_type_reference_serialization_kind`]
    /// against the bound program and the owned [`Checker`] (Go's
    /// `serializeTypeReferenceNode` -> `resolver.GetTypeReferenceSerializationKind`).
    /// The checker is borrowed mutably internally — building the referenced
    /// symbol's declared type may populate caches — so this stays a `&self`
    /// method. `type_node`'s id must be the original (pre-transform) annotation
    /// node so it resolves to the same syntactic node in the bound program.
    ///
    /// # Examples
    /// ```
    /// use tsgo_transformers::EmitReferenceResolver;
    /// use tsgo_checker::TypeReferenceSerializationKind;
    /// # fn demo(r: &EmitReferenceResolver, ty: tsgo_ast::NodeId) -> TypeReferenceSerializationKind {
    /// r.get_type_reference_serialization_kind(ty)
    /// # }
    /// ```
    ///
    /// Side effects: may build and cache declared types in the owned checker.
    // Go: internal/checker/emitresolver.go:EmitResolver.GetTypeReferenceSerializationKind
    pub fn get_type_reference_serialization_kind(
        &self,
        type_node: NodeId,
    ) -> TypeReferenceSerializationKind {
        let mut checker = self.checker.borrow_mut();
        self.resolver.get_type_reference_serialization_kind(
            &mut checker,
            self.program.as_ref(),
            type_node,
        )
    }
}

#[cfg(test)]
#[path = "test_support.rs"]
pub(crate) mod test_support;
