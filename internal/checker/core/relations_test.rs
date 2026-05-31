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

// Go: internal/checker/relater.go:Relater.propertyRelatedTo (skipOptional for comparable)
#[test]
fn comparable_is_lenient_about_optional_vs_required() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface S { a?: string }\ninterface T { a: string }",
    );
    let mut c = Checker::new();
    let s = get_declared_type_of_symbol(&mut c, &p, sym(&p, "S"), None);
    let t = get_declared_type_of_symbol(&mut c, &p, sym(&p, "T"), None);
    // Assignability rejects an optional source against a required target.
    assert!(!c.is_type_assignable_to(&p, s, t));
    // Comparability passes `skipOptional`, so the optional/required mismatch is
    // tolerated and `S` is comparable to `T`.
    assert!(c.is_type_comparable_to(&p, s, t));
}

// Go: internal/checker/relater.go:typeRelatedToEachType (target intersection)
#[test]
fn assignable_to_target_intersection_requires_each_constituent() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { x: number }\ninterface B { y: string }\ninterface AB { x: number; y: string }",
    );
    let mut c = Checker::new();
    let a = get_declared_type_of_symbol(&mut c, &p, sym(&p, "A"), None);
    let b = get_declared_type_of_symbol(&mut c, &p, sym(&p, "B"), None);
    let ab_obj = get_declared_type_of_symbol(&mut c, &p, sym(&p, "AB"), None);
    let inter = c.get_intersection_type(&[a, b]);
    // `AB` has both `x` and `y`, so it is related to each constituent of `A & B`.
    assert!(c.is_type_assignable_to(&p, ab_obj, inter));
    // `A` alone is missing `y`, so it is not related to the `B` constituent.
    assert!(!c.is_type_assignable_to(&p, a, inter));
}

// Go: internal/checker/relater.go:someTypeRelatedToType (source intersection)
#[test]
fn assignable_from_source_intersection_needs_some_constituent() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { x: number }\ninterface B { y: string }",
    );
    let mut c = Checker::new();
    let a = get_declared_type_of_symbol(&mut c, &p, sym(&p, "A"), None);
    let b = get_declared_type_of_symbol(&mut c, &p, sym(&p, "B"), None);
    let inter = c.get_intersection_type(&[a, b]);
    // `A & B` is assignable to either constituent (a constituent relates to it).
    assert!(c.is_type_assignable_to(&p, inter, a));
    assert!(c.is_type_assignable_to(&p, inter, b));
}

// Go: internal/checker/relater.go:structuredTypeRelatedTo (source intersection
// viewed as an object falls back to propertiesRelatedTo over synthesized props)
#[test]
fn source_intersection_relates_structurally_to_object() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { x: number }\ninterface B { y: string }\ninterface AB { x: number; y: string }",
    );
    let mut c = Checker::new();
    let a = get_declared_type_of_symbol(&mut c, &p, sym(&p, "A"), None);
    let b = get_declared_type_of_symbol(&mut c, &p, sym(&p, "B"), None);
    let ab_obj = get_declared_type_of_symbol(&mut c, &p, sym(&p, "AB"), None);
    let inter = c.get_intersection_type(&[a, b]);
    // No single constituent has both members, but `A & B` viewed as an object
    // has `x` and `y`, so it is assignable to `AB`.
    assert!(c.is_type_assignable_to(&p, inter, ab_obj));
    // Guard: a lone `{x}` still lacks `y`, so it is not assignable to `AB`.
    assert!(!c.is_type_assignable_to(&p, a, ab_obj));
}

// 4am: the strictNullChecks assignability gate at the relation level. Under
// `--strictNullChecks false`, `undefined`/`null` are assignable to a
// non-nullable, non-union target (`string`); under `--strictNullChecks true`
// they are not. A checker built over a program carrying the option drives
// `c.strict_null_checks()`, which the simple-type rule reads.
// Go: internal/checker/relater.go:Checker.isSimpleTypeRelatedTo (strictNullChecks null/undefined)
#[test]
fn assignable_null_undefined_gated_on_strict_null_checks() {
    use crate::core::Checker;
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;

    let off = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "",
        CompilerOptions {
            strict_null_checks: Tristate::False,
            ..CompilerOptions::default()
        },
    ));
    let mut c = Checker::new_checker(off);
    let s = c.string_type();
    let null_t = c.null_type();
    let undef_t = c.undefined_type();
    let p = empty_program();
    // Non-strict: both nullable sources flow into the non-nullable `string`.
    assert!(c.is_type_assignable_to(&p, null_t, s));
    assert!(c.is_type_assignable_to(&p, undef_t, s));

    let on = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "",
        CompilerOptions {
            strict_null_checks: Tristate::True,
            ..CompilerOptions::default()
        },
    ));
    let mut c = Checker::new_checker(on);
    let s = c.string_type();
    let null_t = c.null_type();
    let undef_t = c.undefined_type();
    let void_t = c.void_type();
    // Strict: neither is assignable to the non-nullable `string`...
    assert!(!c.is_type_assignable_to(&p, null_t, s));
    assert!(!c.is_type_assignable_to(&p, undef_t, s));
    // ...but the flag-independent rules still hold: `undefined` -> `void`,
    // and each nullable source -> itself.
    assert!(c.is_type_assignable_to(&p, undef_t, void_t));
    assert!(c.is_type_assignable_to(&p, null_t, null_t));
    assert!(c.is_type_assignable_to(&p, undef_t, undef_t));
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
