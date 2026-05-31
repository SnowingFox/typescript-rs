use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;
use tsgo_core::compileroptions::ModuleKind;

// Transforms `input` under `module: <module>` through the ES module transformer
// and asserts the emitted JS.
fn check_esm(input: &str, module: ModuleKind, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.module = module;
    let mut tx = new_es_module_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "esm({input:?})");
}

// Go: esmodule.go:NewESModuleTransformer + visitSourceFile (entry wiring)
// Tracer bullet: a value `import { x } from "m"; x;` under `module: esnext` is
// preserved identically — proving the ESM transformer entry is wired and that
// value import/export passes through (ESM keeps import/export syntax).
#[test]
fn value_import_and_use_preserved_under_esnext() {
    check_esm(
        "import { x } from \"m\"; x;",
        ModuleKind::EsNext,
        "import { x } from \"m\";\nx;",
    );
}

// Go: esmodule.go:visitExportAssignment (export= elision) + visitSourceFile
// (createEmptyImports). `export = x;` is illegal under `--module es2015/esnext`,
// so the transform elides it; the file is still an external module with no
// remaining indicator, so an empty `export {};` is appended.
#[test]
fn export_equals_is_elided_and_empty_imports_appended() {
    check_esm("export = x;", ModuleKind::EsNext, "export {};");
}

// Go: esmodule.go:visitImportEqualsDeclaration (emit module kind < Node16 → nil)
// `import x = require("m")` is not legal in an ES module target below Node16, so
// the transform elides it; the file is still an external module with no
// remaining indicator, so an empty `export {};` is appended.
#[test]
fn import_equals_require_is_elided_and_empty_imports_appended() {
    check_esm(
        "import x = require(\"m\");",
        ModuleKind::EsNext,
        "export {};",
    );
}

// Go: esmodule.go:visitExportAssignment (!IsExportEquals → VisitEachChild)
// `export default e` is preserved under an ES module target.
#[test]
fn export_default_is_preserved() {
    check_esm("export default 1;", ModuleKind::EsNext, "export default 1;");
}

// Go: esmodule.go:visitExportDeclaration (ModuleSpecifier == nil → return node)
// A local `export { x }` (no module specifier) is preserved.
#[test]
fn local_named_export_is_preserved() {
    check_esm(
        "const x = 1; export { x };",
        ModuleKind::EsNext,
        "const x = 1;\nexport { x };",
    );
}

// Go: esmodule.go:visitExportDeclaration (Module > ES2015 → UpdateExportDeclaration)
// `export * from "m"` is preserved under `--module esnext`.
#[test]
fn export_star_is_preserved() {
    check_esm(
        "export * from \"m\";",
        ModuleKind::EsNext,
        "export * from \"m\";",
    );
}

// Go: esmodule.go:visitExportDeclaration (re-export, Module > ES2015 → preserve)
// `export { x } from "m"` re-export is preserved under `--module esnext`.
#[test]
fn re_export_is_preserved() {
    check_esm(
        "export { x } from \"m\";",
        ModuleKind::EsNext,
        "export { x } from \"m\";",
    );
}

// Go: esmodule.go:visitImportDeclaration (RewriteRelativeImportExtensions off → node)
// A namespace import `import * as ns from "m"` and its value use are preserved.
#[test]
fn namespace_import_is_preserved() {
    check_esm(
        "import * as ns from \"m\"; ns;",
        ModuleKind::EsNext,
        "import * as ns from \"m\";\nns;",
    );
}

// Go: esmodule.go:visitSourceFile guard
// `!(IsExternalModule || GetIsolatedModules())`. A plain script (no import/export
// indicator) is returned unchanged — no spurious `export {};` is appended.
#[test]
fn non_module_file_is_passthrough() {
    check_esm("const x = 1;", ModuleKind::EsNext, "const x = 1;");
}

// Go: esmodule.go:visitSourceFile (es2015 target preserves value import/export)
// The tracer also holds under `--module es2015`.
#[test]
fn value_import_and_use_preserved_under_es2015() {
    check_esm(
        "import { x } from \"m\"; x;",
        ModuleKind::Es2015,
        "import { x } from \"m\";\nx;",
    );
}

// Go: esmodule.go:visitExportDeclaration (Module <= ES2015 + IsNamespaceExport → rewrite)
// `export * as ns from "m"` is not legal syntax under `--module es2015`, so the
// transform rewrites it to a namespace import bound to a generated name followed
// by a named re-export of that name. The synthesized name comes from
// `new_generated_name_for_node(ns)` (6p) → `ns_1`.
#[test]
fn namespace_reexport_rewrites_to_import_and_named_export_under_es2015() {
    check_esm(
        "export * as ns from \"m\";",
        ModuleKind::Es2015,
        "import * as ns_1 from \"m\";\nexport { ns_1 as ns };",
    );
}

// Go: esmodule.go:visitExportDeclaration (Module > ES2015 → UpdateExportDeclaration)
// `export * as ns from "m"` is legal syntax in `--module esnext`, so it is
// preserved verbatim — the namespace-export rewrite is gated to `Module <=
// ES2015`.
#[test]
fn namespace_reexport_is_preserved_under_esnext() {
    check_esm(
        "export * as ns from \"m\";",
        ModuleKind::EsNext,
        "export * as ns from \"m\";",
    );
}

// Go: esmodule.go:visitExportDeclaration (IsExportNamespaceAsDefaultDeclaration)
// When the namespace export name is `default`, the named re-export becomes an
// `export default <gen>` assignment instead of `export { <gen> as default }`.
// tsc `--module es2015`: `import * as default_1 from "m";\nexport default default_1;`.
#[test]
fn namespace_reexport_as_default_rewrites_to_export_default_under_es2015() {
    check_esm(
        "export * as default from \"m\";",
        ModuleKind::Es2015,
        "import * as default_1 from \"m\";\nexport default default_1;",
    );
}
