use super::*;
use crate::core::declared_types::{
    get_declared_type_of_symbol, get_properties_of_type, get_type_of_symbol,
    resolve_structured_type_members,
};
use crate::core::mapper::TypeMapper;
use crate::core::test_support::StubProgram;
use crate::core::types::{ObjectFlags, TypeFlags};
use crate::core::Checker;
use tsgo_ast::{CheckFlags, SymbolFlags, SymbolId};

fn local(p: &StubProgram, name: &str) -> SymbolId {
    *p.locals(p.root())
        .expect("locals")
        .get(name)
        .unwrap_or_else(|| panic!("missing local {name}"))
}

// Go: internal/checker/checker.go:Checker.getTypeOfInstantiatedSymbol
#[test]
fn get_type_of_instantiated_symbol_substitutes_mapper() {
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let target = c.new_object_literal_property("x", SymbolFlags::PROPERTY, CheckFlags::empty(), tp);
    let inst = c.new_instantiated_symbol("x", target, TypeMapper::unary(tp, c.number_type()));
    let p = StubProgram::parse_and_bind("/a.ts", "");
    assert_eq!(
        get_type_of_instantiated_symbol(&mut c, &p, inst),
        c.number_type()
    );
}

// Go: internal/checker/checker.go:Checker.getModifiersTypeFromMappedType / getTemplateTypeFromMappedType
#[test]
fn mapped_type_accessors_concrete_homomorphic() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "type M = { [K in keyof { a: number; b: string }]: number };",
    );
    let mut c = Checker::new();
    let m = get_declared_type_of_symbol(&mut c, &p, local(&p, "M"), None);
    assert!(c.get_type(m).object_flags().contains(ObjectFlags::MAPPED));
    let modifiers = get_modifiers_type_from_mapped_type(&mut c, &p, m);
    let props = get_properties_of_type(&mut c, modifiers);
    assert_eq!(props.len(), 2);
    assert_eq!(
        get_template_type_from_mapped_type(&mut c, &p, m),
        c.number_type()
    );
}

// Go: internal/checker/checker.go:Checker.resolveMappedTypeMembers
#[test]
fn resolve_structured_type_members_mapped_type_populates_members() {
    let p =
        StubProgram::parse_and_bind("/a.ts", "type M = { [K in keyof { a: number }]: number };");
    let mut c = Checker::new();
    let m = get_declared_type_of_symbol(&mut c, &p, local(&p, "M"), None);
    let members = resolve_structured_type_members(&mut c, Some(&p), m);
    assert!(members.contains_key("a"));
    let a_sym = members.get("a").copied().unwrap();
    let ty = get_type_of_symbol(&mut c, &p, a_sym, None);
    assert_eq!(ty, c.number_type());
}

// Go: internal/checker/checker.go:Checker.getTypeFromMappedTypeNode / getConstraintTypeFromMappedType
#[test]
fn get_mapped_type_and_constraint_type_from_mapped_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "type M<T> = { [K in keyof T]: T[K] };");
    let mut c = Checker::new();
    let m = get_declared_type_of_symbol(&mut c, &p, local(&p, "M"), None);
    let constraint = get_constraint_type_from_mapped_type(&mut c, &p, m);
    assert!(c.get_type(constraint).flags().contains(TypeFlags::INDEX));
}
