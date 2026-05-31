use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;
use tsgo_core::compileroptions::ModuleKind;

// Lowers `input` under `module: system` and asserts the emitted JS.
fn check_system(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.module = ModuleKind::System;
    let mut tx = new_system_module_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "system({input:?})");
}

// Go: systemmodule.go:transformSourceFile + createSystemModuleBody
// An empty module under `--module system` becomes the bare `System.register`
// wrapper: an empty dependency array, the two generated parameter names
// (`exports_1`/`context_1` via the name generator), the `"use strict"` prologue,
// and a `return { setters: [], execute: function () { } };` with empty setters
// and an empty execute body. The outer module function body is multi-line; the
// return object and the empty execute block stay single-line (the Rust printer
// does not carry the per-node `MultiLine` flag for object literals). Verified
// against tsc --module system.
#[test]
fn empty_module_wraps_in_system_register() {
    check_system(
        "",
        "System.register([], function (exports_1, context_1) {\n    \"use strict\";\n    return { setters: [], execute: function () { } };\n});",
    );
}

// Go: systemmodule.go:transformSourceFile (dependencies) + createSettersArray
// A side-effect `import "m";` contributes the module specifier `"m"` to the
// `System.register` dependency array and a matching (empty) setter function to
// the `setters` array. The setter parameter is `_1` (tsc's
// `createUniqueName("")`); its body is empty because `import "m"` binds no
// names. Verified against tsc --module system (modulo the single-line array
// formatting — the Rust printer does not carry the per-node `MultiLine` flag).
#[test]
fn side_effect_import_adds_dependency_and_setter() {
    check_system(
        "import \"m\";",
        "System.register([\"m\"], function (exports_1, context_1) {\n    \"use strict\";\n    return { setters: [function (_1) { }], execute: function () { } };\n});",
    );
}

// Gate branch: when `module` is not System, the transform is a passthrough.
// Go: systemmodule.go:NewSystemModuleTransformer (module-kind gate)
#[test]
fn non_system_module_kind_is_passthrough() {
    let input = "f();";
    let (ec, source_file) = parse_shared(input);
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    // module defaults to `ModuleKind::None` -> no wrapping.
    let mut tx = new_system_module_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), "f();");
}

// Go: systemmodule.go:createSystemModuleBody (execute body statements)
// A top-level value statement (`f();`) is moved into the `execute` function
// body, in source order. The setters array stays empty (no imports). The
// execute block renders single-line because the Rust printer does not carry the
// per-node `MultiLine` flag (tsc emits it multi-line). Verified against tsc
// --module system (modulo that whitespace).
#[test]
fn top_level_value_statement_moves_into_execute_body() {
    check_system(
        "f();",
        "System.register([], function (exports_1, context_1) {\n    \"use strict\";\n    return { setters: [], execute: function () { f(); } };\n});",
    );
}
