use crate::core::program::BoundProgram;
use crate::core::signatures::{Signature, SignatureFlags};
use crate::core::test_support::{MultiFileProgram, StubProgram};
use crate::core::types::{ObjectFlags, ObjectType};
use crate::core::Checker;
use tsgo_ast::NodeData;

fn empty() -> StubProgram {
    StubProgram::parse_and_bind("/a.ts", "")
}

// Returns the expression of the `idx`-th top-level expression statement.
fn expr_stmt_expression(p: &StubProgram, idx: usize) -> tsgo_ast::NodeId {
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    match arena.data(stmts[idx]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    }
}

// Go: internal/checker/checker.go:Checker.checkIdentifier (tracer bullet)
#[test]
fn check_identifier_yields_declared_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: string;\nx;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let s = c.string_type();
    assert_eq!(c.check_expression(&p, usage), s);
}

// Go: internal/checker/checker.go:Checker.checkIdentifier (Cannot_find_name_0)
#[test]
fn undefined_identifier_reports_cannot_find_name() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "y;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.getDiagnostics (triggers checkSourceFile)
#[test]
fn get_diagnostics_triggers_checking() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "y;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // Go's `getDiagnostics` runs `checkSourceFile` itself; the pool only calls
    // `get_diagnostics(file)` after `new_checker(program)`.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.getDiagnostics (pool-driving surface, 2339)
#[test]
fn get_diagnostics_drives_property_does_not_exist() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Foo {\n  bar: string;\n}\ndeclare const foo: Foo;\nfoo.baz;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // End to end through the exact surface a pool drives: construct over a bound
    // program, then ask for a file's diagnostics (no per-call program).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2339);
    assert_eq!(
        diags[0].message,
        "Property 'baz' does not exist on type 'Foo'."
    );
}

// Go: internal/checker/checker.go:Checker.checkSourceFile (links.typeChecked guard)
#[test]
fn check_source_file_is_idempotent() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "y;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    c.check_source_file(root);
    // Re-checking a file must not re-report; Go guards via
    // `sourceFileLinks.typeChecked`.
    assert_eq!(c.get_diagnostics(root).len(), 1);
}

// Go: internal/checker/checker.go:Checker.checkSourceFile (no errors)
#[test]
fn defined_identifier_reports_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string;\nx;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkExpressionWorker (literals)
#[test]
fn check_literal_expressions() {
    let p = StubProgram::parse_and_bind("/a.ts", "\"a\";\n1;\ntrue;\nnull;");
    let mut c = Checker::new();
    let (str_lit, num_lit, true_lit, null_lit) = (
        expr_stmt_expression(&p, 0),
        expr_stmt_expression(&p, 1),
        expr_stmt_expression(&p, 2),
        expr_stmt_expression(&p, 3),
    );
    let t_str = c.check_expression(&p, str_lit);
    assert_eq!(c.type_to_string(t_str), "\"a\"");
    let t_num = c.check_expression(&p, num_lit);
    assert_eq!(c.type_to_string(t_num), "1");
    let true_ty = c.true_type();
    assert_eq!(c.check_expression(&p, true_lit), true_ty);
    let null_ty = c.null_type();
    assert_eq!(c.check_expression(&p, null_lit), null_ty);
}

// 4bc slice 1 tracer (genuine RED): two occurrences of the same string-literal
// expression intern to ONE `TypeId`. Go's `getStringLiteralType` keeps a
// per-checker `stringLiteralTypes[value]` cache, so every `"a"` literal is the
// same `*Type`; the port previously allocated a fresh literal per occurrence,
// so the two ids differed. Value-keyed interning recovers Go's id semantics.
// Go: internal/checker/checker.go:Checker.getStringLiteralType(25164)
#[test]
fn string_literal_expressions_intern_to_one_type_id() {
    let p = StubProgram::parse_and_bind("/a.ts", "\"a\";\n\"a\";");
    let mut c = Checker::new();
    let first = expr_stmt_expression(&p, 0);
    let second = expr_stmt_expression(&p, 1);
    let t1 = c.check_expression(&p, first);
    let t2 = c.check_expression(&p, second);
    assert_eq!(
        t1, t2,
        "two `\"a\"` literals must share one interned TypeId"
    );
}

// 4bc slice 1 guard (green-on-arrival): distinct string-literal values get
// distinct interned ids (the cache is keyed by value, so `"a"` and `"b"` never
// collide).
// Go: internal/checker/checker.go:Checker.getStringLiteralType(25164)
#[test]
fn distinct_string_literal_values_get_distinct_type_ids() {
    let p = StubProgram::parse_and_bind("/a.ts", "\"a\";\n\"b\";");
    let mut c = Checker::new();
    let first = expr_stmt_expression(&p, 0);
    let second = expr_stmt_expression(&p, 1);
    let t1 = c.check_expression(&p, first);
    let t2 = c.check_expression(&p, second);
    assert_ne!(t1, t2, "`\"a\"` and `\"b\"` must be distinct interned ids");
}

// 4bc slice 2 tracer (genuine RED): two occurrences of the same numeric-literal
// expression intern to ONE `TypeId` (Go's `getNumberLiteralType` cache).
// Go: internal/checker/checker.go:Checker.getNumberLiteralType(25173)
#[test]
fn number_literal_expressions_intern_to_one_type_id() {
    let p = StubProgram::parse_and_bind("/a.ts", "1;\n1;");
    let mut c = Checker::new();
    let first = expr_stmt_expression(&p, 0);
    let second = expr_stmt_expression(&p, 1);
    let t1 = c.check_expression(&p, first);
    let t2 = c.check_expression(&p, second);
    assert_eq!(t1, t2, "two `1` literals must share one interned TypeId");
}

// 4bc slice 2 guard (green-on-arrival): distinct numeric-literal values get
// distinct interned ids.
// Go: internal/checker/checker.go:Checker.getNumberLiteralType(25173)
#[test]
fn distinct_number_literal_values_get_distinct_type_ids() {
    let p = StubProgram::parse_and_bind("/a.ts", "1;\n2;");
    let mut c = Checker::new();
    let first = expr_stmt_expression(&p, 0);
    let second = expr_stmt_expression(&p, 1);
    let t1 = c.check_expression(&p, first);
    let t2 = c.check_expression(&p, second);
    assert_ne!(t1, t2, "`1` and `2` must be distinct interned ids");
}

// 4bc slice 3 guard (green-on-arrival): boolean-literal types are already
// interned (the checker holds one `true_type`/`false_type` singleton minted in
// `NewChecker`), so two `true` expressions share one `TypeId` without any new
// cache. Mirrors Go's `getBooleanLiteralType` returning the pre-allocated
// `trueType`/`falseType`.
// Go: internal/checker/checker.go:NewChecker (trueType/falseType singletons)
#[test]
fn boolean_literal_expressions_intern_to_one_type_id() {
    let p = StubProgram::parse_and_bind("/a.ts", "true;\ntrue;\nfalse;");
    let mut c = Checker::new();
    let first = expr_stmt_expression(&p, 0);
    let second = expr_stmt_expression(&p, 1);
    let third = expr_stmt_expression(&p, 2);
    let t_true_1 = c.check_expression(&p, first);
    let t_true_2 = c.check_expression(&p, second);
    let t_false = c.check_expression(&p, third);
    assert_eq!(
        t_true_1, t_true_2,
        "two `true` literals must share one TypeId"
    );
    assert_ne!(t_true_1, t_false, "`true` and `false` are distinct ids");
}

// 4bd slice 1 tracer (genuine RED): a literal expression produces a FRESH
// literal type paired to the interned regular one. Go's `checkExpressionWorker`
// wraps the value-keyed `getStringLiteralType` in `getFreshTypeOfLiteralType`,
// so the expression's type is the fresh form (distinct id) whose
// `getRegularTypeOfLiteralType` is the interned regular literal. Before this
// round the port returned the interned regular directly (no fresh form), so the
// expression id equalled the regular id.
// Go: internal/checker/checker.go:Checker.getFreshTypeOfLiteralType(25146)
#[test]
fn string_literal_expression_is_fresh_paired_to_interned_regular() {
    let p = StubProgram::parse_and_bind("/a.ts", "\"a\";");
    let mut c = Checker::new();
    let lit = expr_stmt_expression(&p, 0);
    let fresh = c.check_expression(&p, lit);
    let regular = c.get_string_literal_type("a");
    assert_ne!(
        fresh, regular,
        "a literal expression must yield the fresh form, distinct from the interned regular"
    );
    assert_eq!(
        c.regular_type_of_literal_type(fresh),
        regular,
        "the fresh literal's regular counterpart is the interned regular literal"
    );
}

// 4bd slice 1 guard (green-on-arrival): two `"a"` expressions still share one
// id after the fresh wrapping, because `getFreshTypeOfLiteralType` caches the
// fresh form on the regular literal (so both occurrences resolve to the same
// fresh type).
// Go: internal/checker/checker.go:Checker.getFreshTypeOfLiteralType(25146)
#[test]
fn fresh_string_literal_expressions_still_intern_to_one_type_id() {
    let p = StubProgram::parse_and_bind("/a.ts", "\"a\";\n\"a\";");
    let mut c = Checker::new();
    let first = expr_stmt_expression(&p, 0);
    let second = expr_stmt_expression(&p, 1);
    let t1 = c.check_expression(&p, first);
    let t2 = c.check_expression(&p, second);
    assert_eq!(t1, t2, "two fresh `\"a\"` literals must share one TypeId");
}

// Go: internal/checker/checker.go:Checker.checkPropertyAccessExpression
#[test]
fn check_property_access_yields_member_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Foo {\n  bar: string;\n}\ndeclare const foo: Foo;\nfoo.bar;",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let s = c.string_type();
    assert_eq!(c.check_expression(&p, access), s);
}

// Go: internal/checker/checker.go (Property_0_does_not_exist_on_type_1)
#[test]
fn missing_property_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Foo {\n  bar: string;\n}\ndeclare const foo: Foo;\nfoo.baz;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2339);
    // The object type prints as its interface name `Foo`, not `{ ... }`.
    assert_eq!(
        diags[0].message,
        "Property 'baz' does not exist on type 'Foo'."
    );
}

// Go: internal/checker/checker.go:Checker.getPropertyOfUnionOrIntersectionType
// (union branch): a property present on every constituent of a union resolves,
// with the union of the per-constituent property types.
#[test]
fn check_property_access_on_union_yields_union_of_member_types() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { a: number }\ninterface B { a: string }\n\
         type U = A | B;\ndeclare const u: U;\nu.a;",
    );
    let access = expr_stmt_expression(&p, 4);
    let mut c = Checker::new();
    // `u.a` is present on both `A` and `B`, so its type is `number | string`.
    let expected = c.string_or_number_type();
    assert_eq!(c.check_expression(&p, access), expected);
}

// Go: internal/checker/checker.go:Checker.getPropertyOfUnionOrIntersectionType
// (union partial filtering): a property missing from any constituent of a union
// is partial and does not appear on the union, so access reports 2339.
#[test]
fn union_property_absent_from_one_constituent_reports_2339() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { a: number }\ninterface C { b: string }\n\
         type U2 = A | C;\ndeclare const u2: U2;\nu2.a;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2339);
}

// Go: internal/checker/checker.go:Checker.getSignaturesOfType
#[test]
fn signatures_of_function_type() {
    let mut c = Checker::new();
    let num = c.number_type();
    let mut sig = Signature::new(SignatureFlags::NONE);
    sig.resolved_return_type = Some(num);
    let sid = c.new_signature(sig);
    let obj = ObjectType {
        call_signatures: vec![sid],
        ..Default::default()
    };
    let fn_ty = c.new_object_type(ObjectFlags::empty(), None, obj);
    assert_eq!(c.get_signatures_of_type(fn_ty), vec![sid]);
    // A primitive has no call signatures.
    let s = c.string_type();
    assert!(c.get_signatures_of_type(s).is_empty());
}

// Go: internal/checker/checker.go:Checker.getReturnTypeOfSignature (+ inference)
#[test]
fn return_type_of_nongeneric_and_generic_call() {
    let p = empty();
    let mut c = Checker::new();
    let num = c.number_type();
    // Non-generic `() => number`.
    let mut sig = Signature::new(SignatureFlags::NONE);
    sig.resolved_return_type = Some(num);
    let sid = c.new_signature(sig);
    assert_eq!(c.get_return_type_of_call(&p, sid, &[], &[]), num);
    // Generic `<T>(x: T) => T` called with a `number` argument infers `T = number`.
    let tp = c.new_type_parameter(None);
    let mut gsig = Signature::new(SignatureFlags::NONE);
    gsig.type_parameters = vec![tp];
    gsig.resolved_return_type = Some(tp);
    let gsid = c.new_signature(gsig);
    assert_eq!(c.get_return_type_of_call(&p, gsid, &[tp], &[num]), num);
}

// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration (2322, named types)
#[test]
fn variable_initializer_not_assignable_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A {\n  x: number;\n}\ninterface B {\n  x: string;\n}\ndeclare const b: B;\nvar a: A = b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `var a: A = b` checks `typeof b` (B) assignable to the annotation (A); the
    // structural property `x: string` is not assignable to `x: number`.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    // Named object types print as their interface names, not `{ ... }`.
    assert_eq!(diags[0].message, "Type 'B' is not assignable to type 'A'.");
}

// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration (2322, intersection target)
#[test]
fn variable_initializer_not_assignable_to_intersection_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A {\n  x: number;\n}\ninterface B {\n  y: string;\n}\ndeclare const a: A;\nvar v: A & B = a;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `a` has type `A`, which lacks the `B` constituent's `y`, so `A` is not
    // assignable to the target intersection `A & B` and `2322` is reported. The
    // target prints as the intersection `A & B`.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'A' is not assignable to type 'A & B'."
    );
}

// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration (intersection, assignable)
#[test]
fn variable_initializer_assignable_to_intersection_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A {\n  x: number;\n}\ninterface B {\n  y: string;\n}\ndeclare const ab: A & B;\nvar v: A & B = ab;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `ab` already has type `A & B` (the interned intersection), so the
    // initializer is assignable to the annotation and no `2322` is reported.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/relater.go:structuredTypeRelatedTo (source intersection
// structurally relates to an object via synthesized properties)
#[test]
fn intersection_source_assignable_to_object_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A {\n  x: number;\n}\ninterface B {\n  y: string;\n}\ninterface AB {\n  x: number;\n  y: string;\n}\ndeclare const ab: A & B;\nvar v: AB = ab;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `ab` has type `A & B`; viewed as an object the intersection synthesizes
    // both `x` and `y`, so it is assignable to `AB` and no `2322` is reported.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/relater.go:errorReporter.reportRelationError (generalizedSource)
#[test]
fn variable_initializer_literal_generalizes_to_base_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "var x: number = \"s\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The fresh string-literal initializer prints as its base type `string`
    // (Go generalizes a literal source when the target cannot hold singletons).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration (assignable -> ok)
#[test]
fn variable_initializer_assignable_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "var s: string = \"ok\";\nvar n: number = 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // A string literal is assignable to `string` and a number literal to
    // `number`; neither declaration must report `2322`.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration (un-annotated)
#[test]
fn unannotated_variable_initializer_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "var z = \"s\";"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // Without an annotation, 4m's `get_type_of_symbol` yields `any` (initializer
    // inference is deferred), so the initializer check must not false-positive.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkBlock (checkSourceElements of statements)
#[test]
fn variable_declaration_in_block_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "{\n  var x: number = \"s\";\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // A `{ ... }` block's statements are checked too, so the nested declaration
    // still reports `2322`.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/relater.go:Checker.getUnmatchedPropertiesWorker (optional target prop)
#[test]
fn missing_optional_target_property_is_assignable() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface S {\n  x: number;\n}\ninterface T {\n  x: number;\n  a?: string;\n}\ndeclare const s: S;\nvar t: T = s;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `S` lacks `a`, but `a` is optional in `T`; for the assignable relation a
    // missing optional target property is fine, so no `2322` is reported.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/relater.go:Relater.propertyRelatedTo (optional-in-source vs required-in-target)
#[test]
fn optional_source_property_not_assignable_to_required_target() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface S {\n  a?: string;\n}\ninterface T {\n  a: string;\n}\ndeclare const s: S;\nvar t: T = s;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `S.a` is optional but `T.a` is required, so `S` is not assignable to `T`
    // even though the property types match (Go reports the relation as false).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (KindEqualsToken -> checkAssignmentOperator, 2322)
#[test]
fn assignment_expression_not_assignable_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A {\n  x: number;\n}\ninterface B {\n  x: string;\n}\ndeclare const a: A;\ndeclare const b: B;\na = b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `a = b` checks `typeof b` (B) assignable to the LHS reference type (A); the
    // structural property `x: string` is not assignable to `x: number`.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    // Named object types print as their interface names, not `{ ... }`.
    assert_eq!(diags[0].message, "Type 'B' is not assignable to type 'A'.");
}

// Go: internal/checker/relater.go:errorReporter.reportRelationError (assignment, generalizedSource)
#[test]
fn assignment_expression_literal_generalizes_to_base_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const n: number;\nn = \"s\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The fresh string-literal RHS prints as its base type `string` (Go
    // generalizes a literal source when the target cannot hold singletons).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkAssignmentOperator (assignable -> ok)
#[test]
fn assignment_expression_assignable_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A {\n  x: number;\n}\ndeclare const a: A;\ndeclare const a2: A;\na = a2;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `a2` (A) is assignable to the LHS `a` (A); the assignment must not report.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkIfStatement (then-branch recursion)
#[test]
fn statement_in_if_then_body_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "if (true) {\n  y;\n}"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The `if` then-branch is descended into, so the undefined name reports 2304.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkIfStatement (else-branch recursion)
#[test]
fn statement_in_if_else_body_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "if (false) {\n} else {\n  y;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The `else` branch is descended into too.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkWhileStatement (body recursion)
#[test]
fn statement_in_while_body_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "while (true) {\n  y;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // A `while` body is descended into (Go's `checkWhileStatement`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkForStatement (body recursion)
#[test]
fn statement_in_for_body_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "for (;;) {\n  y;\n}"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // A `for` body is descended into (Go's `checkForStatement`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkForStatement (initializer declaration list)
#[test]
fn declaration_in_for_initializer_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (var x: number = \"s\"; ; ) {\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The `for` initializer's variable declaration list is checked too, so the
    // mismatching initializer still reports 2322.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkTryStatement (try block recursion)
#[test]
fn statement_in_try_block_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "try {\n  y;\n} catch (e) {\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The `try` block is descended into (Go's `checkTryStatement` -> `checkBlock`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkCatchClause (catch block recursion)
#[test]
fn statement_in_catch_block_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "try {\n} catch (e) {\n  y;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The `catch` clause's block is descended into.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkTryStatement (finally block recursion)
#[test]
fn statement_in_finally_block_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "try {\n} finally {\n  y;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The `finally` block is descended into.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkDoStatement (body recursion)
#[test]
fn statement_in_do_while_body_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "do {\n  y;\n} while (true);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // A `do ... while` body is descended into (Go's `checkDoStatement`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (relational -> boolean)
#[test]
fn relational_operator_yields_boolean() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: number;\ndeclare const b: number;\na < b;",
    );
    let cmp = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let boolean = c.boolean_type();
    // A relational comparison's result type is `boolean`.
    assert_eq!(c.check_expression(&p, cmp), boolean);
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (relational comparability, 2365)
#[test]
fn relational_operator_incomparable_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: string;\ndeclare const n: number;\ns < n;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `string < number`: neither side is number-ish nor are the types comparable,
    // so the operator cannot be applied (Go's `reportOperatorError`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2365);
    assert_eq!(
        diags[0].message,
        "Operator '<' cannot be applied to types 'string' and 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (relational comparable -> ok)
#[test]
fn relational_operator_comparable_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: number;\ndeclare const b: number;\na < b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `number < number`: both sides are number-ish, so no operator error fires.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (equality -> boolean)
#[test]
fn equality_operator_yields_boolean() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: number;\ndeclare const b: number;\na === b;",
    );
    let eq = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let boolean = c.boolean_type();
    // An equality comparison's result type is `boolean`.
    assert_eq!(c.check_expression(&p, eq), boolean);
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (equality comparability, 2367)
#[test]
fn equality_operator_no_overlap_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: string;\ndeclare const n: number;\ns === n;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `string === number`: the types have no overlap (Go's `reportOperatorError`
    // equality branch).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2367);
    assert_eq!(
        diags[0].message,
        "This comparison appears to be unintentional because the types 'string' and 'number' have no overlap."
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (equality comparable -> ok)
#[test]
fn equality_operator_comparable_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: number;\ndeclare const b: number;\na === b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `number === number`: the operands are comparable, so no overlap error fires.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (arithmetic -> number)
#[test]
fn arithmetic_operator_yields_number() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: number;\ndeclare const b: number;\na - b;",
    );
    let sub = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let number = c.number_type();
    // A non-`+` arithmetic operation's result type is `number`.
    assert_eq!(c.check_expression(&p, sub), number);
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (left, 2362)
#[test]
fn arithmetic_nonnumeric_left_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: string;\ns - 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `string - number`: the left-hand operand is not number-ish.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2362);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (right, 2363)
#[test]
fn arithmetic_nonnumeric_right_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: string;\n1 - s;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `number - string`: the right-hand operand is not number-ish.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2363);
    assert_eq!(
        diags[0].message,
        "The right-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (numeric -> ok)
#[test]
fn arithmetic_numeric_operands_report_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const n: number;\nn * n;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `number * number`: both operands are number-ish, so no operand error fires.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkSwitchStatement (case-clause statement recursion)
#[test]
fn statement_in_switch_case_clause_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "switch (1) {\n  case 2:\n    y;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // A `case` clause's statements are descended into (Go's `checkSwitchStatement`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkSwitchStatement (default-clause statement recursion)
#[test]
fn statement_in_switch_default_clause_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "switch (1) {\n  default:\n    y;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // A `default` clause's statements are descended into too.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkSwitchStatement (switch expression checked)
#[test]
fn switch_expression_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "switch (y) {\n}"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The switched expression is checked (Go's `c.checkExpression(node.Expression())`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkSwitchStatement (case-clause expression checked)
#[test]
fn switch_case_clause_expression_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "switch (1) {\n  case y:\n    ;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // A `case` clause's expression is checked (Go's `c.checkExpression(clause.Expression())`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkForInStatement (body recursion)
#[test]
fn statement_in_for_in_body_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (var k in {}) {\n  y;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // A `for-in` body is descended into (Go's `checkForInStatement`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkForInStatement (iterated expression checked)
#[test]
fn for_in_expression_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (var k in y) {\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The iterated (right-hand) expression is checked (Go's `c.checkExpression(data.Expression)`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkForOfStatement (body recursion)
#[test]
fn statement_in_for_of_body_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (var x of []) {\n  y;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // A `for-of` body is descended into (Go's `checkForOfStatement`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkForOfStatement (iterated expression checked)
#[test]
fn for_of_expression_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (var x of y) {\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The iterated (right-hand) expression is checked even though the
    // element/iterable typing is deferred (blocked-by: lib globals).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkGrammarVariableDeclaration
// A `const` loop variable in a for-of (or for-in) has no initializer, but the
// grammar "const declarations must be initialized" (`1155`) is skipped when the
// declaration's parent-parent is a for-in/for-of statement (Go gates the whole
// `initializer == nil` block on that), so there is NO diagnostic.
#[test]
fn for_of_const_loop_variable_without_initializer_reports_no_grammar_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (const x of []) {\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkForOfStatement (iterated element typing)
// The for-of loop variable `x` is typed as the array element type: iterating
// `a: number[]` types `x` as `number`, so a body declaration `const y: string
// = x` fails assignability and reports `2322` (this proves the loop variable
// carries the element type rather than the un-annotated `any`).
#[test]
fn for_of_loop_variable_is_typed_as_array_element() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\ndeclare const a: number[];\nfor (const x of a) {\n  const y: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// Go: internal/checker/checker.go:Checker.checkForOfStatement (element type guard)
// The complement of `for_of_loop_variable_is_typed_as_array_element`: assigning
// the element to a `number` target is fine (the loop variable `x` really is
// `number`, not a blanket error), so there is NO diagnostic.
#[test]
fn for_of_loop_variable_element_type_assignable_to_matching_target() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\ndeclare const a: number[];\nfor (const x of a) {\n  const y: number = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4af: a for-in loop variable is typed as `string` (Go's
// `getTypeForVariableLikeDeclaration` returns `c.stringType` for a for-in
// `VariableDeclaration`). The un-annotated `k` therefore carries `string`, so a
// body declaration `const n: number = k` fails assignability and reports `2322`
// (proving the loop variable is `string`, not the un-annotated `any`).
// Go: internal/checker/checker.go:Checker.getTypeForVariableLikeDeclaration (ForInStatement)
#[test]
fn for_in_loop_variable_is_typed_as_string() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (const k in {}) {\n  const n: number = k;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.getTypeForVariableLikeDeclaration (for-in guard)
// The complement of `for_in_loop_variable_is_typed_as_string`: assigning the
// `string` loop variable to a `string` target is fine (the variable really is
// `string`, not a blanket error), so there is NO diagnostic.
#[test]
fn for_in_loop_variable_string_assignable_to_matching_target() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (const k in {}) {\n  const s: string = k;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (`+` both number-like -> number)
#[test]
fn plus_operator_both_number_yields_number() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: number;\ndeclare const b: number;\na + b;",
    );
    let add = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let number = c.number_type();
    // `number + number`: both operands are number-like, so the result is `number`.
    assert_eq!(c.check_expression(&p, add), number);
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (`+` string-like -> string)
#[test]
fn plus_operator_string_operand_yields_string() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: string;\ndeclare const n: number;\ns + n;",
    );
    let add = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let string = c.string_type();
    // `string + number`: one operand is string-like, so the result is `string`.
    assert_eq!(c.check_expression(&p, add), string);
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (`+` no applicable result -> 2365)
#[test]
fn plus_operator_incompatible_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O {\n  x: number;\n}\ndeclare const a: O;\ndeclare const b: O;\na + b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `O + O`: neither number-like, bigint-like, string-like, nor `any`, so the
    // operator cannot be applied (Go's `reportOperatorError`). Named object types
    // print as their interface name.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2365);
    assert_eq!(
        diags[0].message,
        "Operator '+' cannot be applied to types 'O' and 'O'."
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (`+` any/error -> no cascade)
#[test]
fn plus_operator_error_operand_does_not_cascade() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "y + 1;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `y` is undefined (error type, which is `any`-like); the `+` arm must treat
    // it as `any` and not cascade a `2365` operator error on top of the `2304`.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (`||` non-falsy left -> left type)
#[test]
fn logical_or_non_falsy_left_yields_left_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "true || 1;");
    let or = expr_stmt_expression(&p, 0);
    let mut c = Checker::new();
    let true_ty = c.true_type();
    // `true || 1`: the left operand has no falsy facts, so the result is the
    // left type (Go leaves `resultType = leftType`).
    assert_eq!(c.check_expression(&p, or), true_ty);
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (`||` falsy left -> union)
#[test]
fn logical_or_falsy_left_yields_union() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: string;\ndeclare const n: number;\ns || n;",
    );
    let or = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    // `string || number`: the left type can be falsy, so the result is the union
    // of the left type's truthy part (`string`) and the right type (`number`).
    let t = c.check_expression(&p, or);
    assert_eq!(c.type_to_string(t), "string | number");
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (`&&` non-truthy left -> left type)
#[test]
fn logical_and_non_truthy_left_yields_left_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "false && 1;");
    let and = expr_stmt_expression(&p, 0);
    let mut c = Checker::new();
    let false_ty = c.false_type();
    // `false && 1`: the left operand has no truthy facts, so the result is the
    // left type (Go leaves `resultType = leftType`).
    assert_eq!(c.check_expression(&p, and), false_ty);
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (`&&` truthy left -> union)
#[test]
fn logical_and_truthy_left_yields_right_when_falsy_part_empty() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface O {\n  x: number;\n}\ndeclare const a: number;\ndeclare const o: O;\na && o;",
    );
    let and = expr_stmt_expression(&p, 3);
    let right = match p.arena().data(and) {
        NodeData::BinaryExpression(d) => d.right,
        _ => panic!("binary expression"),
    };
    let mut c = Checker::new();
    // `number && O`: the left type can be truthy, so the result is the union of
    // the left base type's definitely-falsy part and the right type. The object
    // right type has no falsy part, so the union collapses to the right type.
    let right_type = c.check_expression(&p, right);
    assert_eq!(c.check_expression(&p, and), right_type);
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (`??` non-nullable left -> left type)
#[test]
fn nullish_coalesce_non_nullable_left_yields_left_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: string;\ndeclare const n: number;\ns ?? n;",
    );
    let qq = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let string = c.string_type();
    // `string ?? number`: the left type is never `undefined`/`null`, so the
    // result is the left type (Go leaves `resultType = leftType`).
    assert_eq!(c.check_expression(&p, qq), string);
}

// 4ba slice 1 (genuine RED): `??` refines its result to the union of the left
// type's non-nullable part and the right type when the left can be
// `undefined`/`null` (Go's `KindQuestionQuestionToken` arm ->
// `getUnionType([GetNonNullableType(left), right])` when
// `hasTypeFacts(left, EQUndefinedOrNull)`). With `x: string | undefined`,
// `x ?? "d"` therefore has a type assignable to `string`, so assigning it to a
// `string` variable reports nothing. Before the refinement the arm returned the
// raw left type `string | undefined`, which is not assignable to `string` and
// reported `2322`.
// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12462)
#[test]
fn nullish_coalesce_removes_undefined_assignable_to_string() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | undefined;\nvar s: string = x ?? \"d\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4ba slice 1 (type witness): the `??` result of `x ?? "d"` (with
// `x: string | undefined`) carries no `IsUndefined`/`IsNull` facts \u2014 the nullish
// refinement removed the nullable `undefined` part of the left operand (Go's
// `GetNonNullableType(left)`). Before the refinement the arm returned the raw
// `string | undefined`, whose `undefined` member carries `IsUndefined`.
// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12462)
#[test]
fn nullish_coalesce_result_drops_nullable_facts() {
    let p =
        StubProgram::parse_and_bind("/a.ts", "declare const x: string | undefined;\nx ?? \"d\";");
    let qq = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let result = c.check_expression(&p, qq);
    assert!(
        !c.has_type_facts(result, crate::TypeFacts::IS_UNDEFINED_OR_NULL),
        "expected the `??` result to drop the nullable part"
    );
}

// 4ba slice 2 (`??=` shares the refinement, green-on-arrival): the compound
// nullish assignment `x ??= "d"` produces the same refined result type as `??`
// (Go's `KindQuestionQuestionEqualsToken` shares the arm), so the value of
// `(x ??= "d")` with `x: string | undefined` is assignable to `string`. Using
// it as a `string` initializer reports nothing. Like `??`, the refinement rides
// the slice-1 arm; the compound form additionally runs `checkAssignmentOperator`
// (here `"d"` is assignable to the `string | undefined` reference, so no `2322`).
// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12462)
#[test]
fn nullish_coalesce_assign_removes_undefined_assignable_to_string() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare let x: string | undefined;\nvar s: string = x ??= \"d\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bb slice 2 (genuine RED): the `??` result union is subtype-reduced (Go's
// `getUnionTypeEx([GetNonNullableType(left), right], UnionReductionSubtype)`).
// With `x: "a" | undefined` and `y: string`, `x ?? y` is
// `getNonNullableType("a" | undefined) | string` = `"a" | string`, which
// subtype-reduces to `string` (the literal `"a"` is subsumed by `string`).
// Assigning to `number` therefore reports `2322` whose SOURCE is the reduced
// `string`. Before subtype reduction the result kept both members and the
// message read `Type '"a" | string' ...`.
// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12468)
#[test]
fn nullish_coalesce_result_is_subtype_reduced() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: \"a\" | undefined;\ndeclare const y: string;\nconst n: number = x ?? y;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// 4bb slice 3 (genuine RED): mixing `??` with `||` without parentheses is a
// grammar error (`5076`). `a ?? b || c` parses as `(a ?? b) || c` (`??` and
// `||` share precedence, left-associative), so when the `??` node is checked
// its grandparent is a `||` whose left operand is the `??` expression: Go's
// `checkNullishCoalesceOperands` reports `5076` on that `a ?? b` node.
// Go: internal/checker/checker.go:Checker.checkNullishCoalesceOperands(12859)
#[test]
fn nullish_coalesce_mixed_with_logical_or_reports_5076() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: number;\ndeclare const b: number;\ndeclare const c: number;\na ?? b || c;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 5076);
    assert_eq!(
        diags[0].message,
        "'??' and '||' operations cannot be mixed without parentheses."
    );
}

// 4bb slice 3 (branch 2, green-on-arrival): `a || b ?? c` parses as
// `(a || b) ?? c`, so the `??` node's LEFT operand is a `||` expression. Go's
// `checkNullishCoalesceOperands` reports `5076` on that left operand with the
// operands in `||`-then-`??` order.
// Go: internal/checker/checker.go:Checker.checkNullishCoalesceOperands(12866)
#[test]
fn logical_or_then_nullish_coalesce_reports_5076() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: number;\ndeclare const b: number;\ndeclare const c: number;\na || b ?? c;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 5076);
    assert_eq!(
        diags[0].message,
        "'||' and '??' operations cannot be mixed without parentheses."
    );
}

// 4bb slice 3 (branch 3, green-on-arrival): `a ?? b && c` parses as
// `a ?? (b && c)` (`&&` binds tighter than `??`), so the `??` node's RIGHT
// operand is a `&&` expression. Go reports `5076` on that right operand with
// the operands in `??`-then-`&&` order.
// Go: internal/checker/checker.go:Checker.checkNullishCoalesceOperands(12871)
#[test]
fn nullish_coalesce_with_logical_and_reports_5076() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: number;\ndeclare const b: number;\ndeclare const c: number;\na ?? b && c;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 5076);
    assert_eq!(
        diags[0].message,
        "'??' and '&&' operations cannot be mixed without parentheses."
    );
}

// 4bb slice 3 (contrast, green-on-arrival): explicit parentheses resolve the
// ambiguity, so `(a ?? b) || c` reports NO `5076` — the `??` node's grandparent
// is a parenthesized expression (not a binary `||`), and neither operand of the
// `??` is a logical binary.
// Go: internal/checker/checker.go:Checker.checkNullishCoalesceOperands(12859)
#[test]
fn parenthesized_nullish_coalesce_with_logical_or_reports_nothing() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: number;\ndeclare const b: number;\ndeclare const c: number;\n(a ?? b) || c;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (compound arithmetic operand, 2362)
#[test]
fn compound_arithmetic_assignment_checks_operand() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: string;\ns *= 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `s *= 1`: the compound arithmetic operator checks each operand like its
    // non-compound form, so the non-numeric left-hand side reports `2362`.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2362);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkAssignmentOperator (`+=` result not assignable, 2322)
#[test]
fn plus_equals_assignment_not_assignable_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const n: number;\nn += \"s\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `n += "s"`: the `+` result is `string` (string-like right operand), which
    // is not assignable to the `number` left-hand side, so `2322` is reported.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkAssignmentOperator (`&&=` right not assignable, 2322)
#[test]
fn logical_and_equals_assignment_not_assignable_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const n: number;\nn &&= \"s\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `n &&= "s"`: the logical compound assignment checks the right operand type
    // (`string`) against the `number` left-hand side, so `2322` is reported.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkAssignmentOperator (`+=` assignable -> ok)
#[test]
fn plus_equals_assignment_assignable_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const n: number;\nn += 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `n += 1`: the `+` result is `number`, assignable to the `number` left-hand
    // side, so no diagnostic is reported.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkThrowStatement (expression checked)
#[test]
fn throw_statement_expression_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "throw y;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The thrown expression is checked (Go's `c.checkExpression(throwExpr)`), so
    // the undefined name reports 2304.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkLabeledStatement (labeled statement recursion)
#[test]
fn labeled_statement_body_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "lbl: y;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The labeled statement is descended into (Go's `checkLabeledStatement` ->
    // `checkSourceElement(statement)`), so the undefined name reports 2304.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkAssignmentOperator (`||=` right not assignable, 2322)
#[test]
fn logical_or_equals_assignment_not_assignable_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const n: number;\nn ||= \"s\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `n ||= "s"`: the logical compound assignment checks the right operand type
    // (`string`) against the `number` left-hand side, so `2322` is reported.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkAssignmentOperator (`??=` right not assignable, 2322)
#[test]
fn nullish_coalesce_equals_assignment_not_assignable_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const n: number;\nn ??= \"s\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `n ??= "s"`: the nullish compound assignment checks the right operand type
    // (`string`) against the `number` left-hand side, so `2322` is reported.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkCallExpression (Argument_of_type_0..., 2345)
#[test]
fn call_argument_not_assignable_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(a: number) {}\nf(\"s\");",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `f("s")`: the fresh string-literal argument is not assignable to the
    // `number` parameter, so the call reports `2345` at the argument. The
    // literal source generalizes to its base type `string` in the message.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2345);
    assert_eq!(
        diags[0].message,
        "Argument of type 'string' is not assignable to parameter of type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.getArgumentArityError (too few, 2554)
#[test]
fn call_too_few_arguments_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(a: number) {}\nf();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `f()`: zero arguments for a one-parameter signature is below the minimum
    // argument count, so the call reports `2554`.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2554);
    assert_eq!(diags[0].message, "Expected 1 arguments, but got 0.");
}

// Go: internal/checker/checker.go:Checker.getArgumentArityError (too many, 2554)
#[test]
fn call_too_many_arguments_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(a: number) {}\nf(1, 2);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `f(1, 2)`: two arguments exceed the one-parameter signature's maximum, so
    // the call reports `2554`.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2554);
    assert_eq!(diags[0].message, "Expected 1 arguments, but got 2.");
}

// Go: internal/checker/checker.go:Checker.getSignatureFromDeclaration (optional param -> min count)
#[test]
fn call_optional_parameter_allows_fewer_arguments() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(a: number, b?: number) {}\nf(1);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `f(1)`: the optional `b?` parameter lowers the minimum argument count to 1,
    // so a one-argument call is within arity and reports nothing.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.getArgumentArityError (optional param range, 2554)
#[test]
fn call_optional_parameter_too_many_reports_range() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(a: number, b?: number) {}\nf(1, 2, 3);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `f(1, 2, 3)`: three arguments exceed the 1-2 acceptable range, so the call
    // reports `2554` with the `min-max` parameter range in the message.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2554);
    assert_eq!(diags[0].message, "Expected 1-2 arguments, but got 3.");
}

// Go: internal/checker/checker.go:Checker.checkCallExpression (returnType = getReturnTypeOfSignature)
#[test]
fn call_result_type_is_signature_return_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "function f(a: number): string { return \"\"; }\nf(1);",
    );
    let call = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let string = c.string_type();
    // The call's result type is the (annotated) return type of the resolved
    // signature (Go's `checkCallExpression` -> `getReturnTypeOfSignature`).
    assert_eq!(c.check_expression(&p, call), string);
}

// Go: internal/checker/checker.go:Checker.checkCallExpression (well-typed call -> ok)
#[test]
fn call_well_typed_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(a: number) {}\nf(1);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `f(1)`: correct arity and an assignable argument, so the call reports
    // nothing.
    assert!(c.get_diagnostics(root).is_empty());
}

// 4ba slice 3 (genuine RED): invoking a possibly-`undefined` value reports
// `2722` "Cannot invoke an object which is possibly 'undefined'." Go's
// `resolveCallExpression` types the callee with `checkNonNullTypeWithReporter`
// using the `reportCannotInvokePossiblyNullOrUndefinedError` reporter (the
// 2721/2722/2723 family, distinct from the 18047/18048/18049 property-access
// family). With `f: (() => void) | undefined`, the callee is possibly
// `undefined`, so the invocation reports `2722` and then resolves the call on
// the non-null `() => void`. Before the callee non-null check, the union callee
// had no call signatures, so the call silently yielded `error` (0 diagnostics).
// Go: internal/checker/checker.go:Checker.resolveCallExpression(8478)/reportCannotInvokePossiblyNullOrUndefinedError(9854)
#[test]
fn call_on_possibly_undefined_callee_reports_2722() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const f: (() => void) | undefined;\nf();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2722);
    assert_eq!(
        diags[0].message,
        "Cannot invoke an object which is possibly 'undefined'."
    );
}

// 4ba slice 3 guard (green-on-arrival): invoking a possibly-`null` callee
// reports `2721` "Cannot invoke an object which is possibly 'null'." (the
// `IsNull`-only fact selects the `_null` message). Same code path as the
// undefined case.
// Go: internal/checker/checker.go:Checker.reportCannotInvokePossiblyNullOrUndefinedError(9854)
#[test]
fn call_on_possibly_null_callee_reports_2721() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const f: (() => void) | null;\nf();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2721);
    assert_eq!(
        diags[0].message,
        "Cannot invoke an object which is possibly 'null'."
    );
}

// 4ba slice 3 guard (green-on-arrival): invoking a possibly-`null`-or-
// `undefined` callee reports `2723` (both `IsUndefined` and `IsNull` facts).
// Go: internal/checker/checker.go:Checker.reportCannotInvokePossiblyNullOrUndefinedError(9854)
#[test]
fn call_on_possibly_null_or_undefined_callee_reports_2723() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const f: (() => void) | null | undefined;\nf();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2723);
    assert_eq!(
        diags[0].message,
        "Cannot invoke an object which is possibly 'null' or 'undefined'."
    );
}

// 4ba slice 3 guard (the property-access path, distinct family): a method call
// `o.m()` on a possibly-`undefined` `o` reports `18048` "'o' is possibly
// 'undefined'." \u2014 NOT a `2722`. The diagnostic comes from typing the property
// access object `o` via `checkNonNullExpression` (the 4az property-access path),
// which fires before the call's callee non-null check ever sees the (already
// non-nullable) method type `() => void`. This confirms the task's note that the
// 4az property-access path already covers `o.m`, and the fresh `2722` surface is
// invoking a possibly-`undefined` value directly.
// Go: internal/checker/checker.go:Checker.checkPropertyAccessExpression -> checkNonNullExpression
#[test]
fn call_on_property_access_possibly_undefined_reports_18048() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const o: { m(): void } | undefined;\no.m();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 18048);
    assert_eq!(diags[0].message, "'o' is possibly 'undefined'.");
}

// 4ba slice 3 guard: a non-nullable callee is invoked without any non-null
// diagnostic (the `IsUndefinedOrNull` facts are absent, so the callee non-null
// check is the identity).
// Go: internal/checker/checker.go:Checker.checkNonNullTypeWithReporter(7381)
#[test]
fn call_on_non_nullable_callee_reports_nothing() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const f: () => void;\nf();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.resolveCall/reportCallResolutionErrors (No_overload_matches_this_call, 2769)
#[test]
fn overloaded_call_matching_no_overload_reports_no_overload_matches() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(a: number): void;\ndeclare function f(a: string): void;\nf(true);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `f(true)`: both overloads have correct arity (1 parameter), but `true`
    // (boolean) is assignable to neither the `number` nor the `string`
    // parameter, so no overload applies and Go reports `2769` (the per-overload
    // elaboration chain is deferred).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2769);
    assert_eq!(diags[0].message, "No overload matches this call.");
}

// Go: internal/checker/checker.go:Checker.chooseOverload (an applicable overload -> ok)
#[test]
fn overloaded_call_matching_an_overload_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(a: number): void;\ndeclare function f(a: string): void;\nf(\"s\");",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `f("s")`: the second overload's `string` parameter accepts the argument,
    // so the call resolves and reports nothing.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.isSignatureApplicable (one correct-arity overload -> 2345)
#[test]
fn overloaded_call_single_arity_match_reports_argument_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(a: number): void;\ndeclare function f(a: number, b: number): void;\nf(\"s\");",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `f("s")`: only the first overload has correct arity (1 parameter), and its
    // argument fails, so Go reports that candidate's own `2345` (no overload
    // chain when a single candidate had the argument error).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2345);
    assert_eq!(
        diags[0].message,
        "Argument of type 'string' is not assignable to parameter of type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.getArgumentArityError (no overload arity matches, 2575)
#[test]
fn overloaded_call_no_arity_match_reports_arity_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(a: number): void;\ndeclare function f(a: number, b: number, c: number): void;\ndeclare const n: number;\nf(n, n);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `f(n, n)`: two arguments match neither the 1-parameter nor the 3-parameter
    // overload, but lie strictly between the smallest minimum (1) and the largest
    // maximum (3), so Go reports `2575`.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2575);
    assert_eq!(
        diags[0].message,
        "No overload expects 2 arguments, but overloads do exist that expect either 1 or 3 arguments."
    );
}

// Go: internal/checker/checker.go:Checker.checkClassMember (method body recursion)
#[test]
fn class_method_body_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  m() {\n    y;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // A class method's body is descended into (Go's `checkClassMember` ->
    // `checkSourceElement(body)`), so the undefined name reports 2304.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkPropertyDeclaration (initializer not assignable, 2322)
#[test]
fn class_property_initializer_not_assignable_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  x: number = \"s\";\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `x: number = "s"`: the string-literal initializer is not assignable to the
    // `number` annotation, so the property reports `2322` (the literal source
    // generalizes to its base type `string`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkPropertyDeclaration (assignable -> ok)
#[test]
fn class_property_initializer_assignable_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  x: number = 1;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `x: number = 1`: the number-literal initializer is assignable to `number`,
    // so the property reports nothing.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkPropertyDeclaration (un-annotated -> no false positive)
#[test]
fn class_property_unannotated_initializer_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  x = \"s\";\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // Without an annotation the property type is `any` (initializer inference is
    // deferred), so the initializer check must not false-positive.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration (implements satisfaction, 2420)
#[test]
fn class_incorrectly_implements_interface_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I {\n  x: number;\n}\nclass C implements I {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `class C implements I {}`: the class instance type lacks the required `x`
    // member, so it is not assignable to `I` and the class reports `2420`
    // (Go's implements loop -> `Class_0_incorrectly_implements_interface_1`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2420);
    assert_eq!(
        diags[0].message,
        "Class 'C' incorrectly implements interface 'I'."
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration (implements satisfied -> ok)
#[test]
fn class_correctly_implements_interface_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I {\n  x: number;\n}\nclass C implements I {\n  x: number = 1;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `class C implements I { x: number = 1 }`: the class instance type has the
    // required `x: number`, so it is assignable to `I` and nothing is reported.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration (extends compatibility, 2415)
#[test]
fn class_incorrectly_extends_base_class_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B {\n  x: number = 0;\n}\nclass D extends B {\n  x: string = \"s\";\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `class D extends B { x: string }` (B has `x: number`): the derived `x` is
    // an incompatible override, so the derived instance type is not assignable to
    // the base instance type and the class reports `2415` (Go's extends check ->
    // `Class_0_incorrectly_extends_base_class_1`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2415);
    assert_eq!(
        diags[0].message,
        "Class 'D' incorrectly extends base class 'B'."
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration (compatible override -> ok)
#[test]
fn class_correctly_extends_base_class_with_compatible_override_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B {\n  x: number = 0;\n}\nclass D extends B {\n  x: number = 1;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `class D extends B { x: number }`: the derived `x: number` matches the base
    // `x: number`, so the derived instance type is assignable to the base and
    // nothing is reported.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration (no override -> ok)
#[test]
fn class_extends_base_class_without_override_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B {\n  x: number = 0;\n}\nclass D extends B {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `class D extends B {}`: the derived instance type inherits `x: number`, so
    // it is assignable to the base instance type and nothing is reported.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration (no heritage -> no heritage diags)
#[test]
fn plain_class_without_heritage_reports_no_heritage_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  x: number = 1;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // A class with no `extends`/`implements` clause must not trigger any heritage
    // relation check, so nothing is reported.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration (implements unresolved -> skipped)
#[test]
fn class_implements_unresolved_interface_reports_no_heritage_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C implements Missing {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The implements target does not resolve to a type (the error type), so the
    // satisfaction check is skipped (Go's `if !c.isErrorType(t)`) and no `2420`
    // is reported. The full version also reports the unresolved-name diagnostic
    // via `checkTypeReferenceNode`, which is deferred.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration (implements loop, per-interface)
#[test]
fn class_implements_multiple_interfaces_reports_for_each_unsatisfied() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I {\n  x: number;\n}\ninterface J {\n  y: string;\n}\nclass C implements I, J {\n  x: number = 1;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `class C implements I, J { x: number = 1 }`: `I` is satisfied (C has `x`),
    // but `J` is not (C lacks `y`), so the implements loop reports exactly one
    // `2420` naming `J`.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2420);
    assert_eq!(
        diags[0].message,
        "Class 'C' incorrectly implements interface 'J'."
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration (extends + implements both checked)
#[test]
fn class_extends_and_implements_both_relations_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B {\n  x: number = 0;\n}\ninterface I {\n  y: string;\n}\nclass D extends B implements I {\n  x: string = \"s\";\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `class D extends B implements I { x: string }`: the override of `x` makes
    // `D` incorrectly extend `B` (2415), and `D` also lacks `I`'s `y` member
    // (2420). Both heritage relations are checked, extends before implements.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 2);
    assert_eq!(diags[0].code, 2415);
    assert_eq!(
        diags[0].message,
        "Class 'D' incorrectly extends base class 'B'."
    );
    assert_eq!(diags[1].code, 2420);
    assert_eq!(
        diags[1].message,
        "Class 'D' incorrectly implements interface 'I'."
    );
}

// Go: internal/checker/checker.go:Checker.checkReturnStatement (returned expression checked)
#[test]
fn return_statement_expression_in_function_body_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f() {\n  return y;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The function body is descended into and the returned expression checked
    // (Go's `checkReturnStatement` -> `checkExpression`), so the undefined name
    // reports 2304.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkReturnStatement (return type not assignable, 2322)
#[test]
fn return_type_mismatch_with_annotation_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(): number {\n  return \"s\";\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `function f(): number { return "s"; }`: the returned string-literal is not
    // assignable to the explicit `number` return type, so `2322` is reported (the
    // literal source generalizes to its base type `string`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkReturnStatement (assignable -> ok)
#[test]
fn return_type_assignable_to_annotation_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(): string {\n  return \"s\";\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `function f(): string { return "s"; }`: the returned string-literal is
    // assignable to the explicit `string` return type, so nothing is reported.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkReturnStatement (un-annotated -> deferred, no false positive)
#[test]
fn return_in_unannotated_function_reports_no_return_type_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f() {\n  return \"s\";\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // Without an explicit return-type annotation, return-type checking is
    // deferred (contextual inference), so the assignable check must not run.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkReturnStatement (return inside a method body)
#[test]
fn return_type_mismatch_in_method_body_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  m(): number {\n    return \"s\";\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The method body is descended into (class-member checking) and the return is
    // checked against the method's explicit `number` return type, so `2322` is
    // reported.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkReturnStatement (return inside a function-expression body)
#[test]
fn return_type_mismatch_in_function_expression_body_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const f = function (): number {\n  return \"s\";\n};",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The function-expression body is descended into (Go's
    // `checkFunctionExpressionOrObjectLiteralMethod` -> body check) and the
    // return is checked against the expression's explicit `number` return type,
    // so `2322` is reported.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkReturnStatement (return inside an arrow-function block body)
#[test]
fn return_type_mismatch_in_arrow_function_body_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const f = (): number => {\n  return \"s\";\n};",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The arrow-function block body is descended into (Go's `checkArrowFunction`
    // -> body check) and the return is checked against the arrow's explicit
    // `number` return type, so `2322` is reported.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkReturnStatement (assignable function-expression return -> ok)
#[test]
fn return_type_assignable_in_function_expression_body_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const f = function (): string {\n  return \"s\";\n};",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The returned string-literal is assignable to the explicit `string` return
    // type, so the descended body produces no diagnostic.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkReturnStatement (assignable arrow return -> ok)
#[test]
fn return_type_assignable_in_arrow_function_body_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const f = (): string => {\n  return \"s\";\n};",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The returned string-literal is assignable to the explicit `string` return
    // type, so the descended block body produces no diagnostic.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkFunctionExpressionOrObjectLiteralMethodDeferred
// (concise expression body checked against the annotated return type)
#[test]
fn return_type_mismatch_in_arrow_concise_body_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const f = (): number => \"s\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The arrow's concise (non-block) expression body is checked against the
    // arrow's explicit `number` return type (Go's
    // `checkFunctionExpressionOrObjectLiteralMethodDeferred` -> `checkExpression`
    // body -> `checkReturnExpression`), so the `"s"` body reports `2322`.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkFunctionExpressionOrObjectLiteralMethodDeferred
// (assignable concise expression body -> ok)
#[test]
fn return_type_assignable_in_arrow_concise_body_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const f = (): number => 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The numeric-literal concise body is assignable to the explicit `number`
    // return type, so the checked body produces no diagnostic.
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkFunctionExpressionOrObjectLiteralMethodDeferred
// (concise expression body assignable to a matching `string` annotation -> ok)
#[test]
fn return_type_matching_string_in_arrow_concise_body_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const f = (): string => \"s\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The string-literal concise body is assignable to the explicit `string`
    // return type, so no diagnostic is reported (the concise-body check does not
    // over-report).
    assert!(c.get_diagnostics(root).is_empty());
}

// Go: internal/checker/checker.go:Checker.checkArrowFunction (body descended -> nested name checked)
#[test]
fn arrow_function_body_descends_into_nested_expression() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const f = () => {\n  return y;\n};",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // Even without a return-type annotation the arrow body is descended into, so
    // the undefined name `y` in the returned expression reports `2304` (Go's
    // `checkArrowFunction` -> body -> `checkExpression`).
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/checker.go:Checker.checkIndexedAccess (string-literal index)
#[test]
fn check_element_access_string_index() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Foo {\n  bar: string;\n}\ndeclare const foo: Foo;\nfoo[\"bar\"];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let s = c.string_type();
    assert_eq!(c.check_expression(&p, access), s);
}

// Go: internal/checker/checker.go:Checker.getIndexedAccessType (number index signature)
#[test]
fn check_element_access_number_index_signature() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Box {\n  [n: number]: string;\n}\ndeclare const b: Box;\nb[0];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, access), c.string_type());
}

// Go: internal/checker/checker.go:Checker.getIndexedAccessType (string index signature)
#[test]
fn check_element_access_string_index_signature() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Dict {\n  [k: string]: number;\n}\ndeclare const d: Dict;\nd[\"x\"];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, access), c.number_type());
}

// Go: internal/checker/checker.go:Checker.getIndexedAccessType (computed string index)
#[test]
fn check_element_access_string_index_with_variable_key() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Dict {\n  [k: string]: number;\n}\ndeclare const d: Dict;\ndeclare const key: string;\nd[key];",
    );
    let access = expr_stmt_expression(&p, 3);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, access), c.number_type());
}

// Go: internal/checker/checker.go:Checker.getIndexedAccessType (Array<T>[n:number]:T)
#[test]
fn check_element_access_array_element_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\ndeclare const a: Array<number>;\na[0];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, access), c.number_type());
}

// Go: internal/checker/checker.go:Checker.getTypeFromArrayOrTupleTypeNode (`T[]`)
// An `ArrayTypeNode` (`number[]`) resolves to the global `Array<number>`
// reference, so `a[0]` resolves through the instantiated `[n: number]: T`
// element signature to `number` (the synthetic global `Array` drives it,
// mirroring 4ac's `Array<number>` path but via array syntax).
#[test]
fn check_element_access_number_array_element_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\ndeclare const a: number[];\na[0];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, access), c.number_type());
}

// 4ae: a `TupleType` node `[string, number]` resolves to a fixed-arity tuple
// type whose element types sit by position, so `t[0]` resolves to `string`
// (the first element) through tuple element access by a literal index.
// Go: internal/checker/checker.go:Checker.getTypeFromArrayOrTupleTypeNode (tuple branch)
#[test]
fn check_element_access_tuple_first_element_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const t: [string, number];\nt[0];");
    let access = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, access), c.string_type());
}

// Go: internal/checker/checker.go:Checker.getTypeFromArrayOrTupleTypeNode (tuple branch)
// Guard: the second element (`t[1]`) resolves to `number`, proving element
// access is positional rather than a blanket first-element result.
#[test]
fn check_element_access_tuple_second_element_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const t: [string, number];\nt[1];");
    let access = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, access), c.number_type());
}

// 4af: a NON-literal `number` index over a fixed-arity tuple yields the union of
// all its element types (Go's tuple `[number]` index info is the union of the
// element types). `t[i]` with `i: number` and `t: [string, number]` resolves to
// `string | number` rather than a single positional element.
// Go: internal/checker/checker.go:Checker.getPropertyTypeForIndexType (tuple number index)
#[test]
fn check_element_access_tuple_non_literal_number_index_yields_element_union() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const t: [string, number];\ndeclare const i: number;\nt[i];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, access), c.string_or_number_type());
}

// 4ae: `readonly T[]` is a `readonly` type operator over an array type; Go's
// `getArrayOrTupleTargetType` picks `globalReadonlyArrayType` when the array
// node's parent is a `readonly` operator. A synthetic top-level
// `interface ReadonlyArray<T>` stands in for the lib type, so
// `declare const r: readonly string[]; r[0];` resolves through the instantiated
// `readonly [n: number]: T` element signature to `string`.
// Go: internal/checker/checker.go:Checker.getTypeFromTypeOperatorNode (readonly) + getArrayOrTupleTargetType
#[test]
fn check_element_access_readonly_array_element_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface ReadonlyArray<T> {\n  readonly [n: number]: T;\n  readonly length: number;\n}\ndeclare const r: readonly string[];\nr[0];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, access), c.string_type());
}

// 4ae: `ReadonlyArray<T>` is a plain generic type reference (no `readonly`
// operator), so it resolves through the existing `getTypeFromTypeReference`
// path exactly like `Array<T>`; a synthetic `interface ReadonlyArray<T>` drives
// `r[0]` -> `string`. This confirms the reference form alongside the `readonly
// T[]` operator form, and requires no new construction code.
// Go: internal/checker/checker.go:Checker.getTypeFromTypeReference
#[test]
fn check_element_access_readonly_array_type_reference_element_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface ReadonlyArray<T> {\n  readonly [n: number]: T;\n  readonly length: number;\n}\ndeclare const r: ReadonlyArray<string>;\nr[0];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, access), c.string_type());
}

// 4af: indexing with a key whose type is not string-like/number-like/symbol-like
// (and with no applicable index signature) reports `2538`. `o[k]` with
// `k: boolean` falls through Go's `getPropertyTypeForIndexType` to the final
// "cannot be used as an index type" branch: `boolean` is neither a string/number
// literal name nor string/number, so it never enters the index-signature block
// and lands on the trailing `2538` error.
//
// The type-string argument prints the boolean as its `false | true` union: the
// `false | true` => `boolean` collapse in `typeToString` is DEFER'd to 4j's node
// builder, so the diagnostic code (2538) is the behavior under test here.
// Go: internal/checker/checker.go:Checker.getPropertyTypeForIndexType (2538 branch)
#[test]
fn check_element_access_boolean_index_reports_2538() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O {\n  a: number;\n}\ndeclare const o: O;\ndeclare const k: boolean;\no[k];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2538);
    assert_eq!(
        diags[0].message,
        "Type 'false | true' cannot be used as an index type."
    );
}

// 4af: a fixed-arity tuple's `.length` resolves to the numeric literal type of
// its arity. Go's generated tuple target gives `length` the union of the literal
// lengths `minLength..=arity`, which for a fixed `[string, number]` collapses to
// the single numeric literal `2`. The 4af subset returns that numeric literal
// directly (printed as `2`, distinguishing it from the `number` primitive).
// Go: internal/checker/checker.go:Checker.createTupleTargetType (length member)
#[test]
fn tuple_length_resolves_to_numeric_literal_arity() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const t: [string, number];\nt.length;");
    let access = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let length = c.check_expression(&p, access);
    assert_eq!(c.type_to_string(length), "2");
}

// Go: internal/checker/checker.go:Checker.getApparentType (cross-file lib `String`)
#[test]
fn cross_file_global_resolves_string_property_via_lib() {
    // File A is the "lib" declaring the `String` wrapper; file B is the source
    // referencing `s.length`. Checking B resolves `length` against the lib
    // file's `String` interface via the merged globals + apparent type, so there
    // is NO 2339 (the cross-file global-resolution tracer).
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/lib.d.ts", "interface String {\n  length: number;\n}"),
        ("/b.ts", "declare const s: string;\ns.length;"),
    ]));
    let file_b = p.source_files()[1];
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(file_b);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.getDiagnostics (GetDiagnosticsForFile filtering)
#[test]
fn get_diagnostics_is_filtered_per_file() {
    // File A has a type error; file B is clean. Each file's diagnostics are
    // returned independently — file B's set does not include file A's 2322
    // (Go's `collection.GetDiagnosticsForFile(name)`).
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/a.ts", "var a: number = \"x\";"),
        ("/b.ts", "var b: number = 1;"),
    ]));
    let files = p.source_files();
    let (file_a, file_b) = (files[0], files[1]);
    let mut c = Checker::new_checker(p);
    let a_diags = c.get_diagnostics(file_a).to_vec();
    let b_diags = c.get_diagnostics(file_b).to_vec();
    assert_eq!(a_diags.len(), 1, "file A diagnostics: {a_diags:?}");
    assert_eq!(a_diags[0].code, 2322);
    assert!(
        b_diags.is_empty(),
        "file B must not include file A's diagnostics, got {b_diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkPropertyAccessExpression (no lib -> 2339)
#[test]
fn string_property_without_lib_reports_2339() {
    // Without a lib file declaring `String`, the apparent type of `string` is
    // itself, so `length` does not resolve and the access reports 2339 — the
    // negative half of the cross-file tracer.
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/b.ts",
        "declare const s: string;\ns.length;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2339);
}

// 4ab: `instanceof`/`in` expression checks, reachable now that 4z/4aa give the
// checker global resolution (synthetic global `interface Function {}` drives the
// `instanceof` right-operand check).

// Go: internal/checker/checker.go:Checker.checkInstanceOfExpression (result boolean, tracer)
#[test]
fn instanceof_expression_yields_boolean() {
    // `o instanceof f` always evaluates to the `boolean` primitive type.
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const o: object;\ndeclare function f(): void;\no instanceof f;",
    );
    let expr = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let boolean = c.boolean_type();
    assert_eq!(c.check_expression(&p, expr), boolean);
}

// Go: internal/checker/checker.go:Checker.checkInstanceOfExpression (left primitive, 2358)
#[test]
fn instanceof_primitive_left_reports_diagnostic() {
    // The left-hand side of `instanceof` must be `any`, an object type, or a
    // type parameter; a `string` (primitive) operand reports 2358.
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\ndeclare const s: string;\ns instanceof f;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert_eq!(diags[0].code, 2358);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an 'instanceof' expression must be of type 'any', an object type or a type parameter."
    );
}

// Go: internal/checker/checker.go:Checker.resolveInstanceofExpression (right not Function, 2359)
#[test]
fn instanceof_non_callable_right_reports_diagnostic() {
    // A synthetic global `interface Function { bind: number }` is the lib-like
    // stand-in for the real `Function` interface. The right operand `b: O` has
    // no call/construct signature and is not a subtype of that `Function`
    // (it lacks `bind`), so the right-hand side reports 2359. The left operand
    // `a: O` is object-ish, so no 2358 fires.
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Function {\n  bind: number;\n}\ninterface O {\n  x: number;\n}\ndeclare const a: O;\ndeclare const b: O;\na instanceof b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert_eq!(diags[0].code, 2359);
    assert_eq!(
        diags[0].message,
        "The right-hand side of an 'instanceof' expression must be either of type 'any', a class, function, or other type assignable to the 'Function' interface type, or an object type with a 'Symbol.hasInstance' method."
    );
}

// Go: internal/checker/checker.go:Checker.resolveInstanceofExpression (right subtype of Function -> ok)
#[test]
fn instanceof_function_subtype_right_reports_no_diagnostic() {
    // The right operand `b: Function` is (trivially) a subtype of the synthetic
    // global `Function`, so no 2359 fires; the left operand is object-ish, so no
    // 2358 fires — a clean `instanceof` (the synthetic-global subtype path).
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Function {\n  bind: number;\n}\ndeclare const a: Function;\ndeclare const b: Function;\na instanceof b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkInExpression (result boolean, tracer)
#[test]
fn in_expression_yields_boolean() {
    // `k in o` always evaluates to the `boolean` primitive type.
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const k: string;\ndeclare const o: object;\nk in o;",
    );
    let expr = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let boolean = c.boolean_type();
    assert_eq!(c.check_expression(&p, expr), boolean);
}

// Go: internal/checker/checker.go:Checker.checkInExpression (left not string|number|symbol)
#[test]
fn in_expression_non_string_number_symbol_left_reports_diagnostic() {
    // The left operand of `in` must be assignable to `string | number | symbol`;
    // an object `o: O` is not, so Go's `checkTypeAssignableTo(..., nil)` reports
    // the generic assignability error 2322 (TS-go does NOT use a dedicated 2360).
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O {\n  x: number;\n}\ndeclare const o: O;\ndeclare const r: object;\no in r;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'O' is not assignable to type 'string | number | symbol'."
    );
}

// Go: internal/checker/checker.go:Checker.checkInExpression (right not object)
#[test]
fn in_expression_non_object_right_reports_diagnostic() {
    // The right operand of `in` must be assignable to `object` (the non-primitive
    // intrinsic); a `string` is not, so Go's `checkTypeAssignableTo(..., nil)`
    // reports the generic assignability error 2322. The left `k: string` is
    // assignable to `string | number | symbol`, so no left error fires.
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const k: string;\ndeclare const s: string;\nk in s;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'object'."
    );
}

// Go: internal/checker/checker.go:Checker.checkInExpression (valid operands -> ok)
#[test]
fn in_expression_valid_operands_report_no_diagnostic() {
    // `string in object`: the left is assignable to `string | number | symbol`
    // and the right to `object`, so no diagnostic fires (the guard).
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const k: string;\ndeclare const o: object;\nk in o;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.getTypeFromTypeLiteralOrFunctionOrConstructorTypeNode
// 4ah: a type-literal type node (`{ value: string }`) resolves to an anonymous
// object type carrying its members, so a property access `o.value` resolves the
// `value` member's annotated type `string` (rather than the type literal node
// falling to the error type).
#[test]
fn check_property_access_type_literal_member() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const o: { value: string };\no.value;");
    let access = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, access), c.string_type());
}

// Go: internal/checker/checker.go:Checker.getIteratedTypeOfIterable / checkForOfStatement
// 4ah: a for-of over a `[Symbol.iterator]`-bearing object types the loop
// variable from the iterator protocol. `It` exposes `[Symbol.iterator](): Iterator<string>`
// (late-bound via 4ag to `__@iterator`); `Iterator<T>.next()` returns `{ value: T }`,
// so the element type is the `value` of the instantiated `next()` result =
// `string`. A body `const n: number = x` therefore reports `2322`.
#[test]
fn for_of_iterable_loop_variable_is_typed_as_iterator_value() {
    // `--target es2015` puts the checker in the iterator-protocol world (4al:
    // the real option read replaces the 4ak `Iterable` lib-presence proxy), so
    // the `[Symbol.iterator]` element type is resolved rather than gated `2802`.
    use tsgo_core::compileroptions::{CompilerOptions, ScriptTarget};
    let options = CompilerOptions {
        target: ScriptTarget::Es2015,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "interface SymbolConstructor { readonly iterator: unique symbol; }\ndeclare var Symbol: SymbolConstructor;\ninterface Iterator<T> { next(): { value: T }; }\ninterface It { [Symbol.iterator](): Iterator<string>; }\ndeclare const it: It;\nfor (const x of it) {\n  const n: number = x;\n}",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.getIteratedTypeOfIterable (element guard)
// The complement of `for_of_iterable_loop_variable_is_typed_as_iterator_value`:
// assigning the iterated element to a `string` target is fine (the loop variable
// `x` really is `string`, the iterator value, not a blanket error), so there is
// NO diagnostic.
#[test]
fn for_of_iterable_loop_variable_value_assignable_to_matching_target() {
    // `--target es2015` puts the checker in the iterator-protocol world (4al).
    use tsgo_core::compileroptions::{CompilerOptions, ScriptTarget};
    let options = CompilerOptions {
        target: ScriptTarget::Es2015,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "interface SymbolConstructor { readonly iterator: unique symbol; }\ndeclare var Symbol: SymbolConstructor;\ninterface Iterator<T> { next(): { value: T }; }\ninterface It { [Symbol.iterator](): Iterator<string>; }\ndeclare const it: It;\nfor (const x of it) {\n  const s: string = x;\n}",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4ai slice 1: a for-of over a value whose type has no `[Symbol.iterator]()`
// method (and is neither an array nor a string) reports `2488` (Go's
// `reportTypeNotIterableError` -> `Type_0_must_have_a_Symbol_iterator_method_
// that_returns_an_iterator`). The error is reported on the iterated expression,
// and the type is printed via `type_to_string` (`{ a: number; }`).
// Go: internal/checker/checker.go:Checker.reportTypeNotIterableError
#[test]
fn for_of_non_iterable_object_reports_2488() {
    // `--target es2015` enables the iterator-protocol world (4al: the real
    // option read replaces the 4ak `Iterable` lib-presence proxy), so a
    // non-iterable object reports `2488` (rather than the no-iterator-world
    // array/string-only `2495`; see
    // `for_of_non_iterable_object_without_global_iterable_reports_2495`).
    use tsgo_core::compileroptions::{CompilerOptions, ScriptTarget};
    let options = CompilerOptions {
        target: ScriptTarget::Es2015,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "declare const v: { a: number };\nfor (const x of v) {\n}",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2488);
    assert_eq!(
        diags[0].message,
        "Type '{ a: number; }' must have a '[Symbol.iterator]()' method that returns an iterator."
    );
}

// 4aj slice 2: a for-of over a value whose `[Symbol.iterator]()` method exists
// but whose returned iterator type has no `next()` method reports the top-level
// `2488` "not iterable" diagnostic carrying the `2489` "an iterator must have a
// `next()` method" as *related information* (Go's
// `getIterationTypesOfIterableWorker`: `getIterationTypesOfMethod` for `"next"`
// pushes the `2489` into `diagnosticOutput`, then the worker creates the primary
// `2488` via `reportTypeNotIterableError` and `AddRelatedInfo`s each collected
// diagnostic). The synthetic global `Symbol` drives the `[Symbol.iterator]`
// late-binding (4ag); the iterator return type `{}` has no `next()`.
//
// This restores the Go-faithful structure and fixes the 4ai divergence (which
// surfaced the `2489` as a top-level diagnostic for lack of related-info
// plumbing).
// Go: internal/checker/checker.go:Checker.getIterationTypesOfIterableWorker
#[test]
fn for_of_iterator_without_next_method_reports_2488_with_related_2489() {
    // `--target es2015` enables the iterator-protocol world (4al), so the
    // missing `next()` surfaces as `2488`+related-`2489` (rather than the
    // no-iterator-world array/string-only routing).
    use tsgo_core::compileroptions::{CompilerOptions, ScriptTarget};
    let options = CompilerOptions {
        target: ScriptTarget::Es2015,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "interface SymbolConstructor { readonly iterator: unique symbol; }\ndeclare var Symbol: SymbolConstructor;\ninterface Bad { [Symbol.iterator](): {}; }\ndeclare const b: Bad;\nfor (const x of b) {\n}",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(
        diags.len(),
        1,
        "expected one top-level diagnostic, got {diags:?}"
    );
    assert_eq!(diags[0].code, 2488);
    assert_eq!(
        diags[0].message,
        "Type 'Bad' must have a '[Symbol.iterator]()' method that returns an iterator."
    );
    assert_eq!(
        diags[0].related_information.len(),
        1,
        "expected one related-information entry, got {:?}",
        diags[0].related_information
    );
    assert_eq!(diags[0].related_information[0].code, 2489);
    assert_eq!(
        diags[0].related_information[0].message,
        "An iterator must have a 'next()' method."
    );
}

// 4ai slice 3: a for-of over a `string` types each loop variable as `string`
// (Go's `getIteratedTypeOrElementType` string-input handling / its
// `getElementTypeOfStringType` reachable subset). The un-annotated `c` therefore
// carries `string`, so a body declaration `const n: number = c` fails
// assignability and reports `2322` (proving `c` is `string`, not `any`), and no
// `2488` "not iterable" diagnostic fires for a string.
// Go: internal/checker/checker.go:Checker.getIteratedTypeOrElementType (string input)
#[test]
fn for_of_over_string_types_element_as_string() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: string;\nfor (const c of s) {\n  const n: number = c;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// 4ai slice 3 guard: the complement of `for_of_over_string_types_element_as_
// string`: assigning the iterated character to a `string` target is fine (the
// loop variable `c` really is `string`), so there is NO diagnostic (in
// particular, no `2488`).
// Go: internal/checker/checker.go:Checker.getIteratedTypeOrElementType (string input)
#[test]
fn for_of_over_string_element_assignable_to_string_target() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: string;\nfor (const c of s) {\n  const t: string = c;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4aj slice 1: a for-of over a union of iterables distributes the iterated
// element type over each constituent and combines the results into a union
// (Go's `getIterationTypesOfIterableWorker` union arm + `combineIterationTypes`
// -> `getIterationTypeUnion` -> `getUnionType`): `u: string[] | number[]`
// iterates as `string | number`. A body declaration `const s: string = x`
// therefore fails assignability and reports `2322` (proving `x` is the combined
// `string | number`, not `any` and not a single constituent's element type).
// Go: internal/checker/checker.go:Checker.getIterationTypesOfIterableWorker (union)
#[test]
fn for_of_union_of_iterables_distributes_element_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\ndeclare const u: string[] | number[];\nfor (const x of u) {\n  const s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string | number' is not assignable to type 'string'."
    );
}

// 4aj slice 1 guard: the complement of `for_of_union_of_iterables_distributes_
// element_type`: assigning the combined element to a `string | number` target is
// fine (the loop variable `x` really is `string | number`), so there is NO
// diagnostic (in particular, no `2488` "not iterable").
// Go: internal/checker/checker.go:Checker.getIterationTypesOfIterableWorker (union)
#[test]
fn for_of_union_of_iterables_element_assignable_to_union_target() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\ndeclare const u: string[] | number[];\nfor (const x of u) {\n  const v: string | number = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4ak slice 1: a for-of over a `string | string[]` mixed union types each loop
// variable as `string` (Go's `getIteratedTypeOrElementType`: the string-like
// constituent is removed from the array type leaving `string[]`, whose number
// index type `string` is string-like, so the `string | string[]` optimization
// at line 6169 returns `c.stringType`; equivalently each constituent yields
// `string` so the union folds to `string`). A body declaration `const n: number
// = x` therefore fails assignability and reports `2322` (proving `x` is
// `string`, not `any` and not a union), and no `2488` "not iterable" fires.
// Go: internal/checker/checker.go:Checker.getIteratedTypeOrElementType (string | string[])
#[test]
fn for_of_string_or_string_array_union_types_element_as_string() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\ndeclare const u: string | string[];\nfor (const x of u) {\n  const n: number = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// 4ak slice 2: a for-of over a non-iterable object when NO global `Iterable`
// type is in scope (Go's `getGlobalIterableType() == emptyGenericType`, i.e. the
// default `--target` < `es2015` / no-`--downlevelIteration` world) routes
// through the array-like/string fallback and, since the input is neither an
// array nor a string and no string constituent was involved, reports `2495`
// "Type '...' is not an array type or a string type." (Go's
// `getIterationDiagnosticDetails` with `allowsStrings = true`). This contrasts
// with `for_of_non_iterable_object_reports_2488`, which DOES declare `Iterable`
// and therefore takes the iterator-protocol path (`2488`).
// Go: internal/checker/checker.go:Checker.getIterationDiagnosticDetails (allowsStrings)
#[test]
fn for_of_non_iterable_object_without_global_iterable_reports_2495() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const v: { a: number };\nfor (const x of v) {\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2495);
    assert_eq!(
        diags[0].message,
        "Type '{ a: number; }' is not an array type or a string type."
    );
}

// 4ak slice 3: a for-of over a `string | <non-array>` mixed union with NO global
// `Iterable` in scope splits off the string constituent (Go's
// `getIteratedTypeOrElementType`: string-like constituents are filtered out of
// the array type, leaving the non-string remainder), then reports `2461` "Type
// '...' is not an array type." on the non-string remainder (`getIteration
// DiagnosticDetails` with `allowsStrings == false`, since a string constituent
// WAS present). The iterated element type is still `string` (a string was
// involved), so an empty body produces exactly the one `2461` diagnostic on the
// `{ a: number; }` remainder.
// Go: internal/checker/checker.go:Checker.getIteratedTypeOrElementType (string-constituent split)
#[test]
fn for_of_string_or_non_array_union_reports_2461_on_remainder() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const u: string | { a: number };\nfor (const x of u) {\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2461);
    assert_eq!(
        diags[0].message,
        "Type '{ a: number; }' is not an array type."
    );
}

// 4ak slice 3 guard: the complement of `for_of_string_or_non_array_union_
// reports_2461_on_remainder` proving the iterated element type really is
// `string` (the string constituent survives the split): a body declaration
// `const n: number = x` additionally fails assignability and reports `2322`, so
// there are exactly two diagnostics: the `2461` on the non-array remainder and
// the `2322` on the `number` target (proving `x` is `string`, not `any`).
// Go: internal/checker/checker.go:Checker.getIteratedTypeOrElementType (string-constituent split)
#[test]
fn for_of_string_or_non_array_union_element_is_string() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const u: string | { a: number };\nfor (const x of u) {\n  const n: number = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 2, "expected two diagnostics, got {diags:?}");
    let mut codes: Vec<i32> = diags.iter().map(|d| d.code).collect();
    codes.sort_unstable();
    assert_eq!(codes, vec![2322, 2461]);
}

// 4al: the option-gated `2802` proof. A `[Symbol.iterator]`-bearing iterable
// (`It`, neither an array nor a string) iterated by for-of under a `--target`
// below `es2015` and no `--downlevelIteration` reports `2802` "can only be
// iterated through when using the '--downlevelIteration' flag or with a
// '--target' of 'es2015' or higher." (Go's `getIterationDiagnosticDetails`,
// `yieldType != nil` branch: the type IS iterable via the protocol but the
// flag/target is too low). The companion tests below show the same input is
// allowed once the option permits downlevelling.
// Go: internal/checker/checker.go:Checker.getIterationDiagnosticDetails
#[test]
fn for_of_symbol_iterator_iterable_without_downlevel_iteration_reports_2802() {
    use tsgo_core::compileroptions::{CompilerOptions, ScriptTarget};
    // `--target es5` (< es2015) and no `--downlevelIteration`: downlevelling is
    // not allowed.
    let options = CompilerOptions {
        target: ScriptTarget::Es5,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "interface SymbolConstructor { readonly iterator: unique symbol; }\ndeclare var Symbol: SymbolConstructor;\ninterface Iterator<T> { next(): { value: T }; }\ninterface It { [Symbol.iterator](): Iterator<string>; }\ndeclare const it: It;\nfor (const x of it) {\n}",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2802);
    assert_eq!(
        diags[0].message,
        "Type 'It' can only be iterated through when using the '--downlevelIteration' flag or with a '--target' of 'es2015' or higher."
    );
}

// 4al companion: the SAME `[Symbol.iterator]`-bearing iterable is allowed once
// `--downlevelIteration` is set: no `2802` fires and the loop variable is typed
// through the iterator protocol as `string` (Go's `getIteratedTypeOrElementType`
// when `iterableExists`). A body `const n: number = x` therefore reports `2322`
// (proving `x` is `string`), and there is no `2802`. This is the option-gated
// behavior difference proven against `for_of_symbol_iterator_iterable_without_
// downlevel_iteration_reports_2802`.
// Go: internal/checker/checker.go:Checker.getIteratedTypeOrElementType (iterableExists)
#[test]
fn for_of_symbol_iterator_iterable_with_downlevel_iteration_resolves_element() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        downlevel_iteration: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "interface SymbolConstructor { readonly iterator: unique symbol; }\ndeclare var Symbol: SymbolConstructor;\ninterface Iterator<T> { next(): { value: T }; }\ninterface It { [Symbol.iterator](): Iterator<string>; }\ndeclare const it: It;\nfor (const x of it) {\n  const n: number = x;\n}",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    // No `2802`; the only diagnostic is the body's `2322` proving `x: string`.
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// 4al companion: a `--target` of `es2015` (>= es2015) likewise allows the
// iteration — no `2802`, and the element resolves to `string` (body `2322`).
// Go: internal/checker/checker.go:Checker.getIteratedTypeOrElementType (iterableExists)
#[test]
fn for_of_symbol_iterator_iterable_with_es2015_target_resolves_element() {
    use tsgo_core::compileroptions::{CompilerOptions, ScriptTarget};
    let options = CompilerOptions {
        target: ScriptTarget::Es2015,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "interface SymbolConstructor { readonly iterator: unique symbol; }\ndeclare var Symbol: SymbolConstructor;\ninterface Iterator<T> { next(): { value: T }; }\ninterface It { [Symbol.iterator](): Iterator<string>; }\ndeclare const it: It;\nfor (const x of it) {\n  const n: number = x;\n}",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// 4am tracer (strictNullChecks assignability gate, non-strict direction): under
// `--strictNullChecks false`, `null` is assignable to every type (it lies in the
// domain of every type), so `var x: string = null;` reports NO `2322`. This is
// the genuine flag-differing behavior proven against
// `null_initializer_to_non_nullable_reports_2322_under_strict` below.
// Go: internal/checker/relater.go:Checker.isSimpleTypeRelatedTo (!strictNullChecks null rule)
#[test]
fn null_initializer_to_non_nullable_ok_when_not_strict() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        strict_null_checks: Tristate::False,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "var x: string = null;",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4am companion (strict direction): under `--strictNullChecks true`, `null` is
// NOT assignable to the non-nullable type `string`, so the SAME input reports
// `2322`. The diagnostic difference against
// `null_initializer_to_non_nullable_ok_when_not_strict` is the observable
// strictNullChecks gate.
// Go: internal/checker/relater.go:Checker.isSimpleTypeRelatedTo (strict null rule)
#[test]
fn null_initializer_to_non_nullable_reports_2322_under_strict() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        strict_null_checks: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "var x: string = null;",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'null' is not assignable to type 'string'."
    );
}

// 4am slice 2 (non-strict direction, undefined source): under `--strictNullChecks
// false`, an `undefined`-typed value is assignable to the non-nullable type
// `string`, so `var x: string = u;` (with `u: undefined`) reports NO `2322`.
// Go: internal/checker/relater.go:Checker.isSimpleTypeRelatedTo (!strictNullChecks undefined rule)
#[test]
fn undefined_initializer_to_non_nullable_ok_when_not_strict() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        strict_null_checks: Tristate::False,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "declare const u: undefined;\nvar x: string = u;",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4am slice 2 companion (strict direction, undefined source): under
// `--strictNullChecks true`, `undefined` is NOT assignable to `string`, so the
// SAME input reports `2322`. The diagnostic difference is the observable gate.
// Go: internal/checker/relater.go:Checker.isSimpleTypeRelatedTo (strict undefined rule)
#[test]
fn undefined_initializer_to_non_nullable_reports_2322_under_strict() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        strict_null_checks: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "declare const u: undefined;\nvar x: string = u;",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'undefined' is not assignable to type 'string'."
    );
}

// 4am slice 3 guard (strict direction still permits nullable targets): under
// `--strictNullChecks true`, `undefined` IS assignable to a nullable union
// `string | undefined` (it matches the `undefined` constituent), so no `2322`
// fires. This confirms the strict gate did not over-restrict the structural
// union rule.
// Go: internal/checker/relater.go:structuredTypeRelatedTo (target union member)
#[test]
fn undefined_initializer_to_nullable_union_ok_under_strict() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        strict_null_checks: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "declare const u: undefined;\nvar x: string | undefined = u;",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4ay (genuine RED): a non-null assertion `x!` yields the operand's type with
// `null`/`undefined` removed (Go's `checkNonNullAssertion` ->
// `GetNonNullableType(checkExpression(expr))`). With `x: string | undefined`
// under strictNullChecks, `x!` has type `string`, so assigning it to a `number`
// reports `2322` whose SOURCE is the reduced `string` (not the original union).
// Before the `NonNullExpression` arm existed, `x!` fell through to `error_type`
// and reported nothing.
// Go: internal/checker/checker.go:Checker.checkNonNullAssertion(10582)
#[test]
fn non_null_assertion_strips_undefined_then_reports_2322_against_number() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | undefined;\nvar n: number = x!;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// 4ay contrast (baseline, no `!`): WITHOUT the assertion, `x` keeps its declared
// nullable union, so the SAME assignment reports `2322` whose source is the whole
// union. The differing source string (the `undefined | string` union here vs the
// reduced `string` above) is the observable effect of `!`. (The port orders union
// members by type id, so `undefined` prints first — a known display divergence
// from `tsc`, not a semantic one.)
// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration (union source)
#[test]
fn plain_nullable_reference_reports_2322_with_union_source() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | undefined;\nvar n: number = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'undefined | string' is not assignable to type 'number'."
    );
}

// 4ay guard: assigning `x!` (reduced to `string`) to a `string` target is OK
// under strictNullChecks — the assertion removed the `undefined` that would
// otherwise make `string | undefined` unassignable to `string`.
// Go: internal/checker/checker.go:Checker.checkNonNullAssertion(10582)
#[test]
fn non_null_assertion_assignable_to_string_target() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | undefined;\nvar s: string = x!;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4ay (truthiness narrowing removes nullable): inside `if (x) { ... }` the truthy
// branch narrows `x: string | undefined` to `string` (Go's
// `narrowTypeByTruthiness` -> `getAdjustedTypeWithFacts(t, Truthy)`, which drops
// the falsy-only `undefined`). Assigning the narrowed `x` to a `number` therefore
// reports `2322` whose SOURCE is the reduced `string`, not the union. This rides
// the existing 4t flow walk + 4g truthiness filter; the new observable is the
// nullable case under strictNullChecks.
// Go: internal/checker/flow.go:Checker.narrowTypeByTruthiness
#[test]
fn truthy_branch_narrows_out_nullable() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | undefined;\nif (x) {\n  var n: number = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// 4az slice A (genuine RED): property access on a possibly-`undefined` object
// reports `18048` "'x' is possibly 'undefined'." under strictNullChecks, then
// continues the property lookup on the non-null type `{ a: number }` (so no
// `2339`). Go's `checkPropertyAccessExpression` types the object via
// `checkNonNullExpression` -> `checkNonNullType` -> (the entity-name object `x`
// is an identifier) `reportObjectPossiblyNullOrUndefinedError` emits the
// `_0_is_possibly_undefined` (18048) message and narrows to the non-null type.
// Before the non-null wiring, the union `{ a: number } | undefined` had no
// shared `a` member, so the access reported `2339` instead.
// Go: internal/checker/checker.go:Checker.checkNonNullType(7377)/reportObjectPossiblyNullOrUndefinedError(7424)
#[test]
fn property_access_on_possibly_undefined_reports_18048() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number } | undefined;\nx.a;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 18048);
    assert_eq!(diags[0].message, "'x' is possibly 'undefined'.");
}

// 4az slice A guard (green-on-arrival): a possibly-`null` object reports
// `18047` "'x' is possibly 'null'." (`IS_NULL` fact only). Same code path as
// the undefined case; the fact bits select the `_0_is_possibly_null` message.
// Go: internal/checker/checker.go:Checker.reportObjectPossiblyNullOrUndefinedError(7424)
#[test]
fn property_access_on_possibly_null_reports_18047() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number } | null;\nx.a;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 18047);
    assert_eq!(diags[0].message, "'x' is possibly 'null'.");
}

// 4az slice A guard (green-on-arrival): a possibly-`null`-or-`undefined` object
// reports `18049` "'x' is possibly 'null' or 'undefined'." (both `IS_NULL` and
// `IS_UNDEFINED` facts present).
// Go: internal/checker/checker.go:Checker.reportObjectPossiblyNullOrUndefinedError(7424)
#[test]
fn property_access_on_possibly_null_or_undefined_reports_18049() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number } | null | undefined;\nx.a;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 18049);
    assert_eq!(diags[0].message, "'x' is possibly 'null' or 'undefined'.");
}

// 4az slice A guard (green-on-arrival): element access `x["a"]` on a
// possibly-`undefined` object also runs the non-null check (Go's
// `checkIndexedAccess` -> `checkNonNullExpression`), reporting `18048` and then
// resolving the property on the non-null type.
// Go: internal/checker/checker.go:Checker.checkIndexedAccess
#[test]
fn element_access_on_possibly_undefined_reports_18048() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number } | undefined;\nx[\"a\"];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 18048);
    assert_eq!(diags[0].message, "'x' is possibly 'undefined'.");
}

// 4az slice A guard: a non-nullable object access reports nothing (the
// `IsUndefinedOrNull` facts are absent, so `checkNonNullType` is the identity).
// Go: internal/checker/checker.go:Checker.checkNonNullType(7377)
#[test]
fn property_access_on_non_nullable_object_reports_nothing() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number };\nx.a;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4az slice B (genuine RED): the `undefined` value identifier types as
// `undefined` and resolves without a "Cannot find name" error. Go registers a
// global `undefinedSymbol` (type `undefinedWideningType`) so `undefined` always
// resolves; the stub program has no lib, so before this slice `undefined` fell
// through to `Cannot_find_name_0` (2304). Reported as 0 diagnostics for a bare
// `undefined;` statement.
// Go: internal/checker/checker.go:NewChecker (undefinedSymbol, 949/1339/1456)
#[test]
fn undefined_value_resolves_without_cannot_find_name() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "undefined;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4az slice B (type witness): `check_expression` on the `undefined` value
// identifier yields the `undefined` type (Go's `undefinedSymbol` ->
// `undefinedWideningType`; the widening distinction is not modeled).
// Go: internal/checker/checker.go:Checker.checkIdentifier(10999) (undefinedSymbol)
#[test]
fn undefined_value_checks_as_undefined_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "undefined;");
    let usage = expr_stmt_expression(&p, 0);
    let mut c = Checker::new();
    let undefined = c.undefined_type();
    assert_eq!(c.check_expression(&p, usage), undefined);
}

// 4az slice C end-to-end (the task's slice-2 example): inside `if (x !==
// undefined)`, `x: string | undefined` narrows to `string`, so assigning it to
// a `string` variable reports nothing. This rides slice B (`undefined` resolves,
// no `2304`) + slice C (the `NEUndefined` fact narrowing). Before both, the same
// source reported two `Cannot find name 'undefined'` errors.
// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (NEUndefined)
#[test]
fn ne_undefined_branch_narrows_to_string_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | undefined;\nif (x !== undefined) {\n  var s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4az slice C end-to-end contrast: WITHOUT the guard, `x` keeps its nullable
// union, so the same `var s: string = x` is not assignable and reports `2322`
// (source the whole `undefined | string` union). The differing outcome (0 vs 1
// diagnostic) is the observable effect of the equality narrowing.
// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration (2322)
#[test]
fn plain_nullable_assigned_to_string_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | undefined;\nvar s: string = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'undefined | string' is not assignable to type 'string'."
    );
}

// 4ba slice 4 (typeof narrowing, end-to-end witness, green-on-arrival): inside
// `if (typeof x === "string")`, `x: string | number` narrows to `string` (Go's
// `narrowTypeByTypeof`/`narrowTypeByTypeName`, wired into the flow walk and
// already covered at the flow level by `flow_typeof_narrows_in_then_branch`).
// The diagnostic-level witness: assigning the narrowed `x` to a `string`
// variable inside the guarded block reports nothing.
// Go: internal/checker/flow.go:Checker.narrowTypeByTypeof
#[test]
fn typeof_string_guard_narrows_var_assignment_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | number;\nif (typeof x === \"string\") {\n  var s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4ba slice 4 contrast (baseline): WITHOUT the `typeof` guard, `x` keeps its
// `string | number` union, so the same `var s: string = x` is not assignable
// and reports `2322` (source the whole union). The 0-vs-1 difference is the
// observable effect of the typeof narrowing.
// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration (2322)
#[test]
fn plain_string_or_number_assigned_to_string_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | number;\nvar s: string = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string | number' is not assignable to type 'string'."
    );
}

// 4bb slice 1a (genuine RED): a literal type node (`"a"`/`1`/`true` in type
// position) resolves to the corresponding literal type, not `error` (Go's
// `getTypeFromLiteralTypeNode` -> `getRegularTypeOfLiteralType(checkExpression(
// literal))`). With `x: "a"`, the string-literal type `"a"` is not assignable
// to the distinct literal `"b"`, so `const n: "b" = x` reports `2322`; the
// target `"b"` is itself a unit type, so the source is NOT widened and the
// message preserves both literals. Before the literal type node was wired (4az
// DEFER returned `error_type`), `x` was `error`, assignable everywhere, so
// nothing was reported.
// Go: internal/checker/checker.go:Checker.getTypeFromLiteralTypeNode(22781)
#[test]
fn string_literal_type_node_not_assignable_to_other_literal() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: \"a\";\nconst n: \"b\" = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '\"a\"' is not assignable to type '\"b\"'."
    );
}

// 4bb slice 1a contrast (green-on-arrival): a string-literal type node is
// assignable to its base primitive `string`, so `const s: string = x` with
// `x: "a"` reports nothing (the literal `"a"` is a subtype of `string`).
// Go: internal/checker/checker.go:Checker.getTypeFromLiteralTypeNode(22781)
#[test]
fn string_literal_type_node_assignable_to_string() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: \"a\";\nconst s: string = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bb slice 1b (genuine RED): comparing a literal-union value to one of its
// member literals is a valid comparison, so `k === "a"` with `k: "a" | "b"`
// reports nothing. The two `"a"` literal types (the union member and the
// condition operand) are distinct ids in the port (literals are not interned
// by value as they are in Go), so the comparable relation must relate two
// literals with equal value/flags — mirroring Go's literal interning where
// `"a" === "a"` holds by pointer identity. Before this fix the equality
// comparability check found "no overlap" and reported `2367`.
// Go: internal/checker/relater.go:Checker.isTypeRelatedTo (interned literal identity)
#[test]
fn equality_literal_in_its_union_reports_no_overlap_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const k: \"a\" | \"b\";\nk === \"a\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bb slice 1b contrast (green-on-arrival): comparing a literal-union value to
// a literal that is NOT a member still reports `2367` (no overlap), so the fix
// does not suppress genuine no-overlap comparisons. `k: "a" | "b"` vs `"c"`.
// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (equality 2367)
#[test]
fn equality_literal_outside_union_reports_no_overlap() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const k: \"a\" | \"b\";\nk === \"c\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2367);
    assert_eq!(
        diags[0].message,
        "This comparison appears to be unintentional because the types '\"a\" | \"b\"' and '\"c\"' have no overlap."
    );
}

// 4bb slice 1 (genuine RED): discriminated-union narrowing. Inside
// `if (v.kind === "a")`, the union `A | B` narrows to `A` because `kind` is a
// literal discriminant property (Go's `getDiscriminantPropertyAccess` ->
// `narrowTypeByDiscriminantProperty` -> `narrowTypeByDiscriminant`). The
// narrowed `v: A` then exposes `v.x: number`, so `const n: number = v.x`
// reports nothing. Before the discriminant narrowing, `v` kept the whole union
// and `v.x` reported `2339` ("Property 'x' does not exist on type 'A | B'")
// because `x` is not present on every constituent.
// Go: internal/checker/flow.go:Checker.narrowTypeByDiscriminantProperty(683)
#[test]
fn discriminant_property_eq_narrows_union_in_then_branch() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { kind: \"a\"; x: number };\ntype B = { kind: \"b\"; y: string };\ndeclare const v: A | B;\nif (v.kind === \"a\") {\n  const n: number = v.x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bb slice 1 (narrowing witness, green-on-arrival): inside `if (v.kind ===
// "a")` the union narrows to `A`, so accessing the OTHER constituent's property
// `v.y` (only on `B`) reports `2339` against the NARROWED type `A`
// (`{ kind: "a"; x: number; }`), not the whole union. The narrowed-type message
// distinguishes this from the un-narrowed case.
// Go: internal/checker/flow.go:Checker.narrowTypeByDiscriminantProperty(683)
#[test]
fn discriminant_narrowed_branch_rejects_other_constituent_property() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { kind: \"a\"; x: number };\ntype B = { kind: \"b\"; y: string };\ndeclare const v: A | B;\nif (v.kind === \"a\") {\n  v.y;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2339);
    assert_eq!(
        diags[0].message,
        "Property 'y' does not exist on type '{ kind: \"a\"; x: number; }'."
    );
}

// 4bb slice 1 (negation witness, green-on-arrival): inside `if (v.kind !== "a")`
// the union narrows to the complement `B`, so `v.y` (`y: string`) exists and
// `const s: string = v.y` reports nothing. The `!==` flips `assume_true`, which
// the equality dispatch turns into removing the `"a"` constituent.
// Go: internal/checker/flow.go:Checker.narrowTypeByDiscriminantProperty(683)
#[test]
fn discriminant_not_equal_narrows_to_complement_constituent() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { kind: \"a\"; x: number };\ntype B = { kind: \"b\"; y: string };\ndeclare const v: A | B;\nif (v.kind !== \"a\") {\n  const s: string = v.y;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.resolveInstanceofExpression (right callable -> ok)
#[test]
fn instanceof_callable_right_reports_no_diagnostic() {
    // The right operand `f` has a call signature, so no 2359 fires even without
    // a global `Function` present (the call-signature branch); the left operand
    // `o: O` is object-ish, so no 2358 fires.
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O {\n  x: number;\n}\ndeclare const o: O;\ndeclare function f(): void;\no instanceof f;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bd slice 2 tracer (genuine RED): an un-annotated `let` binding widens its
// fresh string-literal initializer to `string` (Go's
// `widenTypeInferredFromInitializer` -> `getWidenedLiteralTypeForInitializer`,
// which only keeps the literal for `const`/`readonly`). With `let x = "a"`, the
// inferred type of `x` is `string`, which is NOT assignable to the literal
// target `"a"`, so `const y: "a" = x` reports `2322`. Before this round an
// un-annotated variable resolved to `any`, so nothing was reported.
// Go: internal/checker/checker.go:Checker.getWidenedLiteralType(25346)
#[test]
fn let_binding_widens_string_literal_initializer_to_string() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let x = \"a\";\nconst y: \"a\" = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type '\"a\"'."
    );
}

// 4bd slice 2 guard (green-on-arrival): the widened `let` binding is assignable
// to its base primitive, so `let x = "a"; var s: string = x;` reports nothing.
// Go: internal/checker/checker.go:Checker.getWidenedLiteralType(25346)
#[test]
fn let_binding_widened_string_is_assignable_to_string() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let x = \"a\";\nvar s: string = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bd slice 2 guard (green-on-arrival): a `const` binding keeps the literal
// (Go's `getWidenedLiteralTypeForInitializer` returns the type unchanged when
// the combined node flags include `Const`), so `const x = "a"` types `x` as the
// literal `"a"`, assignable to the literal target `"a"`: no diagnostics.
// Go: internal/checker/checker.go:Checker.getWidenedLiteralTypeForInitializer(16756)
#[test]
fn const_binding_keeps_string_literal_assignable_to_literal_target() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x = \"a\";\nconst y: \"a\" = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bd slice 3a tracer (genuine RED): an un-annotated `let` binding widens its
// fresh number-literal initializer to `number` (the `NumberLiteral` arm of Go's
// `getWidenedLiteralType`). With `let n = 1`, `n` is `number`, NOT assignable to
// the literal target `1`, so `const m: 1 = n` reports `2322`. Before the number
// arm landed the fresh `1` was not widened, so `n` stayed assignable to `1`.
// Go: internal/checker/checker.go:Checker.getWidenedLiteralType(25346)
#[test]
fn let_binding_widens_number_literal_initializer_to_number() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let n = 1;\nconst m: 1 = n;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type '1'."
    );
}

// 4bd slice 3a guard (green-on-arrival): the widened `let` number binding is
// assignable to its base primitive, so `let n = 1; var x: number = n;` reports
// nothing.
// Go: internal/checker/checker.go:Checker.getWidenedLiteralType(25346)
#[test]
fn let_binding_widened_number_is_assignable_to_number() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let n = 1;\nvar x: number = n;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bd slice 3b tracer (genuine RED): an un-annotated `let` binding widens its
// fresh boolean-literal initializer to `boolean` (the `BooleanLiteral` arm of
// Go's `getWidenedLiteralType`). With `let b = true`, `b` is `boolean`, NOT
// assignable to the literal target `true`, so `const c: true = b` reports
// `2322`. Before the boolean arm landed the fresh `true` was not widened, so
// `b` stayed assignable to `true`.
//
// The widened source `boolean` prints as its `false | true` union here: the
// `false | true` => `boolean` collapse in `typeToString` (Go's
// `formatUnionTypes`) is DEFER'd to 4j's printer, mirroring the existing
// `check_element_access_boolean_index_reports_2538` test.
// Go: internal/checker/checker.go:Checker.getWidenedLiteralType(25346)
#[test]
fn let_binding_widens_boolean_literal_initializer_to_boolean() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let b = true;\nconst c: true = b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'false | true' is not assignable to type 'true'."
    );
}

// 4bd slice 3b guard (green-on-arrival): the widened `let` boolean binding is
// assignable to its base primitive, so `let b = true; var x: boolean = b;`
// reports nothing.
// Go: internal/checker/checker.go:Checker.getWidenedLiteralType(25346)
#[test]
fn let_binding_widened_boolean_is_assignable_to_boolean() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let b = true;\nvar x: boolean = b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bd slice 3b guard (green-on-arrival): a `const` boolean binding keeps the
// literal, so `const b = true; const c: true = b;` reports nothing (the literal
// `true` is assignable to the literal target `true`).
// Go: internal/checker/checker.go:Checker.getWidenedLiteralTypeForInitializer(16756)
#[test]
fn const_binding_keeps_boolean_literal_assignable_to_literal_target() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const b = true;\nconst c: true = b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4be slice 1 tracer (genuine RED): a `<literal> as const` assertion suppresses
// widening AND preserves the literal type. Go's `checkAssertion` returns
// `getRegularTypeOfLiteralType(exprType)` for a const-type-reference assertion,
// so `"a" as const` is the regular (non-fresh) literal `"a"`. In an un-annotated
// `let` binding the initializer flows through `getWidenedLiteralType`, which is
// a no-op on a regular (non-fresh) literal, so `x` keeps the type `"a"`. That
// `"a"` is NOT assignable to the literal target `"b"`, so `const y: "b" = x`
// reports `2322`. Before this round `as const` was untyped (`error` type, which
// is assignable to anything), so nothing was reported.
// Go: internal/checker/checker.go:Checker.checkAssertion(12238)
#[test]
fn const_assertion_on_string_literal_keeps_literal_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let x = \"a\" as const;\nconst y: \"b\" = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '\"a\"' is not assignable to type '\"b\"'."
    );
}

// 4be slice 1 guard (green-on-arrival): the canonical `as const` behavior. With
// `let x = "a" as const`, `x` keeps the literal type `"a"` (the const assertion
// suppresses the mutable-binding widening that `let x = "a"` would apply), so
// `const y: "a" = x` is assignable: no diagnostics. Contrast the 4bd
// `let_binding_widens_string_literal_initializer_to_string` (without `as const`
// the same shape reports `2322`).
// Go: internal/checker/checker.go:Checker.checkAssertion(12238)
#[test]
fn const_assertion_on_string_literal_is_assignable_to_same_literal() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let x = \"a\" as const;\nconst y: \"a\" = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4be slice 1 guard (green-on-arrival): a preserved string literal is still
// assignable to its base primitive, so `let x = "a" as const; var s: string = x;`
// reports nothing (`"a"` is assignable to `string`).
// Go: internal/checker/checker.go:Checker.checkAssertion(12238)
#[test]
fn const_assertion_on_string_literal_is_assignable_to_string() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let x = \"a\" as const;\nvar s: string = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4be slice 2 (green-on-arrival): `as const` keeps a NUMBER literal too. The
// const branch normalizes any freshable literal via `getRegularTypeOfLiteralType`
// (value-kind agnostic), so it generalizes from the string slice with no further
// code. `let n = 1 as const` keeps `n` as the literal `1`, which is NOT
// assignable to the literal target `2`, so `const m: 2 = n` reports `2322`.
// Go: internal/checker/checker.go:Checker.checkAssertion(12238)
#[test]
fn const_assertion_on_number_literal_keeps_literal_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let n = 1 as const;\nconst m: 2 = n;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(diags[0].message, "Type '1' is not assignable to type '2'.");
}

// 4be slice 2 guard (green-on-arrival): the canonical number `as const`
// behavior. `let n = 1 as const` keeps `n` as `1`, assignable to the literal
// target `1`: no diagnostics (contrast 4bd `let n = 1` which widens to
// `number` and reports `2322`).
// Go: internal/checker/checker.go:Checker.checkAssertion(12238)
#[test]
fn const_assertion_on_number_literal_is_assignable_to_same_literal() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let n = 1 as const;\nconst m: 1 = n;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4be slice 3 (green-on-arrival): `as const` keeps a BOOLEAN literal. `true`
// already types as the construction-time fresh `trueType`, and the const branch
// normalizes it to the regular `true` literal, so `let b = true as const` keeps
// `b` as `true` (NOT widened to `boolean`), which is NOT assignable to the
// literal target `false`: `const c: false = b` reports `2322`.
// Go: internal/checker/checker.go:Checker.checkAssertion(12238)
#[test]
fn const_assertion_on_boolean_literal_keeps_literal_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let b = true as const;\nconst c: false = b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'true' is not assignable to type 'false'."
    );
}

// 4be slice 3 guard (green-on-arrival): the canonical boolean `as const`
// behavior. `let b = true as const` keeps `b` as `true`, assignable to the
// literal target `true`: no diagnostics (contrast 4bd `let b = true` which
// widens to `boolean` and reports `2322`).
// Go: internal/checker/checker.go:Checker.checkAssertion(12238)
#[test]
fn const_assertion_on_boolean_literal_is_assignable_to_same_literal() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let b = true as const;\nconst c: true = b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4be slice 4 tracer (genuine RED): a non-const `expr as T` assertion takes the
// asserted type `T` as its result (Go's `checkAssertion` falls through to
// `getTypeFromTypeNode(typeNode)` when the type node is not a const reference).
// `"a" as string` is therefore `string`, which is NOT assignable to the literal
// target `"a"`, so `const y: "a" = x` reports `2322`. Before this arm was wired
// the non-const branch returned the `error` type (assignable to anything), so
// nothing was reported. (`"a"` is comparable to `string`, so the deferred `2352`
// assertion-comparability check does not apply here.)
// Go: internal/checker/checker.go:Checker.checkAssertion(12238)
#[test]
fn non_const_assertion_takes_asserted_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let x = \"a\" as string;\nconst y: \"a\" = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type '\"a\"'."
    );
}

// 4be slice 4 guard (green-on-arrival): a non-const assertion to the matching
// type is fine. `"a" as string` is `string`, assignable to the `string` target:
// no diagnostics.
// Go: internal/checker/checker.go:Checker.checkAssertion(12238)
#[test]
fn non_const_assertion_to_matching_type_is_assignable() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let x = \"a\" as string;\nvar s: string = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bf slice 1 tracer (genuine RED): an object literal `{ a: 1 }` types as an
// anonymous object type whose property `a` carries the member initializer's
// (widened) type, so reading `o.a` resolves to `number`. Before this arm was
// wired `check_expression` returned the `error` type for an
// `ObjectLiteralExpression`, so `o` was `error` and `o.a` resolved to `error`
// (not `number`). Go's `checkPropertyAssignment` types the initializer through
// `checkExpressionForMutableLocation`, which widens the fresh literal `1` to
// `number`, so the stored property type is `number`.
// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076)
#[test]
fn object_literal_property_reads_member_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { a: 1 };\no.a;");
    let access = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(c.check_expression(&p, access), number);
}

// 4bf slice 1 guard (green-on-arrival): a string-valued member types as
// `string` (the widened `"x"`), confirming non-numeric member initializers
// flow through `checkExpressionForMutableLocation` too.
// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076)
#[test]
fn object_literal_string_property_reads_member_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { b: \"x\" };\no.b;");
    let access = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let string = c.string_type();
    assert_eq!(c.check_expression(&p, access), string);
}

// 4bf slice 1 guard (green-on-arrival): a multi-property literal builds an
// anonymous object type carrying both members, printed structurally by the node
// builder as `{ a: number; b: string; }` (member-declaration order preserved).
// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076) / nodebuilder
#[test]
fn object_literal_prints_structural_member_types() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { a: 1, b: \"x\" };\no;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ a: number; b: string; }"
    );
}

// 4bf slice 2 (green-on-arrival, unlocked by slice 1): an object literal is
// structurally assignable to a matching annotation, so
// `const o: { a: number } = { a: 1 }` reports nothing (member `a: number`
// relates to the target's `a: number`). Before slice 1 the literal was the
// `error` type (assignable to anything), so this also reported nothing — its
// genuine red lived in the mismatch case below.
// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076) + relater
#[test]
fn object_literal_assignable_to_matching_annotation() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: { a: number } = { a: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bf slice 2 (genuine RED before slice 1): a member whose (widened) type does
// not relate to the annotated property type reports `2322`. `{ a: "x" }` has
// `a: string`, which is not assignable to the target's `a: number`. Before
// slice 1 the literal was the `error` type (0 diagnostics); slice 1's
// object-literal typing makes the structural mismatch observable.
// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076) + relater
#[test]
fn object_literal_property_mismatch_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: { a: number } = { a: \"x\" };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '{ a: string; }' is not assignable to type '{ a: number; }'."
    );
}

// 4bf slice 3 tracer (genuine RED): an array literal `[1, 2]` types as the
// global `Array<T>` reference whose element type is the widened union of the
// element expression types (here `number`). Reading `arr[0]` therefore resolves
// to `number` (through the `[n: number]: T` index signature instantiated to
// `number`). Before this arm `check_expression` returned the `error` type for
// an `ArrayLiteralExpression`, so `arr` was `error` and `arr[0]` was `error`
// (not `number`). A synthetic top-level `interface Array<T>` stands in for the
// lib type until P6 loads lib.d.ts.
// Go: internal/checker/checker.go:Checker.checkArrayLiteral(7989)
#[test]
fn array_literal_element_access_resolves_element_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\nconst arr = [1, 2];\narr[0];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(c.check_expression(&p, access), number);
}

// 4bf slice 3 (green-on-arrival, unlocked by the tracer impl): the array element
// type is assignable to a matching annotation. `arr` is `number[]`, so
// `const n: number = arr[0]` reports nothing.
// Go: internal/checker/checker.go:Checker.checkArrayLiteral(7989)
#[test]
fn array_literal_element_assignable_to_number() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\nconst arr = [1, 2];\nconst n: number = arr[0];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bf slice 3 (genuine RED before the tracer impl): the array element type does
// not relate to a mismatched annotation. `arr` is `number[]`, so `arr[0]` is
// `number`, which is not assignable to `string`: `const n: string = arr[0]`
// reports `2322`. Before the array-literal arm `arr` was the `error` type and
// nothing was reported.
// Go: internal/checker/checker.go:Checker.checkArrayLiteral(7989)
#[test]
fn array_literal_element_mismatch_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\nconst arr = [1, 2];\nconst n: string = arr[0];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// 4bf slice 3 guard: an empty array literal `[]` takes the `never` element type
// under strictNullChecks (Go's `implicitNeverType` arm; defaults on via
// `strict != false`), so its type is `Array<never>`. Checked directly on the
// literal node (reading a binding's element via flow would engage the deferred
// evolving-array path).
// Go: internal/checker/checker.go:Checker.checkArrayLiteral(7989)
#[test]
fn empty_array_literal_is_never_array_under_strict_null_checks() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\n[];",
    );
    let literal = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let t = c.check_expression(&p, literal);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "Array<never>"
    );
}

// 4bf slice 3 guard: with strictNullChecks off, an empty array literal `[]`
// takes the `undefined` element type (Go's `undefinedWideningType` arm; the
// widening distinction is not modeled), so its type is `Array<undefined>`.
// Go: internal/checker/checker.go:Checker.checkArrayLiteral(7989)
#[test]
fn empty_array_literal_is_undefined_array_without_strict_null_checks() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        strict_null_checks: Tristate::False,
        ..CompilerOptions::default()
    };
    // The checker reads strictNullChecks off its RETAINED program's options, so
    // the program must be retained (an intrinsic-only `Checker::new()` would use
    // the defaults, where `strict != false` enables strictNullChecks).
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\n[];",
        options,
    ));
    let literal = expr_stmt_expression(&p, 1);
    let view = std::rc::Rc::clone(&p);
    let mut c = Checker::new_checker(p);
    let t = c.check_expression(view.as_ref(), literal);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, view.as_ref(), t),
        "Array<undefined>"
    );
}

// 4bg slice 1 tracer (genuine RED): a FRESH object literal assigned to an
// annotated target whose type lacks one of the literal's properties reports the
// excess-property error 2353 on that property. `{ a: 1, b: 2 }` is fresh and
// `b` does not exist in `{ a: number }`. Before this round the relation ignored
// excess properties (structurally `{ a: number; b: number }` is assignable to
// `{ a: number }`), so 0 diagnostics were reported. Go runs `hasExcessProperties`
// first when the source is a fresh object literal and, on a hit, reports 2353
// and suppresses the 2322 head message.
// Go: internal/checker/relater.go:Relater.hasExcessProperties(2695)
#[test]
fn object_literal_excess_property_reports_2353() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: { a: number } = { a: 1, b: 2 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2353);
    assert_eq!(
        diags[0].message,
        "Object literal may only specify known properties, and 'b' does not exist in type '{ a: number; }'."
    );
}

// 4bg slice 1 positive control (green-on-arrival): a fresh object literal with
// exactly the target's properties reports nothing. `{ a: 1 }` has no property
// absent from `{ a: number }`, so `hasExcessProperties` finds no excess and no
// 2353 fires.
// Go: internal/checker/relater.go:Relater.hasExcessProperties(2695)
#[test]
fn object_literal_no_excess_property_reports_nothing() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: { a: number } = { a: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bg slice 3 (genuine RED): a NON-fresh object source does not trigger
// excess-property checking. Assigning an object literal to a variable widens it
// (Go's `widenTypeForVariableLikeDeclaration` -> `getWidenedType` ->
// `getWidenedTypeOfObjectLiteral`), which drops the `FreshLiteral`/`ObjectLiteral`
// flags, so reading the variable yields a regular object type. `const src =
// { a: 1, b: 2 }` makes `src` a regular `{ a: number; b: number }`, which is
// structurally assignable to `{ a: number }` with the extra `b` tolerated and no
// 2353. Before widening was applied to the variable's type, `src` kept the fresh
// flag and the excess check fired a spurious 2353.
// Go: internal/checker/checker.go:Checker.widenTypeForVariableLikeDeclaration(18101)
#[test]
fn non_fresh_object_source_reports_no_excess_property() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const src = { a: 1, b: 2 };\nconst o: { a: number } = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bg slice 2 (genuine RED): a target with an applicable index signature has no
// excess properties — every literal property is a "known property" through the
// index signature (Go's `isKnownProperty` -> `getApplicableIndexInfoForName`).
// `interface T { [k: string]: number }` accepts any string-named property, so
// `const o: T = { a: 1, b: 2 }` reports nothing. Before the index-signature path
// of `is_known_property`, `a`/`b` were unknown and a spurious 2353 fired.
// Go: internal/checker/relater.go:Checker.isKnownProperty(716)
#[test]
fn index_signature_target_suppresses_excess_property() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface T {\n  [k: string]: number;\n}\nconst o: T = { a: 1, b: 2 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bg slice 2b (genuine RED): the empty object type `{}` accepts any property,
// so excess-property checking is suppressed against it (Go's `hasExcessProperties`
// returns early when the target is an empty object type). `const o: {} = { a: 1 }`
// reports nothing. Before the `is_empty_object_type` guard, `a` was unknown on
// `{}` and a spurious 2353 fired.
// Go: internal/checker/relater.go:Relater.hasExcessProperties(2701) / Checker.isEmptyObjectType(26326)
#[test]
fn empty_object_target_suppresses_excess_property() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: {} = { a: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bh slice 1 tracer (genuine RED): a shorthand property `{ a }` is equivalent to
// `{ a: a }` — the property's type is the type of the referenced identifier `a`,
// typed through `checkExpressionForMutableLocation` (widening a fresh literal).
// `const a = 1` infers the fresh literal `1`, which widens to `number` in the
// shorthand position, so reading `o.a` resolves to `number`. Before the
// shorthand arm, the member loop only handled `PropertyAssignment`, so `{ a }`
// produced an empty object type and `o.a` resolved to the `error` type.
// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13153) / checkShorthandPropertyAssignment(13603)
#[test]
fn object_literal_shorthand_property_reads_referenced_var_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "const a = 1;\nconst o = { a };\no.a;");
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(c.check_expression(&p, access), number);
}

// 4bh slice 2 (genuine RED before slice 1; green-on-arrival after): a shorthand
// property carries the referenced variable's type into the synthesized object
// type, so a type mismatch against the annotation flows as 2322. `const a = 1`
// widens to `number` in the shorthand, so `{ a }` is `{ a: number; }`, which is
// not assignable to `{ a: string; }`. Before the shorthand arm `{ a }` was an
// empty object type missing `a`, which reported a *different* diagnostic
// (missing property) rather than this number/string mismatch.
// Go: internal/checker/checker.go:Checker.checkShorthandPropertyAssignment(13603) + relater
#[test]
fn object_literal_shorthand_property_mismatch_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const a = 1;\nconst o: { a: string } = { a };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '{ a: number; }' is not assignable to type '{ a: string; }'."
    );
}

// 4bh slice 2 positive control (green-on-arrival): a shorthand property whose
// referenced type matches the annotation reports nothing. `const a = 1` widens
// to `number`, so `{ a }` (`{ a: number; }`) is assignable to `{ a: number }`.
// Go: internal/checker/checker.go:Checker.checkShorthandPropertyAssignment(13603) + relater
#[test]
fn object_literal_shorthand_property_assignable_to_matching_annotation() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const a = 1;\nconst o: { a: number } = { a };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bh slice 3 tracer (genuine RED): a computed property name whose expression is
// a non-literal `string` (`const k: string`) contributes a *string index
// signature* to the object literal type rather than a named property (Go's
// `checkObjectLiteral` -> `getObjectLiteralIndexInfo`). The index signature's
// value type is the (widened) member value type `number`, so element access via
// any string key (`o["anything"]`) resolves to `number`. Before the
// computed-name arm, the member was skipped (no name, no index signature), so
// `o` was an empty object type and `o["anything"]` resolved to the `error` type.
// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13125) / getObjectLiteralIndexInfo(19576)
#[test]
fn object_literal_computed_string_name_synthesizes_string_index() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "const k: string = \"x\";\nconst o = { [k]: 1 };\no[\"anything\"];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(c.check_expression(&p, access), number);
}

// 4bh slice 3 positive control (green-on-arrival): a non-computed named property
// declared alongside a computed-name member is still a regular named property
// (it is NOT swallowed by the index signature). `{ b: 2, [k]: 1 }` keeps `b` as
// a named `number` property, readable through `o.b`.
// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13257)
#[test]
fn object_literal_named_property_coexists_with_computed_name() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "const k: string = \"x\";\nconst o = { b: 2, [k]: 1 };\no.b;",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(c.check_expression(&p, access), number);
}

// 4bh slice 3b tracer (genuine RED): a computed property name whose expression
// is a non-literal `number` (`const k: number`) contributes a *number index
// signature*, so element access by a number key (`o[0]`) resolves to the index
// signature's value type `number`. Before the computed-name arm there was no
// index signature and `o[0]` resolved to the `error` type. (Go's
// `hasComputedNumberProperty` branch -> `getObjectLiteralIndexInfo(_, _,
// numberType)`.)
// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13128) / getObjectLiteralIndexInfo(19576)
#[test]
fn object_literal_computed_number_name_synthesizes_number_index() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "const k: number = 0;\nconst o = { [k]: 1 };\no[0];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(c.check_expression(&p, access), number);
}

// 4bh slice 3c (genuine RED before the 2464 emission): a computed property name
// whose expression is not assignable to `string | number | symbol` (and is not
// `any`) reports 2464. `const k: boolean` is neither string/number/symbol nor
// assignable to their union, so `{ [k]: 1 }` reports the diagnostic. Before
// `checkComputedPropertyName` emitted 2464, no diagnostic was reported.
// Go: internal/checker/checker.go:Checker.checkComputedPropertyName(26619)
#[test]
fn object_literal_computed_name_non_indexable_reports_2464() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const k: boolean = true;\nconst o = { [k]: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2464);
    assert_eq!(
        diags[0].message,
        "A computed property name must be of type 'string', 'number', 'symbol', or 'any'."
    );
}

// 4bh slice 3 end-to-end (green-on-arrival, unlocked by slice 3): the synthesized
// string index signature flows through assignability. The index value type is
// `number`, so `const n: number = o["foo"]` reports nothing, while a `string`
// annotation mismatches and reports 2322. This exercises the index signature via
// `getApplicableIndexInfoForName`/`getIndexedAccessType` from the variable path.
// Go: internal/checker/checker.go:Checker.getObjectLiteralIndexInfo(19576) + relater
#[test]
fn object_literal_string_index_value_is_assignable_to_number() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const k: string = \"x\";\nconst o = { [k]: 1 };\nconst n: number = o[\"foo\"];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bh slice 3 end-to-end (genuine RED before slice 3): the synthesized string
// index value type `number` is not assignable to a `string` annotation, so
// `const s: string = o["foo"]` reports 2322. Before slice 3 there was no index
// signature and `o["foo"]` was the `error` type (assignable to anything), so
// nothing was reported.
// Go: internal/checker/checker.go:Checker.getObjectLiteralIndexInfo(19576) + relater
#[test]
fn object_literal_string_index_value_mismatch_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const k: string = \"x\";\nconst o = { [k]: 1 };\nconst s: string = o[\"foo\"];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// 4bh unit test: `is_numeric_literal_name` mirrors Go's `isNumericLiteralName`
// (the JS-number round-trip of the text equals the text). Numeric-form names are
// numeric; hex/leading-zero/non-numeric names are not. Used by
// `getObjectLiteralIndexInfo` to decide which statically-named members feed a
// number index signature.
// Go: internal/checker/utilities.go:isNumericLiteralName(860)
#[test]
fn is_numeric_literal_name_matches_round_trip() {
    assert!(super::is_numeric_literal_name("0"));
    assert!(super::is_numeric_literal_name("123"));
    assert!(super::is_numeric_literal_name("1.5"));
    assert!(!super::is_numeric_literal_name("0xF00D"));
    assert!(!super::is_numeric_literal_name("01"));
    assert!(!super::is_numeric_literal_name("a"));
    assert!(!super::is_numeric_literal_name(""));
    assert!(!super::is_numeric_literal_name("\u{FE}computed"));
}

// 4bi slice 1 tracer (genuine RED): `[1, 2] as const` types the array literal as
// a readonly tuple whose element types are the *preserved* literal types `1` and
// `2` (NOT widened to `number`). Reading `t[0]` resolves positionally to the
// literal `1`. Before this round `check_array_literal` ignored the const context
// and built `Array<number>` (each element widened, unioned), so `t[0]` resolved
// to `number` (≠ the literal `1`). A synthetic top-level `interface Array<T>`
// stands in for the lib type so the pre-change `Array<number>` path is well
// defined.
// Go: internal/checker/checker.go:Checker.checkArrayLiteral(7989) (inConstContext)
#[test]
fn const_assertion_on_array_literal_keeps_literal_element_types() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\nconst t = [1, 2] as const;\nt[0];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let one = c.get_number_literal_type(tsgo_jsnum::Number::from(1.0));
    assert_eq!(c.check_expression(&p, access), one);
}

// 4bi slice 1b tracer (genuine RED): an `[1, 2] as const` readonly tuple prints
// as `readonly [1, 2]` — the element types are the preserved literals `1` and
// `2`, and the `readonly` modifier reflects the const tuple. Before tuple
// printing was wired the node builder fell through to the anonymous-object
// member serializer and printed `{}` (a tuple carries no named properties).
// Go: internal/checker/checker.go:Checker.typeToString (tuple) / nodebuilderimpl.go
#[test]
fn const_assertion_on_array_literal_prints_readonly_tuple() {
    let p = StubProgram::parse_and_bind("/a.ts", "const t = [1, 2] as const;\nt;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "readonly [1, 2]"
    );
}

// 4bi slice 2a (green-on-arrival, shared with slice 1's mutable-location const
// branch): `{ a: 1 } as const`'s property `a` keeps the *literal* type `1` (not
// widened to `number`), because the property value `1` is in the object
// literal's const context. The const branch added for the array literal element
// (`checkExpressionForMutableLocation` -> `getRegularTypeOfLiteralType`) is the
// same path object-literal property values flow through, so reading `o.a`
// resolves to the literal `1`.
// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076) / checkExpressionForMutableLocation(13784)
#[test]
fn const_assertion_on_object_literal_keeps_literal_property_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { a: 1 } as const;\no.a;");
    let access = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let one = c.get_number_literal_type(tsgo_jsnum::Number::from(1.0));
    assert_eq!(c.check_expression(&p, access), one);
}

// 4bi slice 2b tracer (genuine RED): in a const context an object-literal
// property symbol carries the `Readonly` check flag (Go's `checkObjectLiteral`
// sets `checkFlags = CheckFlagsReadonly` when `isConstContext(node)`, then
// `newSymbolEx(..., checkFlags)`). Before this round object-literal property
// symbols were always minted with empty check flags, so the `Readonly` flag was
// absent even under `as const`.
// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13104) (CheckFlagsReadonly)
#[test]
fn const_assertion_on_object_literal_marks_property_readonly() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { a: 1 } as const;\no;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    let prop = crate::core::declared_types::get_property_of_type(&c, t, "a")
        .expect("object literal has property `a`");
    assert!(
        crate::core::is_synthesized_symbol(prop),
        "an object-literal property is a checker-synthesized symbol"
    );
    assert!(
        c.synthesized_symbol_check_flags(prop)
            .contains(tsgo_ast::CheckFlags::READONLY),
        "a const-context object-literal property must carry the Readonly check flag"
    );
}

// 4bi slice 2c tracer (genuine RED): a const-context object literal prints its
// properties with the `readonly` adornment and the preserved literal type, so
// `{ a: 1 } as const` is `{ readonly a: 1; }`. Before the node builder honored
// the property `Readonly` check flag it printed `{ a: 1; }` (no `readonly`).
// Go: internal/checker/checker.go:Checker.typeToString / nodebuilderimpl.go (isReadonlySymbol)
#[test]
fn const_assertion_on_object_literal_prints_readonly_member() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { a: 1 } as const;\no;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ readonly a: 1; }"
    );
}

// 4bi slice 3 negative control (green-on-arrival): WITHOUT `as const`, an object
// literal property is NOT readonly and its type is WIDENED. `{ a: 1 }` (no const
// context) prints `{ a: number; }` — the member value `1` widens to `number`
// through the default (non-const) `checkExpressionForMutableLocation` branch and
// no `readonly` adornment appears. This guards against the const-context typing
// leaking into ordinary object literals (4bf behavior unchanged).
// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076) (no const context)
#[test]
fn non_const_object_literal_property_is_widened_and_mutable() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { a: 1 };\no;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ a: number; }"
    );
    // The property carries no `Readonly` check flag.
    let prop = crate::core::declared_types::get_property_of_type(&c, t, "a")
        .expect("object literal has property `a`");
    assert!(
        !c.synthesized_symbol_check_flags(prop)
            .contains(tsgo_ast::CheckFlags::READONLY),
        "a non-const object-literal property must not be readonly"
    );
}

// 4bi slice 3 negative control (green-on-arrival): WITHOUT `as const`, an array
// literal stays the `Array<T>` reference (NOT a readonly tuple) with the widened
// element type. `[1, 2]` (no const context) prints `Array<number>` and `t[0]`
// resolves to `number`. This guards against the const-context tuple typing
// leaking into ordinary array literals (4bf behavior unchanged).
// Go: internal/checker/checker.go:Checker.checkArrayLiteral(7989) (no const context)
#[test]
fn non_const_array_literal_is_array_not_tuple() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\nconst t = [1, 2];\nt;",
    );
    let usage = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "Array<number>"
    );
}

// 4bi extra: positional tuple access reads the SECOND preserved literal too —
// `[1, 2] as const`'s `t[1]` is the literal `2` (not `number`), confirming the
// tuple stores each element type by position, not a single unioned element.
// Go: internal/checker/checker.go:Checker.checkArrayLiteral(7989) + tuple element access
#[test]
fn const_assertion_on_array_literal_second_element_keeps_literal() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\nconst t = [1, 2] as const;\nt[1];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let two = c.get_number_literal_type(tsgo_jsnum::Number::from(2.0));
    assert_eq!(c.check_expression(&p, access), two);
}

// 4bi extra (nesting depth): `isConstContext` recursion is faithful to Go and
// propagates through ANY depth of nested array/object literals. An array nested
// in an `as const` object literal becomes a readonly tuple with preserved
// literal elements, and the outer property is readonly:
// `{ a: [1, 2] } as const` => `{ readonly a: readonly [1, 2]; }`. This exercises
// the property-assignment -> grandparent (object literal) -> const-assertion
// chain feeding the array literal's element/spread const-context.
// Go: internal/checker/checker.go:Checker.isConstContext(13529)
#[test]
fn const_assertion_propagates_into_nested_array_literal() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { a: [1, 2] } as const;\no;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ readonly a: readonly [1, 2]; }"
    );
}

// 4bi extra (nesting depth): the recursion also propagates through nested ARRAY
// literals, so `[[1]] as const` is a readonly tuple whose single element is
// itself a readonly tuple: `readonly [readonly [1]]`. This confirms the
// array-literal -> array-literal const-context chain.
// Go: internal/checker/checker.go:Checker.isConstContext(13529)
#[test]
fn const_assertion_propagates_into_nested_inner_array_literal() {
    let p = StubProgram::parse_and_bind("/a.ts", "const t = [[1]] as const;\nt;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "readonly [readonly [1]]"
    );
}

// 4bk slice 1 tracer (genuine RED): an object-literal property value that is a
// literal is contextually typed by the matching property of the annotation, so
// a literal-typed property is PRESERVED instead of widened. For
// `const o: { a: "x" } = { a: "x" }` the value `"x"` flows through the
// contextual property type `"x"` (a string-literal context), so member `a`
// stays `"x"` and the literal `{ a: "x"; }` is assignable to `{ a: "x"; }` with
// no diagnostic. Before this round `checkExpressionForMutableLocation` widened
// `"x"` to `string` with no contextual consultation, so the source was
// `{ a: string; }`, NOT assignable to `{ a: "x"; }`, spuriously reporting 2322.
// Go: internal/checker/checker.go:Checker.getWidenedLiteralLikeTypeForContextualType(25374)
#[test]
fn object_literal_property_literal_preserved_by_contextual_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: { a: \"x\" } = { a: \"x\" };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// 4bk slice 1 guard (green-on-arrival): a property value whose (widened) type
// does NOT match the literal annotation still reports 2322 — the contextual
// preservation only keeps a literal whose KIND matches the context. `{ a: 1 }`
// typed by `{ a: "x" }`: `1` is a number literal, the context `"x"` is a string
// literal, so `is_literal_of_contextual_type` is false, `1` widens to `number`,
// and `{ a: number; }` is not assignable to `{ a: "x"; }`.
// Go: internal/checker/checker.go:Checker.isLiteralOfContextualType(25381)
#[test]
fn object_literal_property_mismatched_literal_kind_still_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: { a: \"x\" } = { a: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '{ a: number; }' is not assignable to type '{ a: \"x\"; }'."
    );
}

// 4bk slice 2 tracer (genuine RED): an array-literal element that is a literal
// is contextually typed by the element type of the contextual array, so the
// element is PRESERVED instead of widened. For `const xs: "a"[] = ["a"]` the
// element `"a"` flows through the contextual element type `"a"` (the iterated
// element type of `"a"[]`), so the literal array is `Array<"a">`, not
// `Array<string>`. Before this round each element widened with no contextual
// consultation, so `["a"]` was `Array<string>`.
// Go: internal/checker/checker.go:Checker.getContextualTypeForElementExpression(29648)
#[test]
fn array_literal_element_literal_preserved_by_contextual_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\nconst xs: \"a\"[] = [\"a\"];\nxs;",
    );
    // Navigate to the array literal initializer `["a"]`.
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let list = match arena.data(stmts[1]) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => panic!("variable statement"),
    };
    let decl = match arena.data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes[0],
        _ => panic!("declaration list"),
    };
    let init = match arena.data(decl) {
        NodeData::VariableDeclaration(d) => d.initializer.expect("initializer"),
        _ => panic!("variable declaration"),
    };
    let mut c = Checker::new();
    let t = c.check_expression(&p, init);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "Array<\"a\">"
    );
}

// 4bk slice 2 guard (no regression): an array literal with NO contextual type
// still widens its elements — the literal preservation only fires when a
// contextual element type makes the position a literal context. `const ys =
// ["a"]` (no annotation) is `Array<string>`, confirming the contextual branch
// degrades to the prior `getWidenedLiteralType` behavior when
// `getContextualType` yields nothing.
// Go: internal/checker/checker.go:Checker.getWidenedLiteralLikeTypeForContextualType (nil contextual)
#[test]
fn array_literal_without_context_still_widens_elements() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\nconst ys = [\"a\"];\nys;",
    );
    let usage = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "Array<string>"
    );
}

// 4bk task slice 1 (green-on-arrival): `const xs: number[] = []; xs[0]` resolves
// to `number`. The VARIABLE `xs` takes its type from the annotation `number[]`
// (not from the empty-array initializer), so `xs[0]` is `number` regardless of
// the literal's own type. This already held via the annotation path
// (`getTypeOfVariableOrParameterOrProperty` reads the type node first); 4bk adds
// no change here but pins the behavior the task names.
// Go: internal/checker/checker.go:Checker.getTypeForVariableLikeDeclaration (type node)
#[test]
fn annotated_empty_array_variable_element_access_is_number() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\nconst xs: number[] = [];\nxs[0];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(c.check_expression(&p, access), number);
}

// 4bk task slice 1 negative control (Go-faithful): the empty array literal `[]`
// ITSELF is `Array<never>` even under a `number[]` contextual type. Go's
// `checkArrayLiteral` uses `implicitNeverType` for an element-less array
// regardless of the contextual type (the contextual element type only flows
// into PRESENT elements); the variable's `number[]` comes solely from the
// annotation. This pins that 4bk does NOT (incorrectly) rewrite the empty
// literal's element type from the context.
// Go: internal/checker/checker.go:Checker.checkArrayLiteral(7989) (implicitNeverType)
#[test]
fn annotated_empty_array_literal_itself_stays_never_array() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\nconst xs: number[] = [];\nxs;",
    );
    // Navigate to the empty array literal initializer `[]`.
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let list = match arena.data(stmts[1]) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => panic!("variable statement"),
    };
    let decl = match arena.data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes[0],
        _ => panic!("declaration list"),
    };
    let init = match arena.data(decl) {
        NodeData::VariableDeclaration(d) => d.initializer.expect("initializer"),
        _ => panic!("variable declaration"),
    };
    let mut c = Checker::new();
    let t = c.check_expression(&p, init);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "Array<never>"
    );
}

// 4bk task slice 2 (green-on-arrival): `const o: { xs: number[] } = { xs: [] };
// o.xs[0]` resolves to `number`. The variable `o` takes its type from the
// annotation `{ xs: number[] }`, so `o.xs` is `number[]` and `o.xs[0]` is
// `number` regardless of the empty-array property value's own type. Pins the
// task-named behavior (already held via the annotation path).
// Go: internal/checker/checker.go:Checker.getTypeForVariableLikeDeclaration (type node)
#[test]
fn annotated_object_empty_array_property_element_access_is_number() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\nconst o: { xs: number[] } = { xs: [] };\no.xs[0];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(c.check_expression(&p, access), number);
}
