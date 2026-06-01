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
