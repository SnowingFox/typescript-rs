use super::*;
use crate::test_support::{build_reference_resolver, emit, parse_shared};
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

// Go: commonjsmodule.go:transformCommonJSModule (exportedNames void-0 init) +
// visitTopLevelVariableStatement. `export const y = 1` first emits the
// export-name initializer `exports.y = void 0;` (Go zero-initializes every
// exported name right after the `__esModule` marker), then `exports.y = 1`.
// Verified against tsc --module commonjs.
#[test]
fn export_const_becomes_exports_assignment() {
    check_cjs(
        "export const y = 1;",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nexports.y = void 0;\nexports.y = 1;",
    );
}

// Go: commonjsmodule.go:transformCommonJSModule (exportedNames chunk loop)
// Multiple exported names share ONE chained zero-initializer statement, built by
// folding each name as the outer assignment target, so source order `a, b`
// emits `exports.b = exports.a = void 0;` (the last name is outermost).
// Verified against tsc --module commonjs.
#[test]
fn multiple_exported_names_share_chained_void_zero_init() {
    check_cjs(
        "export const a = 1;\nexport const b = 2;",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nexports.b = exports.a = void 0;\nexports.a = 1;\nexports.b = 2;",
    );
}

// Go: commonjsmodule.go:appendExportsOfDeclaration / createExportExpression (export { x })
// `export { x }` (no module specifier) lowers to `exports.x = x;`, preceded by
// the `exports.x = void 0;` export-name initializer. Verified against tsc.
#[test]
fn local_named_export_becomes_exports_assignment() {
    check_cjs(
        "const x = 1; export { x };",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nexports.x = void 0;\nconst x = 1;\nexports.x = x;",
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

// Go: commonjsmodule.go:visitTopLevelExportDeclaration (re-export) + createExportExpression (liveBinding)
// `export { x } from "m"` becomes the `exports.x = void 0;` export-name
// initializer, then `var m_1 = require("m");` plus a live-binding getter
// `Object.defineProperty(exports, "x", { ... get: ... m_1.x })`. The
// `__esModule` marker is emitted because the module has exports. Verified
// against tsc (re-exported names are zero-initialized like local ones).
#[test]
fn re_export_named_binding_lowers_to_require_and_live_binding_getter() {
    check_cjs(
        "export { x } from \"m\";",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nexports.x = void 0;\nvar m_1 = require(\"m\");\nObject.defineProperty(exports, \"x\", { enumerable: true, get: function () { return m_1.x; } });",
    );
}

// Go: commonjsmodule.go:visitTopLevelExportDeclaration (re-export rename)
// `export { a as b } from "m"` exports `b` (so the initializer is `exports.b =
// void 0;`) whose live-binding getter reads the re-exported source member
// `m_1.a` (coverage of the propertyName path). Verified against tsc.
#[test]
fn re_export_renamed_binding_uses_property_name_for_value() {
    check_cjs(
        "export { a as b } from \"m\";",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nexports.b = void 0;\nvar m_1 = require(\"m\");\nObject.defineProperty(exports, \"b\", { enumerable: true, get: function () { return m_1.a; } });",
    );
}

// Go: commonjsmodule.go:visitTopLevelImportDeclaration + getImportNeedsImportStarHelper
// `import d, { x } from "m"` (combined default + named) requires the `__importStar`
// interop helper (default import mixed with non-default named refs); uses of `d`
// become `m_1.default` and uses of `x` become `m_1.x`.
#[test]
fn combined_default_and_named_import_uses_import_star_helper() {
    let text = emit_cjs("import d, { x } from \"m\"; d; x;");
    assert!(
        text.contains("var __importStar ="),
        "import star helper emitted: {text}"
    );
    assert!(
        text.ends_with("const m_1 = __importStar(require(\"m\"));\nm_1.default;\nm_1.x;"),
        "combined default + named import lowering: {text}"
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

// Go: commonjsmodule.go:appendExportEqualsIfNeeded + visitExportEquals
// `export = x` lowers to `module.exports = x;`. Because `export =` is a
// CommonJS-style whole-module export, the `__esModule` marker is SUPPRESSED.
#[test]
fn export_equals_becomes_module_exports_without_marker() {
    check_cjs("export = x;", "module.exports = x;");
}

// Go: commonjsmodule.go:visitTopLevelFunctionDeclaration + appendExportsOfClassOrFunctionDeclaration
// `export function f() {}` keeps the local `function f() {}` (export modifier
// stripped) and appends `exports.f = f;`. Function declarations are hoisted, so
// Go emits the `exports.f = f;` assignment ahead of the declaration. The
// `__esModule` marker is emitted because the module has a value export.
#[test]
fn exported_function_declaration_keeps_decl_and_assigns_export() {
    check_cjs(
        "export function f() {}",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nexports.f = f;\nfunction f() { }",
    );
}

// Go: commonjsmodule.go:visitTopLevelClassDeclaration + appendExportsOfClassOrFunctionDeclaration
// `export class C {}` keeps the local `class C {}` (export modifier stripped)
// followed by `exports.C = C;`. Unlike functions, class declarations are not
// hoisted, so Go emits the export assignment after the declaration, in place.
// The class name is an exported name, so it is also zero-initialized
// (`exports.C = void 0;`) ahead of the declaration. Verified against tsc; note
// `export function f` does NOT get a void-0 init (functions are tracked
// separately from exported names), only classes/consts/named-exports do.
#[test]
fn exported_class_declaration_keeps_decl_and_assigns_export() {
    check_cjs(
        "export class C {}",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nexports.C = void 0;\nclass C {\n}\nexports.C = C;",
    );
}

// Go: commonjsmodule.go:visitTopLevelFunctionDeclaration + appendExportsOfClassOrFunctionDeclaration (default)
// `export default function f() {}` keeps the local `function f() {}` and exports
// it under the `default` name: `exports.default = f;` (hoisted ahead of the
// declaration, like other exported functions).
#[test]
fn exported_default_function_declaration_assigns_default_export() {
    check_cjs(
        "export default function f() {}",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nexports.default = f;\nfunction f() { }",
    );
}

// Go: commonjsmodule.go:visitTopLevelClassDeclaration + appendExportsOfClassOrFunctionDeclaration (default)
// `export default class C {}` keeps the local `class C {}` followed by
// `exports.default = C;` (classes are not hoisted, so the assignment trails).
#[test]
fn exported_default_class_declaration_assigns_default_export() {
    check_cjs(
        "export default class C {}",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nclass C {\n}\nexports.default = C;",
    );
}

// Go: commonjsmodule.go:visitTopLevelImportEqualsDeclaration (non-export case)
// `import x = require("m")` lowers to `const x = require("m");` (for emit module
// kind below Node16). The bound `x` is a real local `const`, so a use of `x` is
// left unchanged (no member-access rewrite). Verified against tsc --module
// commonjs (`const x = require("./m"); x;`). The import-only module emits no
// `__esModule` marker under the port's value-export simplification.
#[test]
fn import_equals_require_lowers_to_const_require() {
    check_cjs(
        "import x = require(\"m\"); x;",
        "const x = require(\"m\");\nx;",
    );
}

// Builds and emits a source file containing `const p = import(<arg>);` under
// `module: commonjs`, running it through the public transformer entry. The
// parser does not yet parse a dynamic `import(...)` call head (deferred in
// `internal/parser`, out of this round's scope), so the import-call node is
// constructed directly: it is exactly the AST the parser would produce — a
// `CallExpression` whose callee is the `import` keyword expression. `arg` is
// `Some(module)` for `import("module")` or `None` for the no-argument `import()`.
fn emit_cjs_dynamic_import(arg: Option<&str>) -> String {
    let (ec_rc, template) = parse_shared("const p = 0;");
    let (file_name, script_kind, language_variant, end_of_file_token) = {
        let ec = ec_rc.borrow();
        match ec.arena().data(template) {
            NodeData::SourceFile(d) => (
                d.file_name.clone(),
                d.script_kind,
                d.language_variant,
                d.end_of_file_token,
            ),
            _ => unreachable!("template is a source file"),
        }
    };
    let source_file = {
        let mut ec = ec_rc.borrow_mut();
        let arena = ec.arena_mut();
        let import_head = arena.new_keyword_expression(Kind::ImportKeyword);
        let mut args = Vec::new();
        if let Some(module) = arg {
            args.push(arena.new_string_literal(module, tsgo_ast::TokenFlags::NONE));
        }
        let call = arena.new_call_expression(
            import_head,
            None,
            None,
            NodeList::new(args),
            NodeFlags::NONE,
        );
        let name = arena.new_identifier("p");
        let decl = arena.new_variable_declaration(name, None, None, Some(call));
        let list = arena.new_variable_declaration_list(NodeList::new(vec![decl]));
        arena.add_flags(list, NodeFlags::CONST);
        let stmt = arena.new_variable_statement(None, list);
        arena.new_source_file(
            &file_name,
            script_kind,
            language_variant,
            NodeList::new(vec![stmt]),
            end_of_file_token,
        )
    };
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec_rc)),
        ..Default::default()
    };
    opts.compiler_options.module = ModuleKind::CommonJs;
    let mut tx = new_common_js_module_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    emit(&ec_rc, result, "")
}

// Go: commonjsmodule.go:visitImportCallExpression + createImportCallExpressionCommonJS
// A dynamic `import("m")` under `module: commonjs` lowers to
// `Promise.resolve().then(() => __importStar(require("m")))`. Go (and upstream
// typescript-go) ALWAYS wraps `require(...)` in the `__importStar` helper here,
// independent of `esModuleInterop` (`createImportCallExpressionCommonJS` calls
// `NewImportStarHelper` unconditionally). The argument is a simple inlineable
// expression (a string literal), so it is inlined into `require(...)` with no
// `Promise.resolve(`${x}`)` template / `(s) =>` evaluation wrapper.
#[test]
fn dynamic_import_lowers_to_promise_resolve_then_import_star_require() {
    let text = emit_cjs_dynamic_import(Some("m"));
    assert!(
        text.contains("var __importStar ="),
        "import star helper emitted: {text}"
    );
    assert!(
        text.ends_with("const p = Promise.resolve().then(() => __importStar(require(\"m\")));"),
        "dynamic import lowering: {text}"
    );
}

// Go: commonjsmodule.go:createImportCallExpressionCommonJS (arg == nil branch)
// The no-argument dynamic `import()` lowers to
// `Promise.resolve().then(() => __importStar(require()))` — Go builds the
// `require` call with an empty argument list when `arg` is nil (and still wraps
// in `__importStar`).
#[test]
fn no_argument_dynamic_import_lowers_to_require_with_no_args() {
    let text = emit_cjs_dynamic_import(None);
    assert!(
        text.ends_with("const p = Promise.resolve().then(() => __importStar(require()));"),
        "no-arg dynamic import lowering: {text}"
    );
}

// Lowers `input` under `module: commonjs` with a scope-correct reference
// resolver (built from the same source, node ids aligned) and asserts the
// emitted JS. Drives `new_common_js_module_transformer_with_resolver`.
fn check_cjs_scoped(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let resolver = build_reference_resolver(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.module = ModuleKind::CommonJs;
    let mut tx = new_common_js_module_transformer_with_resolver(&opts, resolver);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "cjs_scoped({input:?})");
}

// Go: commonjsmodule.go:visitExpressionIdentifier -> GetReferencedImportDeclaration
// 6ai slice 1: with a scope-correct resolver, a module-level use of a named
// import is rewritten to a qualified member access on the require-alias. The
// use `x` resolves to the import specifier's symbol, so it becomes `m_1.x`.
#[test]
fn scoped_named_import_use_rewrites_to_member_access() {
    check_cjs_scoped(
        "import { x } from \"m\";\nx;",
        "const m_1 = require(\"m\");\nm_1.x;",
    );
}

// Go: commonjsmodule.go:visitExpressionIdentifier (scope-correct, via
// GetReferencedImportDeclaration over the resolved symbol — NOT a name match).
// 6ai slice 2 (the headline `resolve_reference` property): the only `x` use is
// shadowed by an inner `var x`, so it resolves to the local, not the import,
// and is left unchanged. A textual name-match stand-in would wrongly rewrite it
// to `m_1.x`; the scope-correct resolver keeps it bare. The import is still
// lowered to `const m_1 = require("m");` (CommonJS does not elide unused
// imports — that is import elision's job).
#[test]
fn scoped_shadowed_use_is_not_rewritten() {
    check_cjs_scoped(
        "import { x } from \"m\";\nfunction f() {\n    var x = 1;\n    x;\n}",
        "const m_1 = require(\"m\");\nfunction f() {\n    var x = 1;\n    x;\n}",
    );
}

// Go: commonjsmodule.go:visitExpressionIdentifier (use nested in a call arg)
// 6ai slice 3: a use of an imported binding nested inside a call argument is
// rewritten in place: `console.log(x)` -> `console.log(m_1.x)`. The `console`
// and `log` identifiers resolve to nothing in the module (not imports), so they
// are left alone; only `x` is rewritten.
#[test]
fn scoped_import_use_inside_call_argument_is_rewritten() {
    check_cjs_scoped(
        "import { x } from \"m\";\nconsole.log(x);",
        "const m_1 = require(\"m\");\nconsole.log(m_1.x);",
    );
}

// Go: commonjsmodule.go:visitExpressionIdentifier -> GetReferencedImportDeclaration
// 6aj slice 1: with a scope-correct resolver, a module-level use of a *default*
// import is rewritten to a qualified `default` member access on the
// require-alias. The use `d` resolves to the import clause's symbol, so it
// becomes `m_1.default` (the same alias the import lowering already emits via
// the `__importDefault` interop helper).
#[test]
fn scoped_default_import_use_rewrites_to_default_member() {
    check_cjs_scoped(
        "import d from \"m\";\nd;",
        "var __importDefault = (this && this.__importDefault) || function (mod) {\n    return (mod && mod.__esModule) ? mod : { \"default\": mod };\n};\nconst m_1 = __importDefault(require(\"m\"));\nm_1.default;",
    );
}

// Go: commonjsmodule.go:visitExpressionIdentifier -> GetReferencedImportDeclaration
// 6aj slice 2: with a scope-correct resolver, a module-level use of a
// *namespace* import is rewritten to the bare require-alias identifier. The use
// `ns` resolves to the namespace-import's symbol, whose binding carries no
// member, so `ns` becomes `m_1` (the alias the `__importStar` lowering emits).
#[test]
fn scoped_namespace_import_use_rewrites_to_bare_alias() {
    let (ec, source_file) = parse_shared("import * as ns from \"m\";\nns;");
    let resolver = build_reference_resolver("import * as ns from \"m\";\nns;");
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.module = ModuleKind::CommonJs;
    let mut tx = new_common_js_module_transformer_with_resolver(&opts, resolver);
    let result = tx.transform_source_file(source_file);
    let text = emit(&ec, result, "import * as ns from \"m\";\nns;");
    assert!(
        text.contains("var __importStar ="),
        "import star helper emitted: {text}"
    );
    assert!(
        text.ends_with("const m_1 = __importStar(require(\"m\"));\nm_1;"),
        "scoped namespace import use rewrite: {text}"
    );
}

// Go: commonjsmodule.go:visitExpressionIdentifier (scope-correct, via
// GetReferencedImportDeclaration over the resolved symbol — NOT a name match).
// 6aj slice 3 (the headline `resolve_reference` property for the default form):
// the only `d` use is shadowed by an inner `var d`, so it resolves to the
// local, not the default import, and is left unchanged. A textual name-match
// stand-in (the no-resolver fallback) would wrongly rewrite it to `m_1.default`;
// the scope-correct resolver keeps it bare. The import is still lowered to the
// `__importDefault(require("m"))` alias (CommonJS does not elide unused imports).
#[test]
fn scoped_shadowed_default_import_use_is_not_rewritten() {
    check_cjs_scoped(
        "import d from \"m\";\nfunction f() {\n    var d = 1;\n    d;\n}",
        "var __importDefault = (this && this.__importDefault) || function (mod) {\n    return (mod && mod.__esModule) ? mod : { \"default\": mod };\n};\nconst m_1 = __importDefault(require(\"m\"));\nfunction f() {\n    var d = 1;\n    d;\n}",
    );
}

// Go: commonjsmodule.go:visitExpressionIdentifier (scope-correct, via
// GetReferencedImportDeclaration over the resolved symbol — NOT a name match).
// 6aj slice 3 mirrored for the *namespace* form: the only `ns` use is shadowed
// by an inner `var ns`, so it resolves to the local, not the namespace import,
// and is left bare. A name-match stand-in (the no-resolver fallback) would
// wrongly rewrite it to `m_1`; the scope-correct resolver keeps it `ns`. The
// import is still lowered to the `__importStar(require("m"))` alias.
#[test]
fn scoped_shadowed_namespace_import_use_is_not_rewritten() {
    let input = "import * as ns from \"m\";\nfunction f() {\n    var ns = 1;\n    ns;\n}";
    let (ec, source_file) = parse_shared(input);
    let resolver = build_reference_resolver(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.module = ModuleKind::CommonJs;
    let mut tx = new_common_js_module_transformer_with_resolver(&opts, resolver);
    let result = tx.transform_source_file(source_file);
    let text = emit(&ec, result, input);
    assert!(
        text.contains("const m_1 = __importStar(require(\"m\"));"),
        "namespace import still lowered: {text}"
    );
    assert!(
        text.ends_with("function f() {\n    var ns = 1;\n    ns;\n}"),
        "shadowed namespace use stays bare: {text}"
    );
}

// Go: commonjsmodule.go:visitExpressionIdentifier -> GetReferencedExportContainer
// 6ak slice 1: with a scope-correct resolver, a module-level use of a top-level
// *exported variable* is rewritten to a qualified `exports.<name>` access. The
// declaration `export const x = 1;` is already lowered to `exports.x = void 0;`
// + `exports.x = 1;` (6e/6w); this slice adds the USE-SITE rewrite `x;` ->
// `exports.x;` (the use resolves to a top-level export of the current module,
// whose export container is the source file). Verified against tsc --module
// commonjs.
#[test]
fn scoped_exported_variable_use_rewrites_to_exports_access() {
    check_cjs_scoped(
        "export const x = 1;\nx;",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nexports.x = void 0;\nexports.x = 1;\nexports.x;",
    );
}

// Go: commonjsmodule.go:visitExpressionIdentifier (scope-correct, via
// GetReferencedExportContainer over the resolved symbol — NOT a name match).
// 6ak slice 2 (the headline scope-correctness property): the outer `x` is an
// exported variable, but the only bare `x` use lives inside `function f()`
// where an inner `const x` shadows it, so the use resolves to the non-exported
// local and its export container is `None`. The use therefore stays bare; only
// the declaration is lowered to `exports.x`. A textual name match would wrongly
// rewrite the inner use to `exports.x`; the scope-correct resolver keeps it `x`.
#[test]
fn scoped_export_use_shadowed_by_inner_local_is_not_rewritten() {
    check_cjs_scoped(
        "export const x = 1;\nfunction f() {\n    const x = 2;\n    x;\n}",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nexports.x = void 0;\nexports.x = 1;\nfunction f() {\n    const x = 2;\n    x;\n}",
    );
}

// Go: commonjsmodule.go:visitExpressionIdentifier (GetReferencedExportContainer
// returns nil for a non-exported local).
// 6ak slice 3 (the non-export guard): `y` is a plain top-level local (not
// exported), so its use has no export container and stays bare. The module has
// no value exports, so there is no `__esModule` marker either.
#[test]
fn scoped_non_exported_local_use_stays_bare() {
    check_cjs_scoped("const y = 1;\ny;", "const y = 1;\ny;");
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
