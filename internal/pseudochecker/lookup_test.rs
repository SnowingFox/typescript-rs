use crate::{PseudoChecker, PseudoType};
use tsgo_ast::{Kind, NodeArena, NodeFlags, NodeList, TokenFlags};

// A checker whose flags do not affect the expression-core branches under test.
fn checker() -> PseudoChecker {
    PseudoChecker::new(false, false)
}

// Go: internal/pseudochecker/lookup.go:typeFromExpression (KindStringLiteral)
// A string literal is ambiguous: `"abc"` in a const location, else `string`.
#[test]
fn expr_string_literal_is_maybe_const() {
    let mut arena = NodeArena::new();
    let lit = arena.new_string_literal("abc", TokenFlags::empty());
    assert_eq!(
        checker().get_type_of_expression(&arena, lit),
        PseudoType::maybe_const_location(lit, PseudoType::string_literal(lit), PseudoType::String)
    );
}

// Go: internal/pseudochecker/lookup.go:typeFromExpression (KindNumericLiteral)
#[test]
fn expr_numeric_literal_is_maybe_const() {
    let mut arena = NodeArena::new();
    let lit = arena.new_numeric_literal("123", TokenFlags::empty());
    assert_eq!(
        checker().get_type_of_expression(&arena, lit),
        PseudoType::maybe_const_location(lit, PseudoType::numeric_literal(lit), PseudoType::Number)
    );
}

// Go: internal/pseudochecker/lookup.go:typeFromExpression (KindBigIntLiteral)
#[test]
fn expr_bigint_literal_is_maybe_const() {
    let mut arena = NodeArena::new();
    let lit = arena.new_big_int_literal("123n", TokenFlags::empty());
    assert_eq!(
        checker().get_type_of_expression(&arena, lit),
        PseudoType::maybe_const_location(lit, PseudoType::bigint_literal(lit), PseudoType::BigInt)
    );
}

// Go: internal/pseudochecker/lookup.go:typeFromExpression (KindTrueKeyword/KindFalseKeyword)
#[test]
fn expr_true_false_are_maybe_const_boolean() {
    let mut arena = NodeArena::new();
    let t = arena.new_keyword_expression(Kind::TrueKeyword);
    let f = arena.new_keyword_expression(Kind::FalseKeyword);
    assert_eq!(
        checker().get_type_of_expression(&arena, t),
        PseudoType::maybe_const_location(t, PseudoType::True, PseudoType::Boolean)
    );
    assert_eq!(
        checker().get_type_of_expression(&arena, f),
        PseudoType::maybe_const_location(f, PseudoType::False, PseudoType::Boolean)
    );
}

// Go: internal/pseudochecker/lookup.go:typeFromExpression (KindNullKeyword)
#[test]
fn expr_null_keyword_is_null() {
    let mut arena = NodeArena::new();
    let n = arena.new_keyword_expression(Kind::NullKeyword);
    assert_eq!(
        checker().get_type_of_expression(&arena, n),
        PseudoType::Null
    );
}

// Go: internal/pseudochecker/lookup.go:typeFromExpression (KindIdentifier, "undefined")
// The bare identifier `undefined` resolves to the `undefined` pseudo-type;
// any other identifier falls through to `Inferred`.
#[test]
fn expr_identifier_undefined_vs_other() {
    let mut arena = NodeArena::new();
    let undef = arena.new_identifier("undefined");
    let other = arena.new_identifier("x");
    assert_eq!(
        checker().get_type_of_expression(&arena, undef),
        PseudoType::Undefined
    );
    assert_eq!(
        checker().get_type_of_expression(&arena, other),
        PseudoType::inferred(other)
    );
}

// Go: internal/pseudochecker/lookup.go:typeFromExpression (default arm)
// A complex expression with no syntactic type (e.g. a call) is `Inferred`.
#[test]
fn expr_complex_falls_back_to_inferred() {
    let mut arena = NodeArena::new();
    let callee = arena.new_identifier("f");
    let call =
        arena.new_call_expression(callee, None, None, NodeList::new(vec![]), NodeFlags::NONE);
    assert_eq!(
        checker().get_type_of_expression(&arena, call),
        PseudoType::inferred(call)
    );
}

// Go: internal/pseudochecker/lookup.go:typeFromExpression (KindParenthesizedExpression)
// Parentheses are transparent: the inner expression's type is returned.
#[test]
fn expr_parenthesized_unwraps_inner() {
    let mut arena = NodeArena::new();
    let lit = arena.new_string_literal("abc", TokenFlags::empty());
    let paren = arena.new_parenthesized_expression(lit);
    assert_eq!(
        checker().get_type_of_expression(&arena, paren),
        checker().get_type_of_expression(&arena, lit)
    );
}
