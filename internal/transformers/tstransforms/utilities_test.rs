use super::*;
use tsgo_ast::NodeData;
use tsgo_printer::EmitContext;

// Go: internal/transformers/tstransforms/utilities.go:constantExpression
// Materializes each constant-value shape into the right factory node.
#[test]
fn constant_expression_builds_literals() {
    let mut ec = EmitContext::new();

    // String -> string literal.
    let s = {
        let mut f = ec.factory();
        constant_expression(&ConstantValue::Str("hello".to_string()), &mut f)
    };
    assert_eq!(ec.arena().kind(s), Kind::StringLiteral);
    assert_eq!(ec.arena().text(s), "hello");

    // Positive number -> numeric literal with JS string form.
    let n = {
        let mut f = ec.factory();
        constant_expression(&ConstantValue::Num(Number::from(42.0)), &mut f)
    };
    assert_eq!(ec.arena().kind(n), Kind::NumericLiteral);
    assert_eq!(ec.arena().text(n), "42");

    // NaN -> `NaN` identifier.
    let nan = {
        let mut f = ec.factory();
        constant_expression(&ConstantValue::Num(Number::nan()), &mut f)
    };
    assert_eq!(ec.arena().kind(nan), Kind::Identifier);
    assert_eq!(ec.arena().text(nan), "NaN");

    // +Infinity -> `Infinity` identifier.
    let inf = {
        let mut f = ec.factory();
        constant_expression(&ConstantValue::Num(Number::inf(1)), &mut f)
    };
    assert_eq!(ec.arena().kind(inf), Kind::Identifier);
    assert_eq!(ec.arena().text(inf), "Infinity");
}

// Go: internal/transformers/tstransforms/utilities.go:constantExpression (negative + -Infinity)
#[test]
fn constant_expression_negates_with_prefix_unary() {
    let mut ec = EmitContext::new();

    // Negative number -> `-(<abs>)`.
    let neg = {
        let mut f = ec.factory();
        constant_expression(&ConstantValue::Num(Number::from(-3.0)), &mut f)
    };
    let operand = match ec.arena().data(neg) {
        NodeData::PrefixUnaryExpression(d) => {
            assert_eq!(d.operator, Kind::MinusToken);
            d.operand
        }
        other => panic!("expected prefix unary, got {other:?}"),
    };
    assert_eq!(ec.arena().kind(operand), Kind::NumericLiteral);
    assert_eq!(ec.arena().text(operand), "3");

    // -Infinity -> `-Infinity`.
    let neg_inf = {
        let mut f = ec.factory();
        constant_expression(&ConstantValue::Num(Number::inf(-1)), &mut f)
    };
    let operand = match ec.arena().data(neg_inf) {
        NodeData::PrefixUnaryExpression(d) => d.operand,
        other => panic!("expected prefix unary, got {other:?}"),
    };
    assert_eq!(ec.arena().kind(operand), Kind::Identifier);
    assert_eq!(ec.arena().text(operand), "Infinity");
}
