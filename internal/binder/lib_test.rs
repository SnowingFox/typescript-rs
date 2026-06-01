//! Behavior tests for the binder entry point and public helpers.
//!
//! Go has no `Test*` in `binder_test.go` (only `BenchmarkBind`), so these are
//! behavior-level tests per PORTING §8.6, with expected values derived from TS
//! semantics and the Go implementation (anchored via `// Go:`).

use super::*;
use tsgo_ast::{NodeData, SymbolFlags};
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

/// Parses `src` as a `.ts` file and binds it, returning the arena, the source
/// file id, and the bind result.
fn bind(src: &str) -> (NodeArena, NodeId, BindResult) {
    let r = parse_source_file(SourceFileParseOptions::default(), src, ScriptKind::Ts);
    let mut arena = r.arena;
    let sf = r.source_file;
    let result = bind_source_file(&mut arena, sf);
    (arena, sf, result)
}

fn statements(arena: &NodeArena, sf: NodeId) -> Vec<NodeId> {
    match arena.data(sf) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => unreachable!(),
    }
}

// Go: internal/binder/binder.go:bind/declareSourceFileMember (TS: top-level `var`)
#[test]
fn bind_single_var_creates_symbol() {
    let (_arena, sf, result) = bind("var x = 1;");
    let sym = result.local(sf, "x").expect("x should be in file locals");
    assert!(result.symbols[sym.index()]
        .flags
        .contains(SymbolFlags::FUNCTION_SCOPED_VARIABLE));
    assert_eq!(result.symbols[sym.index()].declarations.len(), 1);
    // `node.Symbol` back-mapping records the declaration.
    assert!(result.node_symbol.values().any(|&s| s == sym));
}

// Go: internal/binder/binder.go:bindFunctionDeclaration
#[test]
fn bind_function_declaration_symbol() {
    let (_arena, sf, result) = bind("function f(){}");
    let sym = result.local(sf, "f").expect("f should be in file locals");
    assert!(result.symbols[sym.index()]
        .flags
        .contains(SymbolFlags::FUNCTION));
}

// Go: internal/binder/binder.go:declareModuleMember (TS: `export const`)
#[test]
fn bind_export_const() {
    let (_arena, _sf, result) = bind("export const a = 1;");
    let file_symbol = result
        .file_symbol
        .expect("external module has a file symbol");
    assert!(result.export(file_symbol, "a").is_some());
}

// Go: internal/binder/binder.go:declareSymbolEx (A_module_cannot_have_multiple_default_exports)
#[test]
fn bind_multiple_default_exports() {
    let (_arena, _sf, result) = bind("export default 1; export default 2;");
    assert!(result.has_diagnostic(&tsgo_diagnostics::A_MODULE_CANNOT_HAVE_MULTIPLE_DEFAULT_EXPORTS));
}

// Go: internal/binder/binder.go:GetContainerFlags
#[test]
fn get_container_flags_function() {
    let r = parse_source_file(
        SourceFileParseOptions::default(),
        "function f(){}",
        ScriptKind::Ts,
    );
    let func = statements(&r.arena, r.source_file)[0];
    let flags = get_container_flags(&r.arena, func);
    assert_eq!(
        flags,
        ContainerFlags::IS_CONTAINER
            | ContainerFlags::IS_CONTROL_FLOW_CONTAINER
            | ContainerFlags::HAS_LOCALS
            | ContainerFlags::IS_FUNCTION_LIKE
            | ContainerFlags::IS_THIS_CONTAINER
    );
}

// Go: internal/binder/binder.go:FindUseStrictPrologue
#[test]
fn find_use_strict_prologue_detected() {
    let r = parse_source_file(
        SourceFileParseOptions::default(),
        "\"use strict\"; var x;",
        ScriptKind::Ts,
    );
    let stmts = statements(&r.arena, r.source_file);
    assert_eq!(find_use_strict_prologue(&r.arena, &stmts), Some(stmts[0]));

    let r2 = parse_source_file(SourceFileParseOptions::default(), "var x;", ScriptKind::Ts);
    let stmts2 = statements(&r2.arena, r2.source_file);
    assert_eq!(find_use_strict_prologue(&r2.arena, &stmts2), None);
}

// Go: internal/binder/binder.go:SetValueDeclaration
#[test]
fn set_value_declaration_sets() {
    let mut arena = NodeArena::new();
    let node = arena.new_identifier("x");
    let mut symbols = vec![Symbol::default()];
    set_value_declaration(&mut symbols, &arena, SymbolId(0), node);
    assert_eq!(symbols[0].value_declaration, Some(node));
}

// Go: internal/binder/binder.go:GetSymbolNameForPrivateIdentifier
#[test]
fn private_identifier_symbol_name_format() {
    assert_eq!(
        get_symbol_name_for_private_identifier(SymbolId(7), "#x"),
        "\u{FE}#7@#x"
    );
}

// ── CommonJS module/exports resolution (Round 8) ─────────────────────────────
//
// Go binds `module`/`exports` as file-local symbols once a CommonJS module
// indicator is seen (a `require(...)` call, a `module.exports = X` assignment,
// or an `exports.x = Y` / `module.exports.x = Y` assignment) in a JS file with
// no real external-module indicator. This is what lets `module`/`exports`
// resolve through the normal scope walk so the checker does not report
// TS2304/TS2591 for them. The symbols carry
// `FunctionScopedVariable|ModuleExports`, with `module` owning an `exports`
// member symbol (`ModuleExports|Property`).
// Go: internal/binder/binder.go:declareCommonJSVariable

/// Parses `src` as a `.js` file and binds it.
fn bind_js(src: &str) -> (NodeArena, NodeId, BindResult) {
    let r = parse_source_file(
        SourceFileParseOptions {
            file_name: "a.js".to_string(),
        },
        src,
        ScriptKind::Js,
    );
    let mut arena = r.arena;
    let sf = r.source_file;
    let result = bind_source_file(&mut arena, sf);
    (arena, sf, result)
}

// Go: internal/binder/binder.go:bindModuleExportsAssignment / declareCommonJSVariable
#[test]
fn js_module_exports_assignment_declares_module_and_exports() {
    let (_arena, sf, result) = bind_js("module.exports = {};");

    let module = result
        .local(sf, "module")
        .expect("`module` should be a file local");
    let module_flags = result.symbols[module.index()].flags;
    assert!(
        module_flags.contains(SymbolFlags::FUNCTION_SCOPED_VARIABLE | SymbolFlags::MODULE_EXPORTS),
        "module carries FunctionScopedVariable|ModuleExports, got {module_flags:?}"
    );
    // `module` value declaration is the source file (Go's `newSingleDeclaration`).
    assert_eq!(result.symbols[module.index()].value_declaration, Some(sf));
    // `module` owns an `exports` member symbol (`ModuleExports|Property`).
    let exports_member = result
        .member(module, "exports")
        .expect("`module` should own an `exports` member");
    assert!(result.symbols[exports_member.index()]
        .flags
        .contains(SymbolFlags::MODULE_EXPORTS | SymbolFlags::PROPERTY));
    assert_eq!(result.symbols[exports_member.index()].parent, Some(module));

    let exports = result
        .local(sf, "exports")
        .expect("`exports` should be a file local");
    assert!(result.symbols[exports.index()]
        .flags
        .contains(SymbolFlags::FUNCTION_SCOPED_VARIABLE | SymbolFlags::MODULE_EXPORTS));
}

// Go: internal/binder/binder.go:bindCallExpression / IsRequireCall
#[test]
fn js_require_call_declares_module_and_exports() {
    let (_arena, sf, result) = bind_js("const x = require('y');");
    assert!(
        result.local(sf, "module").is_some(),
        "a require(...) call sets the CommonJS indicator → `module` declared"
    );
    assert!(result.local(sf, "exports").is_some());
    // The require-initialized local still resolves as a plain file local.
    assert!(result.local(sf, "x").is_some());
}

// Go: internal/binder/binder.go:bindExportsOrObjectDefineProperty (exports.x = Y)
#[test]
fn js_exports_property_assignment_declares_module_and_exports() {
    let (_arena, sf, result) = bind_js("exports.foo = 1;");
    assert!(
        result.local(sf, "exports").is_some(),
        "`exports.x = Y` sets the CommonJS indicator → `exports` declared"
    );
    assert!(result.local(sf, "module").is_some());
}

// Go: internal/binder/binder.go:GetAssignmentDeclarationKind (module.exports.x = Y)
#[test]
fn js_module_exports_property_assignment_sets_indicator() {
    let (_arena, sf, result) = bind_js("module.exports.foo = 1;");
    assert!(result.local(sf, "module").is_some());
    assert!(result.local(sf, "exports").is_some());
}

// Go: internal/ast/utilities.go:GetAssignmentDeclarationKind (element-access form)
#[test]
fn js_exports_element_access_assignment_sets_indicator() {
    let (_arena, sf, result) = bind_js("exports[1] = 2;");
    assert!(result.local(sf, "exports").is_some());
    assert!(result.local(sf, "module").is_some());
}

// GUARD — a `.ts` file must NOT get the CommonJS file-local injection, even when
// it writes `module.exports`; `GetAssignmentDeclarationKind` gates on
// `IsInJSFile`. Go: internal/ast/utilities.go:GetAssignmentDeclarationKind
#[test]
fn ts_module_exports_assignment_does_not_declare_commonjs_locals() {
    let (_arena, sf, result) = bind("module.exports = {};");
    assert!(
        result.local(sf, "module").is_none(),
        "TS files must not inject CommonJS `module`/`exports` locals"
    );
    assert!(result.local(sf, "exports").is_none());
}

// GUARD — a JS file with NO require / module.exports / exports.x must keep
// `module` unresolved (no CommonJS indicator → Round 7's TS2591 behavior).
// Go: internal/binder/binder.go:setCommonJSModuleIndicator
#[test]
fn js_without_commonjs_indicator_does_not_declare_module() {
    let (_arena, sf, result) = bind_js("var y = 1; module;");
    assert!(
        result.local(sf, "module").is_none(),
        "a plain JS file with no CJS pattern must not declare `module`"
    );
    assert!(result.local(sf, "exports").is_none());
}

// GUARD — an ES-module JS file (a real external-module indicator) must NOT be
// treated as CommonJS even when it also writes `module.exports`;
// `setCommonJSModuleIndicator` returns false when a real external indicator
// exists. Go: internal/binder/binder.go:setCommonJSModuleIndicator
#[test]
fn js_es_module_does_not_declare_commonjs_locals() {
    let (_arena, sf, result) = bind_js("export const x = 1; module.exports = {};");
    assert!(
        result.local(sf, "module").is_none(),
        "an ES module must not be treated as CommonJS"
    );
    assert!(result.local(sf, "exports").is_none());
}
