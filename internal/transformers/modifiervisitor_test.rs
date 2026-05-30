use super::*;
use crate::test_support::parse_shared;
use tsgo_ast::{Kind, NodeData};

// Pulls the modifier list off the first statement of a parsed source file.
fn first_statement_modifiers(
    arena: &tsgo_ast::NodeArena,
    source_file: tsgo_ast::NodeId,
) -> ModifierList {
    let stmt = match arena.data(source_file) {
        NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => panic!("expected source file"),
    };
    match arena.data(stmt) {
        NodeData::ClassDeclaration(d) => d.modifiers.clone().expect("class has modifiers"),
        _ => panic!("expected class declaration"),
    }
}

// Go: internal/transformers/modifiervisitor.go:ExtractModifiers (nil input)
#[test]
fn extract_modifiers_none_returns_none() {
    let ec = tsgo_printer::EmitContext::new();
    assert!(extract_modifiers(&ec, None, ModifierFlags::EXPORT).is_none());
}

// Go: internal/transformers/modifiervisitor.go:ExtractModifiers
// Keeps only the allowed modifier and recomputes the flag union.
#[test]
fn extract_modifiers_filters_disallowed() {
    let (ec, source_file) = parse_shared("export abstract class C {}");
    let ec_ref = ec.borrow();
    let modifiers = first_statement_modifiers(ec_ref.arena(), source_file);
    assert_eq!(
        modifiers.modifier_flags,
        ModifierFlags::EXPORT | ModifierFlags::ABSTRACT
    );

    let extracted =
        extract_modifiers(&ec_ref, Some(&modifiers), ModifierFlags::EXPORT).expect("some");
    assert_eq!(extracted.modifier_flags, ModifierFlags::EXPORT);
    assert_eq!(extracted.list.nodes.len(), 1);
    assert_eq!(
        ec_ref.arena().kind(extracted.list.nodes[0]),
        Kind::ExportKeyword
    );
    // The filtered list keeps the original list's source range.
    assert_eq!(extracted.list.loc, modifiers.list.loc);
}

// Go: internal/transformers/modifiervisitor.go:ExtractModifiers (unchanged path)
// When every modifier is allowed, all are kept.
#[test]
fn extract_modifiers_keeps_all_when_allowed() {
    let (ec, source_file) = parse_shared("export abstract class C {}");
    let ec_ref = ec.borrow();
    let modifiers = first_statement_modifiers(ec_ref.arena(), source_file);

    let extracted = extract_modifiers(
        &ec_ref,
        Some(&modifiers),
        ModifierFlags::EXPORT | ModifierFlags::ABSTRACT,
    )
    .expect("some");
    assert_eq!(extracted.list.nodes.len(), 2);
    assert_eq!(
        extracted.modifier_flags,
        ModifierFlags::EXPORT | ModifierFlags::ABSTRACT
    );
}
