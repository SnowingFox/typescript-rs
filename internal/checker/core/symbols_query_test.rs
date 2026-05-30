use super::*;
use crate::core::symbols::resolve_name;
use crate::core::test_support::StubProgram;
use crate::core::Checker;
use tsgo_ast::{NodeData, NodeId, SymbolFlags};

// Mirrors the source used by Go's TestGetSymbolAtLocation.
const SRC: &str = "interface Foo {\n  bar: string;\n}\ndeclare const foo: Foo;\nfoo.bar;";

// Extracts (interface name, variable name, property access) the same way the Go
// test navigates the parse tree.
fn query_nodes(p: &StubProgram) -> (NodeId, NodeId, NodeId) {
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("expected a source file"),
    };

    let interface_name = match arena.data(stmts[0]) {
        NodeData::InterfaceDeclaration(d) => d.name.expect("interface has a name"),
        _ => panic!("statement 0 should be an interface declaration"),
    };

    let var_name = match arena.data(stmts[1]) {
        NodeData::VariableStatement(d) => {
            let decls = match arena.data(d.declaration_list) {
                NodeData::VariableDeclarationList(l) => l.declarations.nodes.clone(),
                _ => panic!("expected a variable declaration list"),
            };
            match arena.data(decls[0]) {
                NodeData::VariableDeclaration(v) => v.name,
                _ => panic!("expected a variable declaration"),
            }
        }
        _ => panic!("statement 1 should be a variable statement"),
    };

    let prop_access = match arena.data(stmts[2]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("statement 2 should be an expression statement"),
    };

    (interface_name, var_name, prop_access)
}

// Go: internal/checker/checker_test.go:TestGetSymbolAtLocation
#[test]
fn get_symbol_at_location_resolves_interface_var_and_property() {
    let p = StubProgram::parse_and_bind("/foo.ts", SRC);
    let mut c = Checker::new();
    let (interface_name, var_name, prop_access) = query_nodes(&p);
    for node in [interface_name, var_name, prop_access] {
        let symbol = get_symbol_at_location(&mut c, &p, node, None);
        assert!(
            symbol.is_some(),
            "expected a non-None symbol at the queried node"
        );
    }
}

// Go: internal/checker/checker_test.go:TestGetSymbolAtLocation (symbol identity)
#[test]
fn get_symbol_at_location_returns_expected_symbol_names() {
    let p = StubProgram::parse_and_bind("/foo.ts", SRC);
    let mut c = Checker::new();
    let (interface_name, var_name, prop_access) = query_nodes(&p);

    let interface_symbol = get_symbol_at_location(&mut c, &p, interface_name, None).unwrap();
    assert_eq!(p.symbol(interface_symbol).name, "Foo");

    let var_symbol = get_symbol_at_location(&mut c, &p, var_name, None).unwrap();
    assert_eq!(p.symbol(var_symbol).name, "foo");

    let property_symbol = get_symbol_at_location(&mut c, &p, prop_access, None).unwrap();
    assert_eq!(p.symbol(property_symbol).name, "bar");
}

// Go: internal/checker/checker.go:Checker.getSymbolOfDeclaration
#[test]
fn get_symbol_of_declaration_returns_declaration_symbol() {
    let p = StubProgram::parse_and_bind("/foo.ts", SRC);
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("expected a source file"),
    };
    // statement 0 is the interface declaration itself.
    let symbol = get_symbol_of_declaration(&p, stmts[0]).expect("interface has a symbol");
    assert_eq!(p.symbol(symbol).name, "Foo");
}

// Go: internal/checker/checker.go:Checker.resolveName (meaning + scope)
#[test]
fn resolve_name_respects_meaning_and_scope() {
    let p = StubProgram::parse_and_bind("/foo.ts", SRC);
    let root = p.root();

    // `foo` is a value, not a type.
    let foo_value = resolve_name(&p, root, "foo", SymbolFlags::VALUE, false, None);
    assert_eq!(p.symbol(foo_value.unwrap()).name, "foo");
    assert!(resolve_name(&p, root, "foo", SymbolFlags::TYPE, false, None).is_none());

    // `Foo` is a type.
    let foo_type = resolve_name(&p, root, "Foo", SymbolFlags::TYPE, false, None);
    assert_eq!(p.symbol(foo_type.unwrap()).name, "Foo");

    // An unknown name resolves to nothing.
    assert!(resolve_name(&p, root, "Missing", SymbolFlags::VALUE, false, None).is_none());
}
