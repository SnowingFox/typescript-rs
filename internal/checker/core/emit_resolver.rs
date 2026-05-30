//! Emit resolver: the query surface the declaration/JS transformers (Phase 5)
//! ask the checker, ported from `emitresolver.go`.
//!
//! Go's `EmitResolver` holds a back-reference to the checker plus per-node
//! caches and a mutex. Following this crate's ownership model (no back-pointers,
//! single-threaded), [`EmitResolver`] is a lightweight handle whose query
//! methods take the [`BoundProgram`] (and, for type-backed queries, the
//! [`Checker`]) explicitly. [`Checker::get_emit_resolver`] caches one behind a
//! `OnceCell`, mirroring Go's `GetEmitResolver`.
//!
//! 4k ports the AST-structural core (declaration visibility) plus the
//! type-backed serialization entry points that reuse the 4j node builder. The
//! alias/reference/host-dependent queries are deferred (see the per-method
//! `// blocked-by:` notes).

use tsgo_ast::{ModifierFlags, NodeData, NodeId};

use super::declared_types::get_type_of_symbol;
use super::nodebuilder::type_to_string;
use super::program::BoundProgram;
use super::symbols_query::get_symbol_of_declaration;
use super::Checker;

/// The checker's emit-time query surface (Go's `EmitResolver`).
///
/// A zero-sized handle in 4k: visibility is computed structurally and the
/// type-backed queries take the [`Checker`] explicitly, so no
/// checker back-reference or interior caches are needed yet.
///
/// # Examples
/// ```
/// use tsgo_checker::Checker;
/// let c = Checker::new();
/// let _resolver = c.get_emit_resolver();
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/emitresolver.go:EmitResolver
#[derive(Clone, Copy, Debug, Default)]
pub struct EmitResolver;

impl EmitResolver {
    /// Reports whether `node`'s declaration is visible to declaration emit
    /// (Go's `IsDeclarationVisible`).
    ///
    /// 4k implements the module/declaration-emit rule: a top-level declaration
    /// is visible iff it carries the `export` modifier.
    ///
    /// DEFER(phase-4-checker-post): the global-script-file case (a non-exported
    /// declaration is visible in a non-module script), ambient modules, and
    /// member/nested visibility (`isDeclarationVisible(parent)` recursion).
    /// blocked-by: external-module detection + `compiler.Program` (P6).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// r.is_declaration_visible(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/emitresolver.go:EmitResolver.IsDeclarationVisible(104)
    pub fn is_declaration_visible(&self, program: &dyn BoundProgram, node: NodeId) -> bool {
        modifier_flags(program.arena(), node).contains(ModifierFlags::EXPORT)
    }

    /// Serializes the type of declaration `node` to its printed form (Go's
    /// `SerializeTypeOfDeclaration`), reusing the 4j node builder.
    ///
    /// Used by the declaration transformer to emit explicit type annotations.
    ///
    /// DEFER(phase-4-checker-post): emit the serialized type as a *node*
    /// (`createTypeOfDeclaration`) rather than a string, plus the
    /// widening/freshening and accessibility tracking the transformer needs.
    /// blocked-by: the full node builder + `SymbolTracker` and `compiler.Program`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, c: &mut Checker, p: &P, n: tsgo_ast::NodeId) -> String {
    /// r.serialize_type_of_declaration(c, p, n)
    /// # }
    /// ```
    ///
    /// Side effects: may resolve and cache the declaration's type.
    // Go: internal/checker/emitresolver.go:EmitResolver.SerializeTypeOfDeclaration
    pub fn serialize_type_of_declaration(
        &self,
        checker: &mut Checker,
        program: &dyn BoundProgram,
        node: NodeId,
    ) -> String {
        let ty = match get_symbol_of_declaration(program, node) {
            Some(symbol) => get_type_of_symbol(checker, program, symbol, None),
            None => checker.error_type(),
        };
        type_to_string(checker, program, ty)
    }

    /// Reports whether `node` is the implementation of a set of overloads
    /// (Go's `IsImplementationOfOverload`): a body-bearing function whose symbol
    /// has more than one declaration.
    ///
    /// DEFER(phase-4-checker-post): methods/constructors and the single-signature
    /// case where the lone signature comes from a different declaration; get/set
    /// accessors (never overload implementations).
    /// blocked-by: `getSignaturesOfSymbol` over all declarations.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, EmitResolver};
    /// # fn demo<P: BoundProgram>(r: &EmitResolver, p: &P, n: tsgo_ast::NodeId) -> bool {
    /// r.is_implementation_of_overload(p, n)
    /// # }
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/emitresolver.go:EmitResolver.IsImplementationOfOverload(458)
    pub fn is_implementation_of_overload(&self, program: &dyn BoundProgram, node: NodeId) -> bool {
        let has_body = matches!(
            program.arena().data(node),
            NodeData::FunctionDeclaration(d) if d.body.is_some()
        );
        if !has_body {
            return false;
        }
        match get_symbol_of_declaration(program, node) {
            Some(symbol) => program.symbol(symbol).declarations.len() > 1,
            None => false,
        }
    }
}

// Returns the aggregated modifier flags of `node`, if it bears modifiers.
// Go: internal/ast/ast.go:Node.ModifierFlags
fn modifier_flags(arena: &tsgo_ast::NodeArena, node: NodeId) -> ModifierFlags {
    let modifiers = match arena.data(node) {
        NodeData::FunctionDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ClassDeclaration(d) => d.modifiers.as_ref(),
        NodeData::InterfaceDeclaration(d) => d.modifiers.as_ref(),
        NodeData::TypeAliasDeclaration(d) => d.modifiers.as_ref(),
        NodeData::EnumDeclaration(d) => d.modifiers.as_ref(),
        NodeData::ModuleDeclaration(d) => d.modifiers.as_ref(),
        NodeData::VariableStatement(d) => d.modifiers.as_ref(),
        _ => None,
    };
    modifiers
        .map(|m| m.modifier_flags)
        .unwrap_or(ModifierFlags::empty())
}

#[cfg(test)]
#[path = "emit_resolver_test.rs"]
mod tests;
