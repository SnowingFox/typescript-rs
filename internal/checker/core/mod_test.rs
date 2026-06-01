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

// 4bc: `get_string_literal_type` interns by value — equal strings share one id,
// distinct strings get distinct ids, and the result prints as the quoted value.
// Go: internal/checker/checker.go:Checker.getStringLiteralType(25164)
#[test]
fn get_string_literal_type_interns_by_value() {
    let mut c = Checker::new();
    let a1 = c.get_string_literal_type("a");
    let a2 = c.get_string_literal_type("a");
    assert_eq!(a1, a2, "equal string literals must intern to one id");
    assert_ne!(a1, c.get_string_literal_type("b"));
    assert_eq!(c.type_to_string(a1), "\"a\"");
    assert_eq!(c.get_type(a1).flags(), TypeFlags::STRING_LITERAL);
    // The interned literal is its own regular type (no fresh/regular widening
    // is modeled yet), so the relation/normalization paths treat it as regular.
    assert_eq!(c.regular_type_of_literal_type(a1), a1);
}

// 4bc: `get_number_literal_type` interns by value, with Go-faithful
// canonicalization — all `NaN`s collapse to one type and `+0`/`-0` collapse.
// Go: internal/checker/checker.go:Checker.getNumberLiteralType(25173)
#[test]
fn get_number_literal_type_interns_by_value_with_nan_and_zero_canonicalization() {
    use tsgo_jsnum::Number;
    let mut c = Checker::new();
    let one = c.get_number_literal_type(Number::from(1.0));
    assert_eq!(one, c.get_number_literal_type(Number::from(1.0)));
    assert_ne!(one, c.get_number_literal_type(Number::from(2.0)));
    assert_eq!(c.type_to_string(one), "1");
    assert_eq!(c.get_type(one).flags(), TypeFlags::NUMBER_LITERAL);
    // Signed zeros collapse (Go's float map-key treats `0 == -0`).
    let pos_zero = c.get_number_literal_type(Number::from(0.0));
    let neg_zero = c.get_number_literal_type(Number::from(-0.0));
    assert_eq!(pos_zero, neg_zero, "+0 and -0 must intern to one id");
    // All NaNs collapse to one type (Go caches `nanType` separately because a
    // float NaN map-key never matches itself).
    let nan1 = c.get_number_literal_type(Number::nan());
    let nan2 = c.get_number_literal_type(Number::nan());
    assert_eq!(nan1, nan2, "every NaN literal must intern to one id");
}

// Go: internal/checker/checker.go:Checker.getIntersectionType (intern by members)
#[test]
fn get_intersection_type_interns_by_members() {
    let mut c = Checker::new();
    // Two distinct type parameters never reduce, so `A & B` is a real
    // two-member intersection.
    let a = c.new_type_parameter(None);
    let b = c.new_type_parameter(None);
    let ab = c.get_intersection_type(&[a, b]);
    // The constructed type is an intersection of the two members.
    assert_eq!(c.get_type(ab).flags(), TypeFlags::INTERSECTION);
    assert_eq!(c.get_type(ab).intersection_types().unwrap(), &[a, b]);
    // Re-requesting the same constituents (in either order) yields the same id.
    assert_eq!(c.get_intersection_type(&[a, b]), ab);
    assert_eq!(c.get_intersection_type(&[b, a]), ab);
}

// Go: internal/checker/checker.go:Checker.getIntersectionType (trivial reductions)
#[test]
fn get_intersection_type_trivial_reductions() {
    let mut c = Checker::new();
    let a = c.new_type_parameter(None);
    // An empty intersection is `unknown`; a single member collapses.
    assert_eq!(c.get_intersection_type(&[]), c.unknown_type());
    assert_eq!(c.get_intersection_type(&[a]), a);
    // `unknown` is the identity element: it is dropped from the set.
    assert_eq!(c.get_intersection_type(&[a, c.unknown_type()]), a);
    assert_eq!(
        c.get_intersection_type(&[c.unknown_type()]),
        c.unknown_type()
    );
    // `never` short-circuits the whole intersection.
    assert_eq!(
        c.get_intersection_type(&[a, c.never_type()]),
        c.never_type()
    );
    // `any` short-circuits to `any`.
    assert_eq!(c.get_intersection_type(&[a, c.any_type()]), c.any_type());
}

// Go: internal/checker/checker.go:Checker.addTypeToIntersection (flatten + dedup)
#[test]
fn get_intersection_type_flattens_and_dedups() {
    let mut c = Checker::new();
    let a = c.new_type_parameter(None);
    let b = c.new_type_parameter(None);
    let cc = c.new_type_parameter(None);
    // Duplicate members collapse to a single occurrence.
    let ab = c.get_intersection_type(&[a, b]);
    assert_eq!(c.get_intersection_type(&[a, a, b]), ab);
    // A nested intersection is flattened into the outer set: `(A & B) & C`
    // interns identically to `A & B & C`.
    let abc = c.get_intersection_type(&[a, b, cc]);
    assert_eq!(c.get_intersection_type(&[ab, cc]), abc);
    assert_eq!(c.get_type(abc).intersection_types().unwrap(), &[a, b, cc]);
}

// Go: internal/checker/checker.go:Checker.getIntersectionTypeEx (union
// distribution via getCrossProductIntersections)
#[test]
fn get_intersection_type_distributes_over_union() {
    let mut c = Checker::new();
    let x = c.new_type_parameter(None);
    let a = c.new_type_parameter(None);
    let b = c.new_type_parameter(None);
    let a_or_b = c.get_union_type(&[a, b]);
    // `X & (A | B)` normalizes to `(X & A) | (X & B)`.
    let result = c.get_intersection_type(&[x, a_or_b]);
    let xa = c.get_intersection_type(&[x, a]);
    let xb = c.get_intersection_type(&[x, b]);
    let expected = c.get_union_type(&[xa, xb]);
    assert_eq!(result, expected);
    // The distributed result is a union (of two intersections), not an
    // intersection that still contains a union constituent.
    assert_eq!(c.get_type(result).flags(), TypeFlags::UNION);
}

// Go: internal/checker/checker.go:Checker.getIntersectionTypeEx
// (disjoint-domain reduction via the `TypeFlagsDisjointDomains` guard)
#[test]
fn get_intersection_type_disjoint_domains_reduce_to_never() {
    let mut c = Checker::new();
    // `string & number` is empty: a string-like and a number-like type belong to
    // disjoint domains, so the intersection reduces to `never`.
    assert_eq!(
        c.get_intersection_type(&[c.string_type(), c.number_type()]),
        c.never_type()
    );
    // Other disjoint primitive-domain pairs reduce too.
    assert_eq!(
        c.get_intersection_type(&[c.number_type(), c.bigint_type()]),
        c.never_type()
    );
    assert_eq!(
        c.get_intersection_type(&[c.string_type(), c.boolean_type()]),
        c.never_type()
    );
    assert_eq!(
        c.get_intersection_type(&[c.es_symbol_type(), c.number_type()]),
        c.never_type()
    );
    // `object` (the non-primitive intrinsic) is disjoint from every primitive.
    assert_eq!(
        c.get_intersection_type(&[c.non_primitive_type(), c.string_type()]),
        c.never_type()
    );
    // A primitive intersected with a (non-disjoint) type variable still interns
    // as a real intersection — the disjoint guard must not over-fire.
    let t = c.new_type_parameter(None);
    let st = c.get_intersection_type(&[c.string_type(), t]);
    assert_eq!(c.get_type(st).flags(), TypeFlags::INTERSECTION);
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

// Go: internal/checker/checker.go:NewChecker (program-taking entry, P6 seam)
#[test]
fn new_checker_initializes_intrinsics() {
    let p = std::rc::Rc::new(crate::core::test_support::StubProgram::parse_and_bind(
        "/a.ts", "",
    ));
    let c = Checker::new_checker(p);
    assert_eq!(c.type_count(), Checker::new().type_count());
    assert_eq!(c.type_to_string(c.string_type()), "string");
}

// Go: internal/checker/checker.go:NewChecker (retains `c.program = program`)
#[test]
fn new_checker_retains_program() {
    use crate::core::program::BoundProgram;
    let p = std::rc::Rc::new(crate::core::test_support::StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string;",
    ));
    let root = p.root();
    let c = Checker::new_checker(p);
    // The checker exposes the program it was constructed over.
    assert_eq!(c.program().map(|prog| prog.root()), Some(root));
}

// Go: internal/checker/checker.go:Checker.getGlobalSymbol
#[test]
fn get_global_symbol_resolves_global_value_by_meaning() {
    // A script source file's top-level declarations are the synthetic globals
    // (the merged-globals stand-in until lib.d.ts loading lands in P6).
    let p = std::rc::Rc::new(crate::core::test_support::StubProgram::parse_and_bind(
        "/a.ts",
        "declare var g: number;",
    ));
    let c = Checker::new_checker(p);
    // The global value `g` resolves under VALUE meaning.
    let g = c
        .get_global_symbol("g", SymbolFlags::VALUE)
        .expect("global g");
    assert!(c
        .program()
        .unwrap()
        .symbol(g)
        .flags
        .contains(SymbolFlags::FUNCTION_SCOPED_VARIABLE));
    // A name absent from the globals stays unresolved (Go returns nil → 2304).
    assert_eq!(c.get_global_symbol("nope", SymbolFlags::VALUE), None);
    // Meaning filters the lookup: `g` is a value, not a type.
    assert_eq!(c.get_global_symbol("g", SymbolFlags::TYPE), None);
}

// Go: internal/checker/checker.go:Checker.getGlobalType
#[test]
fn get_global_type_resolves_global_interface_off_program() {
    // A synthetic global `interface Foo` plus a global value `foo` (so a
    // value-only name can be distinguished from a global type).
    let p = std::rc::Rc::new(crate::core::test_support::StubProgram::parse_and_bind(
        "/a.ts",
        "interface Foo {\n  bar: string;\n}\ndeclare const foo: Foo;",
    ));
    let mut c = Checker::new_checker(p);
    // The global type `Foo` resolves to an object (interface) type.
    let foo = c.get_global_type("Foo").expect("global type Foo");
    assert!(c.get_type(foo).as_object().is_some());
    // Cached on a second lookup (same id).
    assert_eq!(c.get_global_type("Foo"), Some(foo));
    // A value-only name is not a global type.
    assert_eq!(c.get_global_type("foo"), None);
    // An unknown name resolves to nothing.
    assert_eq!(c.get_global_type("Missing"), None);
}

// 4al S1: the checker reads its compiler options off the retained program (Go's
// `c.compilerOptions = program.Options()`), so a target set on the program's
// options is visible through `Checker::compiler_options`.
// Go: internal/checker/checker.go:NewChecker (c.compilerOptions = program.Options())
#[test]
fn compiler_options_reflects_program_options() {
    use tsgo_core::compileroptions::{CompilerOptions, ScriptTarget};
    let options = CompilerOptions {
        target: ScriptTarget::Es2015,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(
        crate::core::test_support::StubProgram::parse_and_bind_with_options(
            "/a.ts",
            "declare const x: string;",
            options,
        ),
    );
    let c = Checker::new_checker(p);
    // The program's `--target` is visible through the checker.
    assert_eq!(c.compiler_options().target, ScriptTarget::Es2015);
    // A program without options reports the all-defaults case.
    let q = std::rc::Rc::new(crate::core::test_support::StubProgram::parse_and_bind(
        "/b.ts", "",
    ));
    let d = Checker::new_checker(q);
    assert_eq!(d.compiler_options().target, ScriptTarget::None);
}

// 4al S2: `get_strict_option_value` mirrors Go's `GetStrictOptionValue`: an
// explicit per-option tri-state wins, otherwise the option is enabled iff
// `strict` is not explicitly false (so an unset `strict` enables it — the
// `!= TSFalse` rule, faithful to Go).
// Go: internal/core/compileroptions.go:GetStrictOptionValue
#[test]
fn get_strict_option_value_follows_strict_and_explicit() {
    use tsgo_core::compileroptions::CompilerOptions;
    // With `strict: true`, an unset per-option value resolves to enabled.
    let strict = CompilerOptions {
        strict: Tristate::True,
        ..CompilerOptions::default()
    };
    let c = Checker::new_checker(std::rc::Rc::new(
        crate::core::test_support::StubProgram::parse_and_bind_with_options("/a.ts", "", strict),
    ));
    assert!(c.get_strict_option_value(Tristate::Unknown));
    // An explicit per-option `false` wins over `strict: true`.
    assert!(!c.get_strict_option_value(Tristate::False));
    // With `strict: false`, an unset per-option value is off.
    let off = CompilerOptions {
        strict: Tristate::False,
        ..CompilerOptions::default()
    };
    let d = Checker::new_checker(std::rc::Rc::new(
        crate::core::test_support::StubProgram::parse_and_bind_with_options("/b.ts", "", off),
    ));
    assert!(!d.get_strict_option_value(Tristate::Unknown));
    // An explicit per-option `true` still wins over `strict: false`.
    assert!(d.get_strict_option_value(Tristate::True));
    // With `strict` unset, the `!= TSFalse` rule enables it (Go-faithful).
    assert!(Checker::new().get_strict_option_value(Tristate::Unknown));
}

// 4bi: `create_tuple_type_ex` carries the element types positionally and sets
// the `readonly` flag, while `create_tuple_type` builds a non-readonly tuple.
// This is the const-context (`[...] as const`) readonly tuple distinction.
// Go: internal/checker/checker.go:Checker.createTupleTypeEx (readonly)
#[test]
fn create_tuple_type_ex_sets_readonly_flag() {
    let mut c = Checker::new();
    let s = c.string_type();
    let n = c.number_type();
    let readonly_tuple = c.create_tuple_type_ex(vec![s, n], true);
    let mutable_tuple = c.create_tuple_type(vec![s, n]);
    let ro = c.get_type(readonly_tuple).as_object().expect("object");
    assert_eq!(ro.resolved_type_arguments, vec![s, n]);
    assert!(
        ro.readonly,
        "create_tuple_type_ex(_, true) must be readonly"
    );
    assert!(c
        .get_type(readonly_tuple)
        .object_flags()
        .contains(ObjectFlags::TUPLE));
    assert!(
        !c.get_type(mutable_tuple)
            .as_object()
            .expect("object")
            .readonly,
        "create_tuple_type must be mutable (non-readonly)"
    );
}

// 4al S2: `strict_null_checks` reads `strictNullChecks` through
// `GetStrictOptionValue` (Go's `c.strictNullChecks`): an explicit value wins,
// otherwise it follows `strict` (`!= TSFalse`).
// Go: internal/checker/checker.go:NewChecker (c.strictNullChecks)
#[test]
fn strict_null_checks_reads_option() {
    use tsgo_core::compileroptions::CompilerOptions;
    // Explicit `strictNullChecks: false` wins over `strict: true` -> off.
    let snc_off = CompilerOptions {
        strict: Tristate::True,
        strict_null_checks: Tristate::False,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(
        crate::core::test_support::StubProgram::parse_and_bind_with_options("/a.ts", "", snc_off),
    );
    assert!(!Checker::new_checker(p).strict_null_checks());
    // Implied by `strict: true` (per-option unset) -> on.
    let strict = CompilerOptions {
        strict: Tristate::True,
        ..CompilerOptions::default()
    };
    let q = std::rc::Rc::new(
        crate::core::test_support::StubProgram::parse_and_bind_with_options("/b.ts", "", strict),
    );
    assert!(Checker::new_checker(q).strict_null_checks());
    // Explicitly off when `strict: false` and `strictNullChecks` unset.
    let off = CompilerOptions {
        strict: Tristate::False,
        ..CompilerOptions::default()
    };
    let r = std::rc::Rc::new(
        crate::core::test_support::StubProgram::parse_and_bind_with_options("/c.ts", "", off),
    );
    assert!(!Checker::new_checker(r).strict_null_checks());
}

// Go: internal/checker/checker.go:Checker.newConditionalType + printer.go:typeToString
// `new_conditional_type` allocates a `CONDITIONAL`-flagged type, and the
// program-less printer renders it with placeholder branches.
#[test]
fn new_conditional_type_and_program_less_printing() {
    use crate::core::types::ConditionalRoot;
    use tsgo_ast::NodeId;
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let root = ConditionalRoot {
        node: NodeId(0),
        check_type: tp,
        extends_type: c.string_type(),
        is_distributive: true,
        infer_type_parameters: vec![],
        outer_type_parameters: vec![tp],
    };
    let cond = c.new_conditional_type(root, None);
    assert!(c.get_type(cond).flags().contains(TypeFlags::CONDITIONAL));
    let d = c.get_type(cond).as_conditional().expect("conditional");
    assert_eq!(d.check_type, tp);
    assert_eq!(d.extends_type, c.string_type());
    assert_eq!(c.type_to_string(cond), "T extends string ? ... : ...");
}

// Go: internal/checker/checker.go:Checker.newTemplateLiteralType + printer.go:typeToString
// `new_template_literal_type` allocates a `TEMPLATE_LITERAL`-flagged (interned)
// type, and the program-less printer renders it `` `a${T}b` ``.
#[test]
fn new_template_literal_type_and_program_less_printing() {
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let t = c.new_template_literal_type(vec!["a".into(), "b".into()], vec![tp]);
    assert!(c.get_type(t).flags().contains(TypeFlags::TEMPLATE_LITERAL));
    // Interned: a second identical request yields the same id.
    let t2 = c.new_template_literal_type(vec!["a".into(), "b".into()], vec![tp]);
    assert_eq!(t, t2);
    assert_eq!(c.type_to_string(t), "`a${T}b`");
}

// Go: internal/checker/checker.go:Checker.newStringMappingType + printer.go:typeToString
// `new_string_mapping_type` allocates a `STRING_MAPPING`-flagged (interned)
// type, and the program-less printer renders it `Uppercase<T>`.
#[test]
fn new_string_mapping_type_and_program_less_printing() {
    use crate::core::types::StringMappingKind;
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let t = c.new_string_mapping_type(StringMappingKind::Uppercase, tp);
    assert!(c.get_type(t).flags().contains(TypeFlags::STRING_MAPPING));
    let t2 = c.new_string_mapping_type(StringMappingKind::Uppercase, tp);
    assert_eq!(t, t2);
    assert_eq!(c.type_to_string(t), "Uppercase<T>");
}
