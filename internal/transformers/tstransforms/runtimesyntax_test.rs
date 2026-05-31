use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// Runs the runtime-syntax transformer over `input` and asserts the emitted JS.
fn check_lowered(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_runtime_syntax_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "lowered({input:?})");
}

// Go: internal/transformers/tstransforms/runtimesyntax.go:visitEnumDeclaration
// Tracer bullet: a non-const enum with auto-numbered members lowers to the
// merged `var E;` + IIFE form, each member assigned its ordinal with the
// numeric reverse mapping (`E[E["A"] = 0] = "A";`).
#[test]
fn auto_numbered_enum_lowers_to_iife() {
    check_lowered(
        "enum E { A, B }",
        "var E;\n(function (E) {\n    E[E[\"A\"] = 0] = \"A\";\n    E[E[\"B\"] = 1] = \"B\";\n})(E || (E = {}));",
    );
}

// Go: internal/transformers/tstransforms/runtimesyntax.go:transformEnumMember
// An explicit numeric initializer sets the member's value and auto-incrementing
// continues from it (`A = 5` then `B` is 6).
#[test]
fn explicit_numeric_initializer_sets_value_and_continues_autonumber() {
    check_lowered(
        "enum E { A = 5, B }",
        "var E;\n(function (E) {\n    E[E[\"A\"] = 5] = \"A\";\n    E[E[\"B\"] = 6] = \"B\";\n})(E || (E = {}));",
    );
}

// Go: internal/transformers/tstransforms/runtimesyntax.go:transformEnumMember
// A string-initialized member uses the `E["X"] = "v"` form *without* the
// numeric reverse mapping (string enums are not reverse-mappable).
#[test]
fn string_initialized_member_omits_reverse_mapping() {
    check_lowered(
        "enum E { X = \"v\" }",
        "var E;\n(function (E) {\n    E[\"X\"] = \"v\";\n})(E || (E = {}));",
    );
}

// Go: internal/transformers/tstransforms/runtimesyntax.go:shouldEmitEnumDeclaration
// With `preserveConstEnums` off (the default), a `const enum` declaration is
// omitted entirely (no runtime form). Member-reference *inlining* (`E.A` -> the
// literal) is a separate, deferred concern (the inliners stage; blocked-by:
// checker constant evaluation).
#[test]
fn const_enum_is_omitted() {
    check_lowered("const enum E { A }", "");
}

// Go: internal/transformers/tstransforms/runtimesyntax.go:visitModuleDeclaration
// Tracer bullet: an instantiated namespace lowers to the merged `var N;` + IIFE
// form, with an exported `const x = 1` becoming the namespace-qualified
// assignment `N.x = 1;`.
#[test]
fn instantiated_namespace_lowers_to_iife() {
    check_lowered(
        "namespace N { export const x = 1; }",
        "var N;\n(function (N) {\n    N.x = 1;\n})(N || (N = {}));",
    );
}

// Go: internal/transformers/tstransforms/runtimesyntax.go:shouldEmitModuleDeclaration
// An uninstantiated (type-only) namespace has no runtime form and is omitted
// entirely.
#[test]
fn uninstantiated_namespace_is_omitted() {
    check_lowered("namespace N { interface I {} }", "");
}
