use super::*;
use crate::core::test_support::StubProgram;
use crate::core::Checker;
use tsgo_ast::NodeData;

#[test]
fn duplicate_string_index_signatures_reports_ts2374() {
    let text = "interface Foo { [a: string]: number; [b: string]: string; }";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts2374: Vec<_> = diags.iter().filter(|d| d.code == 2374).collect();
    assert_eq!(ts2374.len(), 2, "expected 2 TS2374: {:?}", ts2374);
    assert!(ts2374[0].message.contains("string"));
}

#[test]
fn different_key_type_index_signatures_no_ts2374() {
    let text = "interface Bar { [a: string]: any; [b: number]: any; }";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2374),
        "no TS2374 for distinct key types"
    );
}

#[test]
fn single_index_signature_no_ts2374() {
    let text = "interface Baz { [a: string]: number; }";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2374),
        "no TS2374 for single index sig"
    );
}

#[test]
fn class_duplicate_number_index_signatures_reports_ts2374() {
    let text = "class C { [a: number]: string; [b: number]: string; }";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts2374: Vec<_> = diags.iter().filter(|d| d.code == 2374).collect();
    assert_eq!(
        ts2374.len(),
        2,
        "expected 2 TS2374 for class dup num index sigs"
    );
}

#[test]
fn is_late_bindable_ast_computed_property() {
    let text = "declare const sym: unique symbol; interface I { [sym]: number; }";
    let p = StubProgram::parse_and_bind("/a.ts", text);
    let stmts = match p.arena().data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("sf"),
    };
    let members = match p.arena().data(stmts[1]) {
        NodeData::InterfaceDeclaration(d) => d.members.nodes.clone(),
        _ => panic!("i"),
    };
    let name = match p.arena().data(members[0]) {
        NodeData::PropertySignature(d) => d.name,
        _ => panic!("ps"),
    };
    assert!(is_late_bindable_ast(p.arena(), name));
}

#[test]
fn is_late_bindable_ast_plain_identifier_is_false() {
    let text = "interface I { x: number; }";
    let p = StubProgram::parse_and_bind("/a.ts", text);
    let stmts = match p.arena().data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("sf"),
    };
    let members = match p.arena().data(stmts[0]) {
        NodeData::InterfaceDeclaration(d) => d.members.nodes.clone(),
        _ => panic!("i"),
    };
    let name = match p.arena().data(members[0]) {
        NodeData::PropertySignature(d) => d.name,
        _ => panic!("ps"),
    };
    assert!(!is_late_bindable_ast(p.arena(), name));
}

#[test]
fn get_excluded_symbol_flags_property() {
    let excluded = get_excluded_symbol_flags(tsgo_ast::SymbolFlags::PROPERTY);
    assert!(excluded.intersects(tsgo_ast::SymbolFlags::PROPERTY_EXCLUDES));
}

#[test]
fn get_excluded_symbol_flags_method() {
    let excluded = get_excluded_symbol_flags(tsgo_ast::SymbolFlags::METHOD);
    assert!(excluded.intersects(tsgo_ast::SymbolFlags::METHOD_EXCLUDES));
}

#[test]
fn get_excluded_symbol_flags_empty() {
    assert!(get_excluded_symbol_flags(tsgo_ast::SymbolFlags::empty()).is_empty());
}

#[test]
fn get_members_of_declaration_interface() {
    let text = "interface I { x: number; y: string; }";
    let p = StubProgram::parse_and_bind("/a.ts", text);
    let stmts = match p.arena().data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("sf"),
    };
    assert_eq!(get_members_of_declaration(&p, stmts[0]).len(), 2);
}

#[test]
fn get_members_of_declaration_class() {
    let text = "class C { x: number = 0; }";
    let p = StubProgram::parse_and_bind("/a.ts", text);
    let stmts = match p.arena().data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("sf"),
    };
    assert_eq!(get_members_of_declaration(&p, stmts[0]).len(), 1);
}

#[test]
fn is_type_usable_as_property_name_string_literal() {
    assert!(is_type_usable_as_property_name(
        crate::core::types::TypeFlags::STRING_LITERAL
    ));
}

#[test]
fn is_type_usable_as_property_name_number_literal() {
    assert!(is_type_usable_as_property_name(
        crate::core::types::TypeFlags::NUMBER_LITERAL
    ));
}

#[test]
fn is_type_usable_as_property_name_string_not() {
    assert!(!is_type_usable_as_property_name(
        crate::core::types::TypeFlags::STRING
    ));
}

#[test]
fn get_property_name_from_string_literal() {
    let mut c = Checker::new();
    let t = c.get_string_literal_type("hello");
    assert_eq!(get_property_name_from_type(&c, t), "hello");
}

#[test]
fn get_property_name_from_number_literal() {
    let mut c = Checker::new();
    let t = c.get_number_literal_type(tsgo_jsnum::Number::from(42.0));
    assert_eq!(get_property_name_from_type(&c, t), "42");
}
