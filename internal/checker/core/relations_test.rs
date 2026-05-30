use super::*;
use crate::core::declared_types::get_declared_type_of_symbol;
use crate::core::program::BoundProgram;
use crate::core::test_support::StubProgram;
use crate::core::types::LiteralValue;
use crate::core::Checker;
use tsgo_ast::SymbolId;

// An empty program for relation checks that never touch the AST.
fn empty_program() -> StubProgram {
    StubProgram::parse_and_bind("/a.ts", "")
}

// Looks up a top-level local symbol by name.
fn sym(p: &StubProgram, name: &str) -> SymbolId {
    *p.locals(p.root())
        .expect("locals")
        .get(name)
        .unwrap_or_else(|| panic!("missing {name}"))
}

// Go: internal/checker/relater.go:isSimpleTypeRelatedTo (any/unknown/never)
#[test]
fn assignable_top_and_bottom() {
    let mut c = Checker::new();
    let p = empty_program();
    let s = c.string_type();
    assert!(c.is_type_assignable_to(&p, s, c.any_type()));
    assert!(c.is_type_assignable_to(&p, s, c.unknown_type()));
    assert!(c.is_type_assignable_to(&p, c.never_type(), s)); // never -> anything
    assert!(!c.is_type_assignable_to(&p, s, c.never_type())); // string -> never is false
    assert!(c.is_type_assignable_to(&p, c.any_type(), s)); // any -> anything (assignable)
}

// Go: internal/checker/relater.go:isSimpleTypeRelatedTo (literal -> primitive)
#[test]
fn assignable_literal_to_primitive() {
    let mut c = Checker::new();
    let p = empty_program();
    let str_lit = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("a".into()),
        None,
    );
    assert!(c.is_type_assignable_to(&p, str_lit, c.string_type()));
    assert!(!c.is_type_assignable_to(&p, str_lit, c.number_type()));
    // Boolean literal -> boolean (the `false | true` union).
    assert!(c.is_type_assignable_to(&p, c.false_type(), c.boolean_type()));
    assert!(!c.is_type_assignable_to(&p, c.false_type(), c.number_type()));
}

// Go: internal/checker/relater.go:isTypeIdenticalTo
#[test]
fn identity_of_intrinsics() {
    let mut c = Checker::new();
    let p = empty_program();
    assert!(c.is_type_identical_to(&p, c.string_type(), c.string_type()));
    assert!(!c.is_type_identical_to(&p, c.string_type(), c.number_type()));
    assert!(c.is_type_identical_to(&p, c.any_type(), c.any_type()));
}

// Go: internal/checker/relater.go:structuredTypeRelatedTo (union source/target)
#[test]
fn assignable_unions() {
    let mut c = Checker::new();
    let p = empty_program();
    let s = c.string_type();
    let n = c.number_type();
    let s_or_n = c.string_or_number_type();
    // member -> union
    assert!(c.is_type_assignable_to(&p, s, s_or_n));
    assert!(c.is_type_assignable_to(&p, n, s_or_n));
    // union -> union (all members relate)
    assert!(c.is_type_assignable_to(&p, s_or_n, s_or_n));
    // union -> member fails (number is not assignable to string)
    assert!(!c.is_type_assignable_to(&p, s_or_n, s));
}

// Go: internal/checker/relater.go:isTypeComparableTo (bidirectional simple)
#[test]
fn comparable_is_bidirectional() {
    let mut c = Checker::new();
    let p = empty_program();
    let str_lit = c.new_literal_type(
        TypeFlags::STRING_LITERAL,
        LiteralValue::String("a".into()),
        None,
    );
    // string-literal comparable to string, and string comparable to string-literal.
    assert!(c.is_type_comparable_to(&p, str_lit, c.string_type()));
    assert!(c.is_type_comparable_to(&p, c.string_type(), str_lit));
    // string not comparable to number.
    assert!(!c.is_type_comparable_to(&p, c.string_type(), c.number_type()));
}

// Go: internal/checker/relater.go:propertiesRelatedTo (structural objects)
#[test]
fn assignable_structural_objects() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { x: number }\ninterface B { x: number }\ninterface C { x: string }",
    );
    let mut c = Checker::new();
    let a = get_declared_type_of_symbol(&mut c, &p, sym(&p, "A"), None);
    let b = get_declared_type_of_symbol(&mut c, &p, sym(&p, "B"), None);
    let cc = get_declared_type_of_symbol(&mut c, &p, sym(&p, "C"), None);
    // A and B are structurally identical -> assignable both ways.
    assert!(c.is_type_assignable_to(&p, a, b));
    assert!(c.is_type_assignable_to(&p, b, a));
    // A (x: number) is not assignable to C (x: string).
    assert!(!c.is_type_assignable_to(&p, a, cc));
}

// Go: internal/checker/relater.go:propertiesRelatedTo (missing property)
#[test]
fn assignable_structural_subset() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface P { x: number }\ninterface Q { x: number; y: string }",
    );
    let mut c = Checker::new();
    let pp = get_declared_type_of_symbol(&mut c, &p, sym(&p, "P"), None);
    let q = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Q"), None);
    // Q has all of P's members (and more) -> Q assignable to P.
    assert!(c.is_type_assignable_to(&p, q, pp));
    // P is missing `y` -> not assignable to Q.
    assert!(!c.is_type_assignable_to(&p, pp, q));
}

// Go: internal/checker/relater.go:Relation.get/set
#[test]
fn relation_cache_get_set() {
    let mut cache = RelationCache::default();
    assert!(cache.is_empty());
    assert_eq!(
        cache.get(RelationKind::Assignable, TypeId(1), TypeId(2)),
        None
    );
    cache.set(RelationKind::Assignable, TypeId(1), TypeId(2), true);
    assert_eq!(
        cache.get(RelationKind::Assignable, TypeId(1), TypeId(2)),
        Some(true)
    );
    // A different relation kind is a distinct key.
    assert_eq!(
        cache.get(RelationKind::Identity, TypeId(1), TypeId(2)),
        None
    );
    assert_eq!(cache.len(), 1);
}
