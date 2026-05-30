use crate::core::program::BoundProgram;
use crate::core::signatures::{Signature, SignatureFlags};
use crate::core::test_support::StubProgram;
use crate::core::types::{ObjectFlags, ObjectType};
use crate::core::Checker;
use tsgo_ast::NodeData;

fn empty() -> StubProgram {
    StubProgram::parse_and_bind("/a.ts", "")
}

// Returns the expression of the `idx`-th top-level expression statement.
fn expr_stmt_expression(p: &StubProgram, idx: usize) -> tsgo_ast::NodeId {
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    match arena.data(stmts[idx]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    }
}

// Go: internal/checker/checker.go:Checker.checkIdentifier (tracer bullet)
#[test]
fn check_identifier_yields_declared_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: string;\nx;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let s = c.string_type();
    assert_eq!(c.check_expression(&p, usage), s);
}

// Go: internal/checker/checker.go:Checker.checkIdentifier (Cannot_find_name_0)
#[test]
fn undefined_identifier_reports_cannot_find_name() {
    let p = StubProgram::parse_and_bind("/a.ts", "y;");
    let mut c = Checker::new();
    c.check_source_file(&p, p.root());
    let diags = c.get_diagnostics(p.root());
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkSourceFile (no errors)
#[test]
fn defined_identifier_reports_no_diagnostics() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: string;\nx;");
    let mut c = Checker::new();
    c.check_source_file(&p, p.root());
    assert!(c.get_diagnostics(p.root()).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkExpressionWorker (literals)
#[test]
fn check_literal_expressions() {
    let p = StubProgram::parse_and_bind("/a.ts", "\"a\";\n1;\ntrue;\nnull;");
    let mut c = Checker::new();
    let (str_lit, num_lit, true_lit, null_lit) = (
        expr_stmt_expression(&p, 0),
        expr_stmt_expression(&p, 1),
        expr_stmt_expression(&p, 2),
        expr_stmt_expression(&p, 3),
    );
    let t_str = c.check_expression(&p, str_lit);
    assert_eq!(c.type_to_string(t_str), "\"a\"");
    let t_num = c.check_expression(&p, num_lit);
    assert_eq!(c.type_to_string(t_num), "1");
    let true_ty = c.true_type();
    assert_eq!(c.check_expression(&p, true_lit), true_ty);
    let null_ty = c.null_type();
    assert_eq!(c.check_expression(&p, null_lit), null_ty);
}

// Go: internal/checker/checker.go:Checker.checkPropertyAccessExpression
#[test]
fn check_property_access_yields_member_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Foo {\n  bar: string;\n}\ndeclare const foo: Foo;\nfoo.bar;",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let s = c.string_type();
    assert_eq!(c.check_expression(&p, access), s);
}

// Go: internal/checker/checker.go (Property_0_does_not_exist_on_type_1)
#[test]
fn missing_property_reports_diagnostic() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Foo {\n  bar: string;\n}\ndeclare const foo: Foo;\nfoo.baz;",
    );
    let mut c = Checker::new();
    c.check_source_file(&p, p.root());
    let diags = c.get_diagnostics(p.root());
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2339);
    assert!(diags[0]
        .message
        .starts_with("Property 'baz' does not exist on type"));
}

// Go: internal/checker/checker.go:Checker.getSignaturesOfType
#[test]
fn signatures_of_function_type() {
    let mut c = Checker::new();
    let num = c.number_type();
    let mut sig = Signature::new(SignatureFlags::NONE);
    sig.resolved_return_type = Some(num);
    let sid = c.new_signature(sig);
    let obj = ObjectType {
        call_signatures: vec![sid],
        ..Default::default()
    };
    let fn_ty = c.new_object_type(ObjectFlags::empty(), None, obj);
    assert_eq!(c.get_signatures_of_type(fn_ty), vec![sid]);
    // A primitive has no call signatures.
    let s = c.string_type();
    assert!(c.get_signatures_of_type(s).is_empty());
}

// Go: internal/checker/checker.go:Checker.getReturnTypeOfSignature (+ inference)
#[test]
fn return_type_of_nongeneric_and_generic_call() {
    let p = empty();
    let mut c = Checker::new();
    let num = c.number_type();
    // Non-generic `() => number`.
    let mut sig = Signature::new(SignatureFlags::NONE);
    sig.resolved_return_type = Some(num);
    let sid = c.new_signature(sig);
    assert_eq!(c.get_return_type_of_call(&p, sid, &[], &[]), num);
    // Generic `<T>(x: T) => T` called with a `number` argument infers `T = number`.
    let tp = c.new_type_parameter(None);
    let mut gsig = Signature::new(SignatureFlags::NONE);
    gsig.type_parameters = vec![tp];
    gsig.resolved_return_type = Some(tp);
    let gsid = c.new_signature(gsig);
    assert_eq!(c.get_return_type_of_call(&p, gsid, &[tp], &[num]), num);
}

// Go: internal/checker/checker.go:Checker.checkIndexedAccess (string-literal index)
#[test]
fn check_element_access_string_index() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Foo {\n  bar: string;\n}\ndeclare const foo: Foo;\nfoo[\"bar\"];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let s = c.string_type();
    assert_eq!(c.check_expression(&p, access), s);
}
