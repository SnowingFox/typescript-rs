use super::*;
use signatures::SignatureFlags;
use symbols::{SymbolFlags, SymbolId};

// Go: internal/checker/checker.go:NewChecker (intrinsic init order/count)
#[test]
fn new_constructs_intrinsics_in_order() {
    let c = Checker::new();
    // 4b constructs 21 types in allocation order (the 4a intrinsics plus the
    // boolean literal/union types and the string|number / number|bigint unions).
    assert_eq!(c.type_count(), 21);
    assert_eq!(c.any_type(), TypeId(1));
    assert_eq!(c.auto_type(), TypeId(2));
    assert_eq!(c.error_type(), TypeId(3));
    assert_eq!(c.unknown_type(), TypeId(4));
    assert_eq!(c.bigint_type(), TypeId(9));
    assert_eq!(c.regular_false_type(), TypeId(10));
    assert_eq!(c.false_type(), TypeId(11));
    assert_eq!(c.regular_true_type(), TypeId(12));
    assert_eq!(c.true_type(), TypeId(13));
    assert_eq!(c.boolean_type(), TypeId(14));
    assert_eq!(c.non_primitive_type(), TypeId(19));
    assert_eq!(c.string_or_number_type(), TypeId(20));
    assert_eq!(c.number_or_bigint_type(), TypeId(21));
}

// Go: internal/checker/checker.go:NewChecker (boolean = false | true union)
#[test]
fn boolean_is_union_of_false_true_literals() {
    let c = Checker::new();
    let members = c.get_type(c.boolean_type()).union_types().unwrap();
    // Union members are id-sorted: regular_false (10) then regular_true (12).
    assert_eq!(members, &[c.regular_false_type(), c.regular_true_type()]);
    assert_eq!(c.get_type(c.boolean_type()).flags(), TypeFlags::UNION);
}

// Go: internal/checker/printer.go:Checker.TypeToString (literal/union shapes)
#[test]
fn type_to_string_of_literals_and_unions() {
    let c = Checker::new();
    assert_eq!(c.type_to_string(c.false_type()), "false");
    assert_eq!(c.type_to_string(c.true_type()), "true");
    assert_eq!(c.type_to_string(c.regular_false_type()), "false");
    assert_eq!(
        c.type_to_string(c.string_or_number_type()),
        "string | number"
    );
    assert_eq!(
        c.type_to_string(c.number_or_bigint_type()),
        "number | bigint"
    );
}

// Go: internal/checker/checker.go:NewChecker (literal fresh/regular pairing)
#[test]
fn boolean_literal_fresh_regular_links() {
    let c = Checker::new();
    let regular_false = c.regular_false_type();
    let false_t = c.false_type();
    match &c.get_type(regular_false).data {
        TypeData::Literal(d) => {
            assert_eq!(d.regular_type, Some(regular_false)); // regular points to self
            assert_eq!(d.fresh_type, Some(false_t));
        }
        _ => panic!("regular_false should be a literal type"),
    }
    match &c.get_type(false_t).data {
        TypeData::Literal(d) => {
            assert_eq!(d.regular_type, Some(regular_false));
            assert_eq!(d.fresh_type, Some(false_t));
        }
        _ => panic!("false should be a literal type"),
    }
}

// Go: internal/checker/checker.go:Checker.getUnionType (dedup / collapse / intern)
#[test]
fn get_union_type_dedups_collapses_and_interns() {
    let mut c = Checker::new();
    // Empty union is `never`.
    assert_eq!(c.get_union_type(&[]), c.never_type());
    // A single (or all-duplicate) member collapses to that member.
    let s = c.string_type();
    assert_eq!(c.get_union_type(&[s]), s);
    assert_eq!(c.get_union_type(&[s, s]), s);
    // Two members intern to the same id regardless of order, matching the
    // `string | number` union built during construction.
    let expected = c.string_or_number_type();
    assert_eq!(
        c.get_union_type(&[c.string_type(), c.number_type()]),
        expected
    );
    assert_eq!(
        c.get_union_type(&[c.number_type(), c.string_type()]),
        expected
    );
}

// Go: internal/checker/printer.go:Checker.TypeToString (intrinsic names)
#[test]
fn type_to_string_of_intrinsics() {
    let c = Checker::new();
    assert_eq!(c.type_to_string(c.any_type()), "any");
    assert_eq!(c.type_to_string(c.unknown_type()), "unknown");
    assert_eq!(c.type_to_string(c.undefined_type()), "undefined");
    assert_eq!(c.type_to_string(c.null_type()), "null");
    assert_eq!(c.type_to_string(c.void_type()), "void");
    assert_eq!(c.type_to_string(c.string_type()), "string");
    assert_eq!(c.type_to_string(c.number_type()), "number");
    assert_eq!(c.type_to_string(c.bigint_type()), "bigint");
    assert_eq!(c.type_to_string(c.es_symbol_type()), "symbol");
    assert_eq!(c.type_to_string(c.never_type()), "never");
    assert_eq!(c.type_to_string(c.non_primitive_type()), "object");
    // `error` and `autoType` both print as their intrinsic name.
    assert_eq!(c.type_to_string(c.error_type()), "error");
    assert_eq!(c.type_to_string(c.auto_type()), "any");
}

// Go: internal/checker/checker.go:NewChecker (intrinsic flags)
#[test]
fn intrinsics_have_expected_flags() {
    let c = Checker::new();
    assert_eq!(c.get_type(c.string_type()).flags(), TypeFlags::STRING);
    assert_eq!(c.get_type(c.number_type()).flags(), TypeFlags::NUMBER);
    assert_eq!(c.get_type(c.any_type()).flags(), TypeFlags::ANY);
    assert_eq!(c.get_type(c.never_type()).flags(), TypeFlags::NEVER);
    assert_eq!(
        c.get_type(c.non_primitive_type()).flags(),
        TypeFlags::NON_PRIMITIVE
    );
    // `autoType` carries the NonInferrable object flag.
    assert_eq!(
        c.get_type(c.auto_type()).object_flags(),
        ObjectFlags::NON_INFERRABLE_TYPE
    );
}

// Go: internal/checker/checker.go:Checker.newType (strips cache-only flags)
#[test]
fn new_type_clears_cache_only_object_flags() {
    let mut c = Checker::new();
    let id = c.new_type(
        TypeFlags::OBJECT,
        ObjectFlags::CLASS
            | ObjectFlags::MEMBERS_RESOLVED
            | ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES,
        TypeData::Intrinsic(IntrinsicType {
            intrinsic_name: "(stub)".to_string(),
        }),
    );
    let of = c.get_type(id).object_flags();
    assert!(of.contains(ObjectFlags::CLASS));
    assert!(!of.contains(ObjectFlags::MEMBERS_RESOLVED));
    assert!(!of.contains(ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES));
}

// Go: internal/checker/types.go:Signature (arena wired into the checker)
#[test]
fn checker_signature_and_index_info_arenas() {
    let mut c = Checker::new();
    let sig = c.new_signature(Signature::new(SignatureFlags::CONSTRUCT));
    assert!(c.signature(sig).flags.contains(SignatureFlags::CONSTRUCT));
    let key = c.string_type();
    let val = c.number_type();
    let info = c.new_index_info(IndexInfo::new(key, val, true));
    assert!(c.index_info(info).is_readonly);
    assert_eq!(c.index_info(info).value_type, val);
}

// Go: internal/checker/types.go:SymbolReferenceLinks (reference-kind accumulation)
#[test]
fn symbol_reference_kinds_accumulate() {
    let mut c = Checker::new();
    let s = SymbolId(42);
    assert_eq!(c.symbol_reference_kinds(s), SymbolFlags::empty());
    c.mark_symbol_referenced(s, SymbolFlags::VALUE);
    c.mark_symbol_referenced(s, SymbolFlags::TYPE);
    assert_eq!(
        c.symbol_reference_kinds(s),
        SymbolFlags::VALUE | SymbolFlags::TYPE
    );
    // A different symbol is unaffected.
    assert_eq!(c.symbol_reference_kinds(SymbolId(43)), SymbolFlags::empty());
}
