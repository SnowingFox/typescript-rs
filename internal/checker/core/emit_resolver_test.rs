use crate::core::program::BoundProgram;
use crate::core::test_support::StubProgram;
use crate::core::Checker;
use tsgo_ast::{NodeData, NodeId};

// Returns the `idx`-th top-level statement node.
fn statement(p: &StubProgram, idx: usize) -> NodeId {
    match p.arena().data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes[idx],
        _ => panic!("source file"),
    }
}

// Go: internal/checker/emitresolver.go:EmitResolver.IsDeclarationVisible (tracer)
#[test]
fn exported_declaration_is_visible() {
    let p = StubProgram::parse_and_bind("/a.ts", "export function f() {}\nfunction g() {}");
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    let f = statement(&p, 0);
    let g = statement(&p, 1);
    // The exported `f` is visible to declaration emit; the bare `g` is not.
    assert!(resolver.is_declaration_visible(&p, f));
    assert!(!resolver.is_declaration_visible(&p, g));
}

// Navigates a `VariableStatement` to its first `VariableDeclaration`.
fn first_var_declaration(p: &StubProgram, stmt_idx: usize) -> NodeId {
    let arena = p.arena();
    let stmt = statement(p, stmt_idx);
    let list = match arena.data(stmt) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => panic!("variable statement"),
    };
    match arena.data(list) {
        NodeData::VariableDeclarationList(l) => l.declarations.nodes[0],
        _ => panic!("variable declaration list"),
    }
}

// Go: internal/checker/emitresolver.go:EmitResolver.SerializeTypeOfDeclaration
#[test]
fn serialize_type_of_declaration_uses_real_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface Foo {}\ndeclare const x: Foo;");
    let mut c = Checker::new();
    let resolver = c.get_emit_resolver();
    let x_decl = first_var_declaration(&p, 1);
    // The declaration's type serializes to the interface name (4j node builder).
    assert_eq!(
        resolver.serialize_type_of_declaration(&mut c, &p, x_decl),
        "Foo"
    );
}

// Go: internal/checker/emitresolver.go:EmitResolver.IsImplementationOfOverload
#[test]
fn implementation_of_overload_is_detected() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "function foo(a: string): string;\nfunction foo(a: number): number;\nfunction foo(a: any) {\n  return a;\n}",
    );
    let c = Checker::new();
    let resolver = c.get_emit_resolver();
    // The body-bearing third `foo` is the overload implementation.
    assert!(resolver.is_implementation_of_overload(&p, statement(&p, 2)));
    // A bodyless overload signature is not.
    assert!(!resolver.is_implementation_of_overload(&p, statement(&p, 0)));

    // A plain (non-overloaded) function is not an overload implementation.
    let p2 = StubProgram::parse_and_bind("/b.ts", "function bar() {}");
    assert!(!resolver.is_implementation_of_overload(&p2, statement(&p2, 0)));
}
