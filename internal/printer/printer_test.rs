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

// Panic-robustness (P10 corpus triage, category b): emitting a source file that
// contains a parser-recovered MISSING (zero-width) node must not panic. With
// top-level `await` unrecognized in this parse context,
// `const foo = await { bar: 42 }` error-recovers into a `VariableDeclaration`
// whose name is an `ObjectBindingPattern` whose binding-name identifiers are
// missing (`pos == end`, sitting on whitespace). `get_source_text_of_node` ran
// `skip_trivia(pos)` past `end` and then sliced `text[pos..end]` with `pos > end`
// (the `begin <= end (35 <= 34)` panic). Go's `GetTextOfNodeFromSourceText`
// short-circuits a missing node to `""`; this asserts the port does the same.
// Go: internal/scanner/utilities.go:GetTextOfNodeFromSourceText (NodeIsMissing guard)
#[test]
fn emit_missing_node_does_not_panic() {
    let out = crate::test_support::emit_allowing_diagnostics("const foo = await { bar: 42 }\n");
    // The recovered text is still emitted (the present `bar` property name); the
    // missing binding-name identifiers emit as empty text rather than panicking.
    assert!(
        out.contains("bar"),
        "emit should still contain the recovered text, got {out:?}"
    );
}
