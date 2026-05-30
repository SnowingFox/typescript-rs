use super::*;
use crate::core::types::{LiteralValue, TypeFlags};
use crate::core::Checker;

// Go: internal/checker/utilities.go:Checker.getTypeWithFacts (truthiness)
#[test]
fn type_with_facts_drops_falsy_literal_subtypes() {
    let mut c = Checker::new();
    let empty = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String(String::new()),
        None,
    );
    let a = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("a".into()),
        None,
    );
    let union = c.get_union_type(&[empty, a]);
    // `"" | "a"` keeps only `"a"` on the truthy side, only `""` on the falsy side.
    assert_eq!(c.get_type_with_facts(union, TypeFacts::TRUTHY), a);
    assert_eq!(c.get_type_with_facts(union, TypeFacts::FALSY), empty);
}

// Go: internal/checker/utilities.go:Checker.getTypeFacts / hasTypeFacts
#[test]
fn type_facts_of_primitives_and_literals() {
    let c = Checker::new();
    // `string` can be either truthy or falsy.
    assert_eq!(
        c.get_type_facts(c.string_type()),
        TypeFacts::TRUTHY | TypeFacts::FALSY
    );
    // `undefined`/`null` are only falsy.
    assert_eq!(c.get_type_facts(c.undefined_type()), TypeFacts::FALSY);
    assert_eq!(c.get_type_facts(c.null_type()), TypeFacts::FALSY);
    // `has_type_facts` is the OR over union members.
    let s = c.string_type();
    assert!(c.has_type_facts(s, TypeFacts::FALSY));
    let n = c.number_type();
    assert!(c.has_type_facts(n, TypeFacts::TRUTHY));
}
