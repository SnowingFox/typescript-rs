use super::*;
use crate::moduletransforms::commonjsmodule::new_common_js_module_transformer;
use crate::moduletransforms::esmodule::new_es_module_transformer;
use crate::test_support::{emit, parse_shared, parse_shared_named};
use std::rc::Rc;
use tsgo_core::compileroptions::ModuleKind;

// Transforms `input` under `module` through the implied-module transformer and
// asserts the emitted JS.
fn check_implied(input: &str, module: ModuleKind, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.module = module;
    let mut tx = new_implied_module_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "implied({input:?})");
}

// Transforms `input` under `module: commonjs` through the CommonJS transformer
// directly, returning the emitted JS the implied transformer must match when it
// delegates to CJS.
fn cjs_output(input: &str) -> String {
    let (ec, source_file) = parse_shared(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.module = ModuleKind::CommonJs;
    let mut tx = new_common_js_module_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    emit(&ec, result, input)
}

// Transforms `input` under `module` through the ES module transformer directly,
// returning the emitted JS the implied transformer must match when it delegates
// to ESM.
fn esm_output(input: &str, module: ModuleKind) -> String {
    let (ec, source_file) = parse_shared(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.module = module;
    let mut tx = new_es_module_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    emit(&ec, result, input)
}

// Go: impliedmodule.go:visitSourceFile (format < ES2015 → NewCommonJSModuleTransformer)
// Tracer bullet: under `module: commonjs` the implied transformer delegates to
// the CommonJS transform, so `export default 1;` lowers to the `__esModule`
// marker + `exports.default = 1;` (identical to the CJS transform's output).
#[test]
fn commonjs_source_delegates_to_common_js_transform() {
    let input = "export default 1;";
    check_implied(input, ModuleKind::CommonJs, &cjs_output(input));
}

// Go: impliedmodule.go:visitSourceFile (format >= ES2015 → NewESModuleTransformer)
// Under `module: esnext` the implied transformer delegates to the ES module
// transform. `export = x;` is illegal under an ES module target, so ESM elides
// it and appends an empty `export {};` — proving the dispatch routed to ESM
// (the CommonJS transform would have left `export = x;` untouched here, since it
// only lowers under `module: commonjs`).
#[test]
fn esnext_source_delegates_to_es_module_transform() {
    let input = "export = x;";
    check_implied(
        input,
        ModuleKind::EsNext,
        &esm_output(input, ModuleKind::EsNext),
    );
}

// Go: impliedmodule.go:visitSourceFile (format >= ES2015 → NewESModuleTransformer)
// The dispatch predicate is `format >= ModuleKind::ES2015`, not an `== EsNext`
// equality: `module: es2015` is also an ES module target, so `export = x;` is
// elided to `export {};` (delegated to ESM) rather than left untouched by CJS.
#[test]
fn es2015_source_routes_to_es_module_transform() {
    let input = "export = x;";
    check_implied(
        input,
        ModuleKind::Es2015,
        &esm_output(input, ModuleKind::Es2015),
    );
}

// Go: impliedmodule.go:visitSourceFile (`node.IsDeclarationFile` → return node)
// A declaration file is returned unchanged: the dispatch is skipped entirely.
// `export = x;` in a `.d.ts` stays `export = x;` (without the guard the CJS
// transform would lower it to `module.exports = x;`).
#[test]
fn declaration_file_is_returned_unchanged() {
    let input = "export = x;";
    let (ec, source_file) = parse_shared_named(input, "/main.d.ts");
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.module = ModuleKind::CommonJs;
    let mut tx = new_implied_module_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), "export = x;");
}
