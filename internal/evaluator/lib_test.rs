//! Behavior-level tests for `tsgo_evaluator`.
//!
//! Go's `internal/evaluator` has no `*_test.go`; its correctness is covered by
//! the checker conformance/enum suites (deferred to P10 parity). These tests
//! exercise the public API directly, with expected values taken from the Go
//! implementation's semantics and the TS language spec. Each case carries a
//! `// Go:` anchor into `evaluator.go`.

use super::*;

use tsgo_ast::{Kind, NodeArena, NodeFlags, NodeId, NodeList, TokenFlags};

/// A stub `evaluate_entity` that never folds (always returns `None`), matching
/// the conservative behavior used where the checker has no real entity to
/// resolve. Mirrors passing a no-op `evaluateEntity` in Go tests.
fn none_entity(_: &NodeArena, _: NodeId, _: Option<NodeId>) -> Result {
    new_result(EvalValue::None, false, false, false)
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (KindStringLiteral branch)
#[test]
fn eval_string_literal() {
    let mut arena = NodeArena::new();
    let lit = arena.new_string_literal("abc", TokenFlags::NONE);
    let eval = new_evaluator(none_entity, OuterExpressionKinds::NONE);
    let r = eval.evaluate(&arena, lit, None);
    assert_eq!(r.value, EvalValue::Str("abc".to_string()));
    assert!(r.is_syntactically_string);
}

/// Builds a numeric-literal node for `text`.
fn num(arena: &mut NodeArena, text: &str) -> NodeId {
    arena.new_numeric_literal(text, TokenFlags::NONE)
}

/// Builds a binary expression `left <op> right`.
fn bin(arena: &mut NodeArena, left: NodeId, op: Kind, right: NodeId) -> NodeId {
    let op_token = arena.new_token(op);
    arena.new_binary_expression(left, op_token, right)
}

/// Folds the numeric binary expression `lhs <op> rhs` with a `None` entity stub.
fn fold_num_bin(lhs: &str, op: Kind, rhs: &str) -> EvalValue {
    let mut arena = NodeArena::new();
    let l = num(&mut arena, lhs);
    let r = num(&mut arena, rhs);
    let expr = bin(&mut arena, l, op, r);
    let eval = new_evaluator(none_entity, OuterExpressionKinds::NONE);
    eval.evaluate(&arena, expr, None).value
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (KindBarToken -> jsnum.BitwiseOR)
#[test]
fn eval_binary_or() {
    assert_eq!(
        fold_num_bin("1", Kind::BarToken, "2"),
        EvalValue::Num(Number::from(3.0))
    );
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (KindAmpersandToken -> jsnum.BitwiseAND)
#[test]
fn eval_binary_and() {
    assert_eq!(
        fold_num_bin("6", Kind::AmpersandToken, "3"),
        EvalValue::Num(Number::from(2.0))
    );
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (KindCaretToken -> jsnum.BitwiseXOR)
#[test]
fn eval_binary_xor() {
    assert_eq!(
        fold_num_bin("5", Kind::CaretToken, "1"),
        EvalValue::Num(Number::from(4.0))
    );
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (KindLessThanLessThanToken -> jsnum.LeftShift)
#[test]
fn eval_shift_left() {
    assert_eq!(
        fold_num_bin("1", Kind::LessThanLessThanToken, "4"),
        EvalValue::Num(Number::from(16.0))
    );
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (KindGreaterThanGreaterThanToken -> jsnum.SignedRightShift)
#[test]
fn eval_shift_right() {
    // -8 >> 1, with -8 built as a real unary-minus node.
    let mut arena = NodeArena::new();
    let eight = num(&mut arena, "8");
    let neg_eight = arena.new_prefix_unary_expression(Kind::MinusToken, eight);
    let one = num(&mut arena, "1");
    let expr = bin(
        &mut arena,
        neg_eight,
        Kind::GreaterThanGreaterThanToken,
        one,
    );
    let eval = new_evaluator(none_entity, OuterExpressionKinds::NONE);
    assert_eq!(
        eval.evaluate(&arena, expr, None).value,
        EvalValue::Num(Number::from(-4.0))
    );
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (KindGreaterThanGreaterThanGreaterThanToken -> jsnum.UnsignedRightShift)
#[test]
fn eval_ushift_right() {
    // -1 >>> 0, with -1 built as a real unary-minus node.
    let mut arena = NodeArena::new();
    let one = num(&mut arena, "1");
    let neg_one = arena.new_prefix_unary_expression(Kind::MinusToken, one);
    let zero = num(&mut arena, "0");
    let expr = bin(
        &mut arena,
        neg_one,
        Kind::GreaterThanGreaterThanGreaterThanToken,
        zero,
    );
    let eval = new_evaluator(none_entity, OuterExpressionKinds::NONE);
    assert_eq!(
        eval.evaluate(&arena, expr, None).value,
        EvalValue::Num(Number::from(4294967295.0))
    );
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (Asterisk/Slash/Plus/Minus/Percent on Numbers)
#[test]
fn eval_mul_div_add_sub_mod() {
    assert_eq!(
        fold_num_bin("2", Kind::AsteriskToken, "3"),
        EvalValue::Num(Number::from(6.0))
    );
    assert_eq!(
        fold_num_bin("6", Kind::SlashToken, "2"),
        EvalValue::Num(Number::from(3.0))
    );
    assert_eq!(
        fold_num_bin("2", Kind::PlusToken, "3"),
        EvalValue::Num(Number::from(5.0))
    );
    assert_eq!(
        fold_num_bin("5", Kind::MinusToken, "2"),
        EvalValue::Num(Number::from(3.0))
    );
    assert_eq!(
        fold_num_bin("5", Kind::PercentToken, "2"),
        EvalValue::Num(Number::from(1.0))
    );
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (KindAsteriskAsteriskToken -> jsnum.Exponentiate)
#[test]
fn eval_exponent() {
    assert_eq!(
        fold_num_bin("2", Kind::AsteriskAsteriskToken, "10"),
        EvalValue::Num(Number::from(1024.0))
    );
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (binary `+` string-concat branch)
#[test]
fn eval_string_concat() {
    let mut arena = NodeArena::new();
    let a = arena.new_string_literal("a", TokenFlags::NONE);
    let b = arena.new_string_literal("b", TokenFlags::NONE);
    let expr = bin(&mut arena, a, Kind::PlusToken, b);
    let eval = new_evaluator(none_entity, OuterExpressionKinds::NONE);
    let r = eval.evaluate(&arena, expr, None);
    assert_eq!(r.value, EvalValue::Str("ab".to_string()));
    assert!(r.is_syntactically_string);
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (string + number, number side via leftNum.String())
#[test]
fn eval_string_plus_number() {
    let mut arena = NodeArena::new();
    let a = arena.new_string_literal("a", TokenFlags::NONE);
    let one = num(&mut arena, "1");
    let expr = bin(&mut arena, a, Kind::PlusToken, one);
    let eval = new_evaluator(none_entity, OuterExpressionKinds::NONE);
    let r = eval.evaluate(&arena, expr, None);
    assert_eq!(r.value, EvalValue::Str("a1".to_string()));
    assert!(r.is_syntactically_string);
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (number + string, number side via leftNum.String())
#[test]
fn eval_number_plus_string() {
    let mut arena = NodeArena::new();
    let one = num(&mut arena, "1");
    let a = arena.new_string_literal("a", TokenFlags::NONE);
    let expr = bin(&mut arena, one, Kind::PlusToken, a);
    let eval = new_evaluator(none_entity, OuterExpressionKinds::NONE);
    let r = eval.evaluate(&arena, expr, None);
    assert_eq!(r.value, EvalValue::Str("1a".to_string()));
    assert!(r.is_syntactically_string);
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (KindPlusToken on a Number)
#[test]
fn eval_unary_plus() {
    let mut arena = NodeArena::new();
    let operand = num(&mut arena, "5");
    let expr = arena.new_prefix_unary_expression(Kind::PlusToken, operand);
    let eval = new_evaluator(none_entity, OuterExpressionKinds::NONE);
    assert_eq!(
        eval.evaluate(&arena, expr, None).value,
        EvalValue::Num(Number::from(5.0))
    );
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (KindMinusToken on a Number)
#[test]
fn eval_unary_minus() {
    let mut arena = NodeArena::new();
    let operand = num(&mut arena, "5");
    let expr = arena.new_prefix_unary_expression(Kind::MinusToken, operand);
    let eval = new_evaluator(none_entity, OuterExpressionKinds::NONE);
    assert_eq!(
        eval.evaluate(&arena, expr, None).value,
        EvalValue::Num(Number::from(-5.0))
    );
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (KindTildeToken -> jsnum.BitwiseNOT)
#[test]
fn eval_unary_tilde() {
    let mut arena = NodeArena::new();
    let operand = num(&mut arena, "0");
    let expr = arena.new_prefix_unary_expression(Kind::TildeToken, operand);
    let eval = new_evaluator(none_entity, OuterExpressionKinds::NONE);
    assert_eq!(
        eval.evaluate(&arena, expr, None).value,
        EvalValue::Num(Number::from(-1.0))
    );
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (KindNumericLiteral branch)
#[test]
fn eval_numeric_literal() {
    let mut arena = NodeArena::new();
    let lit = arena.new_numeric_literal("123", TokenFlags::NONE);
    let eval = new_evaluator(none_entity, OuterExpressionKinds::NONE);
    let r = eval.evaluate(&arena, lit, None);
    assert_eq!(r.value, EvalValue::Num(Number::from(123.0)));
    assert!(!r.is_syntactically_string);
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (KindIdentifier -> evaluateEntity)
#[test]
fn eval_identifier_calls_entity() {
    let mut arena = NodeArena::new();
    let x = arena.new_identifier("x");
    let eval = new_evaluator(
        |_: &NodeArena, _, _| new_result(EvalValue::Num(Number::from(42.0)), false, false, false),
        OuterExpressionKinds::NONE,
    );
    assert_eq!(
        eval.evaluate(&arena, x, None).value,
        EvalValue::Num(Number::from(42.0))
    );
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (SkipOuterExpressions, parentheses)
#[test]
fn eval_skips_outer_parens() {
    let mut arena = NodeArena::new();
    let one = num(&mut arena, "1");
    let two = num(&mut arena, "2");
    let sum = bin(&mut arena, one, Kind::PlusToken, two);
    let paren = arena.new_parenthesized_expression(sum);
    let eval = new_evaluator(none_entity, OuterExpressionKinds::NONE);
    assert_eq!(
        eval.evaluate(&arena, paren, None).value,
        EvalValue::Num(Number::from(3.0))
    );
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (default branch -> Result{nil, ...})
#[test]
fn eval_unsupported_returns_none() {
    let mut arena = NodeArena::new();
    let f = arena.new_identifier("f");
    let call = arena.new_call_expression(f, None, None, NodeList::new(vec![]), NodeFlags::NONE);
    let eval = new_evaluator(none_entity, OuterExpressionKinds::NONE);
    assert_eq!(eval.evaluate(&arena, call, None).value, EvalValue::None);
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (resolvedOtherFiles/hasExternalReferences bubble from operands)
#[test]
fn eval_propagates_resolved_flags() {
    // Entity stub resolves a Number while reporting both bookkeeping flags set.
    let entity = |_: &NodeArena, _: NodeId, _: Option<NodeId>| {
        new_result(EvalValue::Num(Number::from(2.0)), false, true, true)
    };

    // Unary `-x` keeps the flags from its operand.
    let mut arena = NodeArena::new();
    let x = arena.new_identifier("x");
    let neg = arena.new_prefix_unary_expression(Kind::MinusToken, x);
    let eval = new_evaluator(entity, OuterExpressionKinds::NONE);
    let r = eval.evaluate(&arena, neg, None);
    assert_eq!(r.value, EvalValue::Num(Number::from(-2.0)));
    assert!(r.resolved_other_files);
    assert!(r.has_external_references);

    // Binary `x + x` ORs the flags from both operands.
    let y = arena.new_identifier("y");
    let z = arena.new_identifier("z");
    let sum = bin(&mut arena, y, Kind::PlusToken, z);
    let r = eval.evaluate(&arena, sum, None);
    assert_eq!(r.value, EvalValue::Num(Number::from(4.0)));
    assert!(r.resolved_other_files);
    assert!(r.has_external_references);
}

// Go: internal/evaluator/evaluator.go:NewEvaluator (KindPropertyAccessExpression + IsEntityNameExpression)
#[test]
fn eval_property_access_entity_name() {
    let mut arena = NodeArena::new();
    let a = arena.new_identifier("a");
    let b = arena.new_identifier("b");
    let pa = arena.new_property_access_expression(a, None, b);
    let eval = new_evaluator(
        |_: &NodeArena, _, _| new_result(EvalValue::Str("v".to_string()), true, false, false),
        OuterExpressionKinds::NONE,
    );
    assert_eq!(
        eval.evaluate(&arena, pa, None).value,
        EvalValue::Str("v".to_string())
    );
}

// Go: internal/evaluator/evaluator.go:AnyToString (string case)
#[test]
fn any_to_string_str() {
    assert_eq!(any_to_string(&EvalValue::Str("x".to_string())), "x");
}

// Go: internal/evaluator/evaluator.go:AnyToString (jsnum.Number case)
#[test]
fn any_to_string_num() {
    assert_eq!(any_to_string(&EvalValue::Num(Number::from(1.5))), "1.5");
}

// Go: internal/evaluator/evaluator.go:AnyToString (bool case, core.IfElse)
#[test]
fn any_to_string_bool() {
    assert_eq!(any_to_string(&EvalValue::Bool(true)), "true");
    assert_eq!(any_to_string(&EvalValue::Bool(false)), "false");
}

// Go: internal/evaluator/evaluator.go:AnyToString (jsnum.PseudoBigInt case)
#[test]
fn any_to_string_bigint() {
    assert_eq!(
        any_to_string(&EvalValue::BigInt(PseudoBigInt::new("10", false))),
        "10"
    );
}

// Go: internal/evaluator/evaluator.go:IsTruthy (string case: len != 0)
#[test]
fn is_truthy_string() {
    assert!(!is_truthy(&EvalValue::Str(String::new())));
    assert!(is_truthy(&EvalValue::Str("a".to_string())));
}

// Go: internal/evaluator/evaluator.go:IsTruthy (jsnum.Number case: != 0 && !NaN)
#[test]
fn is_truthy_number() {
    assert!(!is_truthy(&EvalValue::Num(Number::from(0.0))));
    assert!(!is_truthy(&EvalValue::Num(Number::nan())));
    assert!(is_truthy(&EvalValue::Num(Number::from(1.0))));
}

// Go: internal/evaluator/evaluator.go:IsTruthy (bool case: identity)
#[test]
fn is_truthy_bool() {
    assert!(!is_truthy(&EvalValue::Bool(false)));
    assert!(is_truthy(&EvalValue::Bool(true)));
}

// Go: internal/evaluator/evaluator.go:IsTruthy (jsnum.PseudoBigInt case: != zero value)
#[test]
fn is_truthy_bigint() {
    assert!(!is_truthy(&EvalValue::BigInt(PseudoBigInt::new(
        "0", false
    ))));
    assert!(is_truthy(&EvalValue::BigInt(PseudoBigInt::new("1", false))));
}
