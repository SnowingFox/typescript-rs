use super::*;
use crate::core::declared_types::{get_declared_type_of_symbol, get_type_of_symbol};
use crate::core::program::BoundProgram;
use crate::core::signatures::Signature;
use crate::core::test_support::StubProgram;
use crate::core::types::{LiteralValue, ObjectType};
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

// Resolves the (function/value) type of a top-level `declare let <name>: T`.
fn type_of_var(c: &mut Checker, p: &StubProgram, name: &str) -> TypeId {
    get_type_of_symbol(c, p, sym(p, name), p.globals())
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
//
// Runs NON-strict: this test isolates `skipOptional` (the optional-vs-required
// leniency). Under strictNullChecks the optional source property `a?: string`
// would read as `string | undefined` (C-D1 `addOptionalityEx`), so the property
// *type* comparison `string | undefined` vs `string` would fail regardless of
// `skipOptional`. Non-strict keeps `a` as bare `string`, so the only S/T
// difference is the optional flag — exactly what `skipOptional` governs.
#[test]
fn comparable_is_lenient_about_optional_vs_required() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        strict_null_checks: Tristate::False,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "interface S { a?: string }\ninterface T { a: string }",
        options,
    ));
    let mut c = Checker::new_checker(p.clone());
    let s = get_declared_type_of_symbol(&mut c, &*p, sym(&p, "S"), None);
    let t = get_declared_type_of_symbol(&mut c, &*p, sym(&p, "T"), None);
    // Assignability rejects an optional source against a required target.
    assert!(!c.is_type_assignable_to(&*p, s, t));
    // Comparability passes `skipOptional`, so the optional/required mismatch is
    // tolerated and `S` is comparable to `T`.
    assert!(c.is_type_comparable_to(&*p, s, t));
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

// Returns the (fresh) type of the initializer of `const o = <init>;`.
fn first_initializer_type(c: &mut Checker, p: &StubProgram) -> TypeId {
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        tsgo_ast::NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let list = match arena.data(stmts[0]) {
        tsgo_ast::NodeData::VariableStatement(d) => d.declaration_list,
        _ => panic!("variable statement"),
    };
    let decl = match arena.data(list) {
        tsgo_ast::NodeData::VariableDeclarationList(d) => d.declarations.nodes[0],
        _ => panic!("declaration list"),
    };
    let init = match arena.data(decl) {
        tsgo_ast::NodeData::VariableDeclaration(d) => d.initializer.expect("initializer"),
        _ => panic!("variable declaration"),
    };
    c.check_expression(p, init)
}

// Go: internal/checker/utilities.go:isObjectLiteralType(801)
#[test]
fn is_object_literal_type_distinguishes_literals() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { a: 1 };");
    let mut c = Checker::new();
    let lit = first_initializer_type(&mut c, &p);
    assert!(c.is_object_literal_type(lit), "a literal expression type");
    // Intrinsics and interface types are not object-literal types.
    let s = c.string_type();
    assert!(!c.is_object_literal_type(s));
    let iface_p = StubProgram::parse_and_bind("/b.ts", "interface I { x: number }");
    let i = get_declared_type_of_symbol(&mut c, &iface_p, sym(&iface_p, "I"), None);
    assert!(!c.is_object_literal_type(i));
}

// Go: internal/checker/relater.go:isExcessPropertyCheckTarget(746)
#[test]
fn is_excess_property_check_target_classifies_types() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { x: number }\ninterface B { y: string }",
    );
    let mut c = Checker::new();
    let a = get_declared_type_of_symbol(&mut c, &p, sym(&p, "A"), None);
    let b = get_declared_type_of_symbol(&mut c, &p, sym(&p, "B"), None);
    // Object types and the non-primitive `object` type are valid targets.
    assert!(c.is_excess_property_check_target(a));
    assert!(c.is_excess_property_check_target(c.non_primitive_type()));
    // Primitives are not.
    let s = c.string_type();
    assert!(!c.is_excess_property_check_target(s));
    // A union is a target if SOME constituent is (object | string).
    let obj_or_string = c.get_union_type(&[a, s]);
    assert!(c.is_excess_property_check_target(obj_or_string));
    // An intersection is a target if EVERY constituent is (object & object).
    let inter = c.get_intersection_type(&[a, b]);
    assert!(c.is_excess_property_check_target(inter));
}

// Go: internal/checker/relater.go:Checker.isKnownProperty(716)
#[test]
fn is_known_property_property_and_index_paths() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Named { x: number }\ninterface Indexed { [k: string]: number }",
    );
    let mut c = Checker::new();
    let named = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Named"), None);
    let indexed = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Indexed"), None);
    // Named property is known by name; a different name is not.
    assert!(c.is_known_property(&p, named, "x"));
    assert!(!c.is_known_property(&p, named, "y"));
    // An index signature makes any string-named property known.
    assert!(c.is_known_property(&p, indexed, "anything"));
    // A union target knows a name when SOME constituent does.
    let union = c.get_union_type(&[named, indexed]);
    assert!(c.is_known_property(&p, union, "x"));
}

// Go: internal/checker/checker.go:Checker.isEmptyObjectType(26326)
#[test]
fn is_empty_object_type_recognizes_empty_targets() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Empty {}\ninterface NonEmpty { x: number }\ninterface Indexed { [k: string]: number }",
    );
    let mut c = Checker::new();
    let empty = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Empty"), None);
    let non_empty = get_declared_type_of_symbol(&mut c, &p, sym(&p, "NonEmpty"), None);
    let indexed = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Indexed"), None);
    assert!(c.is_empty_object_type(empty));
    // The non-primitive `object` type counts as empty.
    assert!(c.is_empty_object_type(c.non_primitive_type()));
    // A type with a property or an index signature is not empty.
    assert!(!c.is_empty_object_type(non_empty));
    assert!(!c.is_empty_object_type(indexed));
    // Primitives are not empty object types.
    let s = c.string_type();
    assert!(!c.is_empty_object_type(s));
}

// Go: internal/checker/checker.go:Checker.getWidenedType(18214) / getWidenedTypeOfObjectLiteral(18259)
#[test]
fn get_widened_type_strips_object_literal_freshness() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { a: 1 };");
    let mut c = Checker::new();
    let lit = first_initializer_type(&mut c, &p);
    assert!(c.is_object_literal_type(lit));
    let widened = c.get_widened_type(lit);
    // Widening produces a distinct, regular (non-object-literal) type.
    assert_ne!(widened, lit);
    assert!(!c.is_object_literal_type(widened));
    // Widening is idempotent and identity for non-literal types.
    assert_eq!(c.get_widened_type(widened), widened);
    let s = c.string_type();
    assert_eq!(c.get_widened_type(s), s);
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

// ---- 4bn: relation-error chain machinery unit tests ----

// Go: internal/checker/relater.go:getPropertyNameArg
#[test]
fn get_property_name_arg_brackets_quoted_names_only() {
    assert_eq!(get_property_name_arg("a"), "a");
    assert_eq!(get_property_name_arg("foo"), "foo");
    assert_eq!(get_property_name_arg("\"a b\""), "[\"a b\"]");
    assert_eq!(get_property_name_arg("'k'"), "['k']");
    assert_eq!(get_property_name_arg("`t`"), "[`t`]");
}

// Go: internal/checker/relater.go:addToDottedName
#[test]
fn add_to_dotted_name_joins_segments() {
    // Plain identifier segments join with a dot.
    assert_eq!(add_to_dotted_name("a", "b"), "a.b");
    // An indexed tail attaches without a dot.
    assert_eq!(add_to_dotted_name("a", "[0]"), "a[0]");
    // A `new ...` head is parenthesized.
    assert_eq!(add_to_dotted_name("new C", "x"), "(new C).x");
    // A parenthesized tail prefix is preserved ahead of the head.
    assert_eq!(add_to_dotted_name("a", "(b)"), "(a.b)");
}

// Go: internal/checker/relater.go:isConversionOrInterfaceImplementationMessage
#[test]
fn is_conversion_or_interface_implementation_message_matches_codes() {
    assert!(is_conversion_or_interface_implementation_message(
        &tsgo_diagnostics::CLASS_0_INCORRECTLY_IMPLEMENTS_INTERFACE_1
    ));
    assert!(!is_conversion_or_interface_implementation_message(
        &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1
    ));
    assert!(!is_conversion_or_interface_implementation_message(
        &tsgo_diagnostics::PROPERTY_0_IS_MISSING_IN_TYPE_1_BUT_REQUIRED_IN_TYPE_2
    ));
}

// Go: internal/checker/relater.go:Relater.getChainMessage / chainArgsMatch
#[test]
fn chain_reporter_get_chain_message_and_args_match() {
    let mut r = ChainReporter::default();
    r.report(
        &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1,
        vec!["string".into(), "number".into()],
    );
    r.report(
        &tsgo_diagnostics::TYPES_OF_PROPERTY_0_ARE_INCOMPATIBLE,
        vec!["a".into()],
    );
    // Head is the most recently reported (prepended) entry.
    assert_eq!(r.chain_message(0).map(Message::code), Some(2326));
    assert_eq!(r.chain_message(1).map(Message::code), Some(2322));
    assert_eq!(r.chain_message(2).map(Message::code), None);
    // chain_args_match compares against the head ('a'); None is a wildcard.
    assert!(r.chain_args_match(&[Some("a")]));
    assert!(r.chain_args_match(&[None]));
    assert!(!r.chain_args_match(&[Some("b")]));
}

// Go: internal/checker/relater.go:Relater.reportError (no collapse, simple prepend)
#[test]
fn chain_reporter_report_prepends_without_collapse() {
    let mut r = ChainReporter::default();
    r.report(
        &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1,
        vec!["string".into(), "number".into()],
    );
    r.report(
        &tsgo_diagnostics::TYPES_OF_PROPERTY_0_ARE_INCOMPATIBLE,
        vec!["a".into()],
    );
    let report = error_chain_to_report(*r.chain.expect("chain"));
    assert_eq!(report.code, 2326);
    assert_eq!(report.message, "Types of property 'a' are incompatible.");
    assert_eq!(report.message_chain.len(), 1);
    assert_eq!(report.message_chain[0].code, 2322);
    assert_eq!(
        report.message_chain[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
    assert!(report.message_chain[0].next.is_empty());
}

// Go: internal/checker/relater.go:Relater.reportError (dotted-name collapse)
#[test]
fn chain_reporter_collapses_nested_property_messages() {
    let mut r = ChainReporter::default();
    // Leaf primitive mismatch.
    r.report(
        &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1,
        vec!["string".into(), "number".into()],
    );
    // Inner property 'b' incompatibility.
    r.report(
        &tsgo_diagnostics::TYPES_OF_PROPERTY_0_ARE_INCOMPATIBLE,
        vec!["b".into()],
    );
    // Inner object-level head (dropped on the collapse below).
    r.report(
        &tsgo_diagnostics::TYPE_0_IS_NOT_ASSIGNABLE_TO_TYPE_1,
        vec!["{ b: string; }".into(), "{ b: number; }".into()],
    );
    // Outer property 'a' incompatibility -> collapses with 'b' into `a.b`.
    r.report(
        &tsgo_diagnostics::TYPES_OF_PROPERTY_0_ARE_INCOMPATIBLE,
        vec!["a".into()],
    );
    let report = error_chain_to_report(*r.chain.expect("chain"));
    assert_eq!(report.code, 2200);
    assert_eq!(
        report.message,
        "The types of 'a.b' are incompatible between these types."
    );
    assert_eq!(report.message_chain.len(), 1);
    assert_eq!(report.message_chain[0].code, 2322);
    assert_eq!(
        report.message_chain[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/relater.go:Checker.getUnmatchedPropertiesWorker
#[test]
fn get_unmatched_property_finds_first_missing_required() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface S { a: number }\ninterface T { a: number; b: number }",
    );
    let mut c = Checker::new();
    let s = get_declared_type_of_symbol(&mut c, &p, sym(&p, "S"), None);
    let t = get_declared_type_of_symbol(&mut c, &p, sym(&p, "T"), None);
    // `S` lacks `b`, which is required in `T`.
    let unmatched = c
        .get_unmatched_property(&p, s, t, false)
        .expect("unmatched");
    assert_eq!(p.symbol(unmatched).name, "b");
    // `T` has every member of `S`, so nothing is unmatched in that direction.
    assert!(c.get_unmatched_property(&p, t, s, false).is_none());
}

// Go: internal/checker/relater.go:Checker.getUnmatchedPropertiesWorker (optional skipped)
#[test]
fn get_unmatched_property_skips_optional_target_unless_required() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface S { a: number }\ninterface T { a: number; b?: number }",
    );
    let mut c = Checker::new();
    let s = get_declared_type_of_symbol(&mut c, &p, sym(&p, "S"), None);
    let t = get_declared_type_of_symbol(&mut c, &p, sym(&p, "T"), None);
    // Optional `b` is not "missing" for assignability.
    assert!(c.get_unmatched_property(&p, s, t, false).is_none());
    // But requiring optional properties (subtype) does flag it.
    assert!(c.get_unmatched_property(&p, s, t, true).is_some());
}

// Go: internal/checker/relater.go:Checker.shouldReportUnmatchedPropertyError
#[test]
fn should_report_unmatched_property_error_is_true_for_object_with_properties() {
    let p = StubProgram::parse_and_bind("/a.ts", "interface A { a: number }\ninterface E {}");
    let mut c = Checker::new();
    let a = get_declared_type_of_symbol(&mut c, &p, sym(&p, "A"), None);
    let e = get_declared_type_of_symbol(&mut c, &p, sym(&p, "E"), None);
    assert!(c.should_report_unmatched_property_error(a));
    // An empty object has no call signatures, so the error is still reported.
    assert!(c.should_report_unmatched_property_error(e));
}

// Go: internal/checker/relater.go:Checker.checkTypeRelatedToEx (chain materialization)
#[test]
fn build_relation_error_chain_single_property_mismatch() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { a: number }\ninterface B { a: string }",
    );
    let mut c = Checker::new();
    let a = get_declared_type_of_symbol(&mut c, &p, sym(&p, "A"), None);
    let b = get_declared_type_of_symbol(&mut c, &p, sym(&p, "B"), None);
    let report = c
        .build_relation_error_chain(&p, b, a, RelationKind::Assignable)
        .expect("chain");
    assert_eq!(report.code, 2322);
    assert_eq!(report.message, "Type 'B' is not assignable to type 'A'.");
    assert_eq!(report.message_chain.len(), 1);
    assert_eq!(report.message_chain[0].code, 2326);
    assert_eq!(report.message_chain[0].next[0].code, 2322);
}

// Go: internal/checker/relater.go:Checker.checkTypeRelatedToEx (assignable -> no chain)
#[test]
fn build_relation_error_chain_returns_none_when_assignable() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { a: number }\ninterface C { a: number; b: number }",
    );
    let mut c = Checker::new();
    let a = get_declared_type_of_symbol(&mut c, &p, sym(&p, "A"), None);
    let cc = get_declared_type_of_symbol(&mut c, &p, sym(&p, "C"), None);
    // `C` is assignable to `A` (C has all of A's members), so no chain is built.
    assert!(c
        .build_relation_error_chain(&p, cc, a, RelationKind::Assignable)
        .is_none());
}

// 4bp: `type_arguments_related_to` relates type arguments pairwise covariantly:
// `source[i]` must relate to `target[i]`. For assignability `number` relates to
// `number | string` (covariant ok) but `number | string` does not relate to
// `number`.
// Go: internal/checker/relater.go:Relater.typeArgumentsRelatedTo (covariant)
#[test]
fn type_arguments_related_to_relates_covariantly() {
    let mut c = Checker::new();
    let p = empty_program();
    let n = c.number_type();
    let s_or_n = c.string_or_number_type();
    // `[number]` -> `[number | string]`: covariant, ok.
    assert!(c.type_arguments_related_to(&p, &[n], &[s_or_n], RelationKind::Assignable));
    // `[number | string]` -> `[number]`: covariant, fails (string is not number).
    assert!(!c.type_arguments_related_to(&p, &[s_or_n], &[n], RelationKind::Assignable));
    // Equal arguments relate.
    assert!(c.type_arguments_related_to(&p, &[n], &[n], RelationKind::Assignable));
    // An empty argument list trivially relates.
    assert!(c.type_arguments_related_to(&p, &[], &[], RelationKind::Assignable));
}

// 4bp: `type_arguments_related_to` rejects an identity check over
// differently-sized argument lists (Go's `len(sources) != len(targets) &&
// identity` guard); a non-identity relation relates the common prefix.
// Go: internal/checker/relater.go:Relater.typeArgumentsRelatedTo (length guard)
#[test]
fn type_arguments_related_to_identity_length_guard() {
    let mut c = Checker::new();
    let p = empty_program();
    let n = c.number_type();
    assert!(!c.type_arguments_related_to(&p, &[n, n], &[n], RelationKind::Identity));
    // Identity of equal-length, equal-type arguments holds.
    assert!(c.type_arguments_related_to(&p, &[n], &[n], RelationKind::Identity));
}

// 4bp: `reference_type_arguments_related_to` fires only for two type references
// to the SAME generic target. Two `Box<...>` references with different element
// types relate by their (covariant) type arguments; references to DIFFERENT
// generic targets and non-reference object types yield `None` (so the caller
// falls back to a structural comparison).
// Go: internal/checker/relater.go:Checker.structuredTypeRelatedToWorker (same-target reference arm)
#[test]
fn reference_type_arguments_related_to_requires_same_target() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Box<T> { v: T }\ninterface Bag<T> { v: T }\ninterface Plain { v: number }",
    );
    let mut c = Checker::new();
    let box_target = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Box"), None);
    let bag_target = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Bag"), None);
    let plain = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Plain"), None);
    let n = c.number_type();
    let s_or_n = c.string_or_number_type();
    let box_num = c.create_type_reference(box_target, vec![n]);
    let box_num_or_str = c.create_type_reference(box_target, vec![s_or_n]);
    let bag_num = c.create_type_reference(bag_target, vec![n]);
    // Same target (`Box`), covariant arguments: `Box<number>` relates to
    // `Box<number | string>` but not the reverse.
    assert_eq!(
        c.reference_type_arguments_related_to(
            &p,
            box_num,
            box_num_or_str,
            RelationKind::Assignable
        ),
        Some(true)
    );
    assert_eq!(
        c.reference_type_arguments_related_to(
            &p,
            box_num_or_str,
            box_num,
            RelationKind::Assignable
        ),
        Some(false)
    );
    // Different generic targets (`Box` vs `Bag`): not the same-target arm.
    assert_eq!(
        c.reference_type_arguments_related_to(&p, box_num, bag_num, RelationKind::Assignable),
        None
    );
    // A non-reference object type is never the same-target arm.
    assert_eq!(
        c.reference_type_arguments_related_to(&p, plain, box_num, RelationKind::Assignable),
        None
    );
}

// 4bp: end-to-end via the public relation API — two `Array<...>` references to
// the same global `Array` target relate by their element type's covariance.
// `Array<number>` is assignable to `Array<number | string>` but not the reverse
// (this is the fix for the `(number | string)[]` vs `number[]` false positive).
// Go: internal/checker/relater.go:Checker.structuredTypeRelatedToWorker (same-target reference arm)
#[test]
fn array_references_relate_by_element_covariance() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}",
    );
    let mut c = Checker::new();
    let array_target = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Array"), None);
    let n = c.number_type();
    let s_or_n = c.string_or_number_type();
    let array_num = c.create_type_reference(array_target, vec![n]);
    let array_num_or_str = c.create_type_reference(array_target, vec![s_or_n]);
    // Covariant: `Array<number>` -> `Array<number | string>` holds.
    assert!(c.is_type_assignable_to(&p, array_num, array_num_or_str));
    // Reverse fails: `Array<number | string>` -> `Array<number>`.
    assert!(!c.is_type_assignable_to(&p, array_num_or_str, array_num));
    // Identical references relate.
    assert!(c.is_type_assignable_to(&p, array_num, array_num));
}

// ---- C-A: function-type signature relations + variance ----

// Builds a nullary signature returning `ret` (a `new` signature when
// `construct`), for the direct signature-relation unit tests.
fn nullary_sig(c: &mut Checker, ret: TypeId, construct: bool) -> SignatureId {
    let mut s = Signature::new(if construct {
        SignatureFlags::CONSTRUCT
    } else {
        SignatureFlags::NONE
    });
    s.resolved_return_type = Some(ret);
    s.min_argument_count = 0;
    c.new_signature(s)
}

// Builds an anonymous object type carrying the given call/construct signatures.
fn signature_obj(c: &mut Checker, calls: Vec<SignatureId>, constructs: Vec<SignatureId>) -> TypeId {
    let obj = ObjectType {
        call_signatures: calls,
        construct_signatures: constructs,
        ..Default::default()
    };
    c.new_object_type(ObjectFlags::ANONYMOUS, None, obj)
}

// C-A unit: `signatures_of_type` is kind-aware — it returns `call_signatures`
// for `Call` and `construct_signatures` for `Construct`.
// Go: internal/checker/checker.go:Checker.getSignaturesOfType
#[test]
fn signatures_of_type_is_kind_aware() {
    let mut c = Checker::new();
    let n = c.number_type();
    let call = nullary_sig(&mut c, n, false);
    let ctor = nullary_sig(&mut c, n, true);
    let obj = signature_obj(&mut c, vec![call], vec![ctor]);
    assert_eq!(c.signatures_of_type(obj, SignatureKind::Call), vec![call]);
    assert_eq!(
        c.signatures_of_type(obj, SignatureKind::Construct),
        vec![ctor]
    );
    // A type with no object data has neither kind.
    let s = c.string_type();
    assert!(c.signatures_of_type(s, SignatureKind::Call).is_empty());
    assert!(c.signatures_of_type(s, SignatureKind::Construct).is_empty());
}

// C-A unit: `signatures_related_to` requires EACH target signature to be matched
// by SOME source signature (Go's N*M matrix). A target `() => number` is matched
// by a source list `[() => string, () => number]`; a target `() => boolean` is
// matched by neither.
// Go: internal/checker/relater.go:Relater.signaturesRelatedTo (default arm)
#[test]
fn signatures_related_to_matches_some_source_signature() {
    let mut c = Checker::new();
    let p = empty_program();
    let n = c.number_type();
    let s = c.string_type();
    let b = c.boolean_type();
    let src_str = nullary_sig(&mut c, s, false);
    let src_num = nullary_sig(&mut c, n, false);
    let source = signature_obj(&mut c, vec![src_str, src_num], Vec::new());
    // Target `() => number` matched by the source's `() => number`.
    let tgt_num = nullary_sig(&mut c, n, false);
    let target_num = signature_obj(&mut c, vec![tgt_num], Vec::new());
    assert!(c.signatures_related_to(
        &p,
        source,
        target_num,
        SignatureKind::Call,
        RelationKind::Assignable,
        None
    ));
    // Target `() => boolean` matched by NEITHER source signature.
    let tgt_bool = nullary_sig(&mut c, b, false);
    let target_bool = signature_obj(&mut c, vec![tgt_bool], Vec::new());
    assert!(!c.signatures_related_to(
        &p,
        source,
        target_bool,
        SignatureKind::Call,
        RelationKind::Assignable,
        None
    ));
    // An empty target signature list is trivially satisfied.
    let empty = signature_obj(&mut c, Vec::new(), Vec::new());
    assert!(c.signatures_related_to(
        &p,
        source,
        empty,
        SignatureKind::Call,
        RelationKind::Assignable,
        None
    ));
}

// C-A unit: `compare_signatures_related` relates return types covariantly, with a
// `void`/`any` target return accepting any source return (no parameters needed).
// Go: internal/checker/relater.go:Checker.compareSignaturesRelated (return type)
#[test]
fn compare_signatures_related_return_covariance_and_void() {
    let mut c = Checker::new();
    let p = empty_program();
    let n = c.number_type();
    let s = c.string_type();
    let v = c.void_type();
    let ret_str = nullary_sig(&mut c, s, false);
    let ret_num = nullary_sig(&mut c, n, false);
    let ret_void = nullary_sig(&mut c, v, false);
    // `() => string` vs `() => number`: covariant return fails.
    assert!(!c.compare_signatures_related(&p, ret_str, ret_num, RelationKind::Assignable, None));
    // `() => number` vs `() => void`: void target accepts any source return.
    assert!(c.compare_signatures_related(&p, ret_num, ret_void, RelationKind::Assignable, None));
    // Identical return relates.
    assert!(c.compare_signatures_related(&p, ret_str, ret_str, RelationKind::Assignable, None));
}

// C-A unit: `compare_signatures_related` rejects a source requiring more
// arguments than the target accepts (the arity guard, before the parameter
// loop), using a real `number` parameter symbol as filler.
// Go: internal/checker/relater.go:Checker.compareSignaturesRelated (arity)
#[test]
fn compare_signatures_related_arity_guard() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare let x: number;");
    let mut c = Checker::new();
    let v = c.void_type();
    let x = sym(&p, "x");
    // Source requires 2 args; target accepts 1.
    let mut more = Signature::new(SignatureFlags::NONE);
    more.parameters = vec![x, x];
    more.min_argument_count = 2;
    more.resolved_return_type = Some(v);
    let more = c.new_signature(more);
    let mut fewer = Signature::new(SignatureFlags::NONE);
    fewer.parameters = vec![x];
    fewer.min_argument_count = 1;
    fewer.resolved_return_type = Some(v);
    let fewer = c.new_signature(fewer);
    // A source requiring MORE args is not assignable to a target accepting fewer.
    assert!(!c.compare_signatures_related(&p, more, fewer, RelationKind::Assignable, None));
    // The reverse (fewer required params) is assignable.
    assert!(c.compare_signatures_related(&p, fewer, more, RelationKind::Assignable, None));
}

// C-A slice 1: call-signature parameters relate CONTRAVARIANTLY (Go's
// `compareSignaturesRelated` parameter loop relates `target` param -> `source`
// param). A function accepting a WIDER parameter is assignable where a
// function accepting a NARROWER parameter is expected, not the reverse.
// Go: internal/checker/relater.go:Checker.compareSignaturesRelated (parameters)
#[test]
fn function_parameters_relate_contravariantly() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare let wide: (x: number | string) => void;\ndeclare let narrow: (x: number) => void;",
    );
    let mut c = Checker::new();
    let wide = type_of_var(&mut c, &p, "wide");
    let narrow = type_of_var(&mut c, &p, "narrow");
    // `(x: number | string) => void` IS assignable to `(x: number) => void`:
    // contravariant `target(number)` -> `source(number | string)` holds.
    assert!(c.is_type_assignable_to(&p, wide, narrow));
    // The reverse fails: `target(number | string)` -> `source(number)` fails.
    assert!(!c.is_type_assignable_to(&p, narrow, wide));
}

// C-A slice 2: call-signature return types relate COVARIANTLY (`source` return
// -> `target` return), with a `void`/`any` target return accepting any source
// return (Go's void-return special case).
// Go: internal/checker/relater.go:Checker.compareSignaturesRelated (return type)
#[test]
fn function_return_types_relate_covariantly() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare let s: () => string;\ndeclare let n: () => number;\ndeclare let v: () => void;",
    );
    let mut c = Checker::new();
    let s = type_of_var(&mut c, &p, "s");
    let n = type_of_var(&mut c, &p, "n");
    let v = type_of_var(&mut c, &p, "v");
    // `() => string` is NOT assignable to `() => number` (covariant return).
    assert!(!c.is_type_assignable_to(&p, s, n));
    // `() => number` IS assignable to `() => void` (void target accepts any).
    assert!(c.is_type_assignable_to(&p, n, v));
    // A matching return relates.
    assert!(c.is_type_assignable_to(&p, s, s));
}

// C-A slice 3: arity tolerance. A signature requiring FEWER arguments is
// assignable where MORE parameters are expected (a callback may ignore trailing
// arguments); a signature requiring MORE arguments is NOT assignable to one
// accepting fewer (Go's `sourceHasMoreParameters` -> `getMinArgumentCount`).
// Go: internal/checker/relater.go:Checker.compareSignaturesRelated (arity)
#[test]
fn function_arity_tolerance() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare let few: (a: number) => void;\ndeclare let many: (a: number, b: number) => void;",
    );
    let mut c = Checker::new();
    let few = type_of_var(&mut c, &p, "few");
    let many = type_of_var(&mut c, &p, "many");
    // Fewer required params -> assignable where more are expected.
    assert!(c.is_type_assignable_to(&p, few, many));
    // More required params -> NOT assignable to a target accepting fewer.
    assert!(!c.is_type_assignable_to(&p, many, few));
}

// C-A slice 5: construct signatures (`new () => T`) relate by their return type
// (covariantly), like call signatures. A bare construct type carries a
// construct signature (the `__new` member), not a call signature.
// Go: internal/checker/relater.go:Relater.signaturesRelatedTo (SignatureKindConstruct)
#[test]
fn construct_signatures_relate_by_return_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "class Base { x: number = 1; }\nclass Other {}\ndeclare let cb: new () => Base;\ndeclare let cc: new () => Other;",
    );
    let mut c = Checker::new();
    let cb = type_of_var(&mut c, &p, "cb");
    let cc = type_of_var(&mut c, &p, "cc");
    // Identity: `new () => Base` is assignable to itself.
    assert!(c.is_type_assignable_to(&p, cb, cb));
    // `new () => Other` is NOT assignable to `new () => Base` (Other lacks `x`).
    assert!(!c.is_type_assignable_to(&p, cc, cb));
}

// 4bp: a generic reference's member types are instantiated through its
// type-argument mapper for the structural relation (Go's `getPropertiesOfType`
// returns instantiated members). `{ v: number }` is assignable to `Box<number>`
// (the `v` member instantiates to `number`), and `Box<number>`'s type parameter
// `T` is NOT exposed as a property (Go's `symbolIsValue` filter).
// Go: internal/checker/relater.go:Relater.propertiesRelatedTo / checker.go:getNamedMembers
#[test]
fn reference_member_types_are_instantiated_for_relation() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Box<T> { v: T }\ninterface NumV { v: number }\ninterface StrV { v: string }",
    );
    let mut c = Checker::new();
    let box_target = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Box"), None);
    let num_v = get_declared_type_of_symbol(&mut c, &p, sym(&p, "NumV"), None);
    let str_v = get_declared_type_of_symbol(&mut c, &p, sym(&p, "StrV"), None);
    let n = c.number_type();
    let box_num = c.create_type_reference(box_target, vec![n]);
    // `{ v: number }` is assignable to `Box<number>` (member instantiates to
    // `number`); the type parameter `T` is not a property of `Box<number>`.
    assert!(c.is_type_assignable_to(&p, num_v, box_num));
    // `{ v: string }` is not assignable to `Box<number>`.
    assert!(!c.is_type_assignable_to(&p, str_v, box_num));
}

// T0-1: When the relation recursion depth reaches 100, the comparison returns
// `false` instead of overflowing the stack — preventing a crash on deeply nested
// structural type comparisons.
// Go: internal/checker/relater.go:recursiveTypeRelatedTo (len(r.sourceStack) == 100)
#[test]
fn relation_depth_guard_prevents_overflow() {
    let p = StubProgram::parse_and_bind("/a.ts", "");
    let mut c = Checker::new();
    // Simulate a deeply nested comparison by artificially setting the depth to 100.
    c.relation_depth = 100;
    // Two distinct object types that would normally need structural comparison.
    let a = c.new_object_type(
        crate::core::types::ObjectFlags::INTERFACE,
        None,
        Default::default(),
    );
    let b = c.new_object_type(
        crate::core::types::ObjectFlags::INTERFACE,
        None,
        Default::default(),
    );
    // At depth 100, the relation returns false (overflow) instead of recursing.
    assert!(
        !c.is_type_assignable_to(&p, a, b),
        "at depth 100, returns false to prevent overflow"
    );
}

// GUARD: at normal depth, structurally compatible types still relate correctly.
// Go: internal/checker/relater.go:recursiveTypeRelatedTo (normal path)
#[test]
fn relation_depth_guard_does_not_affect_normal_checks() {
    let p = StubProgram::parse_and_bind("/a.ts", "");
    let mut c = Checker::new();
    // A type is always assignable to itself at normal depth.
    let s = c.string_type();
    assert!(c.is_type_assignable_to(&p, s, s));
    // And to `any`.
    let any = c.any_type();
    assert!(c.is_type_assignable_to(&p, s, any));
}

// Go: internal/checker/relater.go:Checker.isTypeSubsetOf(2811)
#[test]
fn is_type_subset_of_primitive_in_union() {
    let mut c = Checker::new();
    let ab = c.get_union_type(&[c.string_type(), c.number_type()]);
    assert!(c.is_type_subset_of(c.string_type(), ab));
    assert!(c.is_type_subset_of(c.number_type(), ab));
    assert!(!c.is_type_subset_of(ab, c.string_type()));
}

// Go: internal/checker/relater.go:Checker.isTypeSubsetOfUnion(2815)
#[test]
fn is_type_subset_of_union_of_union_type_ids() {
    let mut c = Checker::new();
    let ab = c.get_union_type(&[c.string_type(), c.number_type()]);
    let abcd = c.get_union_type(&[ab, c.boolean_type()]);
    assert!(c.is_type_subset_of(ab, abcd));
    assert!(!c.is_type_subset_of(abcd, ab));
}

// Go: internal/checker/relater.go:Checker.isTypeSubsetOf(2811)
#[test]
fn is_type_subset_of_never_is_bottom() {
    let c = Checker::new();
    assert!(c.is_type_subset_of(c.never_type(), c.string_type()));
}
