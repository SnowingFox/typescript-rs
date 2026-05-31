//! Behavior tests for symbol creation, merging, and conflict diagnostics.
//!
//! Expected values follow TS semantics and the Go `declareSymbolEx` branches.

use crate::{bind_source_file, BindResult};
use tsgo_ast::symbol::INTERNAL_SYMBOL_NAME_COMPUTED;
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

/// Returns the members of an interface declaration node.
fn interface_members(arena: &NodeArena, node: NodeId) -> Vec<NodeId> {
    match arena.data(node) {
        NodeData::InterfaceDeclaration(d) => d.members.nodes.clone(),
        _ => unreachable!("expected an interface declaration"),
    }
}

// Go: internal/binder/binder.go:bindPropertyOrMethodOrAccessor (HasDynamicName guard)
// A well-known-symbol computed name (`[Symbol.iterator]`) is a dynamic name, so
// the member is bound anonymously under `InternalSymbolNameComputed` (via
// `bindAnonymousDeclaration`) instead of reaching `getDeclarationName` (which
// only handles literal computed names and otherwise panics). The member's
// symbol is attached to its node; the `__@iterator` late-binding into the
// members table is a checker concern, not the binder's.
#[test]
fn bind_computed_well_known_symbol_no_panic() {
    let (arena, sf, result) = bind("interface I { [Symbol.iterator](): void }");
    let i = result.local(sf, "I").expect("I present");
    // The well-known-symbol member is NOT registered in the interface's members
    // table by the binder; only literal-named members are.
    assert!(result.member(i, INTERNAL_SYMBOL_NAME_COMPUTED).is_none());
    let method = interface_members(&arena, first_statement(&arena, sf))[0];
    let method_sym = result
        .node_symbol
        .get(&method)
        .copied()
        .expect("computed method has a node symbol");
    assert_eq!(
        result.symbols[method_sym.index()].name,
        INTERNAL_SYMBOL_NAME_COMPUTED,
        "computed method is bound anonymously as InternalSymbolNameComputed"
    );
}

/// Returns the members of a class declaration node.
fn class_members(arena: &NodeArena, node: NodeId) -> Vec<NodeId> {
    match arena.data(node) {
        NodeData::ClassDeclaration(d) => d.members.nodes.clone(),
        _ => unreachable!("expected a class declaration"),
    }
}

// Go: internal/binder/binder.go:bindPropertyOrMethodOrAccessor (HasDynamicName guard)
// An arbitrary non-literal computed name (`[bar]`) is also dynamic, so a class
// property with one is bound anonymously under `InternalSymbolNameComputed` and
// does not panic. Exercises the property-declaration binding site (distinct from
// the interface-method site above).
#[test]
fn bind_computed_arbitrary_name_no_panic() {
    let (arena, sf, result) = bind("class C { [bar] = 1 }");
    let c = result.local(sf, "C").expect("C present");
    assert!(result.member(c, INTERNAL_SYMBOL_NAME_COMPUTED).is_none());
    let prop = class_members(&arena, first_statement(&arena, sf))[0];
    let prop_sym = result
        .node_symbol
        .get(&prop)
        .copied()
        .expect("computed property has a node symbol");
    assert_eq!(
        result.symbols[prop_sym.index()].name,
        INTERNAL_SYMBOL_NAME_COMPUTED
    );
}

// Go: internal/binder/binder.go:getDeclarationName (literal computed-name branch)
// A string-literal computed name (`["foo"]`) is NOT dynamic: it keeps its literal
// text and is registered in the container's members table under that text. The
// `HasDynamicName` guard must not divert it to `InternalSymbolNameComputed`.
#[test]
fn bind_computed_literal_name_preserved() {
    let (_arena, sf, result) = bind("class C { [\"foo\"]: number }");
    let c = result.local(sf, "C").expect("C present");
    assert!(
        result.member(c, "foo").is_some(),
        "literal name kept as text"
    );
    assert!(result.member(c, INTERNAL_SYMBOL_NAME_COMPUTED).is_none());
}

// Regression: binding a `lib.dom.d.ts`-style interface that mixes well-known
// symbol computed names (`[Symbol.iterator]`, `[Symbol.asyncIterator]`) with a
// regular member must succeed without panicking. The regular member stays
// reachable by name; the computed members are bound anonymously.
#[test]
fn bind_lib_style_well_known_symbols_no_panic() {
    let (arena, sf, result) = bind(
        "interface AsyncIterable<T> { \
            length: number; \
            [Symbol.iterator](): void; \
            [Symbol.asyncIterator](): void; \
        }",
    );
    let i = result
        .local(sf, "AsyncIterable")
        .expect("interface present");
    assert!(
        result.member(i, "length").is_some(),
        "regular member reachable"
    );
    let computed_count = interface_members(&arena, first_statement(&arena, sf))
        .iter()
        .filter_map(|m| result.node_symbol.get(m))
        .filter(|s| result.symbols[s.index()].name == INTERNAL_SYMBOL_NAME_COMPUTED)
        .count();
    assert_eq!(
        computed_count, 2,
        "both well-known-symbol members are bound"
    );
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
