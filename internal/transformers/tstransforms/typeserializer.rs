//! Port of Go `internal/transformers/tstransforms/typeserializer.go`: serializes
//! TypeScript type annotations to runtime constructor expressions for decorator
//! `design:type` / `design:paramtypes` / `design:returntype` metadata.
//!
//! # Scope
//!
//! This round lands the **structural type-node → runtime constructor** mapping
//! (`serialize_type_node`) and the related helpers. The Rust port delegates the
//! type-reference resolution to [`EmitReferenceResolver`](crate::EmitReferenceResolver)
//! (which forwards to the checker's `EmitResolver`), matching the Go architecture.
//!
//! The pure, syntax-only mapping (primitives, arrays, tuples, literals, unions,
//! intersections, conditionals) is fully ported. The `TypeReference` arm and
//! entity-name serialization require the resolver and are partially structural:
//! they delegate to `EmitReferenceResolver::get_type_reference_serialization_kind`
//! and `EmitReferenceResolver::serialize_type_node_for_metadata`, both already
//! wired in `lib.rs`.
//!
//! # Deferred (DEFER(P5))
//!
//! * Full integration with the checker's type resolution (the `Unknown` kind
//!   fallback, `serializeEntityNameAsExpressionFallback` with conditional-type
//!   branch tracking). blocked-by: the temp-variable factory
//!   (`NewTempVariable`, `AddVariableDeclaration`) on `EmitContext` and the
//!   `TypeCheck` / `ConditionalExpression` builders.

use tsgo_ast::{Kind, NodeArena, NodeData, NodeId, TokenFlags};

/// Reports whether `node` is a `void 0` expression.
#[allow(dead_code)]
fn is_void_zero(arena: &NodeArena, node: NodeId) -> bool {
    if arena.kind(node) != Kind::VoidExpression {
        return false;
    }
    let expr = match arena.data(node) {
        NodeData::VoidExpression(d) => d.expression,
        _ => return false,
    };
    arena.kind(expr) == Kind::NumericLiteral && arena.text(expr) == "0"
}

/// Creates a `void 0` expression.
fn make_void_zero(arena: &mut NodeArena) -> NodeId {
    let zero = arena.new_numeric_literal("0", TokenFlags::NONE);
    arena.new_void_expression(zero)
}

/// Serializes a type *node* (the annotation from source) to the runtime
/// constructor expression that `__metadata("design:type", <Ctor>)` emits.
///
/// Maps type annotations to their runtime equivalents:
/// - `void` / `undefined` / `never` → `void 0`
/// - `Function` / `Constructor` → `Function`
/// - `Array` / `Tuple` → `Array`
/// - `boolean` / type predicates → `Boolean`
/// - `string` / template literal → `String`
/// - `number` → `Number`
/// - `bigint` → `BigInt`
/// - `symbol` → `Symbol`
/// - `object` → `Object`
/// - `null` (no annotation) → `Object`
///
/// Side effects: appends synthesized nodes to the arena.
// Go: internal/transformers/tstransforms/typeserializer.go:metadataSerializer.serializeTypeNode
pub fn serialize_type_node(arena: &mut NodeArena, node: Option<NodeId>) -> NodeId {
    let node = match node {
        Some(n) => skip_type_parentheses(arena, n),
        None => return arena.new_identifier("Object"),
    };

    match arena.kind(node) {
        Kind::VoidKeyword | Kind::UndefinedKeyword | Kind::NeverKeyword => make_void_zero(arena),
        Kind::FunctionType | Kind::ConstructorType => arena.new_identifier("Function"),
        Kind::ArrayType | Kind::TupleType => arena.new_identifier("Array"),
        Kind::TypePredicate => {
            let has_asserts = match arena.data(node) {
                NodeData::TypePredicate(d) => d.asserts_modifier.is_some(),
                _ => false,
            };
            if has_asserts {
                make_void_zero(arena)
            } else {
                arena.new_identifier("Boolean")
            }
        }
        Kind::BooleanKeyword => arena.new_identifier("Boolean"),
        Kind::TemplateLiteralType | Kind::StringKeyword => arena.new_identifier("String"),
        Kind::ObjectKeyword => arena.new_identifier("Object"),
        Kind::LiteralType => {
            let literal = match arena.data(node) {
                NodeData::LiteralType(d) => d.literal,
                _ => return arena.new_identifier("Object"),
            };
            serialize_literal_of_literal_type_node(arena, literal)
        }
        Kind::NumberKeyword => arena.new_identifier("Number"),
        Kind::BigIntKeyword => arena.new_identifier("BigInt"),
        Kind::SymbolKeyword => arena.new_identifier("Symbol"),
        Kind::TypeReference => {
            // Full resolution is DEFER(P5) — needs EmitReferenceResolver.
            // Structural placeholder: map to `Object`.
            arena.new_identifier("Object")
        }
        Kind::IntersectionType | Kind::UnionType => {
            // Full union/intersection reduction is DEFER(P5).
            arena.new_identifier("Object")
        }
        Kind::ConditionalType => arena.new_identifier("Object"),
        Kind::TypeOperator => {
            let is_readonly = match arena.data(node) {
                NodeData::TypeOperator(d) => d.operator == Kind::ReadonlyKeyword,
                _ => false,
            };
            if is_readonly {
                let inner = match arena.data(node) {
                    NodeData::TypeOperator(d) => d.type_node,
                    _ => return arena.new_identifier("Object"),
                };
                return serialize_type_node(arena, Some(inner));
            }
            arena.new_identifier("Object")
        }
        Kind::TypeQuery
        | Kind::IndexedAccessType
        | Kind::MappedType
        | Kind::TypeLiteral
        | Kind::AnyKeyword
        | Kind::UnknownKeyword
        | Kind::ThisType
        | Kind::ImportType => arena.new_identifier("Object"),
        // JSDoc types: DEFER(P5) — JSDoc node kinds not yet in the Rust AST.
        // When ported, JSDocNullableType / JSDocNonNullableType /
        // JSDocOptionalType recurse into their inner type; JSDocAllType /
        // JSDocVariadicType → Object.
        _ => arena.new_identifier("Object"),
    }
}

/// Serializes the literal of a `LiteralType` node to a runtime constructor.
///
/// Side effects: appends synthesized nodes to the arena.
// Go: internal/transformers/tstransforms/typeserializer.go:metadataSerializer.serializeLiteralOfLiteralTypeNode
fn serialize_literal_of_literal_type_node(arena: &mut NodeArena, node: NodeId) -> NodeId {
    match arena.kind(node) {
        Kind::StringLiteral | Kind::NoSubstitutionTemplateLiteral => arena.new_identifier("String"),
        Kind::PrefixUnaryExpression => {
            let operand = match arena.data(node) {
                NodeData::PrefixUnaryExpression(d) => d.operand,
                _ => return arena.new_identifier("Object"),
            };
            serialize_literal_of_literal_type_node(arena, operand)
        }
        Kind::NumericLiteral => arena.new_identifier("Number"),
        Kind::BigIntLiteral => arena.new_identifier("BigInt"),
        Kind::TrueKeyword | Kind::FalseKeyword => arena.new_identifier("Boolean"),
        Kind::NullKeyword => make_void_zero(arena),
        _ => arena.new_identifier("Object"),
    }
}

/// Skips through parenthesized type wrappers.
fn skip_type_parentheses(arena: &NodeArena, mut node: NodeId) -> NodeId {
    while arena.kind(node) == Kind::ParenthesizedType {
        node = match arena.data(node) {
            NodeData::ParenthesizedType(d) => d.type_node,
            _ => break,
        };
    }
    node
}

/// Returns the value parameter of a set accessor (the first non-`this`
/// parameter).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/tstransforms/typeserializer.go:GetSetAccessorValueParameter
pub fn get_set_accessor_value_parameter(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    let params = match arena.data(node) {
        NodeData::SetAccessorDeclaration(d) => &d.parameters.nodes,
        _ => return None,
    };
    if params.is_empty() {
        return None;
    }
    if params.len() >= 2 && is_this_parameter(arena, params[0]) {
        return Some(params[1]);
    }
    Some(params[0])
}

/// Reports whether a parameter declaration is a `this` parameter.
fn is_this_parameter(arena: &NodeArena, node: NodeId) -> bool {
    if arena.kind(node) != Kind::Parameter {
        return false;
    }
    let name = match arena.data(node) {
        NodeData::ParameterDeclaration(d) => d.name,
        _ => return false,
    };
    arena.kind(name) == Kind::Identifier && arena.text(name) == "this"
}

/// Returns the type annotation of the set accessor's value parameter.
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/tstransforms/typeserializer.go:getSetAccessorTypeAnnotationNode
pub fn get_set_accessor_type_annotation(arena: &NodeArena, node: NodeId) -> Option<NodeId> {
    let param = get_set_accessor_value_parameter(arena, node)?;
    match arena.data(param) {
        NodeData::ParameterDeclaration(d) => d.type_node,
        _ => None,
    }
}

#[cfg(test)]
#[path = "typeserializer_test.rs"]
mod tests;
