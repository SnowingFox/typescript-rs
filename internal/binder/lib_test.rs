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
