use super::*;
use crate::test_support::{emit, parse_shared, parse_shared_tsx};
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

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/TypeAssertionExpression#2,AsExpression#2,SatisfiesExpression#2
#[test]
fn parenthesized_assertion_drops_parens() {
    check_erase("(<T>x).c", "x.c;");
    check_erase("(x as T).c", "x.c;");
    check_erase("(x satisfies T).c", "x.c;");
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

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/ArrowFunction
#[test]
fn arrow_function_type_params_and_return_erased() {
    check_erase("const f = (x: string): string => x;", "const f = (x) => x;");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/MethodDeclaration
#[test]
fn method_declaration_type_params_and_return_erased() {
    check_erase(
        "class C { m(x: number): string { return \"\"; } }",
        "class C {\n    m(x) { return \"\"; }\n}",
    );
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/Constructor
#[test]
fn constructor_type_annotations_erased() {
    // `public` is preserved by the type eraser (parameter property modifiers
    // are handled later by the runtime syntax transformer). Only the type
    // annotation `: number` is stripped.
    check_erase(
        "class C { constructor(public x: number) {} }",
        "class C {\n    constructor(public x) { }\n}",
    );
    // Plain constructor — types stripped, no modifiers.
    check_erase(
        "class C { constructor(x: number) {} }",
        "class C {\n    constructor(x) { }\n}",
    );
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/GetAccessor
#[test]
fn get_accessor_return_type_erased() {
    check_erase(
        "class C { get x(): number { return 1; } }",
        "class C {\n    get x() { return 1; }\n}",
    );
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/SetAccessor
#[test]
fn set_accessor_param_type_erased() {
    check_erase(
        "class C { set x(v: number) {} }",
        "class C {\n    set x(v) { }\n}",
    );
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/TaggedTemplateExpression
#[test]
fn tagged_template_expression_type_args_erased() {
    check_erase("f<T>``", "f ``;");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/JsxSelfClosingElement
#[test]
fn jsx_self_closing_element_type_args_erased() {
    let input = "<x<T> />";
    let (ec, source_file) = parse_shared_tsx(input);
    let mut tx = new_type_eraser_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), "<x />;");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/JsxOpeningElement
#[test]
fn jsx_opening_element_type_args_erased() {
    let input = "<x<T>></x>";
    let (ec, source_file) = parse_shared_tsx(input);
    let mut tx = new_type_eraser_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), "<x></x>;");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/IndexSignature
#[test]
fn index_signature_in_class_is_elided() {
    check_erase("class C { [key: string]: number; }", "class C {\n}");
}

// Go: internal/transformers/tstransforms/typeeraser.go:visit/KindGetAccessor,KindSetAccessor (abstract)
// Abstract accessors with no body are elided entirely.
// The `abstract` keyword on the class is also stripped (TypeScript-only).
#[test]
fn abstract_accessors_are_elided() {
    check_erase(
        "abstract class C { abstract get x(): number; }",
        "class C {\n}",
    );
    check_erase(
        "abstract class C { abstract set x(v: number); }",
        "class C {\n}",
    );
}

// Go: internal/transformers/tstransforms/typeeraser.go:visit/KindEnumDeclaration
// Const enums pass through unchanged (preserved for the runtime transformer).
// Regular enums get visitEachChild.
#[test]
fn enum_declaration_handling() {
    // A `const enum` is returned unchanged.
    check_erase("const enum E { A }", "const enum E {\n    A\n}");
    // A regular enum is visited but kept (members have no types to strip).
    check_erase("enum E { A }", "enum E {\n    A\n}");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/UninstantiatedNamespace1..3
#[test]
fn uninstantiated_namespace_is_elided() {
    check_erase("namespace N {}", "");
    check_erase("namespace N { export interface I {} }", "");
    check_erase("namespace N { export type T = U; }", "");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/ExportDeclaration#1..#7
#[test]
fn export_declaration_elision() {
    // Non-type re-exports are preserved.
    check_erase("export * from \"m\";", "export * from \"m\";");
    check_erase("export * as x from \"m\";", "export * as x from \"m\";");
    check_erase("export { x } from \"m\";", "export { x } from \"m\";");
    // Type-only exports are fully elided.
    check_erase("export type * from \"m\";", "");
    check_erase("export type * as x from \"m\";", "");
    check_erase("export type { x } from \"m\";", "");
    // Per-specifier `type` elision — all specifiers type-only → entire export elided.
    check_erase("export { type x } from \"m\";", "");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/ConstructorDeclaration2
#[test]
fn public_constructor_accessibility_stripped() {
    check_erase(
        "class C { public constructor() {} }",
        "class C {\n    constructor() { }\n}",
    );
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/MethodDeclaration2,3
#[test]
fn method_modifiers_erased() {
    check_erase(
        "class C { public m<T>(): U {} }",
        "class C {\n    m() { }\n}",
    );
    check_erase(
        "class C { public static m<T>(): U {} }",
        "class C {\n    static m() { }\n}",
    );
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/GetAccessorDeclaration2,3
#[test]
fn get_accessor_modifiers_erased() {
    check_erase(
        "class C { public get m<T>(): U {} }",
        "class C {\n    get m() { }\n}",
    );
    check_erase(
        "class C { public static get m<T>(): U {} }",
        "class C {\n    static get m() { }\n}",
    );
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/SetAccessorDeclaration2,3
#[test]
fn set_accessor_modifiers_erased() {
    check_erase(
        "class C { public set m<T>(v): U {} }",
        "class C {\n    set m(v) { }\n}",
    );
    check_erase(
        "class C { public static set m<T>(v): U {} }",
        "class C {\n    static set m(v) { }\n}",
    );
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/ImportEqualsDeclaration#3,#4
#[test]
fn import_equals_entity_name() {
    check_erase("import x = y;", "import x = y;");
    check_erase("import type x = y;", "");
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/ImportDeclaration#2..#5,#7
#[test]
fn import_declaration_various_forms() {
    check_erase(
        "import * as x from \"m\"; x;",
        "import * as x from \"m\";\nx;",
    );
    check_erase("import x from \"m\"; x;", "import x from \"m\";\nx;");
    check_erase(
        "import { x } from \"m\"; x;",
        "import { x } from \"m\";\nx;",
    );
    check_erase("import type * as x from \"m\";", "");
    check_erase("import type { x } from \"m\";", "");
}

// Guard: plain JS with no type annotations passes through unchanged.
#[test]
fn no_annotations_unchanged() {
    check_erase(
        "function f(a, b) { return a; }",
        "function f(a, b) { return a; }",
    );
    check_erase("const g = (x) => x;", "const g = (x) => x;");
    check_erase(
        "class C { m() { return 1; } }",
        "class C {\n    m() { return 1; }\n}",
    );
}

// Guard: method overload with no body is elided.
// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/MethodDeclaration#overload
#[test]
fn method_overload_is_elided() {
    check_erase("class C { m(); m() {} }", "class C {\n    m() { }\n}");
}

// Guard: constructor overload with no body is elided.
#[test]
fn constructor_overload_is_elided() {
    check_erase(
        "class C { constructor(); constructor() {} }",
        "class C {\n    constructor() { }\n}",
    );
}

// Go: internal/transformers/tstransforms/typeeraser_test.go:TestTypeEraser/ImportSpecifier
#[test]
fn import_type_specifier_elided() {
    // Per-specifier `type` elision: `type Foo` is removed, `bar` stays.
    check_erase(
        "import { type Foo, bar } from \"m\";",
        "import { bar } from \"m\";",
    );
    // All specifiers are type-only → entire import is elided.
    check_erase("import { type Foo } from \"m\";", "");
    // Side-effect-only import is preserved.
    check_erase("import \"m\";", "import \"m\";");
}
