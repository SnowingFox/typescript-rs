use crate::emitcontext::{AutoGenerateOptions, EmitContext};
use crate::generatedidentifierflags::GeneratedIdentifierFlags;
use tsgo_ast::{Kind, NodeFlags};

// Go: internal/printer/factory.go:NodeFactory.NewTempVariable
#[test]
fn new_temp_variable_is_synthesized_identifier() {
    let mut ec = EmitContext::new();
    let t = ec.factory().new_temp_variable();
    assert_eq!(ec.arena().kind(t), Kind::Identifier);
    assert!(ec.arena().flags(t).contains(NodeFlags::SYNTHESIZED));
    assert!(ec.get_auto_generate_info(t).unwrap().flags.is_auto());
}

// Go: internal/printer/factory.go:NodeFactory.NewLoopVariable
#[test]
fn new_loop_variable_records_loop_kind() {
    let mut ec = EmitContext::new();
    let l = ec.factory().new_loop_variable();
    assert!(ec.get_auto_generate_info(l).unwrap().flags.is_loop());
}

// Go: internal/printer/factory.go:NodeFactory.NewUniqueName
#[test]
fn new_unique_name_keeps_text_and_unique_kind() {
    let mut ec = EmitContext::new();
    let u = ec.factory().new_unique_name("foo");
    assert_eq!(ec.arena().text(u), "foo");
    assert!(ec.get_auto_generate_info(u).unwrap().flags.is_unique());
}

// Go: internal/printer/factory.go:NodeFactory.NewUniquePrivateName
#[test]
fn new_unique_private_name_is_private_identifier() {
    let mut ec = EmitContext::new();
    let p = ec.factory().new_unique_private_name("#foo");
    assert_eq!(ec.arena().kind(p), Kind::PrivateIdentifier);
    assert_eq!(ec.arena().text(p), "#foo");
}

// Go: internal/printer/factory.go:NodeFactory.newGeneratedPrivateIdentifier (panics on non-# text)
#[test]
#[should_panic(expected = "First character of private identifier must be #")]
fn new_unique_private_name_requires_hash() {
    let mut ec = EmitContext::new();
    let _ = ec.factory().new_unique_private_name("foo");
}

// Go: internal/printer/factory.go:NodeFactory (embedded ast.NodeFactory.NewIdentifier)
#[test]
fn new_identifier_is_synthesized() {
    let mut ec = EmitContext::new();
    let id = ec.factory().new_identifier("Infinity");
    assert_eq!(ec.arena().kind(id), Kind::Identifier);
    assert_eq!(ec.arena().text(id), "Infinity");
    assert!(ec.arena().flags(id).contains(NodeFlags::SYNTHESIZED));
}

// Go: internal/printer/factory.go:NodeFactory (embedded ast.NodeFactory literal/unary builders)
#[test]
fn new_literals_and_prefix_unary_are_synthesized() {
    use tsgo_ast::TokenFlags;
    let mut ec = EmitContext::new();
    let s = ec.factory().new_string_literal("hi", TokenFlags::NONE);
    assert_eq!(ec.arena().kind(s), Kind::StringLiteral);
    assert!(ec.arena().flags(s).contains(NodeFlags::SYNTHESIZED));

    let n = ec.factory().new_numeric_literal("1", TokenFlags::NONE);
    assert_eq!(ec.arena().kind(n), Kind::NumericLiteral);
    assert!(ec.arena().flags(n).contains(NodeFlags::SYNTHESIZED));

    let operand = ec.factory().new_identifier("Infinity");
    let neg = ec
        .factory()
        .new_prefix_unary_expression(Kind::MinusToken, operand);
    assert_eq!(ec.arena().kind(neg), Kind::PrefixUnaryExpression);
    assert!(ec.arena().flags(neg).contains(NodeFlags::SYNTHESIZED));
}

// Go: internal/printer/factory.go:NodeFactory.newGeneratedIdentifier (flags combine kind + options)
#[test]
fn ex_options_preserve_non_kind_flags() {
    let mut ec = EmitContext::new();
    let t = ec.factory().new_temp_variable_ex(AutoGenerateOptions {
        flags: GeneratedIdentifierFlags::RESERVED_IN_NESTED_SCOPES,
        ..Default::default()
    });
    let info = ec.get_auto_generate_info(t).unwrap();
    assert!(info.flags.is_auto());
    assert!(info.flags.is_reserved_in_nested_scopes());
}
