use super::*;
use crate::core::test_support::StubProgram;
use crate::core::types::TypeFlags;
use crate::core::Checker;
use tsgo_ast::{NodeData, NodeId, SymbolFlags, SymbolId, SymbolTable};

// Looks up a top-level local symbol by name.
fn local(p: &StubProgram, name: &str) -> SymbolId {
    *p.locals(p.root())
        .expect("source file locals")
        .get(name)
        .unwrap_or_else(|| panic!("missing local {name}"))
}

// Go: internal/checker/checker.go:Checker.getIndexInfosOfType (generic Array<T>)
#[test]
fn array_type_reference_index_signature_instantiates_element() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n}\ndeclare const a: Array<number>;",
    );
    let mut c = Checker::new();
    let array_iface = local(&p, "Array");
    let iface_ty = get_declared_type_of_symbol(&mut c, &p, array_iface, None);
    let iface_infos = get_index_infos_of_type(&mut c, iface_ty);
    assert_eq!(iface_infos.len(), 1);
    assert_ne!(
        c.index_info(iface_infos[0]).value_type,
        c.error_type(),
        "interface index value type should resolve type parameter T"
    );

    let a = local(&p, "a");
    let array_ref = get_type_of_symbol(&mut c, &p, a, None);
    assert_ne!(
        array_ref,
        c.error_type(),
        "Array<number> annotation should resolve"
    );
    let infos = get_index_infos_of_type(&mut c, array_ref);
    assert_eq!(infos.len(), 1, "expected one number index signature");
    assert_eq!(
        c.index_info(infos[0]).value_type,
        c.number_type(),
        "Array<number> element type should be number"
    );
}

// 4bc slice 4 (genuine RED): a union of two equal string-literal type nodes
// (`"a" | "a"`) collapses to the single literal `"a"`. With value-keyed
// interning the two `"a"` constituents resolve to one `TypeId`, so the union's
// id-dedup leaves a single member and `getUnionType` returns that member (no
// 2-member union). Before interning the two `"a"` were distinct ids, so the
// union was kept as `"a" | "a"`.
// Go: internal/checker/checker.go:Checker.getUnionType (constituent dedup by id) +
//     Checker.getStringLiteralType(25164)
#[test]
fn union_of_equal_string_literals_collapses_to_single_literal() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: \"a\" | \"a\";");
    let mut c = Checker::new();
    let x = local(&p, "x");
    let t = get_type_of_symbol(&mut c, &p, x, None);
    assert_eq!(c.type_to_string(t), "\"a\"");
    assert!(
        c.get_type(t).union_types().is_none(),
        "equal literals must dedup to a single non-union literal"
    );
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

// Go: internal/checker/checker.go:Checker.getTypeFromIntersectionTypeNode
#[test]
fn type_from_type_node_resolves_intersection() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A {\n  x: number;\n}\ninterface B {\n  y: string;\n}\nvar i: A & B;",
    );
    let mut c = Checker::new();
    let i = local(&p, "i");
    let decl = p.symbol(i).value_declaration.expect("value declaration");
    let type_node: NodeId = match p.arena().data(decl) {
        NodeData::VariableDeclaration(d) => d.type_node.expect("annotation"),
        _ => panic!("expected variable declaration"),
    };
    // `A & B` resolves to the intersection of the two declared interface types.
    let ty = get_type_from_type_node(&mut c, &p, type_node, None);
    assert_eq!(c.get_type(ty).flags(), TypeFlags::INTERSECTION);
    let a = get_declared_type_of_symbol(&mut c, &p, local(&p, "A"), None);
    let b = get_declared_type_of_symbol(&mut c, &p, local(&p, "B"), None);
    let mut expected = [a, b];
    expected.sort();
    assert_eq!(c.get_type(ty).intersection_types().unwrap(), &expected[..]);
}

// Go: internal/checker/checker.go:Checker.getTypeFromUnionTypeNode
#[test]
fn type_from_type_node_resolves_union() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A {\n  x: number;\n}\ninterface B {\n  y: string;\n}\nvar u: A | B;",
    );
    let mut c = Checker::new();
    let u = local(&p, "u");
    let decl = p.symbol(u).value_declaration.expect("value declaration");
    let type_node: NodeId = match p.arena().data(decl) {
        NodeData::VariableDeclaration(d) => d.type_node.expect("annotation"),
        _ => panic!("expected variable declaration"),
    };
    // `A | B` resolves to the union of the two declared interface types.
    let ty = get_type_from_type_node(&mut c, &p, type_node, None);
    assert_eq!(c.get_type(ty).flags(), TypeFlags::UNION);
    let a = get_declared_type_of_symbol(&mut c, &p, local(&p, "A"), None);
    let b = get_declared_type_of_symbol(&mut c, &p, local(&p, "B"), None);
    // Interns to the same union id as `get_union_type([A, B])`.
    assert_eq!(ty, c.get_union_type(&[a, b]));
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

// Go: internal/checker/checker.go:Checker.getTypeOfPropertyOfType (instantiated through reference)
#[test]
fn type_of_property_through_reference_is_instantiated() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface Box<T> {\n  value: T;\n}");
    let mut c = Checker::new();
    let box_ty = get_declared_type_of_symbol(&mut c, &p, local(&p, "Box"), None);
    let tp = c
        .get_type(box_ty)
        .as_object()
        .expect("object")
        .type_parameters[0];
    // Model `value: T` by setting the value member's type to the type parameter.
    let value_sym = get_property_of_type(&c, box_ty, "value").expect("value member");
    c.value_symbol_links.get(value_sym).resolved_type = Some(tp);

    let string_ty = c.string_type();
    let box_string = c.create_type_reference(box_ty, vec![string_ty]); // Box<string>
                                                                       // Through `Box<string>`, `value` has type `string`.
    let value_type = get_type_of_property_of_type(&mut c, &p, box_string, "value");
    assert_eq!(value_type, Some(string_ty));
    // The bare generic still yields the type parameter.
    assert_eq!(
        get_type_of_property_of_type(&mut c, &p, box_ty, "value"),
        Some(tp)
    );
}

// Go: internal/checker/checker.go:Checker.getDeclaredTypeOfTypeParameter
#[test]
fn declared_type_of_type_parameter_symbol() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface Box<T> {\n  value: T;\n}");
    let mut c = Checker::new();
    // Building Box's declared type registers its type parameter symbol's type.
    let box_ty = get_declared_type_of_symbol(&mut c, &p, local(&p, "Box"), None);
    let tp = c
        .get_type(box_ty)
        .as_object()
        .expect("object")
        .type_parameters[0];
    assert!(c.get_type(tp).as_type_parameter().is_some());
}

// Go: internal/checker/checker.go:Checker.getPropertyOfUnionOrIntersectionType
// (intersection branch) + getUnionOrIntersectionProperty.
#[test]
fn intersection_synthesizes_properties_of_each_constituent() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { a: number }\ninterface B { b: string }",
    );
    let mut c = Checker::new();
    let a = get_declared_type_of_symbol(&mut c, &p, local(&p, "A"), None);
    let b = get_declared_type_of_symbol(&mut c, &p, local(&p, "B"), None);
    let inter = c.get_intersection_type(&[a, b]);
    // `A & B` exposes `a` (from A) and `b` (from B) as synthesized properties.
    let pa = get_property_of_type(&c, inter, "a").expect("a property");
    let pb = get_property_of_type(&c, inter, "b").expect("b property");
    // Each synthesized property carries its constituent's type.
    let num = c.number_type();
    let strg = c.string_type();
    let ta = get_type_of_symbol(&mut c, &p, pa, None);
    let tb = get_type_of_symbol(&mut c, &p, pb, None);
    assert_eq!(ta, num);
    assert_eq!(tb, strg);
    // A name absent from every constituent resolves to nothing.
    assert_eq!(get_property_of_type(&c, inter, "nope"), None);
    // `get_properties_of_type` collects both constituents' members.
    let mut names: Vec<String> = get_properties_of_type(&c, inter)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    names.sort();
    assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
}

// Go: internal/checker/checker.go:Checker.createUnionOrIntersectionProperty
// (intersection branch: a name present in two or more constituents synthesizes a
// transient property whose type is the intersection of the per-constituent types).
#[test]
fn intersection_multi_constituent_property_has_intersected_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface X { p: number }\ninterface Y { q: string }\n\
         interface A { a: X }\ninterface B { a: Y }",
    );
    let mut c = Checker::new();
    let a = get_declared_type_of_symbol(&mut c, &p, local(&p, "A"), None);
    let b = get_declared_type_of_symbol(&mut c, &p, local(&p, "B"), None);
    let x = get_declared_type_of_symbol(&mut c, &p, local(&p, "X"), None);
    let y = get_declared_type_of_symbol(&mut c, &p, local(&p, "Y"), None);
    let inter = c.get_intersection_type(&[a, b]);
    // `a` appears in both `A` and `B`, so its synthesized type is `X & Y`.
    let pa = get_property_of_type(&c, inter, "a").expect("a property");
    let ta = get_type_of_symbol(&mut c, &p, pa, None);
    assert_eq!(ta, c.get_intersection_type(&[x, y]));
    assert_eq!(c.get_type(ta).flags(), TypeFlags::INTERSECTION);
}

// Go: internal/checker/checker.go:Checker.createUnionOrIntersectionProperty
// (union branch: `optionalFlag |= prop.Flags & SymbolFlagsOptional` — a union
// property is optional if it is optional in ANY constituent).
#[test]
fn union_property_is_optional_when_optional_in_any_constituent() {
    // The checker must retain the program so the synthesized-property mint can
    // read each constituent symbol's optional flag (Go computes this eagerly
    // inside `createUnionOrIntersectionProperty` with `*Checker`).
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { a: number }\ninterface B { a?: string }\ninterface C { a: string }",
    ));
    let prog: std::rc::Rc<dyn crate::core::program::BoundProgram> = p.clone();
    let mut c = Checker::new_checker(prog);
    let a = get_declared_type_of_symbol(&mut c, p.as_ref(), local(&p, "A"), None);
    let b = get_declared_type_of_symbol(&mut c, p.as_ref(), local(&p, "B"), None);
    let cc = get_declared_type_of_symbol(&mut c, p.as_ref(), local(&p, "C"), None);

    // `a` is required in `A` but optional in `B`; the synthesized union property
    // is optional because it is optional in *some* constituent.
    let ab = c.get_union_type(&[a, b]);
    let pa = get_property_of_type(&c, ab, "a").expect("a property on A | B");
    assert!(c
        .resolved_symbol_flags(p.as_ref(), pa)
        .contains(SymbolFlags::OPTIONAL));

    // `a` is required in both `A` and `C`; the synthesized union property is
    // *not* optional (the OR over constituents is false).
    let ac = c.get_union_type(&[a, cc]);
    let pa2 = get_property_of_type(&c, ac, "a").expect("a property on A | C");
    assert!(!c
        .resolved_symbol_flags(p.as_ref(), pa2)
        .contains(SymbolFlags::OPTIONAL));
}

// Go: internal/checker/checker.go:Checker.createUnionOrIntersectionProperty
// (intersection branch: `optionalFlag &= prop.Flags` starting from
// `SymbolFlagsOptional` — an intersection property is optional only if it is
// optional in ALL constituents that declare it).
#[test]
fn intersection_property_is_optional_only_when_optional_in_all_constituents() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface X {}\ninterface Y {}\n\
         interface A { a?: X }\ninterface B { a?: Y }\ninterface D { a: X }",
    ));
    let prog: std::rc::Rc<dyn crate::core::program::BoundProgram> = p.clone();
    let mut c = Checker::new_checker(prog);
    let a = get_declared_type_of_symbol(&mut c, p.as_ref(), local(&p, "A"), None);
    let b = get_declared_type_of_symbol(&mut c, p.as_ref(), local(&p, "B"), None);
    let d = get_declared_type_of_symbol(&mut c, p.as_ref(), local(&p, "D"), None);

    // `a` is optional in both `A` and `B`; the synthesized intersection property
    // is optional (the AND over constituents is true).
    let ab = c.get_intersection_type(&[a, b]);
    let pa = get_property_of_type(&c, ab, "a").expect("a property on A & B");
    assert!(c
        .resolved_symbol_flags(p.as_ref(), pa)
        .contains(SymbolFlags::OPTIONAL));

    // `a` is optional in `B` but required in `D`; the synthesized intersection
    // property is *not* optional (the AND is false).
    let bd = c.get_intersection_type(&[b, d]);
    let pa2 = get_property_of_type(&c, bd, "a").expect("a property on B & D");
    assert!(!c
        .resolved_symbol_flags(p.as_ref(), pa2)
        .contains(SymbolFlags::OPTIONAL));
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

// Go: internal/checker/checker.go:Checker.getApparentType (primitive -> globalStringType)
#[test]
fn apparent_type_of_string_maps_to_global_string_wrapper() {
    // A synthetic global `interface String { length: number }` is the wrapper
    // for the `string` primitive (Go's `getGlobalType("String")`).
    let p = StubProgram::parse_and_bind("/a.ts", "interface String {\n  length: number;\n}");
    let mut c = Checker::new();
    let mut globals = SymbolTable::default();
    globals.insert("String".to_string(), local(&p, "String"));

    // Before the global wrapper is built, the apparent type of `string` is
    // itself and the primitive has no own `length` property.
    assert_eq!(get_apparent_type(&c, c.string_type()), c.string_type());
    assert_eq!(get_property_of_type(&c, c.string_type(), "length"), None);

    // Building the global `String` type lets the apparent type of `string` (and
    // of a string-literal type) resolve to that wrapper interface.
    let string_wrapper = get_global_type(&mut c, &p, "String", &globals).expect("global String");
    assert_eq!(get_apparent_type(&c, c.string_type()), string_wrapper);
    let lit = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        crate::core::types::LiteralValue::String("abc".to_string()),
        None,
    );
    assert_eq!(get_apparent_type(&c, lit), string_wrapper);

    // A property access on the string literal now resolves `length`, whose type
    // is `number`.
    let length = get_property_of_type(&c, lit, "length").expect("length property");
    assert_eq!(
        get_type_of_symbol(&mut c, &p, length, Some(&globals)),
        c.number_type()
    );
}

// Go: internal/checker/checker.go:Checker.getResolvedMembersOrExportsOfSymbol
// (late-bound well-known-symbol members)
#[test]
fn late_binds_well_known_symbol_iterator_member() {
    // The binder bound `[Symbol.iterator]` anonymously as `__computed`, so it is
    // NOT in `I.members` under any literal name; the checker must late-bind it.
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface SymbolConstructor { readonly iterator: unique symbol; }\n\
         declare var Symbol: SymbolConstructor;\n\
         interface I { [Symbol.iterator](): void; }",
    );
    let mut c = Checker::new();
    let globals = p.locals(p.root());
    let i = local(&p, "I");
    let ty = get_declared_type_of_symbol(&mut c, &p, i, globals);

    // The literal `iterator` name is NOT a member (it bound as `__computed`).
    assert_eq!(get_property_of_type(&c, ty, "iterator"), None);

    // Under the late-bound name `__@iterator`, the member is reachable.
    let late = c.get_property_name_for_known_symbol_name("iterator");
    let member = get_property_of_type(&c, ty, &late)
        .expect("[Symbol.iterator] should late-bind to __@iterator");
    // It is the binder's anonymous computed member (a method declaration).
    let decls = p.symbol(member).declarations.clone();
    assert!(decls
        .iter()
        .any(|&d| p.arena().kind(d) == tsgo_ast::Kind::MethodSignature));
}

// Go: internal/checker/checker.go:Checker.isSymbolOrSymbolForCall
// (a `Symbol` that is not the global symbol is not a well-known symbol)
#[test]
fn computed_symbol_member_without_global_symbol_is_not_late_bound() {
    // No `declare var Symbol`: the `Symbol` in `[Symbol.iterator]` does not
    // resolve to a global value, so Go would not produce a usable property-name
    // type and the member stays anonymous (`__computed`).
    let p = StubProgram::parse_and_bind("/a.ts", "interface I { [Symbol.iterator](): void; }");
    let mut c = Checker::new();
    let globals = p.locals(p.root());
    let i = local(&p, "I");
    let ty = get_declared_type_of_symbol(&mut c, &p, i, globals);

    let late = c.get_property_name_for_known_symbol_name("iterator");
    assert_eq!(
        get_property_of_type(&c, ty, &late),
        None,
        "without a global `Symbol`, `[Symbol.iterator]` must not late-bind"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfSymbol (METHOD) +
// getSignaturesOfSymbol / getSignatureFromDeclaration (MethodSignature)
// 4ah: a method member's type is an anonymous object type carrying its call
// signature(s), so the call signature's return type is reachable (this is the
// per-step the iterator protocol relies on: `[Symbol.iterator]()` and `next()`
// are method signatures whose return types drive `getIteratedTypeOfIterable`).
#[test]
fn method_member_call_signature_return_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface I {\n  m(): string;\n}");
    let mut c = Checker::new();
    let i = get_declared_type_of_symbol(&mut c, &p, local(&p, "I"), None);
    let m = get_property_of_type(&c, i, "m").expect("m member");
    let m_type = get_type_of_symbol(&mut c, &p, m, None);
    let sigs = c.get_signatures_of_type(m_type);
    assert_eq!(sigs.len(), 1, "method type should carry one call signature");
    let ret = c.get_return_type_of_call(&p, sigs[0], &[], &[]);
    assert_eq!(ret, c.string_type());
}

// Go: internal/checker/checker.go:Checker.getTypeFromTypeReference (type parameter in scope)
// A bare `T` type reference inside a generic interface's member resolves to the
// interface's type parameter, so a `value: T` member of `Iterator<T>` carries
// the type parameter (which 4ah then instantiates through the reference).
#[test]
fn bare_type_parameter_reference_resolves_to_enclosing_type_parameter() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Iterator<T> {\n  next(): { value: T };\n}",
    );
    let mut c = Checker::new();
    let iter = get_declared_type_of_symbol(&mut c, &p, local(&p, "Iterator"), None);
    let tp = c
        .get_type(iter)
        .as_object()
        .expect("object")
        .type_parameters[0];
    let next = get_property_of_type(&c, iter, "next").expect("next member");
    let next_type = get_type_of_symbol(&mut c, &p, next, None);
    let result_ty = c.get_return_type_of_call(&p, c.get_signatures_of_type(next_type)[0], &[], &[]);
    let value = get_property_of_type(&c, result_ty, "value").expect("value member");
    let value_ty = get_type_of_symbol(&mut c, &p, value, None);
    assert_eq!(
        value_ty, tp,
        "value should resolve to the interface type parameter"
    );
}
