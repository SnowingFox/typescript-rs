use crate::test_support::check_emit;

// Go: internal/printer/printer_test.go:TestEmit/NumericLiteral#1
#[test]
fn numeric_literal_1() {
    check_emit("0", "0;", false);
}

// Go: internal/printer/printer_test.go:TestEmit/NumericLiteral#2
#[test]
fn numeric_literal_2() {
    check_emit("10_000", "10000;", false);
}

// Go: internal/printer/printer_test.go:TestEmit/BooleanLiteral#1
#[test]
fn boolean_literal_1() {
    check_emit("true", "true;", false);
}

// Go: internal/printer/printer_test.go:TestEmit/BooleanLiteral#2
#[test]
fn boolean_literal_2() {
    check_emit("false", "false;", false);
}

// Go: internal/printer/printer_test.go:TestEmit/NullLiteral
#[test]
fn null_literal() {
    check_emit("null", "null;", false);
}

// Go: internal/printer/printer_test.go:TestEmit/ThisExpression
#[test]
fn this_expression() {
    check_emit("this", "this;", false);
}
