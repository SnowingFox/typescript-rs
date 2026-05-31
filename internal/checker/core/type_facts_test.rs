use super::*;
use crate::core::test_support::StubProgram;
use crate::core::types::{LiteralValue, TypeFlags};
use crate::core::Checker;
use crate::type_to_string;

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

// 4ay tracer (genuine RED): under `strictNullChecks`, `getNonNullableType`
// removes the `undefined` constituent from a union, reducing `string | undefined`
// to `string`. A default-options intrinsic checker has `strictNullChecks` in
// effect (Go's `strict != false` rule), so the reduction applies.
// Go: internal/checker/checker.go:Checker.GetNonNullableType / getAdjustedTypeWithFacts(NEUndefinedOrNull)
#[test]
fn get_non_null_type_strict_removes_undefined() {
    let mut c = Checker::new();
    // A parsed-and-bound program is only needed so `type_to_string` can render
    // the reduced type; `get_non_null_type` itself needs no program.
    let p = StubProgram::parse_and_bind("/a.ts", "");
    let s = c.string_type();
    let u = c.undefined_type();
    let su = c.get_union_type(&[s, u]);
    let reduced = c.get_non_null_type(su);
    assert_eq!(type_to_string(&mut c, &p, reduced), "string");
}

// 4ay coverage guard (green-on-arrival): the same constituent filter drops the
// `null` member, so `string | null` reduces to `string` under strictNullChecks.
// Go: internal/checker/checker.go:Checker.getAdjustedTypeWithFacts (NEUndefinedOrNull)
#[test]
fn get_non_null_type_strict_removes_null() {
    let mut c = Checker::new();
    let p = StubProgram::parse_and_bind("/a.ts", "");
    let s = c.string_type();
    let null = c.null_type();
    let sn = c.get_union_type(&[s, null]);
    let reduced = c.get_non_null_type(sn);
    assert_eq!(type_to_string(&mut c, &p, reduced), "string");
}

// 4ay coverage guard (green-on-arrival): both `null` and `undefined` are dropped,
// so `string | null | undefined` reduces to `string` under strictNullChecks.
// Go: internal/checker/checker.go:Checker.getAdjustedTypeWithFacts (NEUndefinedOrNull)
#[test]
fn get_non_null_type_strict_removes_null_and_undefined() {
    let mut c = Checker::new();
    let p = StubProgram::parse_and_bind("/a.ts", "");
    let s = c.string_type();
    let null = c.null_type();
    let u = c.undefined_type();
    let snu = c.get_union_type(&[s, null, u]);
    let reduced = c.get_non_null_type(snu);
    assert_eq!(type_to_string(&mut c, &p, reduced), "string");
}

// 4ay slice 4 (genuine RED): under `--strictNullChecks false`, `getNonNullableType`
// is the identity (Go gates the whole reduction on `c.strictNullChecks`; in
// non-strict mode unions never carry `null`/`undefined` anyway). The same
// `string | undefined` union therefore comes back unchanged. The diff against the
// strict reduction above is the observable gate.
// Go: internal/checker/checker.go:Checker.GetNonNullableType (`if c.strictNullChecks`)
#[test]
fn get_non_null_type_non_strict_is_identity() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        strict_null_checks: Tristate::False,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts", "", options,
    ));
    let mut c = Checker::new_checker(p);
    let s = c.string_type();
    let u = c.undefined_type();
    let su = c.get_union_type(&[s, u]);
    // Identity: the union id is returned unchanged (no nullable removal).
    assert_eq!(c.get_non_null_type(su), su);
}
