use crate::test_support::check_synthetic;
use tsgo_ast::precedence::{get_binary_operator_precedence, OperatorPrecedence};
use tsgo_ast::{Kind, NodeArena, NodeFlags, NodeId, NodeList};
use tsgo_core::languagevariant::LanguageVariant;
use tsgo_core::scriptkind::ScriptKind;

/// Wraps `statement_expr` in a source file with a single expression statement.
fn file_with_expression(arena: &mut NodeArena, expression: NodeId) -> NodeId {
    let stmt = arena.new_expression_statement(expression);
    let eof = arena.new_token(Kind::EndOfFile);
    arena.new_source_file(
        "/file.ts",
        ScriptKind::Ts,
        LanguageVariant::Standard,
        NodeList::new(vec![stmt]),
        eof,
    )
}

fn is_binary_operator(kind: Kind) -> bool {
    get_binary_operator_precedence(kind) != OperatorPrecedence::Invalid
}

/// Mirrors Go `makeSide`: builds the operand for a binary-precedence test.
fn make_side(arena: &mut NodeArena, label: &str, kind: Kind) -> NodeId {
    if kind == Kind::Identifier || kind == Kind::Unknown {
        arena.new_identifier(label)
    } else if kind == Kind::ArrowFunction {
        let arrow_token = arena.new_token(Kind::EqualsGreaterThanToken);
        let body = arena.new_block(NodeList::new(vec![]));
        arena.new_arrow_function(
            None,
            None,
            NodeList::new(vec![]),
            None,
            None,
            arrow_token,
            body,
        )
    } else if is_binary_operator(kind) {
        let l = arena.new_identifier(&format!("{label}l"));
        let op = arena.new_token(kind);
        let r = arena.new_identifier(&format!("{label}r"));
        arena.new_binary_expression(l, op, r)
    } else {
        panic!("unsupported kind {kind:?}")
    }
}

// Go: internal/printer/printer_test.go:TestParenthesizeBinary
#[test]
fn parenthesize_binary() {
    let cases: &[(Kind, Kind, Kind, &str)] = &[
        (Kind::Unknown, Kind::CommaToken, Kind::Unknown, "l, r"),
        (
            Kind::PlusToken,
            Kind::CommaToken,
            Kind::Unknown,
            "ll + lr, r",
        ),
        (
            Kind::PlusToken,
            Kind::AsteriskToken,
            Kind::Unknown,
            "(ll + lr) * r",
        ),
        (
            Kind::Unknown,
            Kind::AsteriskToken,
            Kind::PlusToken,
            "l * (rl + rr)",
        ),
        (
            Kind::AsteriskToken,
            Kind::PlusToken,
            Kind::Unknown,
            "ll * lr + r",
        ),
        (
            Kind::Unknown,
            Kind::PlusToken,
            Kind::AsteriskToken,
            "l + rl * rr",
        ),
        (
            Kind::AsteriskToken,
            Kind::SlashToken,
            Kind::Unknown,
            "ll * lr / r",
        ),
        (
            Kind::AsteriskAsteriskToken,
            Kind::SlashToken,
            Kind::Unknown,
            "ll ** lr / r",
        ),
        (
            Kind::AsteriskToken,
            Kind::AsteriskAsteriskToken,
            Kind::Unknown,
            "(ll * lr) ** r",
        ),
        (
            Kind::AsteriskAsteriskToken,
            Kind::AsteriskAsteriskToken,
            Kind::Unknown,
            "(ll ** lr) ** r",
        ),
        (
            Kind::Unknown,
            Kind::AsteriskToken,
            Kind::AsteriskToken,
            "l * rl * rr",
        ),
        (Kind::Unknown, Kind::BarToken, Kind::BarToken, "l | rl | rr"),
        (
            Kind::Unknown,
            Kind::AmpersandToken,
            Kind::AmpersandToken,
            "l & rl & rr",
        ),
        (
            Kind::Unknown,
            Kind::CaretToken,
            Kind::CaretToken,
            "l ^ rl ^ rr",
        ),
        (
            Kind::Unknown,
            Kind::AmpersandAmpersandToken,
            Kind::ArrowFunction,
            "l && (() => { })",
        ),
    ];
    for (left, op, right, output) in cases {
        let mut arena = NodeArena::new();
        let l = make_side(&mut arena, "l", *left);
        let op_token = arena.new_token(*op);
        let r = make_side(&mut arena, "r", *right);
        let binary = arena.new_binary_expression(l, op_token, r);
        let sf = file_with_expression(&mut arena, binary);
        check_synthetic(arena, sf, &format!("{output};"));
    }
}

/// Builds a synthetic binary `a <op> b` over fresh identifiers.
fn binary(arena: &mut NodeArena, left: &str, op: Kind, right: &str) -> NodeId {
    let l = arena.new_identifier(left);
    let t = arena.new_token(op);
    let r = arena.new_identifier(right);
    arena.new_binary_expression(l, t, r)
}

// Go: internal/printer/printer_test.go:TestParenthesizeConditional1
#[test]
fn parenthesize_conditional_1() {
    let mut arena = NodeArena::new();
    let cond = binary(&mut arena, "a", Kind::CommaToken, "b");
    let q = arena.new_token(Kind::QuestionToken);
    let c = arena.new_identifier("c");
    let colon = arena.new_token(Kind::ColonToken);
    let d = arena.new_identifier("d");
    let conditional = arena.new_conditional_expression(cond, q, c, colon, d);
    let sf = file_with_expression(&mut arena, conditional);
    check_synthetic(arena, sf, "(a, b) ? c : d;");
}

// Go: internal/printer/printer_test.go:TestParenthesizeConditional2
#[test]
fn parenthesize_conditional_2() {
    let mut arena = NodeArena::new();
    let cond = binary(&mut arena, "a", Kind::EqualsToken, "b");
    let q = arena.new_token(Kind::QuestionToken);
    let c = arena.new_identifier("c");
    let colon = arena.new_token(Kind::ColonToken);
    let d = arena.new_identifier("d");
    let conditional = arena.new_conditional_expression(cond, q, c, colon, d);
    let sf = file_with_expression(&mut arena, conditional);
    check_synthetic(arena, sf, "(a = b) ? c : d;");
}

// Go: internal/printer/printer_test.go:TestParenthesizeSpreadElement1
#[test]
fn parenthesize_spread_element_1() {
    let mut arena = NodeArena::new();
    let comma = binary(&mut arena, "a", Kind::CommaToken, "b");
    let spread = arena.new_spread_element(comma);
    let array = arena.new_array_literal_expression(NodeList::new(vec![spread]));
    let sf = file_with_expression(&mut arena, array);
    check_synthetic(arena, sf, "[...(a, b)];");
}

// Go: internal/printer/printer_test.go:TestParenthesizeCall4
#[test]
fn parenthesize_call_4() {
    let mut arena = NodeArena::new();
    let callee = arena.new_identifier("a");
    let comma = binary(&mut arena, "b", Kind::CommaToken, "c");
    let call = arena.new_call_expression(
        callee,
        None,
        None,
        NodeList::new(vec![comma]),
        NodeFlags::NONE,
    );
    let sf = file_with_expression(&mut arena, call);
    check_synthetic(arena, sf, "a((b, c));");
}

// Go: internal/printer/printer_test.go:TestParenthesizeNew2
#[test]
fn parenthesize_new_2() {
    // `new (C())` — a `new` whose callee is a call must be parenthesized.
    let mut arena = NodeArena::new();
    let c = arena.new_identifier("C");
    let inner_call =
        arena.new_call_expression(c, None, None, NodeList::new(vec![]), NodeFlags::NONE);
    let new_expr = arena.new_new_expression(inner_call, None, None);
    let sf = file_with_expression(&mut arena, new_expr);
    check_synthetic(arena, sf, "new (C());");
}

// Go: internal/printer/printer_test.go:TestParenthesizeAsExpression
#[test]
fn parenthesize_as_expression() {
    let mut arena = NodeArena::new();
    let comma = binary(&mut arena, "a", Kind::CommaToken, "b");
    let type_ref = {
        let name = arena.new_identifier("c");
        arena.new_type_reference_node(name, None)
    };
    let as_expr = arena.new_as_expression(comma, type_ref);
    let sf = file_with_expression(&mut arena, as_expr);
    check_synthetic(arena, sf, "(a, b) as c;");
}
