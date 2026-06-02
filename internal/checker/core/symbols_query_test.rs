use super::*;
use crate::core::symbols::resolve_name;
use crate::core::test_support::StubProgram;
use crate::core::types::TypeFlags;
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

// Navigates to the first top-level `VariableDeclaration` node and its name, the
// shape `const`/`let` type-at-location tests need.
fn first_variable_declaration(p: &StubProgram) -> (NodeId, NodeId) {
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("expected a source file"),
    };
    for stmt in stmts {
        if let NodeData::VariableStatement(d) = arena.data(stmt) {
            let decls = match arena.data(d.declaration_list) {
                NodeData::VariableDeclarationList(l) => l.declarations.nodes.clone(),
                _ => panic!("expected a variable declaration list"),
            };
            let decl = decls[0];
            let name = match arena.data(decl) {
                NodeData::VariableDeclaration(v) => v.name,
                _ => panic!("expected a variable declaration"),
            };
            return (decl, name);
        }
    }
    panic!("no variable declaration found");
}

// Go: internal/checker/checker.go:Checker.GetTypeAtLocation -> getTypeOfNode
// (IsDeclarationNameOrImportPropertyName -> getTypeOfSymbol). The inferred type
// of an un-annotated mutable `let x = 1` is the *widened* base primitive
// `number` (NOT `any`): the query resolves the name to its symbol and reads the
// symbol's initializer-inferred type.
#[test]
fn get_type_at_location_infers_widened_number_for_let() {
    let p = StubProgram::parse_and_bind("/a.ts", "let x = 1;");
    let mut c = Checker::new();
    let (decl, name) = first_variable_declaration(&p);
    // The declaration name resolves to `number` (widened from the literal `1`).
    let name_type = get_type_at_location(&mut c, &p, name, None);
    assert_eq!(c.type_to_string(name_type), "number");
    // The declaration node itself yields the same type (the `IsDeclaration` arm).
    let decl_type = get_type_at_location(&mut c, &p, decl, None);
    assert_eq!(c.type_to_string(decl_type), "number");
}

// Go: internal/checker/checker.go:getTypeOfNode — a `const` keeps the literal
// type of its initializer (`const x = 1` -> `1`, not widened to `number` and
// not `any`). This is the real inferred type, distinct from the `let` case
// above.
#[test]
fn get_type_at_location_keeps_literal_for_const() {
    let p = StubProgram::parse_and_bind("/a.ts", "const x = 1;");
    let mut c = Checker::new();
    let (decl, name) = first_variable_declaration(&p);
    let name_type = get_type_at_location(&mut c, &p, name, None);
    assert_eq!(c.type_to_string(name_type), "1");
    let decl_type = get_type_at_location(&mut c, &p, decl, None);
    assert_eq!(c.type_to_string(decl_type), "1");
}

// Go: internal/checker/checker.go:getTypeOfNode — an un-annotated variable
// initialized from a CALL infers the call's return type (`const x = f()` where
// `f(): string` -> `string`), so the inference flows through the type query.
#[test]
fn get_type_at_location_infers_call_return_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare function f(): string;\nconst x = f();");
    let mut c = Checker::new();
    let (decl, _name) = first_variable_declaration(&p);
    let decl_type = get_type_at_location(&mut c, &p, decl, None);
    assert_eq!(c.type_to_string(decl_type), "string");
}

// Go: internal/checker/checker.go:getTypeOfNode — an object-literal initializer
// infers an anonymous object type (rendered `{ ... }` by the reachable
// nodebuilder subset). The point is the query returns a real object type, not
// `any`.
#[test]
fn get_type_at_location_infers_object_literal_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { a: 1 };");
    let mut c = Checker::new();
    let (decl, _name) = first_variable_declaration(&p);
    let t = get_type_at_location(&mut c, &p, decl, None);
    assert!(c.get_type(t).as_object().is_some(), "object-literal type");
    assert!(
        !c.get_type(t).flags().contains(TypeFlags::ANY),
        "inferred object type is not any"
    );
}

// Go: internal/checker/checker.go:getTypeOfNode (SourceFile / fallthrough) — a
// node with no answerable type yields the error type rather than panicking. A
// non-module source file is the first guard.
#[test]
fn get_type_at_location_source_file_is_error_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "const x = 1;");
    let mut c = Checker::new();
    let t = get_type_at_location(&mut c, &p, p.root(), None);
    assert_eq!(t, c.error_type());
}

// Navigates to the call expression that is the expression of statement
// `stmt_idx` (an `ExpressionStatement` whose expression is a `CallExpression`).
fn call_expression(p: &StubProgram, stmt_idx: usize) -> NodeId {
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("expected a source file"),
    };
    match arena.data(stmts[stmt_idx]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expected an expression statement"),
    }
}

// Go: internal/checker/checker.go:Checker.GetResolvedSignature — resolving a
// call expression returns the chosen signature, whose first parameter symbol is
// the declared parameter `a`. This is the keystone the language-service
// parameter-name inlay hints stand on (it maps a call site to its signature's
// parameters).
#[test]
fn get_resolved_signature_returns_signature_with_named_parameter() {
    let p = StubProgram::parse_and_bind("/a.ts", "function f(a: number) {}\nf(1);");
    let mut c = Checker::new();
    let call = call_expression(&p, 1);
    let sig = get_resolved_signature(&mut c, &p, call).expect("a resolved signature");
    let params = c.signature(sig).parameters.clone();
    assert_eq!(params.len(), 1);
    assert_eq!(p.symbol(params[0]).name, "a");
}

// GUARD Go: internal/checker/checker.go:Checker.GetResolvedSignature — an
// unresolved (undefined) callee has no call signatures, so resolution yields
// `None` (and does not panic): the language-service hint guard relies on this.
#[test]
fn get_resolved_signature_none_for_unresolved_call() {
    let p = StubProgram::parse_and_bind("/a.ts", "g(1);");
    let mut c = Checker::new();
    let call = call_expression(&p, 0);
    assert!(get_resolved_signature(&mut c, &p, call).is_none());
}

// GUARD: a node that is not a call / `new` expression resolves to no signature.
// Go: internal/checker/checker.go:Checker.getResolvedSignature (non-call node)
#[test]
fn get_resolved_signature_none_for_non_call_node() {
    let p = StubProgram::parse_and_bind("/a.ts", "const x = 1;");
    let mut c = Checker::new();
    assert!(get_resolved_signature(&mut c, &p, p.root()).is_none());
}

// Go: internal/checker/checker.go:Checker.GetResolvedSignature — a call to a
// rest-parameter function resolves to a signature flagged
// `HAS_REST_PARAMETER`, whose first parameter symbol is the rest parameter
// `xs`. The language-service hint maps the rest position via this flag.
#[test]
fn get_resolved_signature_exposes_rest_parameter() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\n\
         function g(...xs: number[]) {}\n\
         g(1);",
    );
    let mut c = Checker::new();
    let call = call_expression(&p, 2);
    let sig = get_resolved_signature(&mut c, &p, call).expect("a resolved signature");
    assert!(c
        .signature(sig)
        .flags
        .contains(crate::core::signatures::SignatureFlags::HAS_REST_PARAMETER));
    let params = c.signature(sig).parameters.clone();
    assert_eq!(params.len(), 1);
    assert_eq!(p.symbol(params[0]).name, "xs");
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
