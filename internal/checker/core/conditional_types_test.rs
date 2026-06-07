use super::*;
use crate::core::test_support::StubProgram;
use crate::Checker;
use crate::{
    get_declared_type_of_symbol, get_string_mapping_type, get_template_literal_type,
    StringMappingKind, TypeFlags,
};
use std::rc::Rc;
use tsgo_ast::SymbolId;

fn local(p: &StubProgram, name: &str) -> SymbolId {
    p.globals()
        .expect("globals")
        .get(name)
        .copied()
        .expect("symbol")
}

#[test]
fn is_distributive_conditional_type_true_for_type_parameter() {
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    assert!(is_distributive_conditional_type(&c, tp));
}

#[test]
fn is_distributive_conditional_type_false_for_concrete_type() {
    let c = Checker::new();
    let s = c.string_type();
    assert!(!is_distributive_conditional_type(&c, s));
}

#[test]
fn get_conditional_branch_types_from_deferred_conditional() {
    let p = Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type IsString<T> = T extends string ? \"yes\" : \"no\";",
    ));
    let prog: Rc<dyn crate::core::program::BoundProgram> = p.clone();
    let mut c = Checker::new_checker(prog);
    let declared = get_declared_type_of_symbol(&mut c, p.as_ref(), local(&p, "IsString"), None);
    let true_branch = get_true_type_from_conditional_type(&mut c, p.as_ref(), declared);
    let false_branch = get_false_type_from_conditional_type(&mut c, p.as_ref(), declared);
    assert_eq!(c.type_to_string(true_branch), "\"yes\"");
    assert_eq!(c.type_to_string(false_branch), "\"no\"");
}

#[test]
fn get_conditional_type_distributes_over_union() {
    let p = Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\n\
         type ToArray<T> = T extends unknown ? T[] : never;",
    ));
    let prog: Rc<dyn crate::core::program::BoundProgram> = p.clone();
    let mut c = Checker::new_checker(prog);
    let declared = get_declared_type_of_symbol(&mut c, p.as_ref(), local(&p, "ToArray"), None);
    let tp = c
        .get_type(declared)
        .as_conditional()
        .expect("conditional")
        .root
        .check_type;
    let string = c.string_type();
    let number = c.number_type();
    let union = c.get_union_type(&[string, number]);
    let resolved = c.instantiate_type(declared, &crate::core::mapper::TypeMapper::unary(tp, union));
    assert!(c.get_type(resolved).flags().contains(TypeFlags::UNION));
}

#[test]
fn get_template_literal_type_concrete_folds_to_string_literal() {
    let mut c = Checker::new();
    let x = c.get_string_literal_type("x");
    let t = get_template_literal_type(&mut c, &["a".into(), "b".into()], &[x]);
    assert_eq!(c.type_to_string(t), "\"axb\"");
}

#[test]
fn get_template_literal_type_distributes_over_union() {
    let mut c = Checker::new();
    let a = c.get_string_literal_type("a");
    let b = c.get_string_literal_type("b");
    let union = c.get_union_type(&[a, b]);
    let t = get_template_literal_type(&mut c, &["x".into(), "".into()], &[union]);
    assert_eq!(c.type_to_string(t), "\"xa\" | \"xb\"");
}

#[test]
fn get_template_literal_type_generic_is_deferred() {
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let t = get_template_literal_type(&mut c, &["p_".into(), "".into()], &[tp]);
    assert!(c.get_type(t).flags().contains(TypeFlags::TEMPLATE_LITERAL));
}

#[test]
fn get_string_mapping_type_transforms_concrete_literals() {
    let mut c = Checker::new();
    let abc = c.get_string_literal_type("abc");
    let mapped = get_string_mapping_type(&mut c, StringMappingKind::Uppercase, abc);
    assert_eq!(c.type_to_string(mapped), "\"ABC\"");
}

#[test]
fn get_string_mapping_type_distributes_and_defers() {
    let mut c = Checker::new();
    let a = c.get_string_literal_type("a");
    let b = c.get_string_literal_type("b");
    let union = c.get_union_type(&[a, b]);
    let mapped = get_string_mapping_type(&mut c, StringMappingKind::Uppercase, union);
    assert_eq!(c.type_to_string(mapped), "\"A\" | \"B\"");
}

#[test]
fn is_pattern_literal_type_recognizes_primitive_placeholder_templates() {
    let mut c = Checker::new();
    let number = c.number_type();
    let pat = get_template_literal_type(&mut c, &["p".into(), "".into()], &[number]);
    assert!(is_pattern_literal_type(&c, pat));
}

#[test]
fn is_pattern_literal_type_false_for_generic_placeholder() {
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let generic = get_template_literal_type(&mut c, &["p_".into(), "".into()], &[tp]);
    assert!(!is_pattern_literal_type(&c, generic));
}

#[test]
fn is_pattern_literal_type_string_mapping_over_pattern_target() {
    let mut c = Checker::new();
    let number = c.number_type();
    let inner = get_template_literal_type(&mut c, &["n".into(), "".into()], &[number]);
    let mapped = get_string_mapping_type(&mut c, StringMappingKind::Uppercase, inner);
    assert!(is_pattern_literal_type(&c, mapped));
}
