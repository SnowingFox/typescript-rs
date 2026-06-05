//! Port of Go `internal/transformers/estransforms/utilities.go`: shared helpers
//! used by multiple ES-transform stages (async, for-await, class fields, etc.).
//!
//! # Scope
//!
//! This round lands the pure, dependency-light helpers:
//!
//! * [`create_not_null_condition`] — builds `left !== null && right !== void 0`
//!   (or the inverted `=== null || === void 0` form), consumed by the optional
//!   chaining and nullish coalescing transforms.
//! * [`is_update_expression`] — tests whether a prefix/postfix unary expression
//!   is `++` or `--`, consumed by the super-access tracker.
//! * [`assignment_target_contains_super_property`] — recursively checks whether
//!   the LHS of an assignment contains `super.x` or `super[x]`.
//!
//! # Deferred (DEFER(P5))
//!
//! * [`SuperAccessState`] full rewriting visitor — blocked-by: callback-based
//!   NodeVisitor, factory surface for arrow/variable/call builders.
//! * `convert_class_declaration_to_class_expression` — blocked-by:
//!   `NodeFactory::new_class_expression`.
//! * `create_accessor_property_backing_field` — blocked-by:
//!   `NodeFactory::new_generated_private_name_for_node_ex`.

use tsgo_ast::utilities::is_assignment_operator;
use tsgo_ast::{Kind, NodeArena, NodeData, NodeId, TokenFlags};
use tsgo_printer::EmitContext;

/// Builds a not-null/not-undefined guard:
///
/// ```text
/// left !== null && right !== void 0      (invert = false)
/// left === null || right === void 0      (invert = true)
/// ```
///
/// Consumed by the optional-chaining and nullish-coalescing transforms.
///
/// Side effects: appends synthesized nodes to the arena.
// Go: internal/transformers/estransforms/utilities.go:createNotNullCondition
pub fn create_not_null_condition(
    ec: &mut EmitContext,
    left: NodeId,
    right: NodeId,
    invert: bool,
) -> NodeId {
    let (eq_token, logical_op) = if invert {
        (Kind::EqualsEqualsEqualsToken, Kind::BarBarToken)
    } else {
        (
            Kind::ExclamationEqualsEqualsToken,
            Kind::AmpersandAmpersandToken,
        )
    };

    let arena = ec.arena_mut();
    let null_kw = arena.new_keyword_expression(Kind::NullKeyword);
    let eq_tok_left = arena.new_token(eq_token);
    let left_cmp = arena.new_binary_expression(left, eq_tok_left, null_kw);

    let zero = arena.new_numeric_literal("0", TokenFlags::NONE);
    let void_zero = arena.new_void_expression(zero);
    let eq_tok_right = arena.new_token(eq_token);
    let right_cmp = arena.new_binary_expression(right, eq_tok_right, void_zero);

    let logical_tok = arena.new_token(logical_op);
    arena.new_binary_expression(left_cmp, logical_tok, right_cmp)
}

/// Reports whether a prefix/postfix unary expression is `++` or `--` (an
/// update expression, which may trigger a super-property assignment).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/estransforms/async.go:isUpdateExpression
pub fn is_update_expression(arena: &NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::PrefixUnaryExpression(d) => {
            d.operator == Kind::PlusPlusToken || d.operator == Kind::MinusMinusToken
        }
        NodeData::PostfixUnaryExpression(d) => {
            d.operator == Kind::PlusPlusToken || d.operator == Kind::MinusMinusToken
        }
        _ => false,
    }
}

/// Recursively checks whether the left-hand side of an assignment contains a
/// `super.x` or `super[x]` property access (which triggers super-property-
/// assignment tracking for async/for-await).
///
/// Side effects: none (reads the arena).
// Go: internal/transformers/estransforms/async.go:assignmentTargetContainsSuperProperty
pub fn assignment_target_contains_super_property(arena: &NodeArena, node: NodeId) -> bool {
    match arena.data(node) {
        NodeData::PropertyAccessExpression(d) => arena.kind(d.expression) == Kind::SuperKeyword,
        NodeData::ElementAccessExpression(d) => arena.kind(d.expression) == Kind::SuperKeyword,
        NodeData::ParenthesizedExpression(d) => {
            assignment_target_contains_super_property(arena, d.expression)
        }
        NodeData::ArrayLiteralExpression(d) => d
            .list
            .nodes
            .iter()
            .any(|&e| assignment_target_contains_super_property(arena, e)),
        NodeData::ObjectLiteralExpression(d) => {
            d.list.nodes.iter().any(|&prop| match arena.data(prop) {
                NodeData::PropertyAssignment(pa) => pa
                    .initializer
                    .is_some_and(|init| assignment_target_contains_super_property(arena, init)),
                NodeData::ShorthandPropertyAssignment(spa) => {
                    assignment_target_contains_super_property(arena, spa.name)
                }
                NodeData::SpreadAssignment(sa) => {
                    assignment_target_contains_super_property(arena, sa.expression)
                }
                _ => false,
            })
        }
        NodeData::SpreadElement(d) => {
            assignment_target_contains_super_property(arena, d.expression)
        }
        _ => false,
    }
}

/// Tracks super property/element accesses and super property assignments within
/// async function or async generator bodies. The tracking state is shared by
/// both `asyncTransformer` and `forawaitTransformer`.
///
/// The full rewriting visitor (`visitSuperAccessNode`, `substituteSuperAccessesInBody`,
/// `createSuperAccessVariableStatement`) is DEFER(P5) — blocked-by: callback-based
/// NodeVisitor, arrow/variable/call factory builders.
// Go: internal/transformers/estransforms/utilities.go:superAccessState
pub struct SuperAccessState {
    /// Keeps track of property names accessed on super (`super.x`).
    pub captured_super_properties: Vec<String>,
    /// Whether the async function contains an element access on super (`super[x]`).
    pub has_super_element_access: bool,
    /// Whether the async function contains a super property assignment.
    pub has_super_property_assignment: bool,
}

impl SuperAccessState {
    /// Creates a new, empty super-access state.
    pub fn new() -> Self {
        SuperAccessState {
            captured_super_properties: Vec::new(),
            has_super_element_access: false,
            has_super_property_assignment: false,
        }
    }

    /// Records a super property/element access or super property assignment.
    ///
    /// Side effects: mutates the tracking fields.
    // Go: internal/transformers/estransforms/utilities.go:superAccessState.trackSuperAccess
    pub fn track_super_access(&mut self, arena: &NodeArena, node: NodeId) {
        match arena.data(node) {
            NodeData::PropertyAccessExpression(d) => {
                if arena.kind(d.expression) == Kind::SuperKeyword {
                    self.captured_super_properties
                        .push(arena.text(d.name).to_string());
                }
            }
            NodeData::ElementAccessExpression(d) => {
                if arena.kind(d.expression) == Kind::SuperKeyword {
                    self.has_super_element_access = true;
                }
            }
            NodeData::BinaryExpression(d) => {
                if is_assignment_operator(arena.kind(d.operator_token))
                    && assignment_target_contains_super_property(arena, d.left)
                {
                    self.has_super_property_assignment = true;
                }
            }
            NodeData::PrefixUnaryExpression(d) => {
                if is_update_expression(arena, node)
                    && assignment_target_contains_super_property(arena, d.operand)
                {
                    self.has_super_property_assignment = true;
                }
            }
            NodeData::PostfixUnaryExpression(d) => {
                if is_update_expression(arena, node)
                    && assignment_target_contains_super_property(arena, d.operand)
                {
                    self.has_super_property_assignment = true;
                }
            }
            _ => {}
        }
    }
}

impl Default for SuperAccessState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "utilities_test.rs"]
mod tests;
