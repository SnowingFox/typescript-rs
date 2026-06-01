use super::*;
use tsgo_ast::{Kind, ModifierFlags, ModifierList, NodeArena, NodeList};

// Builds a single modifier token list with the union flags computed.
fn modifier_list(arena: &mut NodeArena, kinds: &[Kind]) -> ModifierList {
    let nodes: Vec<_> = kinds.iter().map(|&k| arena.new_token(k)).collect();
    let flags = modifiers_to_flags(arena, &nodes);
    ModifierList {
        list: NodeList::new(nodes),
        modifier_flags: flags,
    }
}

// Go: util.go:isAlwaysType — only interfaces are always types.
#[test]
fn is_always_type_only_interface() {
    let mut a = NodeArena::new();
    let iname = a.new_identifier("I");
    let iface = a.new_interface_declaration(None, Some(iname), None, None, NodeList::new(vec![]));
    let tname = a.new_identifier("T");
    let num = a.new_token(Kind::NumberKeyword);
    let alias = a.new_type_alias_declaration(None, tname, None, num);
    assert!(is_always_type(&a, iface));
    assert!(!is_always_type(&a, alias));
}

// Go: ast.IsModifier — modifier keywords are modifiers; decorators and other
// tokens are not.
#[test]
fn is_modifier_recognizes_keywords() {
    assert!(is_modifier(Kind::ExportKeyword));
    assert!(is_modifier(Kind::DeclareKeyword));
    assert!(is_modifier(Kind::StaticKeyword));
    assert!(!is_modifier(Kind::Decorator));
    assert!(!is_modifier(Kind::NumberKeyword));
}

// Go: ast.CreateModifiersFromModifierFlags — canonical order export, declare,
// then the rest.
#[test]
fn modifier_kinds_from_flags_canonical_order() {
    assert_eq!(
        modifier_kinds_from_flags(ModifierFlags::EXPORT | ModifierFlags::AMBIENT),
        vec![Kind::ExportKeyword, Kind::DeclareKeyword],
    );
    assert_eq!(
        modifier_kinds_from_flags(ModifierFlags::STATIC | ModifierFlags::READONLY),
        vec![Kind::StaticKeyword, Kind::ReadonlyKeyword],
    );
    assert_eq!(
        modifier_kinds_from_flags(ModifierFlags::PRIVATE),
        vec![Kind::PrivateKeyword],
    );
    assert!(modifier_kinds_from_flags(ModifierFlags::empty()).is_empty());
}

// Go: ast.ModifiersToFlags — folds keyword kinds to their flag union.
#[test]
fn modifiers_to_flags_folds_union() {
    let mut a = NodeArena::new();
    let exp = a.new_token(Kind::ExportKeyword);
    let declare = a.new_token(Kind::DeclareKeyword);
    assert_eq!(
        modifiers_to_flags(&a, &[exp, declare]),
        ModifierFlags::EXPORT | ModifierFlags::AMBIENT,
    );
}

// Go: util.go:maskModifierFlags — masks, adds, then default-export fixups.
#[test]
fn mask_modifier_flags_adds_ambient() {
    let mut a = NodeArena::new();
    let name = a.new_identifier("f");
    let f = a.new_function_declaration(
        None,
        None,
        Some(name),
        None,
        NodeList::new(vec![]),
        None,
        None,
        None,
    );
    assert_eq!(
        mask_modifier_flags(&a, f, ModifierFlags::ALL, ModifierFlags::AMBIENT),
        ModifierFlags::AMBIENT,
    );
}

// Go: maskModifierFlags — a non-exported `default` regains `export`, and
// `declare` is dropped next to `default`.
#[test]
fn mask_modifier_flags_default_export_fixups() {
    let mut a = NodeArena::new();
    let mods = modifier_list(&mut a, &[Kind::DefaultKeyword]);
    let name = a.new_identifier("f");
    let f = a.new_function_declaration(
        Some(mods),
        None,
        Some(name),
        None,
        NodeList::new(vec![]),
        None,
        None,
        None,
    );
    // default but not exported -> export added; ambient addition dropped.
    let out = mask_modifier_flags(&a, f, ModifierFlags::ALL, ModifierFlags::AMBIENT);
    assert!(out.contains(ModifierFlags::DEFAULT));
    assert!(out.contains(ModifierFlags::EXPORT));
    assert!(!out.contains(ModifierFlags::AMBIENT));
}

// Go: ast.GetCombinedModifierFlags — a function's own flags (no folding).
#[test]
fn combined_modifier_flags_own() {
    let mut a = NodeArena::new();
    let mods = modifier_list(&mut a, &[Kind::ExportKeyword]);
    let name = a.new_identifier("f");
    let f = a.new_function_declaration(
        Some(mods),
        None,
        Some(name),
        None,
        NodeList::new(vec![]),
        None,
        None,
        None,
    );
    assert_eq!(combined_modifier_flags(&a, f), ModifierFlags::EXPORT);
    assert_eq!(
        effective_declaration_flags(&a, f, ModifierFlags::ALL & !ModifierFlags::EXPORT),
        ModifierFlags::empty(),
    );
}

// Go: ast.GetCombinedModifierFlags — a variable declaration folds the wrapping
// statement's `export` (requires parent wiring).
#[test]
fn combined_modifier_flags_folds_variable_statement() {
    use crate::test_support::parse_shared;
    let (ec, sf) = parse_shared("export const x: number = 1;");
    let ec = ec.borrow();
    let arena = ec.arena();
    // Find the VariableDeclaration node.
    let decl = (0..arena.node_count() as u32)
        .map(tsgo_ast::NodeId)
        .find(|&n| arena.kind(n) == Kind::VariableDeclaration)
        .expect("variable declaration");
    let _ = sf;
    assert!(combined_modifier_flags(arena, decl).contains(ModifierFlags::EXPORT));
}

// Go: util.go:hasSyntacticModifier(ParameterPropertyModifier) — `public x` is a
// parameter property; a plain parameter is not.
#[test]
fn has_parameter_property_modifier_detects_field_params() {
    let mut a = NodeArena::new();
    let public_mods = modifier_list(&mut a, &[Kind::PublicKeyword]);
    let pname = a.new_identifier("x");
    let num = a.new_token(Kind::NumberKeyword);
    let pp = a.new_parameter_declaration(Some(public_mods), None, pname, None, Some(num), None);
    let plain_name = a.new_identifier("y");
    let plain = a.new_parameter_declaration(None, None, plain_name, None, None, None);
    assert!(has_parameter_property_modifier(&a, pp));
    assert!(!has_parameter_property_modifier(&a, plain));
}

// Go: checker.isOptionalParameter (reachable) — `?` or a default makes a
// parameter optional; a rest parameter is not.
#[test]
fn is_optional_parameter_detects_optionality() {
    let mut a = NodeArena::new();
    let n1 = a.new_identifier("a");
    let q = a.new_token(Kind::QuestionToken);
    let p_question = a.new_parameter_declaration(None, None, n1, Some(q), None, None);
    let n2 = a.new_identifier("b");
    let init = a.new_numeric_literal("1", tsgo_ast::TokenFlags::empty());
    let p_init = a.new_parameter_declaration(None, None, n2, None, None, Some(init));
    let n3 = a.new_identifier("c");
    let p_plain = a.new_parameter_declaration(None, None, n3, None, None, None);
    let n4 = a.new_identifier("d");
    let dots = a.new_token(Kind::DotDotDotToken);
    let init2 = a.new_numeric_literal("2", tsgo_ast::TokenFlags::empty());
    let p_rest = a.new_parameter_declaration(None, Some(dots), n4, None, None, Some(init2));
    assert!(is_optional_parameter(&a, p_question));
    assert!(is_optional_parameter(&a, p_init));
    assert!(!is_optional_parameter(&a, p_plain));
    assert!(!is_optional_parameter(&a, p_rest));
}

// Go: ast.GetThisParameter / IsThisParameter — first parameter named `this`.
#[test]
fn this_parameter_detection() {
    let mut a = NodeArena::new();
    let this_name = a.new_identifier("this");
    let this_param = a.new_parameter_declaration(None, None, this_name, None, None, None);
    let other_name = a.new_identifier("x");
    let other = a.new_parameter_declaration(None, None, other_name, None, None, None);
    assert!(is_this_parameter(&a, this_param));
    assert!(!is_this_parameter(&a, other));
    assert_eq!(
        get_this_parameter(&a, &[this_param, other]),
        Some(this_param)
    );
    assert_eq!(get_this_parameter(&a, &[other]), None);
}

// Go: ast.GetFirstConstructorWithBody — first constructor with a body.
#[test]
fn get_first_constructor_with_body_finds_impl() {
    let mut a = NodeArena::new();
    let block = a.new_block(NodeList::new(vec![]));
    let ctor =
        a.new_constructor_declaration(None, None, NodeList::new(vec![]), None, None, Some(block));
    let bodyless =
        a.new_constructor_declaration(None, None, NodeList::new(vec![]), None, None, None);
    let pname = a.new_identifier("x");
    let prop = a.new_property_declaration(None, pname, None, None, None);
    assert_eq!(
        get_first_constructor_with_body(&a, &[prop, ctor]),
        Some(ctor)
    );
    assert_eq!(get_first_constructor_with_body(&a, &[bodyless, prop]), None);
}

// Go: node.Modifiers() — reads a node's own modifier list.
#[test]
fn node_modifiers_reads_list() {
    let mut a = NodeArena::new();
    let mods = modifier_list(&mut a, &[Kind::ExportKeyword]);
    let name = a.new_identifier("f");
    let f = a.new_function_declaration(
        Some(mods),
        None,
        Some(name),
        None,
        NodeList::new(vec![]),
        None,
        None,
        None,
    );
    let read = node_modifiers(&a, f).expect("modifiers");
    assert_eq!(read.modifier_flags, ModifierFlags::EXPORT);
    let bodyless =
        a.new_constructor_declaration(None, None, NodeList::new(vec![]), None, None, None);
    assert!(node_modifiers(&a, bodyless).is_none());
}
