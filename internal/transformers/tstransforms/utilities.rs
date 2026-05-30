//! Port of Go `internal/transformers/tstransforms/utilities.go`: shared helpers
//! for the TypeScript transforms.

use tsgo_ast::{Kind, NodeId, TokenFlags};
use tsgo_jsnum::Number;
use tsgo_printer::NodeFactory;

/// A constant enum-member value to be materialized as an expression, mirroring
/// the Go `any` argument (`string` or `jsnum.Number`) of `constantExpression`.
///
/// Side effects: none (a value type).
// Consumed by the enum/const-enum runtime lowering in `runtimesyntax` (DEFER(P5)),
// so it is only exercised by unit tests this round.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ConstantValue {
    /// A string constant.
    Str(String),
    /// A numeric constant.
    Num(Number),
}

/// Builds an expression node materializing the constant `value`, using the emit
/// factory so the node is synthesized.
///
/// Mirrors Go's `constantExpression`: strings become string literals; finite
/// non-negative numbers become numeric literals; `NaN`/`Infinity` become the
/// matching global identifier; negative values (including `-Infinity`) are
/// wrapped in a `-` prefix unary over their absolute form.
///
/// Side effects: appends synthesized nodes to the factory's arena.
// Go: internal/transformers/tstransforms/utilities.go:constantExpression
// Consumed by `runtimesyntax` enum lowering (DEFER(P5)); test-only this round.
#[allow(dead_code)]
pub(crate) fn constant_expression(value: &ConstantValue, factory: &mut NodeFactory) -> NodeId {
    match value {
        ConstantValue::Str(s) => factory.new_string_literal(s, TokenFlags::NONE),
        ConstantValue::Num(n) => {
            let n = *n;
            let zero = Number::from(0.0);
            if n.is_inf() {
                let infinity = factory.new_identifier("Infinity");
                if n > zero {
                    return infinity;
                }
                return factory.new_prefix_unary_expression(Kind::MinusToken, infinity);
            }
            if n.is_nan() {
                return factory.new_identifier("NaN");
            }
            if n < zero {
                let abs = ConstantValue::Num(Number::from(-f64::from(n)));
                let inner = constant_expression(&abs, factory);
                return factory.new_prefix_unary_expression(Kind::MinusToken, inner);
            }
            factory.new_numeric_literal(&n.to_string(), TokenFlags::NONE)
        }
    }
}

#[cfg(test)]
#[path = "utilities_test.rs"]
mod tests;
