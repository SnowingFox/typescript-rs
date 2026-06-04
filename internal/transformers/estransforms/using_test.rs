use super::*;
use tsgo_ast::NodeFlags;

// Go: internal/transformers/estransforms/using.go:getUsingKindOfVariableDeclarationList
#[test]
fn using_kind_sync() {
    assert_eq!(get_using_kind_of_flags(NodeFlags::USING), UsingKind::Sync);
}

#[test]
fn using_kind_async() {
    assert_eq!(
        get_using_kind_of_flags(NodeFlags::AWAIT_USING),
        UsingKind::Async
    );
}

#[test]
fn using_kind_none_for_const() {
    assert_eq!(get_using_kind_of_flags(NodeFlags::CONST), UsingKind::None);
}

#[test]
fn using_kind_none_for_let() {
    assert_eq!(get_using_kind_of_flags(NodeFlags::LET), UsingKind::None);
}

#[test]
fn using_kind_none_for_empty() {
    assert_eq!(get_using_kind_of_flags(NodeFlags::NONE), UsingKind::None);
}

// Go: internal/transformers/estransforms/using.go:newUsingDeclarationTransformer
// The transformer is a pass-through (DEFER until parser supports `using`).
#[test]
fn transformer_is_pass_through() {
    use crate::test_support::{emit, parse_shared};
    use std::rc::Rc;
    let (ec, source_file) = parse_shared("var x = 1;");
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let mut tx = new_using_declaration_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, "var x = 1;"), "var x = 1;");
}

// ───────────────────────────────────────────────────────────────────────
// T2-10 integration tests: using declaration verification
// ───────────────────────────────────────────────────────────────────────

// Go: internal/transformers/estransforms/using.go:getUsingKind
// A `var` statement is not a using declaration.
#[test]
fn get_using_kind_var_statement_is_none() {
    use crate::test_support::parse_shared;
    let (ec, source_file) = parse_shared("var x = 1;");
    let ec_ref = ec.borrow();
    let first_stmt = match ec_ref.arena().data(source_file) {
        tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => panic!("expected source file"),
    };
    assert_eq!(get_using_kind(ec_ref.arena(), first_stmt), UsingKind::None);
}

// Go: internal/transformers/estransforms/using.go:getUsingKind
// A `let` statement is not a using declaration.
#[test]
fn get_using_kind_let_statement_is_none() {
    use crate::test_support::parse_shared;
    let (ec, source_file) = parse_shared("let x = 1;");
    let ec_ref = ec.borrow();
    let first_stmt = match ec_ref.arena().data(source_file) {
        tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => panic!("expected source file"),
    };
    assert_eq!(get_using_kind(ec_ref.arena(), first_stmt), UsingKind::None);
}

// Go: internal/transformers/estransforms/using.go:getUsingKind
// A `const` statement is not a using declaration.
#[test]
fn get_using_kind_const_statement_is_none() {
    use crate::test_support::parse_shared;
    let (ec, source_file) = parse_shared("const x = 1;");
    let ec_ref = ec.borrow();
    let first_stmt = match ec_ref.arena().data(source_file) {
        tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => panic!("expected source file"),
    };
    assert_eq!(get_using_kind(ec_ref.arena(), first_stmt), UsingKind::None);
}

// Go: internal/transformers/estransforms/using.go:getUsingKind
// A non-VariableStatement node returns None.
#[test]
fn get_using_kind_function_decl_is_none() {
    use crate::test_support::parse_shared;
    let (ec, source_file) = parse_shared("function f() {}");
    let ec_ref = ec.borrow();
    let first_stmt = match ec_ref.arena().data(source_file) {
        tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => panic!("expected source file"),
    };
    assert_eq!(get_using_kind(ec_ref.arena(), first_stmt), UsingKind::None);
}

// Go: internal/transformers/estransforms/using.go:getUsingKindOfStatements
// An empty slice returns None.
#[test]
fn get_using_kind_of_statements_empty() {
    use tsgo_ast::NodeArena;
    let arena = NodeArena::new();
    assert_eq!(get_using_kind_of_statements(&arena, &[]), UsingKind::None);
}

// Go: internal/transformers/estransforms/using.go:getUsingKindOfStatements
// A slice of non-using statements returns None.
#[test]
fn get_using_kind_of_statements_all_non_using() {
    use crate::test_support::parse_shared;
    let (ec, source_file) = parse_shared("var a = 1; let b = 2;");
    let ec_ref = ec.borrow();
    let stmts = match ec_ref.arena().data(source_file) {
        tsgo_ast::NodeData::SourceFile(d) => &d.statements.nodes,
        _ => panic!("expected source file"),
    };
    assert_eq!(
        get_using_kind_of_statements(ec_ref.arena(), stmts),
        UsingKind::None
    );
}

// Go: internal/transformers/estransforms/using.go:getUsingKindOfFlags
// AWAIT_USING flag yields Async.
#[test]
fn using_kind_await_using_flag_is_async() {
    assert_eq!(
        get_using_kind_of_flags(NodeFlags::AWAIT_USING),
        UsingKind::Async
    );
}

// Go: internal/transformers/estransforms/using.go:UsingKind ordering
// Async > Sync > None.
#[test]
fn using_kind_ordering() {
    assert!(UsingKind::Async > UsingKind::Sync);
    assert!(UsingKind::Sync > UsingKind::None);
    assert!(UsingKind::Async > UsingKind::None);
}
