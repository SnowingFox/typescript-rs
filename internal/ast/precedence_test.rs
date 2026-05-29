use super::*;
use crate::{Kind, NodeArena, NodeFlags, NodeList};

// Go: internal/ast/precedence.go:OperatorPrecedence (iota values + aliases)
#[test]
fn operator_precedence_values() {
    assert_eq!(OperatorPrecedence::Comma as i32, 0);
    assert_eq!(OperatorPrecedence::Spread as i32, 1);
    assert_eq!(OperatorPrecedence::Assignment as i32, 3);
    assert_eq!(OperatorPrecedence::Additive as i32, 13);
    assert_eq!(OperatorPrecedence::Parentheses as i32, 22);
    assert_eq!(OperatorPrecedence::Invalid as i32, -1);
    assert_eq!(OperatorPrecedence::LOWEST, OperatorPrecedence::Comma);
    assert_eq!(OperatorPrecedence::HIGHEST, OperatorPrecedence::Parentheses);
    assert_eq!(OperatorPrecedence::COALESCE, OperatorPrecedence::LogicalOR);
}

// Go: internal/ast/precedence.go:GetBinaryOperatorPrecedence
#[test]
fn get_binary_operator_precedence_matches_go() {
    assert_eq!(
        get_binary_operator_precedence(Kind::PlusToken),
        OperatorPrecedence::Additive
    );
    assert_eq!(
        get_binary_operator_precedence(Kind::AsteriskToken),
        OperatorPrecedence::Multiplicative
    );
    assert_eq!(
        get_binary_operator_precedence(Kind::AsteriskAsteriskToken),
        OperatorPrecedence::Exponentiation
    );
    assert_eq!(
        get_binary_operator_precedence(Kind::BarBarToken),
        OperatorPrecedence::LogicalOR
    );
    assert_eq!(
        get_binary_operator_precedence(Kind::QuestionQuestionToken),
        OperatorPrecedence::COALESCE
    );
    assert_eq!(
        get_binary_operator_precedence(Kind::InKeyword),
        OperatorPrecedence::Relational
    );
    assert_eq!(
        get_binary_operator_precedence(Kind::SemicolonToken),
        OperatorPrecedence::Invalid
    );
}

// Go: internal/ast/precedence.go:GetOperatorPrecedence
#[test]
fn get_operator_precedence_matches_go() {
    let none = OperatorPrecedenceFlags::NONE;
    assert_eq!(
        get_operator_precedence(Kind::SpreadElement, Kind::Unknown, none),
        OperatorPrecedence::Spread
    );
    assert_eq!(
        get_operator_precedence(Kind::BinaryExpression, Kind::CommaToken, none),
        OperatorPrecedence::Comma
    );
    assert_eq!(
        get_operator_precedence(Kind::BinaryExpression, Kind::EqualsToken, none),
        OperatorPrecedence::Assignment
    );
    assert_eq!(
        get_operator_precedence(Kind::BinaryExpression, Kind::PlusToken, none),
        OperatorPrecedence::Additive
    );
    assert_eq!(
        get_operator_precedence(Kind::Identifier, Kind::Unknown, none),
        OperatorPrecedence::Primary
    );
    assert_eq!(
        get_operator_precedence(Kind::CallExpression, Kind::Unknown, none),
        OperatorPrecedence::Member
    );
    assert_eq!(
        get_operator_precedence(
            Kind::CallExpression,
            Kind::Unknown,
            OperatorPrecedenceFlags::OPTIONAL_CHAIN
        ),
        OperatorPrecedence::OptionalChain
    );
}

// Go: internal/ast/precedence.go:getOperator / GetExpressionPrecedence
#[test]
fn get_operator_and_expression_precedence_on_arena() {
    let mut arena = NodeArena::new();
    let l = arena.new_identifier("a");
    let op = arena.new_token(Kind::PlusToken);
    let r = arena.new_identifier("b");
    let bin = arena.new_binary_expression(l, op, r);
    assert_eq!(get_operator(&arena, bin), Kind::PlusToken);
    assert_eq!(
        get_expression_precedence(&arena, bin),
        OperatorPrecedence::Additive
    );

    // A `new` expression with no arguments is left-hand-side precedence.
    let callee = arena.new_identifier("C");
    let new_no_args = arena.new_new_expression(callee, None, None);
    assert_eq!(
        get_expression_precedence(&arena, new_no_args),
        OperatorPrecedence::LeftHandSide
    );
    // With arguments it is member precedence.
    let callee2 = arena.new_identifier("C");
    let new_with_args = arena.new_new_expression(callee2, None, Some(NodeList::new(vec![])));
    assert_eq!(
        get_expression_precedence(&arena, new_with_args),
        OperatorPrecedence::Member
    );
}

// Optional-chain call expression uses optional-chain precedence.
// Go: internal/ast/precedence.go:GetExpressionPrecedence
#[test]
fn get_expression_precedence_optional_chain() {
    let mut arena = NodeArena::new();
    let callee = arena.new_identifier("a");
    let call = arena.new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![]),
        NodeFlags::OPTIONAL_CHAIN,
    );
    assert_eq!(
        get_expression_precedence(&arena, call),
        OperatorPrecedence::OptionalChain
    );
}
