use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;
use tsgo_core::compileroptions::ModuleKind;

// Lowers `input` under `module: commonjs` and asserts the emitted JS.
fn check_cjs(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.module = ModuleKind::CommonJs;
    let mut tx = new_common_js_module_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "cjs({input:?})");
}

// Go: commonjsmodule.go:visitExportAssignment (export default) + createUnderscoreUnderscoreESModule
// `export default e` becomes `exports.default = e`, with the `__esModule` marker
// emitted at the top because the module has exports.
#[test]
fn export_default_becomes_exports_default_with_marker() {
    check_cjs(
        "export default 1;",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nexports.default = 1;",
    );
}

// Go: commonjsmodule.go:visitTopLevelVariableStatement / transformInitializedVariable
// `export const y = 1` lowers to `exports.y = 1`.
#[test]
fn export_const_becomes_exports_assignment() {
    check_cjs(
        "export const y = 1;",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nexports.y = 1;",
    );
}

// Go: commonjsmodule.go:appendExportsOfDeclaration / createExportExpression (export { x })
// `export { x }` (no module specifier) lowers to `exports.x = x;`.
#[test]
fn local_named_export_becomes_exports_assignment() {
    check_cjs(
        "const x = 1; export { x };",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nconst x = 1;\nexports.x = x;",
    );
}

// Go: commonjsmodule.go:visitTopLevelImportDeclaration + getHelperExpressionForImport (default)
// `import d from "m"` uses the `__importDefault` interop helper; uses of `d`
// become `m_1.default`.
#[test]
fn default_import_uses_import_default_helper() {
    check_cjs(
        "import d from \"m\"; d;",
        "var __importDefault = (this && this.__importDefault) || function (mod) {\n    return (mod && mod.__esModule) ? mod : { \"default\": mod };\n};\nconst m_1 = __importDefault(require(\"m\"));\nm_1.default;",
    );
}

// Emits `input` under `module: commonjs` (no trailing-newline trim handled by `emit`).
fn emit_cjs(input: &str) -> String {
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

// Go: commonjsmodule.go:visitTopLevelImportDeclaration + getHelperExpressionForImport (namespace)
// `import * as ns from "m"` uses the `__importStar` interop helper (+ its
// `__createBinding`/`__setModuleDefault` deps); uses of `ns` become `m_1`.
#[test]
fn namespace_import_uses_import_star_helper() {
    let text = emit_cjs("import * as ns from \"m\"; ns;");
    assert!(
        text.contains("var __importStar ="),
        "import star helper emitted: {text}"
    );
    assert!(
        text.ends_with("const m_1 = __importStar(require(\"m\"));\nm_1;"),
        "namespace import lowering: {text}"
    );
}

// Go: commonjsmodule.go:visitTopLevelExportDeclaration (export *) + NewExportStarHelper
// `export * from "m"` lowers to `__exportStar(require("m"), exports);` (+ helper).
#[test]
fn export_star_uses_export_star_helper() {
    let text = emit_cjs("export * from \"m\";");
    assert!(
        text.contains("var __exportStar ="),
        "export star helper emitted: {text}"
    );
    assert!(
        text.contains("__exportStar(require(\"m\"), exports);"),
        "export star lowering: {text}"
    );
}

// Go: internal/transformers/moduletransforms/commonjsmodule.go (named import + use)
// End-to-end validation of 6e-2: under `module: commonjs`, a named import becomes
// a `require` binding and the imported use `x` is rewritten to `m_1.x` via the
// new emit-substitution infrastructure (track 1) + compilerOptions gate (track 2).
#[test]
fn named_import_and_use_lower_to_require_and_member_access() {
    let input = "import { x } from \"m\"; x;";
    let (ec, source_file) = parse_shared(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.module = ModuleKind::CommonJs;
    let mut tx = new_common_js_module_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(
        emit(&ec, result, input),
        "const m_1 = require(\"m\");\nm_1.x;"
    );
}

// Track 2 branch: when `module` is not CommonJS, the transform is a passthrough.
#[test]
fn non_commonjs_module_kind_is_passthrough() {
    let input = "import { x } from \"m\"; x;";
    let (ec, source_file) = parse_shared(input);
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    // module defaults to `ModuleKind::None` -> no lowering.
    let mut tx = new_common_js_module_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), "import { x } from \"m\";\nx;");
}
