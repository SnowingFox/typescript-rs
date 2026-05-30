//! Behavior tests for symbol creation, merging, and conflict diagnostics.
//!
//! Expected values follow TS semantics and the Go `declareSymbolEx` branches.

use crate::{bind_source_file, BindResult};
use tsgo_ast::{NodeArena, NodeData, NodeId, SymbolFlags};
use tsgo_core::scriptkind::ScriptKind;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

fn bind(src: &str) -> (NodeArena, NodeId, BindResult) {
    let r = parse_source_file(SourceFileParseOptions::default(), src, ScriptKind::Ts);
    let mut arena = r.arena;
    let sf = r.source_file;
    let result = bind_source_file(&mut arena, sf);
    (arena, sf, result)
}

fn first_statement(arena: &NodeArena, sf: NodeId) -> NodeId {
    match arena.data(sf) {
        NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => unreachable!(),
    }
}

// Go: internal/binder/binder.go:declareSymbolEx (merge: two `var` of the same name)
#[test]
fn bind_var_merge() {
    let (_arena, sf, result) = bind("var x; var x;");
    let sym = result.local(sf, "x").expect("x present");
    assert_eq!(result.symbols[sym.index()].declarations.len(), 2);
    assert!(result.diagnostics.is_empty());
}

// Go: internal/binder/binder.go:declareSymbolEx (Cannot_redeclare_block_scoped_variable_0)
#[test]
fn bind_let_redeclare_conflict() {
    let (_arena, _sf, result) = bind("let x; let x;");
    assert!(result.has_diagnostic(&tsgo_diagnostics::CANNOT_REDECLARE_BLOCK_SCOPED_VARIABLE_0));
}

// Go: internal/binder/binder.go:declareSymbolEx (Duplicate_identifier_0)
// DIVERGENCE(port): the tests.md example `class C{} function C(){}` actually
// merges at the binder level (the checker reports that conflict); two classes
// are a non-mergeable pair the binder itself flags.
#[test]
fn bind_duplicate_identifier() {
    let (_arena, _sf, result) = bind("class C {} class C {}");
    assert!(result.has_diagnostic(&tsgo_diagnostics::DUPLICATE_IDENTIFIER_0));
}

// Go: internal/binder/binder.go:bindContainer (function locals don't leak)
#[test]
fn bind_function_locals_scope() {
    let (arena, sf, result) = bind("function f(){ var y; }");
    let func = first_statement(&arena, sf);
    assert!(result.local(func, "y").is_some());
    assert!(result.local(sf, "y").is_none());
}

// Go: internal/binder/binder.go:declareClassMember (members table)
#[test]
fn bind_class_members() {
    let (_arena, sf, result) = bind("class C { m(){} p = 1; }");
    let c = result.local(sf, "C").expect("C present");
    let m = result.member(c, "m").expect("method m present");
    let p = result.member(c, "p").expect("property p present");
    assert!(result.symbols[m.index()]
        .flags
        .contains(SymbolFlags::METHOD));
    assert!(result.symbols[p.index()]
        .flags
        .contains(SymbolFlags::PROPERTY));
}

// Go: internal/binder/binder.go:bindBlockScopedDeclaration (interface merge)
#[test]
fn bind_interface_merge() {
    let (_arena, sf, result) = bind("interface I { a: number } interface I { b: string }");
    let i = result.local(sf, "I").expect("I present");
    assert_eq!(result.symbols[i.index()].declarations.len(), 2);
    assert!(result.diagnostics.is_empty());
}

// Go: internal/binder/binder.go:declareSymbolEx (Enum_declarations_can_only_merge...)
#[test]
fn bind_enum_namespace_merge() {
    let (_arena, _sf, result) = bind("enum E {} var E;");
    assert!(result.has_diagnostic(
        &tsgo_diagnostics::ENUM_DECLARATIONS_CAN_ONLY_MERGE_WITH_NAMESPACE_OR_OTHER_ENUM_DECLARATIONS
    ));
}

// Go: internal/binder/binder.go:getDeclarationName (private identifier name format)
#[test]
fn bind_private_identifier_name() {
    let (_arena, sf, result) = bind("class C { #x = 1; }");
    let c = result.local(sf, "C").expect("C present");
    let has_private = result.symbols[c.index()]
        .members
        .keys()
        .any(|k| k.starts_with("\u{FE}#") && k.ends_with("@#x"));
    assert!(has_private, "expected a private-identifier member key");
}
