use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/VariableDeclaration2
// Tracer bullet: a real type-erasure stage runs through the 6a Transformer
// driver and re-emits JS with the type annotation stripped.
#[test]
fn variable_declaration_type_is_erased() {
    let input = "var a: number";
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_type_eraser_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), "var a;");
}

// Runs the type eraser over `input` and asserts the emitted JS equals `expected`.
fn check_erase(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_type_eraser_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "erase({input:?})");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/CallExpression
#[test]
fn call_expression_type_arguments_erased() {
    check_erase("f<T>()", "f();");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/NewExpression1,NewExpression2
#[test]
fn new_expression_type_arguments_erased() {
    check_erase("new f<T>()", "new f();");
    check_erase("new f<T>", "new f;");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/ExpressionWithTypeArguments
#[test]
fn expression_with_type_arguments_erased() {
    check_erase("F<T>", "F;");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/FunctionDeclaration3
#[test]
fn function_declaration_type_params_and_return_erased() {
    check_erase("function f<T>(): U {}", "function f() { }");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/ClassDeclaration2
#[test]
fn class_declaration_type_params_erased() {
    check_erase("class C<T> {}", "class C {\n}");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/ClassExpression
#[test]
fn class_expression_type_params_erased() {
    check_erase("(class C<T> {})", "(class C {\n});");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/FunctionExpression
#[test]
fn function_expression_type_params_and_return_erased() {
    check_erase("(function f<T>(): U {})", "(function f() { });");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/InterfaceDeclaration,TypeAliasDeclaration
#[test]
fn type_only_declarations_are_elided() {
    check_erase("interface I {}", "");
    check_erase("type T = U;", "");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/VariableDeclaration1,ClassDeclaration1,FunctionDeclaration1
#[test]
fn ambient_declarations_are_elided() {
    check_erase("declare var a;", "");
    check_erase("declare class C {}", "");
    check_erase("declare function f() {}", "");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/NamespaceExportDeclaration
#[test]
fn namespace_export_declaration_is_elided() {
    check_erase("export as namespace N;", "");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/Modifiers,PropertyDeclaration2,PropertyDeclaration3
#[test]
fn property_accessibility_modifiers_and_type_erased() {
    check_erase(
        "class C { public x; private y }",
        "class C {\n    x;\n    y;\n}",
    );
    check_erase("class C { public x: number; }", "class C {\n    x;\n}");
    check_erase(
        "class C { public static x: number; }",
        "class C {\n    static x;\n}",
    );
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/PropertyDeclaration1
#[test]
fn declare_property_is_removed() {
    check_erase("class C { declare x; }", "class C {\n}");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/HeritageClause
#[test]
fn implements_clause_is_elided() {
    check_erase("class C implements I {}", "class C {\n}");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/ParameterDeclaration
#[test]
fn this_parameter_and_parameter_types_erased() {
    check_erase(
        "function f(this: x, a: number, b?: boolean) {}",
        "function f(a, b) { }",
    );
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/NonNullExpression,TypeAssertionExpression,AsExpression,SatisfiesExpression
#[test]
fn type_assertions_lower_to_inner_expression() {
    check_erase("x!", "x;");
    check_erase("x as T", "x;");
    check_erase("<T>x", "x;");
    check_erase("x satisfies T", "x;");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/FunctionDeclaration2
#[test]
fn function_overload_signature_is_elided() {
    check_erase("function f();", "");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/ImportEqualsDeclaration#2,ImportDeclaration#6
#[test]
fn type_only_imports_are_elided() {
    check_erase("import type x = require(\"m\");", "");
    check_erase("import type x from \"m\";", "");
    // Value imports are preserved (usage-based elision is the checker's job).
    check_erase("import x = require(\"m\");", "import x = require(\"m\");");
}
