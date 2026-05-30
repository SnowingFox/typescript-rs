use super::*;
use crate::core::test_support::StubProgram;
use crate::core::Checker;
use tsgo_ast::{NodeData, NodeId, SymbolId, SymbolTable};

// Looks up a top-level local symbol by name.
fn local(p: &StubProgram, name: &str) -> SymbolId {
    *p.locals(p.root())
        .expect("source file locals")
        .get(name)
        .unwrap_or_else(|| panic!("missing local {name}"))
}

// Go: internal/checker/checker.go:Checker.getDeclaredTypeOfClassOrInterface
#[test]
fn declared_interface_type_exposes_members() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface Foo {\n  bar: string;\n}");
    let mut c = Checker::new();
    let foo = local(&p, "Foo");

    let ty = get_declared_type_of_symbol(&mut c, &p, foo, None);
    assert!(c.get_type(ty).as_object().is_some());
    let bar = get_property_of_type(&c, ty, "bar").expect("bar property");
    assert_eq!(p.symbol(bar).name, "bar");
    // Missing members are None.
    assert_eq!(get_property_of_type(&c, ty, "nope"), None);

    // The declared type is cached (same id on a second call).
    let ty2 = get_declared_type_of_symbol(&mut c, &p, foo, None);
    assert_eq!(ty, ty2);
}

// Go: internal/checker/checker.go:Checker.getTypeOfSymbol (variable annotation)
#[test]
fn type_of_value_symbol_resolves_annotation_to_declared_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Foo {\n  bar: string;\n}\ndeclare const foo: Foo;",
    );
    let mut c = Checker::new();
    let foo_var = local(&p, "foo");
    let foo_interface = local(&p, "Foo");

    let var_type = get_type_of_symbol(&mut c, &p, foo_var, None);
    let interface_type = get_declared_type_of_symbol(&mut c, &p, foo_interface, None);
    assert_eq!(var_type, interface_type);

    // Property lookup through the value's type finds the interface member.
    let bar = get_property_of_type(&c, var_type, "bar").expect("bar property");
    assert_eq!(p.symbol(bar).name, "bar");
}

// Go: internal/checker/checker.go:Checker.getTypeFromTypeNode (keyword types)
#[test]
fn type_from_type_node_maps_keyword_types() {
    let p = StubProgram::parse_and_bind("/a.ts", "var x: number;");
    let mut c = Checker::new();
    let x = local(&p, "x");
    let decl = p.symbol(x).value_declaration.expect("value declaration");
    let type_node: NodeId = match p.arena().data(decl) {
        NodeData::VariableDeclaration(d) => d.type_node.expect("annotation"),
        _ => panic!("expected variable declaration"),
    };
    assert_eq!(
        get_type_from_type_node(&mut c, &p, type_node, None),
        c.number_type()
    );
}

// Go: internal/checker/checker.go:Checker.getDeclaredTypeOfTypeAlias
#[test]
fn declared_type_of_type_alias_resolves_rhs() {
    let p = StubProgram::parse_and_bind("/a.ts", "type T = number;");
    let mut c = Checker::new();
    let t = local(&p, "T");
    assert_eq!(
        get_declared_type_of_symbol(&mut c, &p, t, None),
        c.number_type()
    );
}

// Go: internal/checker/checker.go:Checker.getDeclaredTypeOfEnum (4c simplification)
#[test]
fn declared_type_of_enum_exposes_members() {
    let p = StubProgram::parse_and_bind("/a.ts", "enum E {\n  A,\n}");
    let mut c = Checker::new();
    let e = local(&p, "E");
    let ty = get_declared_type_of_symbol(&mut c, &p, e, None);
    assert!(c.get_type(ty).as_object().is_some());
    let a = get_property_of_type(&c, ty, "A").expect("enum member A");
    assert_eq!(p.symbol(a).name, "A");
}

// Go: internal/checker/checker.go:Checker.getApparentType (4c identity)
#[test]
fn apparent_type_is_identity_in_4c() {
    let c = Checker::new();
    assert_eq!(get_apparent_type(&c, c.string_type()), c.string_type());
    assert_eq!(get_apparent_type(&c, c.boolean_type()), c.boolean_type());
}

// Go: internal/checker/checker.go:Checker.getPropertyOfType (primitives have no own members)
#[test]
fn property_of_primitive_is_none() {
    let c = Checker::new();
    assert_eq!(get_property_of_type(&c, c.string_type(), "length"), None);
}

// Go: internal/checker/checker.go:getDeclaredTypeOfClassOrInterface (extends merge)
#[test]
fn declared_interface_inherits_extends_members() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Base {\n  a: number;\n}\ninterface Derived extends Base {\n  b: string;\n}",
    );
    let mut c = Checker::new();
    let derived = get_declared_type_of_symbol(&mut c, &p, local(&p, "Derived"), None);
    // Own member.
    let b = get_property_of_type(&c, derived, "b").expect("own member b");
    assert_eq!(p.symbol(b).name, "b");
    // Inherited member from Base.
    let a = get_property_of_type(&c, derived, "a").expect("inherited member a");
    assert_eq!(p.symbol(a).name, "a");
}

// Go: internal/checker/checker.go:appendLocalTypeParametersOfClassOrInterfaceOrTypeAlias
#[test]
fn generic_interface_records_type_parameters() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface Box<T> {\n  value: T;\n}");
    let mut c = Checker::new();
    let ty = get_declared_type_of_symbol(&mut c, &p, local(&p, "Box"), None);
    let obj = c.get_type(ty).as_object().expect("object type");
    assert_eq!(obj.type_parameters.len(), 1);
    assert!(obj.this_type.is_some());
}

// Go: internal/checker/checker.go:getTypeFromTypeReference (type arguments)
#[test]
fn type_reference_with_arguments_resolves_member() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Box<T> {\n  value: T;\n}\ndeclare const b: Box<string>;",
    );
    let mut c = Checker::new();
    let b_type = get_type_of_symbol(&mut c, &p, local(&p, "b"), None);
    let obj = c.get_type(b_type).as_object().expect("type reference");
    assert!(obj.target.is_some());
    assert_eq!(obj.resolved_type_arguments, vec![c.string_type()]);
    // The member symbol resolves through the reference target.
    let value = get_property_of_type(&c, b_type, "value").expect("value member via reference");
    assert_eq!(p.symbol(value).name, "value");
}

// Go: internal/checker/checker.go:Checker.getGlobalType
#[test]
fn get_global_type_resolves_builds_and_caches() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Foo {\n  bar: string;\n}\ndeclare const foo: Foo;",
    );
    let mut c = Checker::new();
    let mut globals = SymbolTable::default();
    globals.insert("Foo".to_string(), local(&p, "Foo"));
    globals.insert("foo".to_string(), local(&p, "foo"));

    let ty = get_global_type(&mut c, &p, "Foo", &globals).expect("global Foo");
    assert!(c.get_type(ty).as_object().is_some());
    // Cached on second lookup.
    assert_eq!(get_global_type(&mut c, &p, "Foo", &globals), Some(ty));
    // A value-only name is not a global type.
    assert_eq!(get_global_type(&mut c, &p, "foo", &globals), None);
    // An unknown name resolves to nothing.
    assert_eq!(get_global_type(&mut c, &p, "Missing", &globals), None);
}
