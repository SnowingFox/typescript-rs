use super::*;
use crate::test_support::emit_after;

// Go: internal/printer/helpers.go:EmitHelper (identity)
// The `is` identity check distinguishes distinct helper definitions.
#[test]
fn helper_identity_by_name() {
    assert!(SET_FUNCTION_NAME_HELPER.is(&SET_FUNCTION_NAME_HELPER));
}

// Go: internal/printer/helpers.go:compareEmitHelpers
// Lower priority sorts earlier; `None` priority sorts last.
#[test]
fn compare_orders_by_priority_then_none_last() {
    use core::cmp::Ordering;
    // create_binding(1) before awaiter(5)
    assert_eq!(
        compare_emit_helpers(&CREATE_BINDING_HELPER, &AWAITER_HELPER),
        Ordering::Less
    );
    // awaiter(5) before set_function_name(None)
    assert_eq!(
        compare_emit_helpers(&AWAITER_HELPER, &SET_FUNCTION_NAME_HELPER),
        Ordering::Less
    );
    // None sorts after an explicit priority
    assert_eq!(
        compare_emit_helpers(&SET_FUNCTION_NAME_HELPER, &AWAITER_HELPER),
        Ordering::Greater
    );
    // equal priorities (both None) compare equal
    assert_eq!(
        compare_emit_helpers(&REST_HELPER, &IMPORT_DEFAULT_HELPER),
        Ordering::Equal
    );
}

// Go: internal/printer/printer.go:emitHelpers
// Tracer bullet: a helper attached to the source file is emitted, verbatim, in
// the module prologue (before the statements).
#[test]
fn requested_helper_definition_emitted_in_prologue() {
    let text = emit_after("const a = b;", |ec, source_file| {
        ec.add_emit_helper(source_file, &SET_FUNCTION_NAME_HELPER);
    });
    let expected = "var __setFunctionName = (this && this.__setFunctionName) || function (f, name, prefix) {\n    if (typeof name === \"symbol\") name = name.description ? \"[\".concat(name.description, \"]\") : \"\";\n    return Object.defineProperty(f, \"name\", { configurable: true, value: prefix ? \"\".concat(prefix, \" \", name) : name });\n};\nconst a = b;\n";
    assert_eq!(text, expected);
}

// Go: internal/printer/printer.go:emitHelpers (compareEmitHelpers sort)
// Multiple helpers are emitted in priority order regardless of attach order:
// `__awaiter` (priority 5) precedes `__setFunctionName` (no priority).
#[test]
fn prologue_emits_helpers_in_priority_order() {
    let text = emit_after("const a = b;", |ec, source_file| {
        ec.add_emit_helper(source_file, &SET_FUNCTION_NAME_HELPER);
        ec.add_emit_helper(source_file, &AWAITER_HELPER);
    });
    let awaiter = text.find("var __awaiter").expect("awaiter emitted");
    let set_name = text
        .find("var __setFunctionName")
        .expect("setFunctionName emitted");
    assert!(awaiter < set_name, "awaiter should precede setFunctionName");
}
