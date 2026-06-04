use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

fn check_esdecorator(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_es_decorator_transformer(&crate::TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "esdecorator({input:?})");
}

// Go: internal/transformers/estransforms/esdecorator.go:visit
// GUARD: a class with no decorators passes through unchanged.
#[test]
fn no_decorators_class_unchanged() {
    check_esdecorator("class C { m() {} }", "class C {\n    m() { }\n}");
}

// Go: internal/transformers/estransforms/esdecorator.go:visit
// GUARD: a plain variable declaration is unaffected.
#[test]
fn no_decorators_variable_unchanged() {
    check_esdecorator("let x = 1;", "let x = 1;");
}

// Go: internal/transformers/estransforms/esdecorator.go:visit
// GUARD: a non-decorated class with an extends clause passes through.
#[test]
fn no_decorators_derived_class_unchanged() {
    check_esdecorator(
        "class C extends B { m() {} }",
        "class C extends B {\n    m() { }\n}",
    );
}

// Go: internal/transformers/estransforms/esdecorator.go:visitClassDeclaration
// A class decorator: `@dec class C {}` is lowered into an IIFE wrapping the
// class with __esDecorate / __runInitializers calls.
#[test]
fn class_decorator_lowers_to_iife() {
    let input = "@dec class C {}";
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_es_decorator_transformer(&crate::TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    let output = emit(&ec, result, input);
    // Verify key structural elements of the lowered output:
    assert!(
        output.contains("__esDecorate"),
        "output should contain __esDecorate call: {output}"
    );
    assert!(
        output.contains("__runInitializers"),
        "output should contain __runInitializers call: {output}"
    );
    assert!(
        output.contains("_classDecorators"),
        "output should contain _classDecorators variable: {output}"
    );
    assert!(
        output.contains("_classThis"),
        "output should contain _classThis variable: {output}"
    );
    assert!(
        output.contains("typeof Symbol"),
        "output should contain metadata check: {output}"
    );
    // The output is wrapped in `let C = (() => { ... })();`
    assert!(
        output.contains("let C"),
        "output should start with let C declaration: {output}"
    );
    assert!(
        output.contains("(() =>"),
        "output should contain IIFE arrow: {output}"
    );
}

// Go: internal/transformers/estransforms/esdecorator.go:visitClassDeclaration
// A method decorator: `class C { @dec m() {} }` is also lowered into an IIFE.
// The class has a decorated child member so `is_decorated_class_like` returns
// true and the class goes through `transformClassLike`.
//
// The full method-decorator lowering (partialTransformClassElement) generates a
// per-method __esDecorate context with `{ kind: "method", name: "m", ... }`;
// this is DEFER(P5) — this test verifies the IIFE scaffold is produced.
#[test]
fn method_decorator_produces_iife() {
    let input = "class C { @dec m() {} }";
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_es_decorator_transformer(&crate::TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    let output = emit(&ec, result, input);
    // Method decorator on a member makes the class a "decorated class-like":
    // it gets the IIFE wrapping even though there's no class-level decorator.
    assert!(
        output.contains("(() =>"),
        "method-decorated class should produce IIFE: {output}"
    );
    assert!(
        output.contains("let C"),
        "method-decorated class should produce let binding: {output}"
    );
    assert!(
        output.contains("typeof Symbol"),
        "output should contain metadata check: {output}"
    );
    // The method body itself should still appear in the output
    assert!(
        output.contains("m()"),
        "method body should be preserved: {output}"
    );
}

// Go: internal/transformers/estransforms/esdecorator.go:visitClassDeclaration
// When the class has a decorated member but the class is NOT itself decorated
// (no @dec on the class), the IIFE should NOT have _classDecorators/_classThis
// variables (those are only for class-level decorators).
#[test]
fn member_only_decorator_no_class_decorator_vars() {
    let input = "class C { @dec m() {} }";
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_es_decorator_transformer(&crate::TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    let output = emit(&ec, result, input);
    // No class-level decorator → no _classDecorators, _classDescriptor, _classThis
    assert!(
        !output.contains("_classDecorators"),
        "no class decorators var for member-only: {output}"
    );
    assert!(
        !output.contains("_classThis"),
        "no _classThis for member-only: {output}"
    );
}

// Go: internal/transformers/estransforms/esdecorator.go:newESDecoratorTransformer
// When `--experimentalDecorators` is true, the ES decorator transformer is
// skipped entirely (the legacy decorator transformer handles all decorators).
#[test]
fn experimental_decorators_skips_transform() {
    let input = "class C { m() {} }";
    let (ec, source_file) = parse_shared(input);
    let mut opts = crate::TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.experimental_decorators = tsgo_core::tristate::Tristate::True;
    let mut tx = new_es_decorator_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(
        emit(&ec, result, input),
        "class C {\n    m() { }\n}",
        "experimental_decorators"
    );
}
