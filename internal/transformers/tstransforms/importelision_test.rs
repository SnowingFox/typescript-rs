use super::*;
use crate::test_support::{build_reference_resolver, emit, parse_shared};
use crate::tstransforms::typeeraser::new_type_eraser_transformer;
use std::rc::Rc;

// Runs the import-elision transform over `input` (with a scope-correct resolver
// built from the same source) and asserts the emitted JS.
fn check_elision(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let resolver = build_reference_resolver(input);
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let mut tx = new_import_elision_transformer(&opts, resolver);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "elision({input:?})");
}

// Go: importelision.go:ImportElisionTransformer.visit (KindImportSpecifier /
// KindNamedImports / KindImportClause / KindImportDeclaration elision chain)
// An unused named import is elided: the specifier drops, the named-imports
// clause empties, the import clause has nothing left, and the whole import
// declaration is removed (`is_referenced` is false). tsc --module esnext emits
// nothing for this input.
#[test]
fn unused_named_import_is_elided() {
    check_elision("import { x } from \"m\";", "");
}

// Go: importelision.go:ImportElisionTransformer.visit (KindImportSpecifier kept)
// A used named import is preserved: `x` is referenced in value position, so
// `is_referenced` is true and the specifier (and its enclosing import) survive.
#[test]
fn used_named_import_is_kept() {
    check_elision(
        "import { x } from \"m\";\nx;",
        "import { x } from \"m\";\nx;",
    );
}

// Go: importelision.go:isReferencedAliasDeclaration (scope-correct, NOT a name
// match). The only `x` use is shadowed by an inner `var x`, so the import is
// unused and elided. A textual name-match stand-in would wrongly keep it; the
// scope-correct `EmitResolver::is_referenced` correctly drops it.
#[test]
fn shadowed_use_does_not_keep_import_alive() {
    check_elision(
        "import { x } from \"m\";\nfunction f() {\n    var x = 1;\n    x;\n}",
        "function f() {\n    var x = 1;\n    x;\n}",
    );
}

// Go: importelision.go:ImportElisionTransformer.visit (KindNamedImports rebuild)
// Per-specifier drop: only the unreferenced specifier is removed; the
// referenced one and the enclosing import declaration survive (Go's
// `UpdateNamedImports` over the surviving elements).
#[test]
fn unused_specifier_dropped_referenced_specifier_kept() {
    check_elision(
        "import { a, b } from \"m\";\na;",
        "import { a } from \"m\";\na;",
    );
}

// Go: importelision.go:ImportElisionTransformer.visit (KindImportDeclaration,
// `n.ImportClause != nil` guard). A side-effect-only import has no clause and is
// never elided.
#[test]
fn side_effect_only_import_is_kept() {
    check_elision("import \"m\";", "import \"m\";");
}

// Go: importelision.go:ImportElisionTransformer.visit (KindNamespaceImport)
// An unused namespace import is elided (its clause then has nothing left).
#[test]
fn unused_namespace_import_is_elided() {
    check_elision("import * as ns from \"m\";", "");
}

// Go: importelision.go:ImportElisionTransformer.visit (KindNamespaceImport kept)
// A used namespace import survives.
#[test]
fn used_namespace_import_is_kept() {
    check_elision(
        "import * as ns from \"m\";\nns;",
        "import * as ns from \"m\";\nns;",
    );
}

// Go: importelision.go:ImportElisionTransformer.visit (KindImportClause, default
// name). An unused default import is elided; a used one survives.
#[test]
fn unused_default_import_is_elided() {
    check_elision("import d from \"m\";", "");
}

#[test]
fn used_default_import_is_kept() {
    check_elision("import d from \"m\";\nd;", "import d from \"m\";\nd;");
}

// Go: importelision.go:ImportElisionTransformer.visit (KindExportSpecifier /
// KindNamedExports / KindExportDeclaration elision chain). `I` is an interface,
// so the export specifier is not a value alias (`is_value_alias_declaration` is
// false); the specifier drops, the named-exports clause empties, and the whole
// `export { I }` declaration is removed. The interface itself is left untouched
// by import elision (the type eraser, not chained here, would remove it).
#[test]
fn type_only_export_specifier_is_elided() {
    check_elision("interface I {}\nexport { I };", "interface I {\n}");
}

// Go: importelision.go:ImportElisionTransformer.visit (KindExportSpecifier kept)
// `f` is a local function (a value), so `export { f }` is a value alias
// (`is_value_alias_declaration` is true) and the specifier (with its enclosing
// export declaration) is preserved.
#[test]
fn value_export_specifier_is_kept() {
    check_elision(
        "function f() {}\nexport { f };",
        "function f() { }\nexport { f };",
    );
}

// Go: importelision.go:ImportElisionTransformer.visit (KindNamedExports rebuild)
// Per-specifier drop: the type-only `I` specifier is removed while the value
// `f` specifier (and the enclosing export declaration) survive (Go's
// `UpdateNamedExports` over the surviving elements).
#[test]
fn type_only_export_specifier_dropped_value_specifier_kept() {
    check_elision(
        "interface I {}\nfunction f() {}\nexport { I, f };",
        "interface I {\n}\nfunction f() { }\nexport { f };",
    );
}

// Go: importelision.go:ImportElisionTransformer.visit (KindExportDeclaration,
// `n.ExportClause == nil` guard). A bare `export * from "m"` has no export
// clause and is never elided by import elision (re-export wildcard preserved).
#[test]
fn export_star_reexport_is_kept() {
    check_elision("export * from \"m\";", "export * from \"m\";");
}

// Go: importelision.go:ImportElisionTransformer.visit (KindImportEqualsDeclaration,
// external-module form). An unused `import x = require("m")` is an alias that is
// never referenced as a value, so `is_referenced_alias_declaration` is false
// (checker 4ap excludes the binding's own name `x` when scanning) and the whole
// import-equals declaration is dropped.
#[test]
fn unused_import_equals_require_is_elided() {
    check_elision("import x = require(\"m\");", "");
}

// Go: importelision.go:ImportElisionTransformer.visit (KindImportEqualsDeclaration
// kept). A used `import x = require("m")` is referenced as a value, so
// `is_referenced_alias_declaration` is true and the import-equals (and its use)
// survive. Guards against the elision arm over-dropping referenced imports.
#[test]
fn used_import_equals_require_is_kept() {
    check_elision(
        "import x = require(\"m\");\nx;",
        "import x = require(\"m\");\nx;",
    );
}

// Go: importelision.go:ImportElisionTransformer.visit (KindExportAssignment).
// `I` is an interface, so `export = I` does not alias a value
// (`is_value_alias_declaration` is false) and the whole `export =` statement is
// removed. The interface itself is left untouched by import elision (the type
// eraser, not chained here, would remove it).
#[test]
fn type_only_export_equals_is_elided() {
    check_elision("interface I {}\nexport = I;", "interface I {\n}");
}

// Go: importelision.go:ImportElisionTransformer.visit (KindExportAssignment
// kept). `f` is a local function (a value), so `export = f` is a value alias
// (`is_value_alias_declaration` true) and the `export =` statement survives.
// Guards against the elision arm over-dropping value `export =`.
#[test]
fn value_export_equals_is_kept() {
    check_elision(
        "function f() {}\nexport = f;",
        "function f() { }\nexport = f;",
    );
}

// -- Pipeline tests: type eraser + import elision (matching Go's chained transform) --

/// Runs type eraser → import elision over `input` (matching Go's chained
/// `TypeEraserTransformer → ImportElisionTransformer` pipeline) and asserts the
/// emitted JS. The scope-correct resolver is built from the same source.
fn check_elision_pipeline(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let resolver = build_reference_resolver(input);
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let mut te = new_type_eraser_transformer(&opts);
    let after_type_erase = te.transform_source_file(source_file);
    let mut ie = new_import_elision_transformer(&opts, resolver);
    let result = ie.transform_source_file(after_type_erase);
    assert_eq!(emit(&ec, result, input), expected, "pipeline({input:?})");
}

// Go: TestImportElision (chained type eraser + import elision). An inline
// type-only import specifier `import { type Foo }` is dropped by the type
// eraser (the `is_type_only` flag on the specifier); the named-imports clause
// then empties, the import clause has nothing left, and the whole import
// declaration is removed. Matches tsc `--module esnext` output.
#[test]
fn inline_type_only_import_specifier_is_completely_elided() {
    check_elision_pipeline("import { type Foo } from \"bar\";", "");
}

// Go: TestImportElision (chained type eraser + import elision). A mixed import
// `import { Foo, type Bar }` has the `type Bar` specifier stripped by the type
// eraser; `Foo` is referenced as a value, so the import elision keeps it. The
// result is `import { Foo } from "bar";` — only the value binding survives.
#[test]
fn mixed_value_and_type_import_keeps_only_value_specifier() {
    check_elision_pipeline(
        "import { Foo, type Bar } from \"bar\";\nFoo;",
        "import { Foo } from \"bar\";\nFoo;",
    );
}

// Go: TestImportElision (chained type eraser + import elision). A full type-only
// import `import type { Foo }` has the whole import clause elided by the type
// eraser (its `phase_modifier == TypeKeyword`), so the import declaration is
// removed entirely before import elision even runs.
#[test]
fn full_type_only_import_is_completely_elided() {
    check_elision_pipeline("import type { Foo } from \"bar\";", "");
}
