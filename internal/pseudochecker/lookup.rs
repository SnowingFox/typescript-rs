//! Port of Go `internal/pseudochecker/lookup.go`: the derivation entry points
//! that map AST shapes onto [`PseudoType`] skeletons.
//!
//! # Scope of this port
//!
//! `lookup.go` leans on a large slice of AST surface (declaration/accessor/
//! object-literal node data, `Symbol`, `FindAncestor`, `GetAllAccessorDeclarations`,
//! `IsPrimitiveLiteralValue`, `IsInConstContext`, contextual-typing helpers, ...)
//! that the current representative `tsgo_ast` port does not yet expose. Only the
//! expression core ([`PseudoChecker::get_type_of_expression`] / `type_from_expression`)
//! is ported here, and within it the branches that require the missing surface
//! are left as `DEFER` stubs. The declaration/accessor/signature entry points
//! (`GetTypeOfDeclaration`, `GetReturnTypeOfSignature`, `GetTypeOfAccessor`) and
//! the supporting free functions are deferred wholesale; see the package
//! `impl.md`/`tests.md` for the blocked-by inventory.

use crate::PseudoChecker;
use crate::PseudoType;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId};

impl PseudoChecker {
    /// Returns the pseudo-type of an expression node (the public entry point;
    /// thin wrapper over `type_from_expression`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::{NodeArena, TokenFlags};
    /// use tsgo_pseudochecker::{PseudoChecker, PseudoType};
    /// let mut arena = NodeArena::new();
    /// let lit = arena.new_string_literal("abc", TokenFlags::empty());
    /// let ch = PseudoChecker::new(false, false);
    /// // A bare string literal is ambiguous between `"abc"` and `string`.
    /// assert!(matches!(
    ///     ch.get_type_of_expression(&arena, lit),
    ///     PseudoType::MaybeConstLocation { .. }
    /// ));
    /// ```
    ///
    /// Side effects: none (reads `arena`; allocates the returned skeleton).
    // Go: internal/pseudochecker/lookup.go:GetTypeOfExpression
    pub fn get_type_of_expression(&self, arena: &NodeArena, node: NodeId) -> PseudoType {
        self.type_from_expression(arena, node)
    }

    /// Derives the pseudo-type of an expression, the pseudochecker's analogue of
    /// `checkExpression`.
    ///
    /// Side effects: none (reads `arena`; allocates the returned skeleton).
    // Go: internal/pseudochecker/lookup.go:typeFromExpression
    fn type_from_expression(&self, arena: &NodeArena, node: NodeId) -> PseudoType {
        match arena.kind(node) {
            Kind::OmittedExpression => PseudoType::Undefined,
            Kind::ParenthesizedExpression => {
                // Assertions are transformed away on reparse, so just unwrap.
                let NodeData::ParenthesizedExpression(d) = arena.data(node) else {
                    unreachable!(
                        "ParenthesizedExpression kind must carry ParenthesizedExpression data"
                    )
                };
                self.type_from_expression(arena, d.expression)
            }
            // The real checker uses symbol resolution to confirm this refers to
            // the global `undefined`; here we only have the syntactic name.
            Kind::Identifier if arena.text(node) == "undefined" => PseudoType::Undefined,
            Kind::NullKeyword => PseudoType::Null,
            // DEFER(phase-4): typeFromFunctionLikeExpression (signature + parameter clones). // blocked-by: tsgo_ast lacks function-like node data + FunctionLikeData/FullSignature
            Kind::ArrowFunction | Kind::FunctionExpression => todo!(),
            // DEFER(phase-4): typeFromTypeAssertion for `<T>x`. // blocked-by: tsgo_ast lacks TypeAssertion node data + is_const_type_reference
            Kind::TypeAssertionExpression => todo!(),
            // DEFER(phase-4): typeFromTypeAssertion for `x as T`. // blocked-by: tsgo_ast lacks AsExpression node data + is_const_type_reference
            Kind::AsExpression => todo!(),
            // DEFER(phase-4): typeFromPrimitiveLiteralPrefix (`-1`, `+1n`). // blocked-by: tsgo_ast lacks is_primitive_literal_value
            Kind::PrefixUnaryExpression => todo!(),
            // DEFER(phase-4): typeFromArrayLiteral. // blocked-by: tsgo_ast lacks is_in_const_context/find_ancestor + contextual-typing helpers
            Kind::ArrayLiteralExpression => todo!(),
            // DEFER(phase-4): typeFromObjectLiteral. // blocked-by: tsgo_ast lacks ObjectLiteral node data + Symbol/accessor helpers
            Kind::ObjectLiteralExpression => todo!(),
            // DEFER(phase-4): template-expression const-context typing. // blocked-by: tsgo_ast lacks is_in_const_context + TemplateExpression node data
            Kind::TemplateExpression => todo!(),
            Kind::NumericLiteral => PseudoType::maybe_const_location(
                node,
                PseudoType::numeric_literal(node),
                PseudoType::Number,
            ),
            Kind::NoSubstitutionTemplateLiteral => PseudoType::maybe_const_location(
                node,
                PseudoType::string_literal(node),
                PseudoType::String,
            ),
            Kind::StringLiteral => PseudoType::maybe_const_location(
                node,
                PseudoType::string_literal(node),
                PseudoType::String,
            ),
            Kind::BigIntLiteral => PseudoType::maybe_const_location(
                node,
                PseudoType::bigint_literal(node),
                PseudoType::BigInt,
            ),
            Kind::TrueKeyword => {
                PseudoType::maybe_const_location(node, PseudoType::True, PseudoType::Boolean)
            }
            Kind::FalseKeyword => {
                PseudoType::maybe_const_location(node, PseudoType::False, PseudoType::Boolean)
            }
            // Covers `KindClassExpression` and any other expression with no
            // syntactically mappable type (Go's fall-through `return inferred`).
            _ => PseudoType::inferred(node),
        }
    }
}

#[cfg(test)]
#[path = "lookup_test.rs"]
mod tests;
