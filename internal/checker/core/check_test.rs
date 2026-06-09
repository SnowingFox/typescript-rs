use crate::core::check::{get_effective_return_type_node, DiagnosticMessageChain};
use crate::core::program::BoundProgram;
use crate::core::signatures::{Signature, SignatureFlags};
use crate::core::test_support::{MultiFileProgram, StubProgram};
use crate::core::types::{LiteralValue, ObjectFlags, ObjectType, TypeFlags};
use crate::core::Checker;
use tsgo_ast::{Kind, NodeData, SymbolId};
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::tristate::Tristate;

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

// A parser-recovered MISSING identifier (zero-width, empty text) must NOT be
// resolved or reported by the checker — Go's `getResolvedSymbol` guards
// `if !ast.NodeIsMissing(node)` before calling `resolveName`, so a missing
// identifier resolves to `unknownSymbol` with no "Cannot find name ''"
// diagnostic. Here `do` at EOF error-recovers into a do-statement whose body and
// `while` condition are missing identifiers; the parser reports `TS1109`
// (syntactic) but the checker must add nothing.
// Go: internal/checker/checker.go:Checker.getResolvedSymbol (NodeIsMissing guard)
#[test]
fn missing_identifier_from_recovery_reports_no_cannot_find_name() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "do"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2304),
        "a missing (recovered) identifier must not report a cannot-find-name 2304, got {diags:?}",
    );
}

// Same root via a different recovery shape: `for (let in)` (an empty
// declaration list) error-recovers into a missing identifier; the checker must
// not report `TS2304: Cannot find name ''`.
// Go: internal/checker/checker.go:Checker.getResolvedSymbol (NodeIsMissing guard)
#[test]
fn missing_identifier_in_for_in_reports_no_cannot_find_name() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "for (let in) { y; }"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.message != "Cannot find name ''."),
        "the empty-name identifier must not be reported, got {diags:?}",
    );
}

// Guard: a genuine (present) undefined identifier STILL reports `TS2304` — the
// NodeIsMissing guard must only suppress zero-width recovered nodes, not real
// references.
// Go: internal/checker/checker.go:Checker.getResolvedSymbol (resolveName on present node)
#[test]
fn present_undefined_identifier_still_reports_cannot_find_name() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "missingName;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected exactly the 2304, got {diags:?}");
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'missingName'.");
}

// Round 20 (RED->GREEN headline): a top-level EXPORTED enum referenced as a
// VALUE within the SAME module must resolve (no TS2304). The binder gives an
// exported value declaration TWO symbols — a phantom `ExportValue` local in the
// module's `locals` and the real symbol in `exports` (reached via
// `export_symbol`) — so a `Value`-only `resolveName` misses the phantom. Go's
// `getResolvedSymbol` resolves with meaning `Value | ExportValue` and maps the
// phantom to the export symbol via `getExportSymbolOfValueSymbolIfExported`.
// Go: internal/checker/checker.go:Checker.getResolvedSymbol + getExportSymbolOfValueSymbolIfExported
#[test]
fn same_module_exported_enum_value_access_resolves_no_2304() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "export enum E { A }\nconst y = E.A;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2304),
        "an exported enum referenced as a value within the same module must resolve, got {diags:?}",
    );
}

// Round 20: an exported FUNCTION (and an exported self-referencing CLASS)
// referenced as a value within the same module also resolve through the
// `ExportValue` phantom -> `export_symbol` map (the assertion-function /
// class-self-reference corpus shapes).
// Go: internal/checker/checker.go:Checker.getResolvedSymbol + getExportSymbolOfValueSymbolIfExported
#[test]
fn same_module_exported_function_and_class_self_ref_resolve_no_2304() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "export function assertWeird(value?: string): asserts value {}\n\
         assertWeird();\n\
         export class Foo {\n  static instance = new Foo();\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2304),
        "an exported function called and an exported class self-reference must resolve, got {diags:?}",
    );
}

// Round 20 (GUARD): adding `ExportValue` to the value-lookup meaning must NOT
// blanket-resolve a genuinely-undefined name. In a MODULE (so the `ExportValue`
// phantom mechanism is active), a bare undefined reference still reports TS2304.
// Go: internal/checker/checker.go:Checker.getResolvedSymbol (resolveName failure)
#[test]
fn same_module_undefined_name_still_reports_2304() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "export const x = 1;\nnope;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let names: Vec<_> = diags
        .iter()
        .filter(|d| d.code == 2304)
        .map(|d| d.message.clone())
        .collect();
    assert_eq!(
        names,
        vec!["Cannot find name 'nope'.".to_string()],
        "a genuinely-undefined name must still report exactly one TS2304, got {diags:?}",
    );
}

// Round 20 (GUARD): resolving the exported enum as a value must NOT silently
// resolve a NON-EXISTENT member. `E.B` (no member `B`) reports TS2339, not a
// missed property and never a TS2304 on `E`.
// Go: internal/checker/checker.go:Checker.checkPropertyAccessExpression (TS2339)
#[test]
fn same_module_exported_enum_missing_member_reports_2339_not_2304() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "export enum E { A }\nconst z = E.B;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2304),
        "the exported enum `E` must resolve (no TS2304), got {diags:?}",
    );
    assert!(
        diags.iter().any(|d| d.code == 2339),
        "accessing a non-existent enum member must report TS2339, got {diags:?}",
    );
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

/// Counts the `TS2300` (`Duplicate identifier`) diagnostics produced for `src`.
fn duplicate_identifier_count(src: &str) -> usize {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.get_diagnostics(root)
        .iter()
        .filter(|d| d.code == 2300)
        .count()
}

// Round 18 (checkObjectTypeForDuplicateDeclarations): within ONE interface, a
// property that shares a name with an accessor (or another property) is a
// duplicate identifier. `get x; x: number; set x;` merges into a single symbol
// with three declarations (a getter, a property, a setter), so the checker's
// duplicate-member state machine reports all three `x` members (property +
// accessor combination). This is the corpus's `duplicateIdentifierChecks.ts`
// I5/I6 gap (the binder's excludes do NOT flag property-vs-accessor).
// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3122)
#[test]
fn interface_property_accessor_duplicate_reports_2300() {
    let count = duplicate_identifier_count(
        "interface I {\n    get x(): number;\n    x: number;\n    set x(value: number);\n}",
    );
    assert_eq!(count, 3, "all three `x` members are duplicate identifiers");
}

// Round 18 (class members): a `declare class` with `get x; x: number; set x;`
// reports the same property/accessor duplicate (the corpus C5/C6 gap). The
// checker runs `checkObjectTypeForDuplicateDeclarations` from
// `checkClassLikeDeclaration` for classes too.
// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4279)
#[test]
fn class_property_accessor_duplicate_reports_2300() {
    let count = duplicate_identifier_count(
        "declare class C {\n    get x(): number;\n    x: number;\n    set x(value: number);\n}",
    );
    assert_eq!(count, 3, "all three `x` members are duplicate identifiers");
}

// Round 18 (two same-name properties): `x: number; x: string;` in one interface
// merges into one symbol (the binder's `PropertyExcludes` excludes `Property`),
// so the checker reports both as duplicates (property + property).
// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations (state==1)
#[test]
fn interface_duplicate_property_reports_2300() {
    let count = duplicate_identifier_count("interface I {\n    x: number;\n    x: string;\n}");
    assert_eq!(count, 2, "both duplicate properties are reported");
}

// Round 18 GUARD (no over-fire): a LEGAL get/set accessor pair in an interface
// merges into a single accessor symbol but is NOT a duplicate (state stays `2`,
// `kind == 2`), so NO TS2300.
// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations (state==2 && kind==2)
#[test]
fn interface_legal_get_set_pair_no_duplicate() {
    let count = duplicate_identifier_count(
        "interface I {\n    get x(): number;\n    set x(value: number);\n}",
    );
    assert_eq!(count, 0, "a legal get/set accessor pair is not a duplicate");
}

// Round 18 GUARD (no over-fire): distinct member names in one interface produce
// no duplicate-identifier diagnostic.
#[test]
fn interface_distinct_members_no_duplicate() {
    let count = duplicate_identifier_count(
        "interface I {\n    a: number;\n    b: string;\n    c(): void;\n}",
    );
    assert_eq!(count, 0, "distinct members are not duplicates");
}

// Round 18 GUARD (no over-fire): a static and an instance member sharing a name
// in a class are NOT duplicates (they live in distinct member tables, so the
// checker keys instance vs static names separately).
// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations (static/instance maps)
#[test]
fn class_static_and_instance_same_name_no_duplicate() {
    let count =
        duplicate_identifier_count("declare class C {\n    static x: number;\n    x: number;\n}");
    assert_eq!(
        count, 0,
        "a static member and an instance member may share a name"
    );
}

// Round 18 GUARD (no over-fire): two empty interface declarations legally merge
// (no members) and produce no duplicate-identifier diagnostic.
#[test]
fn merged_empty_interfaces_no_duplicate() {
    let count = duplicate_identifier_count("interface I {}\ninterface I {}");
    assert_eq!(count, 0, "empty merged interfaces are legal");
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

// Property access on a value typed `any` yields `any` with NO 2339 — Go's
// `checkPropertyAccessExpressionOrQualifiedName` short-circuits when the
// apparent type `isTypeAny`, returning the any type before any member lookup.
// Without this guard, `any.<anything>` cascaded into a spurious "Property does
// not exist on type 'any'".
// Go: internal/checker/checker.go:Checker.checkPropertyAccessExpressionOrQualifiedName (isAnyLike)
#[test]
fn property_access_on_any_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: any;\nx.whatever;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "property access on `any` must not report 2339, got {diags:?}"
    );
}

// Cascade guard: an UNRESOLVED bare identifier (whose type is the `error` type,
// which carries the `Any` flag) accessed via property must report ONLY the
// single `TS2304` ("Cannot find name") — NOT a follow-on `TS2339` on the
// `error`-typed receiver. Go's `checkPropertyAccessExpressionOrQualifiedName`
// short-circuits the any-like (error) receiver, so the cascade stops at the
// 2304. This is the amplifier behind the dominant P10 corpus `extra TS2339`.
// Go: internal/checker/checker.go:Checker.checkPropertyAccessExpressionOrQualifiedName (isAnyLike on errorType)
#[test]
fn property_access_on_unresolved_name_reports_only_2304() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "unresolvedThing.member;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected only the 2304, got {diags:?}");
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'unresolvedThing'.");
}

// Round 17 (function-expando, headline): `function f(){}; f.x = 1; f.x;` adds
// an expando member `x` to the function symbol's exports (binder), which the
// function's anonymous object type exposes as a property, so `f.x` resolves with
// NO TS2339.
// Go: internal/checker/checker.go:Checker.getTypeOfFuncClassEnumModuleWorker
//     + bindDeferredExpandoAssignment
#[test]
fn function_expando_property_resolves_no_2339() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f() {}\nf.x = 1;\nf.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2339),
        "f.x should resolve as a synthesized expando member, got {diags:?}"
    );
}

// Round 17 (faithful expando member type): `function f(){}; f.x = 1; f.x;`
// types `f.x` as the WIDENED assigned value `number` (Go's
// `getWidenedTypeForAssignmentDeclaration` -> widened union of the assigned
// right-hand sides), not a bare `any`.
// Go: internal/checker/checker.go:Checker.getWidenedTypeForAssignmentDeclaration
#[test]
fn function_expando_property_yields_assigned_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "function f() {}\nf.x = 1;\nf.x;");
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(
        c.check_expression(&p, access),
        number,
        "f.x should have the widened assigned type `number`"
    );
}

// Round 17 GUARD (no over-resolution): an expando member is synthesized ONLY
// for the assigned name. A genuinely-absent property on the same function still
// reports TS2339 — populating the function type's members must not blanket-mute
// member access.
// Go: internal/checker/checker.go:Checker.reportNonexistentProperty
#[test]
fn function_expando_absent_property_still_reports_2339() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f() {}\nf.x = 1;\nf.y;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags
            .iter()
            .any(|d| d.code == 2339 && d.message.contains("'y'")),
        "the non-expando property `f.y` must still report TS2339, got {diags:?}"
    );
    assert!(
        diags.iter().all(|d| !d.message.contains("'x'")),
        "the expando property `f.x` must NOT report TS2339, got {diags:?}"
    );
}

// Round 17 (this-property, JS): `this.x = 1` in a JS class constructor adds an
// instance member `x` (binder), so reading `this.x` in a method resolves with NO
// TS2339.
// Go: internal/checker/checker.go (this-type property resolution) +
//     bindThisPropertyAssignment
#[test]
fn this_property_assignment_resolves_no_2339() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_js(
        "/a.js",
        "class C {\n  constructor() { this.x = 1; }\n  m() { return this.x; }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2339),
        "this.x should resolve as a synthesized member, got {diags:?}"
    );
}

// Round 24 (static-side type of a class value): a class referenced as a VALUE
// has the static (constructor) side type, whose members are the class's STATIC
// members (the binder's `exports` table). So a static member access on the
// class value — `Other.Baz` where `Other` declares `static Baz` — resolves with
// NO spurious TS2339 (Go's `getTypeOfSymbol` -> class arm ->
// `getTypeOfFuncClassEnumModuleWorker`).
// Go: internal/checker/checker.go:Checker.getTypeOfFuncClassEnumModuleWorker
#[test]
fn class_static_member_access_resolves_no_2339() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class Other {\n  static Baz = 42;\n}\nconst x = Other.Baz;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2339),
        "Other.Baz (static member on a class value) should resolve, got {diags:?}"
    );
}

// Round 24 (static-side member type): `Other.Baz` for `static Baz = 42` yields
// the widened initializer type `number` (the static member resolved off the
// class value's static-side object type).
// Go: internal/checker/checker.go:Checker.getTypeOfFuncClassEnumModuleWorker
#[test]
fn class_static_member_access_yields_member_type() {
    let p =
        StubProgram::parse_and_bind("/a.ts", "class Other {\n  static Baz = 42;\n}\nOther.Baz;");
    let access = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let number = c.number_type();
    assert_eq!(
        c.check_expression(&p, access),
        number,
        "Other.Baz should have the widened static-member type `number`"
    );
}

// Round 24 GUARD (no over-suppress): a genuinely-absent static member on a class
// value still reports TS2339. Exposing the static members must not blanket-mute
// member access on the class value.
// Go: internal/checker/checker.go:Checker.reportNonexistentProperty
#[test]
fn class_value_absent_static_member_still_reports_2339() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class Other {\n  static Baz = 42;\n}\nOther.nope;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags
            .iter()
            .any(|d| d.code == 2339 && d.message.contains("'nope'")),
        "the absent static member `Other.nope` must still report TS2339, got {diags:?}"
    );
}

// Round 24 GUARD (instance member is NOT on the static side): an INSTANCE member
// accessed off the class VALUE (`Other.inst` where `inst` is a non-static
// member) still reports TS2339 — the static-side type carries only the static
// members, never the instance members.
// Go: internal/checker/checker.go:Checker.getTypeOfFuncClassEnumModuleWorker
#[test]
fn class_value_instance_member_reports_2339() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class Other {\n  inst = 1;\n}\nOther.inst;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags
            .iter()
            .any(|d| d.code == 2339 && d.message.contains("'inst'")),
        "the instance member `Other.inst` accessed on the class value must report TS2339, got {diags:?}"
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

// Go: internal/scanner/scanner.go:GetErrorRangeForNode (KindVariableDeclaration ->
//     GetNameOfDeclaration) + internal/checker/checker.go:checkVariableLikeDeclaration(5869)
// A `const x: number = "";` relation error narrows its span to the declaration
// NAME `x` (start at `x` after `skipTrivia`, length = the name), NOT the whole
// `x: number = ""` declaration. tsc reports `(1,7)` with a single-character
// underline; the byte span is therefore `start == <index of x>`, `length == 1`.
#[test]
fn variable_declaration_2322_span_is_the_name() {
    let src = "const x: number = \"\";";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    let x = src.find('x').expect("name `x` present") as i32;
    assert_eq!(
        diags[0].start, x,
        "span starts at the name `x`, not the leading trivia / declaration"
    );
    assert_eq!(
        diags[0].length, 1,
        "span length is the name `x` (1 char), not the whole declaration"
    );
}

// Go: internal/scanner/scanner.go:GetErrorRangeForNode (default case, expression
//     node) + internal/checker/checker.go:checkAssignmentOperator(12701)
// GUARD: an assignment-expression `2322` still reports on the LHS reference
// expression, with the span at the LHS identifier (`skipTrivia(pos)..end`),
// length 1 — narrowing applies to declarations, not to a bare assignment LHS.
#[test]
fn assignment_2322_span_is_the_lhs_identifier() {
    let src = "declare let n: number;\nn = \"s\";";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    // The LHS `n` of the second statement (after the newline); `skip_trivia`
    // moves the start past the leading line break to the identifier itself.
    let lhs = src.find("n = ").expect("LHS `n` present") as i32;
    assert_eq!(diags[0].start, lhs, "span starts at the LHS identifier `n`");
    assert_eq!(
        diags[0].length, 1,
        "span length is the LHS identifier (1 char)"
    );
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

// Go: internal/checker/relater.go:Relater.typeRelatedToSomeType (union target,
// object-literal source kept fresh by the union contextual type)
#[test]
fn object_literal_assignable_to_discriminated_union_target() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type U = { k: \"a\"; x: number } | { k: \"b\"; y: string };\nconst u: U = { k: \"a\", x: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The fresh object literal `{ k: "a", x: 1 }` keeps its literal property
    // types under the union contextual type `U`, so it relates to the
    // `{ k: "a"; x: number }` constituent and no `2322`/`2353` is reported.
    assert!(
        c.get_diagnostics(root).is_empty(),
        "expected no diagnostics, got {:?}",
        c.get_diagnostics(root)
    );
}

// Go: internal/checker/relater.go:Relater.hasExcessProperties (union discriminant
// reduction via findMatchingDiscriminantType)
#[test]
fn object_literal_discriminant_selects_constituent_for_excess_property() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Item = { kind: \"a\"; subkind: 0 } | { kind: \"b\" };\nconst i: Item = { kind: \"b\", subkind: 0 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    // The discriminant `kind: "b"` selects the `{ kind: "b" }` constituent, so
    // `subkind` is an excess property reported as `2353` against THAT
    // constituent (matching tsc), not against the whole union and not a generic
    // union `2322`.
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].code, 2353);
    assert_eq!(
        diags[0].message,
        "Object literal may only specify known properties, and 'subkind' does not exist in type '{ kind: \"b\"; }'."
    );
}

// GUARD — Go: internal/checker/relater.go:Relater.typeRelatedToSomeType (no
// constituent relates -> the relation still fails)
#[test]
fn object_literal_matching_no_union_constituent_still_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type U = { k: \"a\"; x: number } | { k: \"b\"; y: string };\nconst u: U = { k: \"c\", x: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    // `k: "c"` matches no constituent's discriminant, so the object relates to
    // NONE of them and a genuine `2322` still fires — the union relate is not
    // over-relaxed.
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].code, 2322);
}

// Go: internal/checker/relater.go:Relater.typeRelatedToSomeType (reportErrors ->
// getBestMatchingType -> findMatchingDiscriminantType elaboration)
#[test]
fn object_literal_wrong_member_elaborates_against_matched_constituent() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type U = { k: \"a\"; x: number } | { k: \"b\"; y: string };\nconst u: U = { k: \"a\", x: \"oops\" };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    // The discriminant `k: "a"` selects `{ k: "a"; x: number }`, so the failure
    // elaborates against THAT constituent down to the incompatible `x` (a nested
    // `2326` chain), instead of a flat union failure with no child.
    fn mentions_x_incompatible(chain: &[crate::core::check::DiagnosticMessageChain]) -> bool {
        chain.iter().any(|node| {
            (node.code == 2326 && node.message.contains("'x'"))
                || mentions_x_incompatible(&node.next)
        })
    }
    assert!(
        mentions_x_incompatible(&d.message_chain),
        "expected an x-incompatibility elaboration, got {:?}",
        d.message_chain
    );
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
        "interface A {\n  x: number;\n}\ninterface B {\n  x: string;\n}\ndeclare let a: A;\ndeclare const b: B;\na = b;",
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
        "declare let n: number;\nn = \"s\";",
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
        "interface A {\n  x: number;\n}\ndeclare let a: A;\ndeclare const a2: A;\na = a2;",
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
        "declare let s: string;\ns *= 1;",
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
        "declare let n: number;\nn += \"s\";",
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
        "declare let n: number;\nn &&= \"s\";",
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
        "declare let n: number;\nn += 1;",
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
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "loop: while (true) { unknownName; break loop; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // The labeled statement is descended into (Go's `checkLabeledStatement` ->
    // `checkSourceElement(statement)`), so the undefined name reports 2304.
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "expected cannot-find-name in labeled body; got: {:?}",
        diags
    );
}

// Go: internal/checker/checker.go:Checker.checkAssignmentOperator (`||=` right not assignable, 2322)
#[test]
fn logical_or_equals_assignment_not_assignable_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare let n: number;\nn ||= \"s\";",
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
        "declare let n: number;\nn ??= \"s\";",
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

// Round 16 (rest-parameter expansion): an argument at a rest-parameter position
// relates to the rest ELEMENT type (`number`), not the whole rest array
// (`number[]`). `f(1)` is therefore well-typed and reports nothing.
// Go: internal/checker/relater.go:Checker.tryGetTypeAtPosition (rest indexed access)
#[test]
fn rest_parameter_call_accepts_assignable_argument() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\n\
         function f(...args: number[]): void {}\n\
         f(1);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Round 16: a rest parameter (`...args`) lifts the arity cap, so MANY trailing
// arguments are accepted without a `2554` (Go's `hasEffectiveRestParameter`
// short-circuits the "too many arguments" check). Each argument still relates
// to the element type `number`, so all-assignable arguments report nothing.
// Go: internal/checker/checker.go:Checker.hasCorrectArity (effective rest cap)
#[test]
fn rest_parameter_call_accepts_many_assignable_arguments() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\n\
         function f(...args: number[]): void {}\n\
         f(1, 2, 3, 4);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Round 16 GUARD (no over-relaxation): a rest argument that is NOT assignable to
// the rest ELEMENT type still reports `2345`, and the parameter type in the
// message is the element type (`number`), not the rest array (`number[]`) — so
// the fix narrows the target without muting genuine incompatibilities.
// Go: internal/checker/checker.go:Checker.isSignatureApplicable (2345 at rest element)
#[test]
fn rest_parameter_call_incompatible_argument_still_reports_2345() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\n\
         function f(...args: number[]): void {}\n\
         f(\"x\");",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code, 2345);
    assert_eq!(
        diags[0].message,
        "Argument of type 'string' is not assignable to parameter of type 'number'."
    );
}

// Round 16: a fixed parameter before a rest parameter keeps its own type, while
// trailing arguments relate to the rest element type. `f("a", 1, 2)` is
// well-typed; a wrong-typed trailing argument still reports `2345` on the rest
// element.
// Go: internal/checker/relater.go:Checker.tryGetTypeAtPosition (fixed + rest)
#[test]
fn rest_parameter_after_fixed_parameter_relates_each_position() {
    let ok = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\n\
         function f(first: string, ...rest: number[]): void {}\n\
         f(\"a\", 1, 2);",
    ));
    let root = ok.root();
    let mut c = Checker::new_checker(ok);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "{:?}",
        c.get_diagnostics(root)
    );

    let bad = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\n\
         function f(first: string, ...rest: number[]): void {}\n\
         f(\"a\", \"b\");",
    ));
    let root = bad.root();
    let mut c = Checker::new_checker(bad);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code, 2345);
    assert_eq!(
        diags[0].message,
        "Argument of type 'string' is not assignable to parameter of type 'number'."
    );
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
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.issueMemberSpecificError (2416)
#[test]
fn class_incorrectly_extends_base_class_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B {\n  x: number = 0;\n}\nclass D extends B {\n  x: string = \"s\";\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    // `class D extends B { x: string }` (B has `x: number`): the derived `x` is
    // an incompatible override, so the checker reports the member-specific
    // `2416` before the broad `2415` extends error.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2416);
    assert!(
        diags[0].message.contains(
            "Property 'x' in type 'D' is not assignable to the same property in base type 'B'."
        ),
        "unexpected message: {}",
        diags[0].message
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
    // `D` incorrectly extend `B` (2416 member-specific), and `D` also lacks
    // `I`'s `y` member (2420). Both heritage relations are checked, extends
    // before implements.
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 2);
    assert_eq!(diags[0].code, 2416);
    assert!(
        diags[0].message.contains(
            "Property 'x' in type 'D' is not assignable to the same property in base type 'B'."
        ),
        "unexpected extends message: {}",
        diags[0].message
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

// A bare identifier reference in expression position resolves against the
// program's MERGED GLOBALS, not just the reference's own scope chain: file A
// (the "lib") declares a global value `GlobalThing`, file B references it as a
// bare identifier. `checkIdentifier` must consult `c.globals` — Go's
// `resolveName` always does — so there is NO 2304: file A's global is visible
// in file B. This is the cross-file / lib global VALUE resolution that lib
// globals (`Error`, `Object`, `Date`, ...) rely on; before the fix
// `checkIdentifier` passed `None` for the globals table, so any name absent
// from the reference's own scope chain cascaded into a spurious 2304 (and a
// follow-on 2339 on its `error`-typed members).
// Go: internal/checker/checker.go:Checker.checkIdentifier -> resolveName (consults c.globals)
#[test]
fn bare_identifier_resolves_against_merged_globals() {
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/lib.d.ts", "declare var GlobalThing: number;"),
        ("/b.ts", "GlobalThing;"),
    ]));
    let file_b = p.source_files()[1];
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(file_b);
    assert!(
        diags.is_empty(),
        "a global value must resolve cross-file (no 2304), got {diags:?}"
    );
}

// Guard for the merged-globals fix: a bare identifier that is in NO file's
// globals (and not in scope) still reports 2304. This proves the fix resolves
// real globals rather than blanket-muting the "Cannot find name" diagnostic.
// Go: internal/checker/checker.go:Checker.checkIdentifier (Cannot_find_name_0)
#[test]
fn bare_identifier_not_in_globals_still_reports_2304() {
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/lib.d.ts", "declare var GlobalThing: number;"),
        ("/b.ts", "NotAGlobalAnywhere;"),
    ]));
    let file_b = p.source_files()[1];
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(file_b);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'NotAGlobalAnywhere'.");
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
//
// 4bo update: a fresh object-literal RHS now elaborates element-wise (Go's
// `elaborateError`), so the message is the per-element leaf "Type 'string' is
// not assignable to type 'number'." (was the whole-object chain head before
// 4bo). See `elaborate_object_literal_wrong_property_type_points_at_property_name`
// for the full anchor + related-info assertions.
// Go: internal/checker/relater.go:Checker.elaborateObjectLiteral / elaborateElement
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
        "Type 'string' is not assignable to type 'number'."
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
// and the `a` member is not assignable to `"x"`.
//
// 4bo update: the fresh object-literal RHS elaborates element-wise, so the
// message is the per-element leaf "Type 'number' is not assignable to type
// '"x"'." (was the whole-object chain head before 4bo), with a `6500` related
// info pointing at the target property declaration. Verified against `cmd/tsgo`.
// Go: internal/checker/relater.go:Checker.elaborateElement
#[test]
fn object_literal_property_mismatched_literal_kind_still_reports_2322() {
    let src = "const o: { a: \"x\" } = { a: 1 };";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type 'number' is not assignable to type '\"x\"'."
    );
    // Anchored at the literal's `a` (in the RHS), with the `6500` related info.
    let eq = src.find('=').unwrap();
    assert!(d.start as usize > eq);
    assert_eq!(d.related_information.len(), 1);
    assert_eq!(d.related_information[0].code, 6500);
    assert_eq!(
        d.related_information[0].message,
        "The expected type comes from property 'a' which is declared here on type '{ a: \"x\"; }'"
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

// 4bn: relation-engine diagnostic elaboration chains.
// Go: internal/checker/relater.go:Relater.propertyRelatedTo (2326 over leaf 2322)
#[test]
fn assignability_chain_single_level_property_mismatch() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { a: number }\ninterface B { a: string }\ndeclare const b: B;\nvar x: A = b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message, "Type 'B' is not assignable to type 'A'.");
    // The head carries a chain: 2326 "Types of property 'a' are incompatible."
    // over a leaf 2322 "Type 'string' is not assignable to type 'number'.".
    assert_eq!(d.message_chain.len(), 1);
    let prop = &d.message_chain[0];
    assert_eq!(prop.code, 2326);
    assert_eq!(prop.message, "Types of property 'a' are incompatible.");
    assert_eq!(prop.next.len(), 1);
    let leaf = &prop.next[0];
    assert_eq!(leaf.code, 2322);
    assert_eq!(
        leaf.message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// 4bn slice 2: a nested object-property mismatch collapses to a single dotted
// `2200` "The types of 'a.b' are incompatible between these types." over the
// leaf `2322` (Go's `reportError` `addToDottedName` transform).
// Go: internal/checker/relater.go:Relater.reportError (dotted-name collapse)
#[test]
fn assignability_chain_nested_property_collapses_to_dotted_message() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: { a: { b: string } };\nconst o: { a: { b: number } } = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type '{ a: { b: string; }; }' is not assignable to type '{ a: { b: number; }; }'."
    );
    // The two object levels collapse into one `2200` dotted `a.b` message.
    assert_eq!(d.message_chain.len(), 1);
    let dotted = &d.message_chain[0];
    assert_eq!(dotted.code, 2200);
    assert_eq!(
        dotted.message,
        "The types of 'a.b' are incompatible between these types."
    );
    assert_eq!(dotted.next.len(), 1);
    let leaf = &dotted.next[0];
    assert_eq!(leaf.code, 2322);
    assert_eq!(
        leaf.message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// 4bn slice 3: a single missing required target property surfaces as a `2741`
// head (Go's `reportRelationError` suppresses the `2322` head when the chain
// leads with `Property_0_is_missing_in_type_1_but_required_in_type_2` and the
// source/target names match).
// Go: internal/checker/relater.go:Relater.reportUnmatchedProperty + reportRelationError
#[test]
fn assignability_missing_required_property_reports_2741_head() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface S { a: number }\ninterface T { a: number; b: number }\ndeclare const s: S;\nvar t: T = s;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.code, 2741);
    assert_eq!(
        d.message,
        "Property 'b' is missing in type 'S' but required in type 'T'."
    );
    // The `2322` head is suppressed, so there is no nested chain.
    assert!(d.message_chain.is_empty());
}

// 4bn slice 3b: a nested missing required property keeps the outer `2326`
// "Types of property 'a' are incompatible." over the inner `2741` (the inner
// `2322` head is suppressed, so `2326` does NOT collapse to a dotted message).
// Go: internal/checker/relater.go:Relater.reportError (no collapse over 2741)
#[test]
fn assignability_chain_nested_missing_property_keeps_2326_over_2741() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: { a: { b: number } };\nconst o: { a: { b: number; c: number } } = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    let prop = &d.message_chain[0];
    assert_eq!(prop.code, 2326);
    assert_eq!(prop.message, "Types of property 'a' are incompatible.");
    assert_eq!(prop.next.len(), 1);
    let missing = &prop.next[0];
    assert_eq!(missing.code, 2741);
    assert_eq!(
        missing.message,
        "Property 'c' is missing in type '{ b: number; }' but required in type '{ b: number; c: number; }'."
    );
    assert!(missing.next.is_empty());
}

// 4bn slice 3c: several missing required properties surface as a single `2739`
// head "Type '{}' is missing the following properties from type 'T': a, b"
// (Go's `reportUnmatchedProperty` multi-property arm; the `2322` head is
// suppressed).
// Go: internal/checker/relater.go:Relater.reportUnmatchedProperty (2739)
#[test]
fn assignability_multiple_missing_properties_report_2739_head() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: {};\nconst o: { a: number; b: number } = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.code, 2739);
    assert_eq!(
        d.message,
        "Type '{}' is missing the following properties from type '{ a: number; b: number; }': a, b"
    );
    assert!(d.message_chain.is_empty());
}

// 4bn slice 4 (no regression): a flat primitive mismatch keeps a single chain-less
// `2322`; the reporting path adds no spurious elaboration.
// Go: internal/checker/relater.go:Relater.reportRelationError (leaf 2322 only)
#[test]
fn assignability_flat_primitive_mismatch_has_no_chain() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const n: number = \"x\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type 'string' is not assignable to type 'number'."
    );
    assert!(d.message_chain.is_empty());
}

// 4bn: the optional-source vs required-target property surfaces as `2327`
// "Property 'a' is optional in type 'S' but required in type 'T'." under the
// `2322` head. This 2327 arm is only reached in NON-strict mode: under
// strictNullChecks the optional source property `a?: string` reads as
// `string | undefined` (C-D1 `addOptionalityEx`), so the property-type
// comparison fails first and Go reports the "Types of property 'a' are
// incompatible" chain instead (verified against `cmd/tsgo`: strict ->
// property-incompatible chain; non-strict -> 2327). The test pins the 2327 arm,
// so it runs non-strict.
// Go: internal/checker/relater.go:Relater.propertyRelatedTo (2327 arm)
#[test]
fn assignability_chain_optional_source_required_target_reports_2327() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        strict_null_checks: Tristate::False,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "interface S { a?: string }\ninterface T { a: string }\ndeclare const s: S;\nvar t: T = s;",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message, "Type 'S' is not assignable to type 'T'.");
    assert_eq!(d.message_chain.len(), 1);
    let opt = &d.message_chain[0];
    assert_eq!(opt.code, 2327);
    assert_eq!(
        opt.message,
        "Property 'a' is optional in type 'S' but required in type 'T'."
    );
    assert!(opt.next.is_empty());
}

// 4bo slice 1: a fresh object-literal RHS with a wrong property type elaborates
// element-wise onto the offending property's name node (Go's `elaborateError` ->
// `elaborateObjectLiteral` -> `elaborateElement`), reporting `2322`
// "Type 'string' is not assignable to type 'number'." anchored at the literal's
// `a` (NOT the whole assignment / object), with a `6500` related-info "The
// expected type comes from property 'a' which is declared here on type
// '{ a: number; }'" pointing at the target property's declaration.
// Verified against `cmd/tsgo`: `a.ts(1,28): error TS2322 ...` + related at 1:12.
// Go: internal/checker/relater.go:Checker.elaborateObjectLiteral / elaborateElement
#[test]
fn elaborate_object_literal_wrong_property_type_points_at_property_name() {
    let src = "const o: { a: number } = { a: \"x\" };";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type 'string' is not assignable to type 'number'."
    );
    // The element-wise leaf carries no further chain.
    assert!(d.message_chain.is_empty());
    // The error is anchored at the literal's property name `a` (in the RHS),
    // not at the whole assignment / object literal.
    let eq = src.find('=').unwrap();
    let span = &src[d.start as usize..(d.start + d.length) as usize];
    assert_eq!(span.trim(), "a");
    assert!(d.start as usize > eq, "error should be in the RHS literal");
    // The `6500` related info points at the target property's declaration (the
    // `a` in the `{ a: number }` annotation, before the `=`).
    assert_eq!(d.related_information.len(), 1);
    let rel = &d.related_information[0];
    assert_eq!(rel.code, 6500);
    assert_eq!(
        rel.message,
        "The expected type comes from property 'a' which is declared here on type '{ a: number; }'"
    );
    let rel_span = &src[rel.start as usize..(rel.start + rel.length) as usize];
    assert_eq!(rel_span.trim(), "a");
    assert!(
        (rel.start as usize) < eq,
        "related info should point at the type annotation"
    );
}

// 4bo slice 2: a nested object-literal value recurses (Go's `elaborateElement`
// calls `elaborateError` on the value node), so the error lands on the
// innermost offending property `b` rather than the outer `a`. Before 4bo this
// took the 4bn generic chain (a dotted `2200` "a.b" hung on the whole
// assignment); after 4bo the diagnostic is anchored at the inner `b` with a
// `6500` "...property 'b'... on type '{ b: number; }'".
// Verified against `cmd/tsgo`: `a.ts(1,40): error TS2322 ...` + related at 1:17.
// Go: internal/checker/relater.go:Checker.elaborateElement (next != nil recursion)
#[test]
fn elaborate_nested_object_literal_points_at_innermost_property() {
    let src = "const o: { a: { b: number } } = { a: { b: \"x\" } };";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type 'string' is not assignable to type 'number'."
    );
    assert!(d.message_chain.is_empty());
    // The error anchors at the inner `b` (in the RHS literal), the innermost
    // offending element, not the outer `a` and not the whole assignment.
    let eq = src.find('=').unwrap();
    let span = &src[d.start as usize..(d.start + d.length) as usize];
    assert_eq!(span.trim(), "b");
    assert!(d.start as usize > eq, "error should be in the RHS literal");
    // The `6500` related info points at the inner target property `b`.
    assert_eq!(d.related_information.len(), 1);
    let rel = &d.related_information[0];
    assert_eq!(rel.code, 6500);
    assert_eq!(
        rel.message,
        "The expected type comes from property 'b' which is declared here on type '{ b: number; }'"
    );
    let rel_span = &src[rel.start as usize..(rel.start + rel.length) as usize];
    assert_eq!(rel_span.trim(), "b");
    assert!((rel.start as usize) < eq);
}

// 4bo slice 3: a fresh array-literal RHS elaborates element-wise (Go's
// `elaborateError` -> `elaborateArrayLiteral`): the literal is re-typed as a
// fixed-arity tuple `[number, string]`, and the offending element `"x"` (index
// 1) reports `2322` "Type 'string' is not assignable to type 'number'." anchored
// at that element. The matching element `1` (index 0) is silent. There is NO
// `6500` related info (the target `number[]` element type comes from an index
// signature, whose `6501` arm is DEFER).
// Verified against `cmd/tsgo`: `a.ts(1,26): error TS2322 ...` (no related info).
//
// `elaborate_array_literal` is driven directly here as an isolated unit of the
// 4bo elaboration machinery. As of 4bp the same scenario is ALSO reachable
// end-to-end through the var-decl path (the relation engine now rejects
// `Array<number | string>` against `Array<number>` via type-argument variance);
// see `array_literal_wrong_element_reports_2322_end_to_end` for that coverage.
// Go: internal/checker/relater.go:Checker.elaborateArrayLiteral / elaborateElement
#[test]
fn elaborate_array_literal_wrong_element_points_at_element() {
    let src = "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\nconst xs: number[] = [1, \"x\"];";
    let p = StubProgram::parse_and_bind("/a.ts", src);
    let (init, decl) = last_var_init_and_decl(&p);
    let mut c = Checker::new();
    let sym = p.symbol_of_node(decl).expect("symbol");
    let declared = crate::core::declared_types::get_type_of_symbol(&mut c, &p, sym, p.globals());
    let initializer_type = c.check_expression(&p, init);
    // Drive the element-wise elaboration directly (the bool fast path wrongly
    // accepts this assignment, so the var-decl wiring never gets here).
    let elaborated = c.elaborate_error(
        &p,
        init,
        initializer_type,
        declared,
        crate::RelationKind::Assignable,
    );
    assert!(
        elaborated,
        "the array literal should elaborate element-wise"
    );
    let diags = c
        .diagnostics_by_file
        .get(&p.file_handle())
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type 'string' is not assignable to type 'number'."
    );
    assert!(d.message_chain.is_empty());
    // The error anchors at the `"x"` element, not the matching `1`.
    let span = &src[d.start as usize..(d.start + d.length) as usize];
    assert_eq!(span.trim(), "\"x\"");
    // The array element type is an index-signature type, so Go emits no `6500`.
    assert!(d.related_information.is_empty());
}

// 4bo slice 4 (interplay): a fresh object literal with BOTH a wrong-type
// property and an excess property reports ONLY the element-wise `2322` on the
// wrong-type member; the `2353` excess message is suppressed. Go's
// `checkTypeRelatedToAndOptionallyElaborate` calls `elaborateError` before
// `checkTypeRelatedToEx`, so once an element reports, the relation (which is
// where excess checking lives in Go) never runs.
// Verified against `cmd/tsgo`: only `a.ts:1:28 error TS2322 ...` (no `2353`).
// Go: internal/checker/relater.go:Checker.checkTypeRelatedToAndOptionallyElaborate
#[test]
fn elaborate_object_literal_wrong_type_suppresses_excess_property() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: { a: number } = { a: \"x\", b: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(
        diags.len(),
        1,
        "expected only the element 2322, got {diags:?}"
    );
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// 4bo slice 4 (no regression): a fresh object literal MISSING a required target
// property is not flagged element-wise (the literal has no node for the absent
// member), so `elaborateError` reports nothing and the generic 4bn relation
// chain fires `2741` at the declaration. The fallback path is unchanged.
// Verified against `cmd/tsgo`: `a.ts:1:7 error TS2741: Property 'b' is missing
// ...`.
// Go: internal/checker/relater.go:Checker.elaborateObjectLiteral (no element) -> checkTypeRelatedToEx
#[test]
fn object_literal_missing_property_falls_back_to_2741_chain() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: { a: number; b: number } = { a: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2741);
    assert_eq!(
        d.message,
        "Property 'b' is missing in type '{ a: number; }' but required in type '{ a: number; b: number; }'."
    );
    // The generic-chain fallback has no element-anchored related info.
    assert!(d.related_information.is_empty());
}

// 4bo slice 4 (no regression): a NON-literal RHS (an identifier) is not an
// object/array literal, so `elaborateError` returns false immediately and the
// 4bn generic relation chain still reports at the declaration with its nested
// `2326` "Types of property 'a' are incompatible." chain. elaborateError does
// not hijack non-literal right-hand sides.
// Go: internal/checker/relater.go:Checker.elaborateError (no literal arm) -> checkTypeRelatedToEx
#[test]
fn non_literal_rhs_keeps_4bn_generic_chain() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { a: number }\ninterface B { a: string }\ndeclare const b: B;\nconst o: A = b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message, "Type 'B' is not assignable to type 'A'.");
    // The nested chain is preserved (not the element-wise leaf), and there is no
    // element-anchored related info.
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2326);
    assert!(d.related_information.is_empty());
}

// Returns the initializer node and variable-declaration node of the LAST
// top-level `const`/`var` declaration (used by elaboration fixtures to grab the
// literal RHS and its declaration directly).
fn last_var_init_and_decl(p: &StubProgram) -> (tsgo_ast::NodeId, tsgo_ast::NodeId) {
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let last = *stmts.last().unwrap();
    let list = match arena.data(last) {
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
    (init, decl)
}

// 4bo: `elaborate_error` unwraps a parenthesized expression and elaborates its
// inner literal (Go's `KindParenthesizedExpression` arm). `({ a: "x" })` against
// `{ a: number }` reports the element-wise `2322` on the inner `a`.
// Verified against `cmd/tsgo`: `a.ts:1:29 error TS2322 ...` + related at 1:12.
//
// Driven directly as an isolated unit of the `elaborateError` parenthesized
// unwrap arm. As of 4bp `check_expression` types a `ParenthesizedExpression` as
// its inner expression, so the var-decl path does reach `elaborateError` for a
// parenthesized literal RHS too (see `parenthesized_expression_takes_inner_type`
// for the end-to-end paren typing).
// Go: internal/checker/relater.go:Checker.elaborateError (parenthesized unwrap)
#[test]
fn elaborate_error_unwraps_parenthesized_object_literal() {
    let src = "const o: { a: number } = ({ a: \"x\" });";
    let p = StubProgram::parse_and_bind("/a.ts", src);
    let (paren, decl) = last_var_init_and_decl(&p);
    let inner = match p.arena().data(paren) {
        NodeData::ParenthesizedExpression(d) => d.expression,
        other => panic!("expected parenthesized expression, got {other:?}"),
    };
    let mut c = Checker::new();
    let sym = p.symbol_of_node(decl).expect("symbol");
    let declared = crate::core::declared_types::get_type_of_symbol(&mut c, &p, sym, p.globals());
    let source = c.check_expression(&p, inner);
    // Drive the unwrap arm: `elaborate_error(paren)` -> inner object literal.
    let reported = c.elaborate_error(&p, paren, source, declared, crate::RelationKind::Assignable);
    assert!(reported, "the parenthesized literal should elaborate");
    let diags = c
        .diagnostics_by_file
        .get(&p.file_handle())
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type 'string' is not assignable to type 'number'."
    );
    let eq = src.find('=').unwrap();
    assert!(
        d.start as usize > eq,
        "error in the parenthesized RHS literal"
    );
    assert_eq!(d.related_information.len(), 1);
    assert_eq!(d.related_information[0].code, 6500);
}

// 4bo: a simple-assignment RHS that is a fresh object literal elaborates
// element-wise at the assignment site too (Go wires
// `checkTypeAssignableToAndOptionallyElaborate(rightType, leftType, left,
// right, ...)` in `checkAssignmentOperator`). `x = { a: "y" }` reports `2322`
// on the literal's `a` with a `6500` related-info pointing at `x`'s declared
// property.
// Verified against `cmd/tsgo`: `a.ts:2:7 error TS2322 ...` + related at 1:10.
// Go: internal/checker/checker.go:Checker.checkAssignmentOperator(12701)
#[test]
fn elaborate_simple_assignment_object_literal_rhs() {
    let src = "let x: { a: number } = { a: 1 };\nx = { a: \"y\" };";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type 'string' is not assignable to type 'number'."
    );
    // The error is on the RHS literal's `a` (line 2); the related info points at
    // `x`'s declared property `a` (line 1).
    let nl = src.find('\n').unwrap();
    assert!(
        d.start as usize > nl,
        "error should be on the assignment RHS"
    );
    assert_eq!(d.related_information.len(), 1);
    let rel = &d.related_information[0];
    assert_eq!(rel.code, 6500);
    assert!((rel.start as usize) < nl, "related info on the declaration");
}

// 4bo unit: `elaborate_error` returns false for a non-object/array-literal
// expression (here an identifier), reporting nothing — the dispatch's default
// arm. This is what lets a non-literal RHS fall through to the generic chain.
// Go: internal/checker/relater.go:Checker.elaborateError (default: return false)
#[test]
fn elaborate_error_returns_false_for_non_literal_expression() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: string;\nx;");
    let ident = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let s = c.string_type();
    let n = c.number_type();
    let reported = c.elaborate_error(&p, ident, s, n, crate::RelationKind::Assignable);
    assert!(!reported, "a non-literal expression must not elaborate");
    let diags = c
        .diagnostics_by_file
        .get(&p.file_handle())
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    assert!(diags.is_empty(), "no diagnostic should be reported");
}

// 4bo unit: `elaborate_object_literal` returns false for a primitive (or
// `never`) target, since a primitive has no member structure to elaborate
// (Go's early `target.flags & (Primitive|Never)` guard). Driven directly with a
// `number` target so the var-decl path's structural check is bypassed.
// Go: internal/checker/relater.go:Checker.elaborateObjectLiteral (primitive guard)
#[test]
fn elaborate_object_literal_primitive_target_returns_false() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { a: 1 };");
    let (init, _) = last_var_init_and_decl(&p);
    let mut c = Checker::new();
    let source = c.check_expression(&p, init);
    let number = c.number_type();
    let reported = c.elaborate_error(&p, init, source, number, crate::RelationKind::Assignable);
    assert!(!reported, "a primitive target has nothing to elaborate");
    let diags = c
        .diagnostics_by_file
        .get(&p.file_handle())
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    assert!(diags.is_empty());
}

// 4bo unit: `is_tuple_like_type` recognizes a fixed-arity tuple (the `TUPLE`
// object-flag arm) and an object with a `"0"` member (the `getPropertyOfType(t,
// "0")` arm), and rejects a plain object with neither.
// Go: internal/checker/checker.go:Checker.isTupleLikeType(23405)
#[test]
fn is_tuple_like_type_recognizes_tuples_and_zero_indexed_objects() {
    let mut c = Checker::new();
    let n = c.number_type();
    let tuple = c.create_tuple_type_ex(vec![n], false);
    assert!(
        c.is_tuple_like_type(tuple),
        "a fixed-arity tuple is tuple-like"
    );
    let empty = c.new_object_type(ObjectFlags::ANONYMOUS, None, ObjectType::default());
    assert!(
        !c.is_tuple_like_type(empty),
        "a plain object with no '0' is not tuple-like"
    );
    // An object literal with a `0` member exposes a `"0"` property.
    let p = StubProgram::parse_and_bind("/a.ts", "const t = { 0: 1 };");
    let (init, _) = last_var_init_and_decl(&p);
    let zero_indexed = c.check_expression(&p, init);
    assert!(
        c.is_tuple_like_type(zero_indexed),
        "an object with a '0' member is tuple-like"
    );
}

// The synthetic `interface Array<T>` lib stand-in used by the array-reference
// relation tests below (mirrors the existing array tests' fixture).
const ARRAY_LIB: &str = "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\n";

// 4bp slice 1 (end-to-end, genuine RED -> GREEN): a fresh array literal whose
// element type is not assignable to the annotation's element type now reports
// `2322` on the offending element. Before 4bp the relation engine compared the
// two `Array<...>` references structurally (sharing the target's members) and
// wrongly accepted `Array<number | string>` against `Array<number>`, so the
// var-decl path never reached `elaborateError` and produced 0 diagnostics (the
// 4bo DEFER note). With type-argument variance the bool relation now rejects the
// assignment, so `elaborateArrayLiteral` fires end-to-end and the leaf `2322`
// lands on `"x"`.
// Verified against `cmd/tsgo`: `a.ts(1,26): error TS2322: Type 'string' is not
// assignable to type 'number'.` (no related info; the element type comes from an
// index signature).
// Go: internal/checker/relater.go:Checker.typeArgumentsRelatedTo / elaborateArrayLiteral
#[test]
fn array_literal_wrong_element_reports_2322_end_to_end() {
    let src = format!("{ARRAY_LIB}const xs: number[] = [1, \"x\"];");
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type 'string' is not assignable to type 'number'."
    );
    // The leaf carries no further chain and anchors at the `"x"` element.
    assert!(d.message_chain.is_empty());
    let span = &src[d.start as usize..(d.start + d.length) as usize];
    assert_eq!(span.trim(), "\"x\"");
    // The element type comes from an index signature, so Go emits no `6500`.
    assert!(d.related_information.is_empty());
}

// 4bp slice 1 (positive control): a fresh array literal whose elements are all
// assignable to the annotation's element type reports nothing.
// Verified against `cmd/tsgo`: `const xs: number[] = [1, 2];` -> no diagnostics.
// Go: internal/checker/relater.go:Checker.typeArgumentsRelatedTo (covariant ok)
#[test]
fn array_literal_matching_elements_reports_nothing() {
    let src = format!("{ARRAY_LIB}const xs: number[] = [1, 2];");
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(c.get_diagnostics(root).is_empty());
}

// 4bp slice 2 (covariance holds): `number[]` IS assignable to
// `(number | string)[]` (the element type `number` is assignable to
// `number | string`), so no diagnostic is reported.
// Verified against `cmd/tsgo`: `const a: number[] = [1]; const b:
// (number|string)[] = a;` -> no diagnostics.
// Go: internal/checker/relater.go:Checker.typeArgumentsRelatedTo (covariant)
#[test]
fn array_reference_covariance_is_assignable() {
    let src = format!("{ARRAY_LIB}const a: number[] = [1];\nconst b: (number | string)[] = a;");
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "number[] is assignable to (number | string)[]"
    );
}

// 4bp slice 2 (reverse fails, genuine RED -> GREEN): `(number | string)[]` is
// NOT assignable to `number[]`. The RHS is an identifier (not an array literal),
// so it flows through the generic relation chain (`report_type_not_assignable`),
// which now rejects the references via type-argument variance and produces the
// top-level `2322` with a nested type-argument elaboration. Before 4bp this was
// 0 diagnostics (the references were wrongly accepted structurally).
//
// Verified against `cmd/tsgo`: `a.ts(1,43): error TS2322: Type '(string |
// number)[]' is not assignable to type 'number[]'.` with a nested chain "Type
// 'string | number' is not assignable to type 'number'." -> "Type 'string' is
// not assignable to type 'number'.".
//
// Known print divergences (NOT relation bugs, both pre-existing / DEFER):
//   - the port prints an `Array<T>` reference as `Array<...>`, not the `T[]`
//     shorthand (the array-shorthand `typeToString` arm is unported), so the
//     head reads `Type 'Array<string | number>' is not assignable to type
//     'Array<number>'.`;
//   - the union-constituent leaf (`Type 'string' is not assignable to type
//     'number'.`) is the union elaboration deferred since 4bo, so the port's
//     chain has one nested entry where Go has two.
// The diagnostic CODE (2322), the offending span, and the union member ordering
// (`string | number`) match Go.
// Go: internal/checker/relater.go:Checker.typeArgumentsRelatedTo (reportErrors)
#[test]
fn array_reference_reverse_reports_2322() {
    let src = format!("{ARRAY_LIB}const c: (number | string)[] = [\"x\"];\nconst d: number[] = c;");
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type 'Array<string | number>' is not assignable to type 'Array<number>'."
    );
    // The type-argument failure hangs a nested elaboration under the head.
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2322);
    assert_eq!(
        d.message_chain[0].message,
        "Type 'string | number' is not assignable to type 'number'."
    );
    // Anchored at the `d` declaration (Go's col 43 == the `d` declaration name).
    let span = &src[d.start as usize..(d.start + d.length) as usize];
    assert!(span.trim_start().starts_with('d'), "span = {span:?}");
}

// 4bp slice 3 (same-target user generic, wrong property): a `Box<number>`
// annotation with an object-literal RHS whose `v` is a `string` reports `2322`
// on the offending `"s"` value (the reference's `v` member is instantiated to
// `number` through the type-argument mapper, and `string` is not assignable to
// it). The object-literal RHS elaborates element-wise (4bo).
// Verified against `cmd/tsgo`: `a.ts(1,52): error TS2322: Type 'string' is not
// assignable to type 'number'.`.
// Go: internal/checker/relater.go:Checker.elaborateObjectLiteral / getTypeOfSymbol (reference member)
#[test]
fn generic_reference_target_wrong_property_reports_2322() {
    let src = "interface Box<T> { v: T }\nconst x: Box<number> = { v: \"s\" };";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type 'string' is not assignable to type 'number'."
    );
    // 4bo object-literal elaboration anchors at the literal property NAME `v`
    // (Go's `a.ts(2,26)` is the `v` in the RHS `{ v: "s" }`), with a `6500`
    // related-info pointing at the target property's declaration.
    let span = &src[d.start as usize..(d.start + d.length) as usize];
    assert_eq!(span.trim(), "v");
    assert_eq!(d.related_information.len(), 1);
    assert_eq!(d.related_information[0].code, 6500);
    assert_eq!(
        d.related_information[0].message,
        "The expected type comes from property 'v' which is declared here on type 'Box<number>'"
    );
}

// 4bp slice 3 (same-target user generic, matching property, genuine RED ->
// GREEN): a `Box<number>` annotation with a matching `{ v: 1 }` reports nothing.
// Before 4bp this wrongly reported `2741 "Property 'T' is missing..."` because
// the generic interface's type parameter `T` leaked into the type's property
// list (Go excludes it via `getNamedMembers`/`symbolIsValue`), and the `v`
// member was compared against the un-instantiated `T`. After 4bp the type
// parameter is filtered from the member table and the `v` member is instantiated
// to `number`, so `{ v: 1 }` is assignable to `Box<number>`.
// Verified against `cmd/tsgo`: `const y: Box<number> = { v: 1 };` -> no
// diagnostics.
// Go: internal/checker/checker.go:Checker.symbolIsValue / getTypeOfSymbol (reference member)
#[test]
fn generic_reference_target_matching_property_reports_nothing() {
    let src = "interface Box<T> { v: T }\nconst y: Box<number> = { v: 1 };";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "{{ v: 1 }} is assignable to Box<number>"
    );
}

// 4bp slice 4 (parenthesized expression, genuine RED -> GREEN): `(expr)` is
// typed as its inner expression (Go's `checkParenthesizedExpression`). Before
// 4bp `check_expression` returned the `error` type for a `ParenthesizedExpression`
// (assignable to anything), so `const s: string = (1);` produced 0 diagnostics.
// After 4bp the parenthesized `(1)` is typed as `number`, so it is correctly
// rejected against `string`.
// Verified against `cmd/tsgo`: `const n: number = (1);` -> no diagnostics;
// `const s: string = (1);` -> `a.ts(1,7): error TS2322: Type 'number' is not
// assignable to type 'string'.`.
// Go: internal/checker/checker.go:Checker.checkParenthesizedExpression
#[test]
fn parenthesized_expression_takes_inner_type() {
    // Matching annotation: no diagnostic.
    let p_ok = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const n: number = (1);",
    ));
    let root_ok = p_ok.root();
    let mut c_ok = Checker::new_checker(p_ok);
    assert!(c_ok.get_diagnostics(root_ok).is_empty());
    // Mismatching annotation: `2322`.
    let p_bad = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const s: string = (1);",
    ));
    let root_bad = p_bad.root();
    let mut c_bad = Checker::new_checker(p_bad);
    let diags = c_bad.get_diagnostics(root_bad);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// ---- C-A: function-type signature relations + variance (end-to-end) ----

// C-A slice 1 (param mismatch, contravariant): assigning a `(x: string) => void`
// to a `(x: number) => void` fails because parameters relate contravariantly.
// The head `2322` carries a `2328` "Types of parameters 'x' and 'x' are
// incompatible." over the contravariant leaf `2322` "Type 'number' is not
// assignable to type 'string'." (target param `number` -> source param `string`).
// Verified against `cmd/tsgo`:
//   v_param.ts(2,5): error TS2322: Type '(x: string) => void' is not assignable
//   to type '(x: number) => void'.
//     Types of parameters 'x' and 'x' are incompatible.
//       Type 'number' is not assignable to type 'string'.
// Go: internal/checker/relater.go:Checker.compareSignaturesRelated (parameters)
#[test]
fn function_parameter_mismatch_reports_2328_chain() {
    let src = "declare let a: (x: string) => void;\nlet b: (x: number) => void = a;";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type '(x: string) => void' is not assignable to type '(x: number) => void'."
    );
    // 2328 parameter incompatibility over the contravariant leaf.
    assert_eq!(d.message_chain.len(), 1);
    let params = &d.message_chain[0];
    assert_eq!(params.code, 2328);
    assert_eq!(
        params.message,
        "Types of parameters 'x' and 'x' are incompatible."
    );
    assert_eq!(params.next.len(), 1);
    assert_eq!(params.next[0].code, 2322);
    assert_eq!(
        params.next[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
    // Anchored at the `b` declaration name.
    let span = &src[d.start as usize..(d.start + d.length) as usize];
    assert!(span.trim_start().starts_with('b'), "span = {span:?}");
}

// C-A slice 2 (return mismatch, covariant + marker elision): assigning a
// `() => string` to a `() => number` fails on the covariant return type. The
// "Call signatures with no arguments have incompatible return types ..." marker
// (2204) is `elidedInCompatibilityPyramid`, so the materialized chain collapses
// to the head `2322` over the inner return relation's own `2322` leaf.
// Verified against `cmd/tsgo`:
//   v_return.ts(2,5): error TS2322: Type '() => string' is not assignable to
//   type '() => number'.
//     Type 'string' is not assignable to type 'number'.
// Go: internal/checker/relater.go:Checker.compareSignaturesRelated (return marker)
//     + createDiagnosticChainFromErrorChain (elision)
#[test]
fn function_return_mismatch_reports_collapsed_chain() {
    let src = "declare let a: () => string;\nlet b: () => number = a;";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type '() => string' is not assignable to type '() => number'."
    );
    // The 2204 return-type marker is elided; the child is the return leaf.
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2322);
    assert_eq!(
        d.message_chain[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
    assert!(d.message_chain[0].next.is_empty());
}

// C-A slice 2 (void return accepts any): a `() => number` is assignable to a
// `() => void` because a `void` target return accepts any source return.
// Verified against `cmd/tsgo`: no diagnostics.
// Go: internal/checker/relater.go:Checker.compareSignaturesRelated (void return)
#[test]
fn function_void_return_accepts_any_source_return() {
    let src = "declare let a: () => number;\nlet b: () => void = a;";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "() => number is assignable to () => void"
    );
}

// C-A slice 3 (arity, too few target arguments): assigning a
// `(x: number, y: number) => void` to a `(x: number) => void` fails because the
// source requires more arguments than the target supplies. The head `2322`
// carries a `2849` "Target signature provides too few arguments. Expected 2 or
// more, but got 1." (not `elidedInCompatibilityPyramid`, so it is shown).
// Verified against `cmd/tsgo`:
//   v_arity_bad.ts(2,5): error TS2322: Type '(x: number, y: number) => void' is
//   not assignable to type '(x: number) => void'.
//     Target signature provides too few arguments. Expected 2 or more, but got 1.
// Go: internal/checker/relater.go:Checker.compareSignaturesRelated (arity)
#[test]
fn function_arity_too_few_target_args_reports_2849() {
    let src = "declare let a: (x: number, y: number) => void;\nlet b: (x: number) => void = a;";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type '(x: number, y: number) => void' is not assignable to type '(x: number) => void'."
    );
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2849);
    assert_eq!(
        d.message_chain[0].message,
        "Target signature provides too few arguments. Expected 2 or more, but got 1."
    );
}

// C-A slice 3 (arity, fewer source params is fine): a `(a: number) => void` is
// assignable to a `(a: number, b: number) => void` (callbacks may ignore
// trailing arguments). Verified against `cmd/tsgo`: no diagnostics.
// Go: internal/checker/relater.go:Checker.compareSignaturesRelated (arity)
#[test]
fn function_fewer_source_params_is_assignable() {
    let src = "declare let a: (a: number) => void;\nlet b: (a: number, b: number) => void = a;";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "fewer-param source is assignable"
    );
}

// C-A slice 4 (method parameters are bivariant): under the default options
// (strict on), a method-declared parameter relates BIVARIANTLY, so assigning
// `A { f(x: number): void }` to `B { f(x: number | string): void }` is allowed
// (the forward `number -> number | string` direction holds).
// Verified against `cmd/tsgo`: no diagnostics.
// Go: internal/checker/relater.go:Checker.compareSignaturesRelated (method bivariance)
#[test]
fn method_parameters_are_bivariant() {
    let src = "interface A { f(x: number): void }\ninterface B { f(x: number | string): void }\ndeclare let a: A;\nlet b: B = a;";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "method params are bivariant, so A is assignable to B"
    );
}

// C-A slice 4 (arrow-property parameters are strictly contravariant): the same
// shapes as `method_parameters_are_bivariant`, but with the member declared as
// an arrow-typed PROPERTY, relate strictly contravariantly under
// `strictFunctionTypes`, so the assignment fails.
// Verified against `cmd/tsgo`:
//   m_prop.ts(4,5): error TS2322: Type 'A2' is not assignable to type 'B2'.
//     Types of property 'f' are incompatible.
//       Type '(x: number) => void' is not assignable to type '(x: string | number) => void'.
//         Types of parameters 'x' and 'x' are incompatible.
//           Type 'string | number' is not assignable to type 'number'.
//             Type 'string' is not assignable to type 'number'.   (union elaboration DEFER)
// Go: internal/checker/relater.go:Checker.compareSignaturesRelated (strict contravariance)
#[test]
fn arrow_property_parameters_are_contravariant() {
    let src = "interface A2 { f: (x: number) => void }\ninterface B2 { f: (x: number | string) => void }\ndeclare let a: A2;\nlet b: B2 = a;";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message, "Type 'A2' is not assignable to type 'B2'.");
    // 2326 (property 'f') -> 2322 (function type) -> 2328 (parameters) -> leaf.
    assert_eq!(d.message_chain.len(), 1);
    let prop = &d.message_chain[0];
    assert_eq!(prop.code, 2326);
    assert_eq!(prop.message, "Types of property 'f' are incompatible.");
    assert_eq!(prop.next.len(), 1);
    let func = &prop.next[0];
    assert_eq!(func.code, 2322);
    assert_eq!(
        func.message,
        "Type '(x: number) => void' is not assignable to type '(x: string | number) => void'."
    );
    assert_eq!(func.next.len(), 1);
    let params = &func.next[0];
    assert_eq!(params.code, 2328);
    assert_eq!(
        params.message,
        "Types of parameters 'x' and 'x' are incompatible."
    );
    // The leaf is the contravariant `target(string | number) -> source(number)`
    // relation. The deeper per-constituent union leaf is the 4bo/4bp union
    // elaboration DEFER, so the port stops one level shy of Go here.
    assert_eq!(params.next.len(), 1);
    assert_eq!(params.next[0].code, 2322);
    assert_eq!(
        params.next[0].message,
        "Type 'string | number' is not assignable to type 'number'."
    );
}

// C-A slice 4 (strictFunctionTypes off -> bivariant property params): with
// `--strictFunctionTypes false`, even an arrow-typed PROPERTY relates its
// parameters bivariantly, so the `arrow_property_parameters_are_contravariant`
// shapes become assignable.
// Verified against `cmd/tsgo --strict --strictFunctionTypes false`: no diagnostics.
// Go: internal/checker/relater.go:Checker.compareSignaturesRelated (strictVariance off)
#[test]
fn arrow_property_parameters_are_bivariant_without_strict_function_types() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let src = "interface A2 { f: (x: number) => void }\ninterface B2 { f: (x: number | string) => void }\ndeclare let a: A2;\nlet b: B2 = a;";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        src,
        CompilerOptions {
            strict: Tristate::True,
            strict_function_types: Tristate::False,
            ..CompilerOptions::default()
        },
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "without strictFunctionTypes, arrow-property params are bivariant"
    );
}

// C-A slice 5 (construct signature, matching return): a `new () => Base` is
// assignable to a `new () => Base`. Verified against `cmd/tsgo`: no diagnostics.
// Go: internal/checker/relater.go:Relater.signaturesRelatedTo (SignatureKindConstruct)
#[test]
fn construct_signature_matching_return_reports_nothing() {
    let src = "class Base {}\ndeclare let c: new () => Base;\nlet d: new () => Base = c;";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "new () => Base is assignable to new () => Base"
    );
}

// C-A slice 5 (construct signature, incompatible return): a `new () => Other` is
// not assignable to a `new () => Base` because the constructed `Other` lacks the
// required `x` of `Base`. The construct-signature return-type marker (2205) is
// elided, so the head `2322` carries the inner missing-property `2741`.
// Verified against `cmd/tsgo`:
//   ctor_bad.ts(4,5): error TS2322: Type 'new () => Other' is not assignable to
//   type 'new () => Base'.
//     Property 'x' is missing in type 'Other' but required in type 'Base'.
// Go: internal/checker/relater.go:Relater.signaturesRelatedTo (SignatureKindConstruct)
#[test]
fn construct_signature_incompatible_return_reports_2322() {
    let src = "class Base { x: number = 1; }\nclass Other {}\ndeclare let c: new () => Other;\nlet d: new () => Base = c;";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(
        d.message,
        "Type 'new () => Other' is not assignable to type 'new () => Base'."
    );
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2741);
    assert_eq!(
        d.message_chain[0].message,
        "Property 'x' is missing in type 'Other' but required in type 'Base'."
    );
}

// C-A slice 6 (variance + signatures compose, non-regression): an
// `Array<(x: number) => void>` is NOT assignable to an
// `Array<(x: string) => void>` because the (covariant) element relation relates
// the element function types, whose parameters are contravariant
// (`target(string)` -> `source(number)` fails). This exercises 4bp's covariant
// type-argument variance feeding into the new signature relation.
// Verified against `cmd/tsgo`:
//   combo.ts(2,5): error TS2322: Type '((x: number) => void)[]' is not
//   assignable to type '((x: string) => void)[]'.
//     Type '(x: number) => void' is not assignable to type '(x: string) => void'.
//       Types of parameters 'x' and 'x' are incompatible.
//         Type 'string' is not assignable to type 'number'.
// (The head prints the element array as `Array<...>` rather than `(...)[]` — the
// 4bp-documented array-shorthand printing divergence; CODE/structure/leaf match.)
// Go: internal/checker/relater.go:typeArgumentsRelatedTo + compareSignaturesRelated
#[test]
fn array_of_function_types_relates_by_element_signature() {
    let src =
        format!("{ARRAY_LIB}declare let a: Array<(x: number) => void>;\nlet b: Array<(x: string) => void> = a;");
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    // Element function-type mismatch nested under the array head.
    assert_eq!(d.message_chain.len(), 1);
    let elem = &d.message_chain[0];
    assert_eq!(elem.code, 2322);
    assert_eq!(
        elem.message,
        "Type '(x: number) => void' is not assignable to type '(x: string) => void'."
    );
    assert_eq!(elem.next.len(), 1);
    let params = &elem.next[0];
    assert_eq!(params.code, 2328);
    assert_eq!(
        params.message,
        "Types of parameters 'x' and 'x' are incompatible."
    );
    assert_eq!(params.next.len(), 1);
    assert_eq!(params.next[0].code, 2322);
    assert_eq!(
        params.next[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// C-A slice 6 (variance + signatures compose, positive): an
// `Array<(x: number | string) => void>` IS assignable to an
// `Array<(x: number) => void>` because the element function types' parameters
// relate contravariantly (`target(number)` -> `source(number | string)` holds).
// Verified against `cmd/tsgo`: no diagnostics.
// Go: internal/checker/relater.go:typeArgumentsRelatedTo + compareSignaturesRelated
#[test]
fn array_of_function_types_contravariant_element_is_assignable() {
    let src = format!(
        "{ARRAY_LIB}declare let a: Array<(x: number | string) => void>;\nlet b: Array<(x: number) => void> = a;"
    );
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "contravariant element params make the array assignable"
    );
}

// ===========================================================================
// C-B1: generics foundation — explicit type arguments + constraints +
// defaults + arity + instantiation.
// ===========================================================================

// C-B1 slice 1 (end-to-end): a generic function called with an explicit type
// argument instantiates its signature so the result type is the substituted
// type. `id<number>(1)` yields `number`, so `const s: string = r` reports 2322.
// Verified against `cmd/tsgo`: `t.ts(3,7): error TS2322: Type 'number' is not
// assignable to type 'string'.`
// Go: internal/checker/checker.go:getSignatureInstantiation / instantiateSignature
#[test]
fn generic_function_explicit_type_argument_instantiates_return() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function id<T>(x: T): T { return x; }\nconst r = id<number>(1);\nconst s: string = r;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// C-B1 slice 3 (constraint, call site): an explicit type argument that does not
// satisfy its `extends` constraint reports 2344 on the type-argument node, and
// the call aborts (no follow-on 2345). `f<string>("a")` with
// `f<T extends number>` reports exactly one 2344.
// Verified against `cmd/tsgo`: `t.ts(2,3): error TS2344: Type 'string' does not
// satisfy the constraint 'number'.`
// Go: internal/checker/checker.go:Checker.checkTypeArguments
#[test]
fn explicit_type_argument_violates_constraint_reports_2344() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T extends number>(x: T) {}\nf<string>(\"a\");",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2344, got {diags:?}");
    assert_eq!(diags[0].code, 2344);
    assert_eq!(
        diags[0].message,
        "Type 'string' does not satisfy the constraint 'number'."
    );
}

// C-B1 slice 3 (constraint, call site): a failing constraint suppresses the
// argument-assignability error — `f<string>(1)` reports only the 2344, not a
// 2345 for `1` vs the instantiated `string` parameter.
// Verified against `cmd/tsgo`: a single `TS2344`.
// Go: internal/checker/checker.go:Checker.checkTypeArguments (returns nil)
#[test]
fn failing_constraint_suppresses_argument_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T extends number>(x: T) {}\nf<string>(1);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "only the 2344, no 2345: {diags:?}");
    assert_eq!(diags[0].code, 2344);
}

// C-B1 slice 3 (constraint satisfied, call site): an explicit type argument that
// satisfies its constraint produces no diagnostics. `f<1>(1)` with
// `f<T extends number>` is accepted.
// Verified against `cmd/tsgo`: no diagnostics.
// Go: internal/checker/checker.go:Checker.checkTypeArguments
#[test]
fn explicit_type_argument_satisfies_constraint_ok() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T extends number>(x: T) {}\nf<1>(1);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "1 satisfies `extends number`"
    );
}

// C-B1 slice 3 (constraint, type-reference site): a type-alias instantiation
// whose explicit argument violates the alias type parameter's constraint reports
// 2344 on the argument node. `type G<T extends number> = T; type X = G<string>;`
// reports one 2344.
// Verified against `cmd/tsgo`: `t.ts(2,12): error TS2344: Type 'string' does not
// satisfy the constraint 'number'.`
// Go: internal/checker/checker.go:Checker.checkTypeArgumentConstraints
#[test]
fn type_alias_reference_violates_constraint_reports_2344() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type G<T extends number> = T;\ntype X = G<string>;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2344, got {diags:?}");
    assert_eq!(diags[0].code, 2344);
    assert_eq!(
        diags[0].message,
        "Type 'string' does not satisfy the constraint 'number'."
    );
}

// C-B1 slice 3 (constraint satisfied, type-reference site): `G<1>` satisfies
// `T extends number`, so no diagnostics.
// Verified against `cmd/tsgo`: no diagnostics.
#[test]
fn type_alias_reference_satisfies_constraint_ok() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type G<T extends number> = T;\ntype X = G<1>;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "1 satisfies the constraint"
    );
}

// C-B1 slice 4 (default applied): an omitted type argument uses the type
// parameter's default. `interface C<T = number> { v: T }` referenced bare as `C`
// resolves `v` to `number`, so `const w: string = c.v` reports 2322.
// Verified against `cmd/tsgo`: `t.ts(3,7): error TS2322: Type 'number' is not
// assignable to type 'string'.`
// Go: internal/checker/checker.go:Checker.fillMissingTypeArguments
#[test]
fn default_type_argument_applied_to_member() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface C<T = number> { v: T }\nconst c: C = { v: 1 };\nconst w: string = c.v;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// C-B1 slice 4 (default overridden): an explicit type argument overrides the
// default. `C<string>` makes `v` `string`, so `const w: number = c2.v` reports
// 2322 ('string' not assignable to 'number').
// Verified against `cmd/tsgo`: `t.ts(3,7): error TS2322: Type 'string' is not
// assignable to type 'number'.`
#[test]
fn default_type_argument_overridden_by_explicit() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface C<T = number> { v: T }\nconst c2: C<string> = { v: \"x\" };\nconst w: number = c2.v;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// C-B1 slice 5 (arity, too many on interface): `Box<number, string>` reports
// 2314 with the interface name printed as `Box<T>`.
// Verified against `cmd/tsgo`: `t.ts(2,10): error TS2314: Generic type 'Box<T>'
// requires 1 type argument(s).`
// Go: internal/checker/checker.go:Checker.getTypeFromClassOrInterfaceReference
#[test]
fn interface_too_many_type_arguments_reports_2314() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Box<T> { v: T }\ntype X = Box<number, string>;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2314, got {diags:?}");
    assert_eq!(diags[0].code, 2314);
    assert_eq!(
        diags[0].message,
        "Generic type 'Box<T>' requires 1 type argument(s)."
    );
}

// C-B1 slice 5 (arity, too few on interface): `Pair<number>` reports 2314 with
// the interface name `Pair<A, B>` and the required count 2.
// Verified against `cmd/tsgo`: `t.ts(2,10): error TS2314: Generic type
// 'Pair<A, B>' requires 2 type argument(s).`
#[test]
fn interface_too_few_type_arguments_reports_2314() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Pair<A, B> { a: A; b: B }\ntype X = Pair<number>;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2314, got {diags:?}");
    assert_eq!(diags[0].code, 2314);
    assert_eq!(
        diags[0].message,
        "Generic type 'Pair<A, B>' requires 2 type argument(s)."
    );
}

// C-B1 slice 5 (arity, alias): a type alias prints as just its name `G`.
// `type G<T> = T; type X = G<number, string>;` reports 2314 'Generic type 'G'...'.
// Verified against `cmd/tsgo`: `t.ts(2,10): error TS2314: Generic type 'G'
// requires 1 type argument(s).`
// Go: internal/checker/checker.go:Checker.getTypeFromTypeAliasReference
#[test]
fn type_alias_too_many_type_arguments_reports_2314() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type G<T> = T;\ntype X = G<number, string>;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2314, got {diags:?}");
    assert_eq!(diags[0].code, 2314);
    assert_eq!(
        diags[0].message,
        "Generic type 'G' requires 1 type argument(s)."
    );
}

// C-B1 slice 5 (arity range, 2707): when defaults make the count a range, too
// many arguments report 2707. `C<T = number, U = string>` with three arguments
// reports 'requires between 0 and 2'.
// Verified against `cmd/tsgo`: `t.ts(2,10): error TS2707: Generic type 'C<T, U>'
// requires between 0 and 2 type arguments.`
// Go: internal/checker/checker.go:Checker.getTypeFromClassOrInterfaceReference
#[test]
fn interface_type_arguments_out_of_range_reports_2707() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface C<T = number, U = string> { a: T; b: U }\ntype X = C<number, string, boolean>;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2707, got {diags:?}");
    assert_eq!(diags[0].code, 2707);
    assert_eq!(
        diags[0].message,
        "Generic type 'C<T, U>' requires between 0 and 2 type arguments."
    );
}

// C-B1 slice 5 (arity, call site, 2558): a generic function called with too many
// type arguments reports 2558. `id<number, string>(1)` reports 'Expected 1 type
// arguments, but got 2.'
// Verified against `cmd/tsgo`: `t.ts(2,14): error TS2558: Expected 1 type
// arguments, but got 2.`
// Go: internal/checker/checker.go:Checker.getTypeArgumentArityError
#[test]
fn generic_call_too_many_type_arguments_reports_2558() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function id<T>(x: T): T { return x; }\nconst r = id<number, string>(1);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2558, got {diags:?}");
    assert_eq!(diags[0].code, 2558);
    assert_eq!(diags[0].message, "Expected 1 type arguments, but got 2.");
}

// C-B1 slice 5 (grammar, 2706): a required type parameter following an optional
// (defaulted) one reports 2706 on the required parameter.
// Verified against `cmd/tsgo`: `t.ts(1,25): error TS2706: Required type
// parameters may not follow optional type parameters.`
// Go: internal/checker/checker.go:Checker.checkTypeParameters
#[test]
fn required_type_parameter_after_optional_reports_2706() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface C<T = number, U> { a: T; b: U }\ntype X = C<number, string>;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    // Exactly the 2706 grammar error (the `C<number, string>` reference is then
    // within the [min=2, max=2] range, so no arity error).
    assert!(
        diags.iter().any(|d| d.code == 2706),
        "expected a 2706, got {diags:?}"
    );
    let d = diags.iter().find(|d| d.code == 2706).unwrap();
    assert_eq!(
        d.message,
        "Required type parameters may not follow optional type parameters."
    );
}

// C-B1 slice 6 (composition, nested reference type argument): a generic function
// instantiated with a generic type argument substitutes through the reference.
// `id<Box<number>>(bn)` yields `Box<number>`, so `r.v` is `number` and
// `const bad: string = r.v` reports 2322.
// Verified against `cmd/tsgo`: `t.ts(5,7): error TS2322: Type 'number' is not
// assignable to type 'string'.`
// Go: internal/checker/checker.go:Checker.instantiateType (type reference args)
#[test]
fn generic_call_with_nested_reference_type_argument() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Box<T> { v: T }\nfunction id<T>(x: T): T { return x; }\ndeclare const bn: Box<number>;\nconst r = id<Box<number>>(bn);\nconst bad: string = r.v;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// C-B1 slice 6 (composition, return reference instantiation): a generic function
// whose return type is a generic reference instantiates the reference's argument.
// `wrap<number>(1)` returns `Box<number>`, so `r.v` is `number` and
// `const bad: string = r.v` reports 2322.
// Verified against `cmd/tsgo`: `t.ts(4,7): error TS2322: Type 'number' is not
// assignable to type 'string'.`
// Go: internal/checker/checker.go:Checker.instantiateSignature (return type)
#[test]
fn generic_function_returning_reference_instantiates_argument() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Box<T> { v: T }\nfunction wrap<T>(x: T): Box<T> { return { v: x }; }\nconst r = wrap<number>(1);\nconst bad: string = r.v;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// C-B1 slice 2 (end-to-end): a generic interface instantiation reads its member
// type through the reference's `T -> arg` mapper. `b: Box<number>` exposes `v`
// as `number`, so `const n: string = b.v` reports 2322. (Builds on 4bp's member
// instantiation; here the member TYPE is read back through a property access.)
// Verified against `cmd/tsgo`: `t.ts(3,7): error TS2322: Type 'number' is not
// assignable to type 'string'.`
// Go: internal/checker/checker.go:Checker.getTypeOfPropertyOfType (instantiated)
#[test]
fn generic_interface_member_reads_instantiated_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Box<T> { v: T }\nconst b: Box<number> = { v: 1 };\nconst n: string = b.v;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// C-B1 slice 2 (positive): assigning `b.v` to its real `number` type holds.
// Verified against `cmd/tsgo`: no diagnostics.
#[test]
fn generic_interface_member_assignable_to_match() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Box<T> { v: T }\nconst b: Box<number> = { v: 1 };\nconst n: number = b.v;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "b.v is number through Box<number>"
    );
}

// C-B1 slice 1 (positive): the same call assigned to the matching type is
// accepted — `const n: number = r` produces no diagnostics, confirming `r` is
// genuinely `number` (not `any`/`unknown`/error).
// Verified against `cmd/tsgo`: no diagnostics.
#[test]
fn generic_function_explicit_type_argument_assignable_to_match() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function id<T>(x: T): T { return x; }\nconst r = id<number>(1);\nconst n: number = r;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "id<number>(1) is number; assignment to number holds"
    );
}

// ===========================================================================
// C-B2: type-argument INFERENCE from call arguments (no explicit type args).
// ===========================================================================

// C-B2 slice 1: a generic call WITHOUT explicit type arguments infers the type
// parameter from the argument. `id(1)` infers `T = 1` (a fresh literal that
// widens to `number` in the assignability message), so `const s: string = r`
// reports 2322. Before C-B2 the call inferred `unknown` (empty sources), which
// produced a `Type 'unknown' is not assignable...` 2322 instead.
// Verified against `cmd/tsgo`: `s1.ts(3,7): error TS2322: Type 'number' is not
// assignable to type 'string'.`
// Go: internal/checker/checker.go:Checker.inferTypeArguments + getSignatureInstantiation
#[test]
fn generic_call_infers_type_argument_from_argument() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function id<T>(x: T): T { return x; }\nconst r = id(1);\nconst s: string = r;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// C-B2 slice 2 (cross-base candidates): two arguments matched to the same type
// parameter accumulate two candidates. When the candidates are literals of
// DIFFERENT base types, `getCommonSupertype` returns the leftmost (no later
// candidate is a supertype), so `pick(1, "x")` infers `T = 1` and the second
// argument `"x"` is then not assignable to the instantiated parameter `1`
// (2345). NOTE: this is the REAL `cmd/tsgo` behavior — it does NOT infer a
// `string | number` union (the union only forms for same-base literals; see the
// next test). A literal target suppresses literal generalization, so the source
// prints as `"x"`, not `string`.
// Verified against `cmd/tsgo`: `s2b.ts(2,19): error TS2345: Argument of type
// '"x"' is not assignable to parameter of type '1'.`
// Go: internal/checker/inference.go:getCommonSupertype/getSingleCommonSupertype
#[test]
fn generic_call_cross_base_candidates_infer_leftmost() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function pick<T>(a: T, b: T): T { return a; }\nconst r = pick(1, \"x\");",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2345, got {diags:?}");
    assert_eq!(diags[0].code, 2345);
    assert_eq!(
        diags[0].message,
        "Argument of type '\"x\"' is not assignable to parameter of type '1'."
    );
}

// C-B2 slice 2 (same-base candidates): when both candidates are literals of the
// SAME base type, `getCommonSupertype` returns their union, so `pick(1, 2)`
// infers `T = 1 | 2`. Assigning that to the literal `1` reports 2322 (`2` is not
// assignable to `1`), confirming the inferred type is the union `1 | 2` (not the
// widened `number`, and not the leftmost `1`).
// Verified against `cmd/tsgo`: `probe.ts(2,7): error TS2322: Type '1 | 2' is not
// assignable to type '1'.`
// Go: internal/checker/inference.go:getCommonSupertype (literalTypesWithSameBaseType)
#[test]
fn generic_call_same_base_candidates_infer_union() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function pick<T>(a: T, b: T): T { return a; }\nconst n: 1 = pick(1, 2);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '1 | 2' is not assignable to type '1'."
    );
}

// C-B2 slice 2 (same-base union accepted): `pick(1, 2)` infers `T = 1 | 2`, which
// IS assignable to `number`, so `const n: number = pick(1, 2)` is accepted.
// Verified against `cmd/tsgo`: no diagnostics.
// Go: internal/checker/inference.go:getCommonSupertype
#[test]
fn generic_call_same_base_union_assignable_to_base() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function pick<T>(a: T, b: T): T { return a; }\nconst n: number = pick(1, 2);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "1 | 2 is assignable to number"
    );
}

// C-B2 slice 4 (inference through an object property): `f<T>(o: { v: T })` called
// with `{ v: 1 }` infers `T` from the matching member's type (object-wise
// inference: `inferFromObjectTypes`/`inferFromProperties`). The result `r` is
// then a `number`-based type, so `const s: string = r` reports 2322.
// Verified against `cmd/tsgo`: `s4.ts(3,7): error TS2322: Type 'number' is not
// assignable to type 'string'.`
// Go: internal/checker/inference.go:Checker.inferFromObjectTypes/inferFromProperties
#[test]
fn generic_call_infers_through_object_property() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T>(o: { v: T }): T { return o.v; }\nconst r = f({ v: 1 });\nconst s: string = r;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// C-B2 slice 4 (positive): the same call assigned to the matching type is
// accepted, confirming `T` was inferred (not left as `unknown`/error).
// Verified against `cmd/tsgo`: no diagnostics.
// Go: internal/checker/inference.go:Checker.inferFromObjectTypes
#[test]
fn generic_call_infers_through_object_property_assignable() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T>(o: { v: T }): T { return o.v; }\nconst r = f({ v: 1 });\nconst n: number = r;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "f of an object literal infers T from the property; r is number"
    );
}

// C-B2 slice 5 (constraint satisfied): `f<T extends number>(x: T)` called with a
// `number` literal infers `T = 1`, which satisfies the constraint, so `f(1)` is
// accepted (no diagnostics).
// Verified against `cmd/tsgo`: no diagnostics.
// Go: internal/checker/inference.go:Checker.getInferredType (constraint branch)
#[test]
fn generic_call_inference_satisfies_constraint() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T extends number>(x: T): T { return x; }\nf(1);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "1 satisfies `T extends number`"
    );
}

// C-B2 slice 5 (constraint violated): when the covariantly-inferred type violates
// the type parameter's constraint, Go infers the CONSTRAINT instead. So
// `f<T extends number>(x: T)` called with `"x"` infers `T = number` (not the
// `string`-based candidate), and the argument `"x"` is then not assignable to the
// instantiated parameter `number` (2345, with the source generalized to
// `string`).
// Verified against `cmd/tsgo`: `s5b.ts(2,3): error TS2345: Argument of type
// 'string' is not assignable to parameter of type 'number'.`
// Go: internal/checker/inference.go:Checker.getInferredType (constraint branch)
#[test]
fn generic_call_inference_violates_constraint_infers_constraint() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T extends number>(x: T): T { return x; }\nf(\"x\");",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2345, got {diags:?}");
    assert_eq!(diags[0].code, 2345);
    assert_eq!(
        diags[0].message,
        "Argument of type 'string' is not assignable to parameter of type 'number'."
    );
}

// C-B2 slice 6 (no-candidate fallback): a generic call from which no inferences
// can be made falls back to `unknown` (Go's `getInferredType` with no candidates
// and no default, `InferenceFlagsNone`). `f<T>(): T` called as `f()` infers
// `T = unknown`, so the bare call alone is not an error.
// Verified against `cmd/tsgo`: no diagnostics.
// Go: internal/checker/inference.go:Checker.getInferredType (no candidates)
#[test]
fn generic_call_no_candidate_infers_unknown() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T>(): T { return undefined as any; }\nconst r = f();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "f() with no candidate infers unknown; the bare call is not an error"
    );
}

// C-B2 slice 6 (no-candidate reveal): `f()` infers `T = unknown`, so a property
// access on the result diagnoses against the type `unknown`, confirming the
// result type is `unknown` (not `any`/error). NOTE: this is the
// no-contextual-type path; an annotated context (`const n: number = f()`) would
// in Go infer `T` from the contextual return type — that contextual-return
// inference is DEFERRED to C-B3.
//
// DIVERGENCE(port): `cmd/tsgo` reports 2571 `Object is of type 'unknown'.` for
// any property access on `unknown` (it checks the object type first); this port
// reaches the property lookup and reports 2339 `Property '0' does not exist on
// type 'unknown'.`. That is a pre-existing property-access-on-`unknown`
// difference outside C-B2's inference scope; either way the diagnosed type is
// `unknown`, which is what this slice asserts.
// Go: internal/checker/inference.go:Checker.getInferredType (no candidates)
#[test]
fn generic_call_no_candidate_result_is_unknown() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T>(): T { return undefined as any; }\nf().toFixed();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert!(
        diags[0].message.contains("type 'unknown'"),
        "the result of `f()` is diagnosed as `unknown`: {:?}",
        diags[0].message
    );
}

// C-B2 slice 7 (explicit type arguments still win): when a generic call supplies
// explicit type arguments, the C-B1 explicit path takes precedence over
// inference — the type parameters are bound to the EXPLICIT arguments, not
// inferred from the call arguments. `id<string>(1)` binds `T = string`, so the
// argument `1` is not assignable to the instantiated parameter `string` (2345),
// exactly as before C-B2 (no inference for this call).
// Verified against `cmd/tsgo`: `s7.ts(2,12): error TS2345: Argument of type
// 'number' is not assignable to parameter of type 'string'.`
// Go: internal/checker/checker.go:Checker.resolveCall (typeArguments branch)
#[test]
fn generic_call_explicit_type_argument_wins_over_inference() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function id<T>(x: T): T { return x; }\nid<string>(1);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2345, got {diags:?}");
    assert_eq!(diags[0].code, 2345);
    assert_eq!(
        diags[0].message,
        "Argument of type 'number' is not assignable to parameter of type 'string'."
    );
}

// C-B2 slice 3 (callback inference — the keystone): `map<T, U>(a: T[], f: (x: T)
// => U)` called as `map([1, 2], (x) => ...)` infers `T = number` from the FIRST
// (non-context-sensitive) argument, then contextually types the (deferred)
// callback argument by the INSTANTIATED parameter type `(x: number) => U`, so the
// callback's un-annotated parameter `x` is `number`. The concise body `x`
// checked against the explicit return annotation `string` then reports 2322.
//
// This exercises the two-pass: array-argument inference fixes `T`, the resolved
// (instantiated) signature is memoized on the call node, and the callback's
// contextual signature is read from it (`(x: T) => U` deep-instantiated to
// `(x: number) => U`). Inferring `U` from the callback's return type (so the
// result is `string[]`) is DEFERRED to C-B3.
// Verified against `cmd/tsgo`: `(3,28): error TS2322: Type 'number' is not
// assignable to type 'string'.` (the synthetic `Array` lib stand-in clashes with
// cmd/tsgo's real lib, adding 2374s there; this port loads no real lib).
// Go: internal/checker/checker.go:Checker.inferTypeArguments (context-sensitive args)
#[test]
fn generic_call_callback_parameter_inferred_from_array_argument() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\nfunction map<T, U>(a: T[], f: (x: T) => U): U[] { return [] as any; }\nmap([1, 2], (x): string => x);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// C-B2 slice 3 (positive): the same callback whose explicit return annotation
// MATCHES the inferred parameter type produces no diagnostics, confirming `x` is
// genuinely `number` (so `(x): number => x` type-checks).
// Verified against `cmd/tsgo`: no relevant diagnostics (only the synthetic-`Array`
// lib-clash 2374s, absent in this port).
// Go: internal/checker/checker.go:Checker.inferTypeArguments
#[test]
fn generic_call_callback_parameter_inferred_accepts_matching_return() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\nfunction map<T, U>(a: T[], f: (x: T) => U): U[] { return [] as any; }\nmap([1, 2], (x): number => x);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "x is inferred as number, so (x): number => x type-checks: {:?}",
        c.get_diagnostics(root)
    );
}

// ===========================================================================
// C-B3: generic call resolution completion — callback RESULT inference
// (`inferFromSignatures`), contextual-return inference, the lazy inference
// mapper, and instantiation caching.
// ===========================================================================

// C-B3 slice 1 (callback RESULT inference — the headline): `map<T, U>(a: T[],
// f: (x: T) => U): U[]` called as `map([1, 2], x => x + "")` infers `T = number`
// from the array argument, contextually types the callback parameter `x` as the
// fixed `number`, then infers `U` from the callback's BODY return type. The body
// `x + ""` types to `string` (string concatenation, using the contextually-typed
// `x: number`), so `U = string` and the call result is `string[]`. Assigning
// that to `number` reports 2322 with the result element type `string` (NOT
// `unknown`, the C-B2 behavior before callback-return inference).
//
// This exercises the keystone: the second inference pass over context-sensitive
// arguments (`inferTypeArguments`), the lazy inference mapper that fixes `T`
// before contextually typing the callback, `getReturnTypeFromBody`, and
// `inferFromSignatures` (covariant return-type inference of `U`).
// Verified against `cmd/tsgo` (lib-free body equivalent): `(3,7): error TS2322:
// Type 'string[]' is not assignable to type 'number'.` (the headline
// `x => x.toFixed()` needs the real `Number.toFixed` lib; `x + ""` is the
// lib-free equivalent that likewise yields a `string` element type from the
// callback body using the contextually-typed `x: number`).
//
// DIVERGENCE(port): `cmd/tsgo` prints the array element form `string[]`; this
// port has no `X[]` print sugar yet (a separate nodebuilder feature), so it
// prints the underlying reference `Array<string>`. The element type `string`
// is what C-B3's callback-return inference establishes (before C-B3 it was
// `Array<unknown>`).
// Go: internal/checker/inference.go:Checker.inferFromSignatures/inferFromSignature
#[test]
fn generic_call_infers_callback_result_from_body() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\nfunction map<T, U>(a: T[], f: (x: T) => U): U[] { return [] as any; }\nconst r = map([1, 2], x => x + \"\");\nconst n: number = r;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'Array<string>' is not assignable to type 'number'."
    );
}

// C-B3 slice 2 (callback param + result together): the identity callback
// `x => x` infers `U = T`. `T = number` is fixed from the array argument, so the
// callback parameter `x` is `number`, and its body `x` returns `number`, hence
// `U = number` and the result is `number[]`. Assigning that to `string[]` reports
// 2322 (`number[]` not assignable to `string[]`), proving `U` tracks the
// (contextually-typed) parameter through the body.
// Verified against `cmd/tsgo`: `(3,7): error TS2322: Type 'number[]' is not
// assignable to type 'string[]'.` (with a nested "Type 'number' is not
// assignable to type 'string'." elaboration that this port's relation chain does
// not yet materialize for array element mismatches — DEFER, message-chain
// elaboration). DIVERGENCE(port): array element print `number[]`/`string[]` is
// rendered as `Array<number>`/`Array<string>` (no `X[]` sugar yet).
// Go: internal/checker/inference.go:Checker.inferFromSignature (identity callback)
#[test]
fn generic_call_callback_identity_infers_param_and_result() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\nfunction map<T, U>(a: T[], f: (x: T) => U): U[] { return [] as any; }\nconst s: string[] = map([1, 2], x => x);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'Array<number>' is not assignable to type 'Array<string>'."
    );
}

// C-B3 unit: the lazy inference mapper (`get_fixing_inference_mapper`) maps a
// type parameter that has inference candidates to its current inferred type
// (fixing `T -> number`), while a parameter WITHOUT candidates is omitted from
// the mapper so it stays itself (`U -> U`). This is what instantiates a
// callback's parameter type `(x: T) => U` to `(x: number) => U`, leaving `U`
// open for the subsequent callback-result inference.
// Go: internal/checker/mapper.go:Checker.newInferenceTypeMapper (fixing)
#[test]
fn fixing_inference_mapper_maps_inferred_and_skips_empty() {
    use crate::core::inference::InferenceContext;
    let p = empty();
    let mut c = Checker::new();
    let t = c.new_type_parameter(None);
    let u = c.new_type_parameter(None);
    let num = c.number_type();
    let mut ctx = InferenceContext::new(&[t, u]);
    ctx.inferences[0].candidates = vec![num]; // T inferred = number
                                              // U has no candidates.
    let mapper = c.get_fixing_inference_mapper(&p, &mut ctx);
    assert_eq!(c.map_type(&mapper, t), num, "T maps to its inferred number");
    assert_eq!(c.map_type(&mapper, u), u, "U (no candidates) stays itself");
}

// C-B3 unit: `get_return_type_from_body` infers a concise arrow body's return
// type and widens it — a string-literal body `"s"` yields the WIDENED `string`
// (not the literal `"s"`), mirroring Go's `getReturnTypeFromBody` ->
// `getWidenedType`. This is why `map([1,2], x => "s")` infers `U = string`.
// Go: internal/checker/checker.go:Checker.getReturnTypeFromBody (concise body)
#[test]
fn get_return_type_from_body_widens_concise_literal() {
    let p = StubProgram::parse_and_bind("/a.ts", "const f = (x: number) => \"s\";");
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let list = match arena.data(stmts[0]) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => panic!("variable statement"),
    };
    let decl = match arena.data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes[0],
        _ => panic!("declaration list"),
    };
    let arrow = match arena.data(decl) {
        NodeData::VariableDeclaration(d) => d.initializer.expect("initializer"),
        _ => panic!("variable declaration"),
    };
    let mut c = Checker::new();
    let s = c.string_type();
    assert_eq!(
        c.get_return_type_from_body(&p, arrow),
        s,
        "the concise string-literal body widens to `string`"
    );
}

// C-B3 slice 4 (nested generic calls): a `map` whose array argument is itself a
// `map` call exercises the lazy inference mapper across two nested resolutions.
// The inner `map([1, 2], x => x)` resolves to `number[]` (T = U = number); the
// outer `map(number[], y => y + "")` then fixes `T = number` from that array,
// types `y` as `number`, and infers `U = string` from the callback body, so the
// whole expression is `string[]`. Assigning that to `number` reports 2322 with
// the result element type `string`.
// Verified against `cmd/tsgo`: `(3,7): error TS2322: Type 'string[]' is not
// assignable to type 'number'.` (DIVERGENCE(port): `Array<string>` print form).
// Go: internal/checker/checker.go:Checker.inferTypeArguments (nested calls)
#[test]
fn generic_call_nested_callbacks_infer_result() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\nfunction map<T, U>(a: T[], f: (x: T) => U): U[] { return [] as any; }\nconst r = map(map([1, 2], x => x), y => y + \"\");\nconst n: number = r;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'Array<string>' is not assignable to type 'number'."
    );
}

// C-B3 slice 3 (contextual-return inference): a generic call with NO arguments
// can still infer its type parameter from the call's CONTEXTUAL type, by
// inferring from that contextual type to the signature's (generic) return type.
// `make<T>(): T[]` called as `const xs: number[] = make()` infers `T = number`
// from the annotation `number[]` (the contextual type) matched against the
// return type `T[]`, so `make()` is `number[]` and the assignment is accepted.
// An explicit `make<number>()` assigned to `string[]` reports 2322, confirming
// the explicit (C-B1) path still wins and produces `number[]`.
// Verified against `cmd/tsgo`: line 3 (`const xs: number[] = make()`) is accepted
// (contextual-return inference fixes `T = number`); only line 4
// (`const ys: string[] = make<number>()`) reports `(4,7): error TS2322: Type
// 'number[]' is not assignable to type 'string[]'.` (DIVERGENCE(port): `Array<X>`
// print form; message-chain elaboration deferred).
// Go: internal/checker/checker.go:Checker.inferTypeArguments (contextual return)
#[test]
fn generic_call_contextual_return_inference() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\nfunction make<T>(): T[] { return [] as any; }\nconst xs: number[] = make();\nconst ys: string[] = make<number>();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'Array<number>' is not assignable to type 'Array<string>'."
    );
}

// C-C1 slice 1: `keyof` over a concrete object type is the union of its
// property-name literals, so a value not among them reports 2322.
// Verified against `cmd/tsgo --strict`: only line 3 (`const k2: K = "c"`) reports
// `(3,7): error TS2322: Type '"c"' is not assignable to type '"a" | "b"'.`
// (line 2 `const k: K = "a"` is accepted).
// Go: internal/checker/checker.go:Checker.getIndexType / getLiteralTypeFromProperties
#[test]
fn keyof_concrete_object_union_of_name_literals_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type K = keyof { a: number; b: string };\nconst k: K = \"a\";\nconst k2: K = \"c\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '\"c\"' is not assignable to type '\"a\" | \"b\"'."
    );
}

// C-C1 slice 2: a concrete indexed-access type node `T["a"]` resolves to the
// named property's type (`number`), so a `string` initializer reports 2322.
// Verified against `cmd/tsgo --strict`: only line 3 (`const y: T["a"] = "s"`)
// reports `(3,7): error TS2322: Type 'string' is not assignable to type 'number'.`
// Go: internal/checker/checker.go:Checker.getTypeFromIndexedAccessTypeNode
#[test]
fn concrete_indexed_access_type_node_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = { a: number };\nconst x: T[\"a\"] = 1;\nconst y: T[\"a\"] = \"s\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// C-C1 slice 3 (headline): `keyof` + indexed-access + generic inference. A call
// `get({ a: 1, b: "x" }, "a")` infers `T = { a: number; b: string }`, `K = "a"`
// (kept literal by the `keyof`-primitive constraint), so the return type `T[K]`
// resolves to `number`; assigning that to `string` reports 2322.
// Verified against `cmd/tsgo --strict`: only line 3 (`const s: string = r`)
// reports `(3,7): error TS2322: Type 'number' is not assignable to type 'string'.`
// Go: internal/checker/checker.go:Checker.getIndexedAccessType + inferTypeArguments
#[test]
fn generic_keyof_indexed_access_call_inference_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function get<T, K extends keyof T>(o: T, k: K): T[K] { return o[k]; }\nconst r = get({ a: 1, b: \"x\" }, \"a\");\nconst s: string = r;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// C-C1 slice 5: indexing a generic object with a non-key. `get({ a: 1 }, "b")`
// infers `T = { a: number }`; the constraint `K extends keyof T` instantiates to
// `keyof { a: number }` = `"a"`, so the inferred `K = "b"` is clamped to `"a"`
// and the argument `"b"` is not assignable to the parameter `"a"` -> 2345.
// Verified against `cmd/tsgo --strict`: line 2 reports `(2,25): error TS2345:
// Argument of type '"b"' is not assignable to parameter of type '"a"'.`
// Go: internal/checker/checker.go:Checker.getInferredType (constraint clamp) + isSignatureApplicable
#[test]
fn generic_keyof_index_with_non_key_reports_2345_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function get<T, K extends keyof T>(o: T, k: K): T[K] { return o[k]; }\nconst r = get({ a: 1 }, \"b\");",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2345, got {diags:?}");
    assert_eq!(diags[0].code, 2345);
    assert_eq!(
        diags[0].message,
        "Argument of type '\"b\"' is not assignable to parameter of type '\"a\"'."
    );
}

// C-C2 slice 1: a concrete conditional type `T extends string ? "yes" : "no"`
// resolves to a branch once the alias is instantiated. `IsString<string>` is
// `"yes"`, so assigning `"no"` to it reports 2322.
// Verified against `cmd/tsgo --strict`: only line 3 (`const b: IsString<string>
// = "no"`) reports `(3,7): error TS2322: Type '"no"' is not assignable to type
// '"yes"'.` (line 2 `const a: IsString<string> = "yes"` is accepted).
// Go: internal/checker/checker.go:Checker.getConditionalType (definitely-true branch)
#[test]
fn conditional_concrete_true_branch_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type IsString<T> = T extends string ? \"yes\" : \"no\";\nconst a: IsString<string> = \"yes\";\nconst b: IsString<string> = \"no\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '\"no\"' is not assignable to type '\"yes\"'."
    );
}

// C-C2 slice 1 (false branch): a concrete conditional whose check fails resolves
// to the false branch. `IsString<number>` is `"no"`, so assigning `"yes"` to it
// reports 2322.
// Verified against `cmd/tsgo --strict`: only line 3 (`const d: IsString<number>
// = "yes"`) reports `(3,7): error TS2322: Type '"yes"' is not assignable to type
// '"no"'.` (line 2 `const c: IsString<number> = "no"` is accepted).
// Go: internal/checker/checker.go:Checker.getConditionalType (definitely-false branch)
#[test]
fn conditional_concrete_false_branch_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type IsString<T> = T extends string ? \"yes\" : \"no\";\nconst c: IsString<number> = \"no\";\nconst d: IsString<number> = \"yes\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '\"yes\"' is not assignable to type '\"no\"'."
    );
}

// C-C2 slice 2: a distributive conditional `T extends unknown ? T[] : never`
// distributes over a union check type. `ToArray<string | number>` is
// `string[] | number[]`, so a `string[]` is assignable but a `boolean[]` is not.
// Verified against `cmd/tsgo --strict`: only the `boolean[]` assignment reports
// `error TS2322: Type 'boolean[]' is not assignable to type 'string[] |
// number[]'.` (the `string[]` assignment is accepted).
//
// DIVERGENCE(port): two pre-existing printing divergences appear in the message
// (the distribution *behavior* — exactly the `boolean[]` assignment errors —
// matches Go):
//   1. the port prints an array type as `Array<T>` (it models `T[]` as a
//      reference to the synthetic global `Array`), not Go's `T[]` shorthand; and
//   2. the port models `boolean` as the union `false | true` (no `BOOLEAN`
//      bit), so a `boolean[]` source prints `Array<false | true>`.
// The instantiated target prints `Array<string> | Array<number>`, proving the
// distribution produced `string[] | number[]`.
// blocked-by: array-type shorthand printing + `false | true` -> `boolean`
// collapse in the node builder.
// Go: internal/checker/checker.go:Checker.getConditionalTypeInstantiation (distribution)
#[test]
fn conditional_distributive_over_union_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\n\
         type ToArray<T> = T extends unknown ? T[] : never;\n\
         declare const sa: string[];\n\
         declare const ba: boolean[];\n\
         const x: ToArray<string | number> = sa;\n\
         const y: ToArray<string | number> = ba;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'Array<false | true>' is not assignable to type 'Array<string> | Array<number>'."
    );
}

// C-C2 slice 3: `infer U` in an array-element position. `type ElementType<T> = T
// extends (infer U)[] ? U : never` applied to `number[]` infers `U = number`, so
// the conditional resolves to `number`; a `"x"` initializer reports 2322.
// Verified against `cmd/tsgo --strict`: only line 4 (`const s: ElementType<...>
// = "x"`) reports `error TS2322: Type 'string' is not assignable to type
// 'number'.` (line 3 `const n: ElementType<...> = 1` is accepted).
// Go: internal/checker/checker.go:Checker.getConditionalType (infer inference) + getInferTypeParameters
#[test]
fn conditional_infer_element_type_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\n\
         type ElementType<T> = T extends (infer U)[] ? U : never;\n\
         const n: ElementType<number[]> = 1;\n\
         const s: ElementType<number[]> = \"x\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// C-C2 slice 4: `infer R` in a function return-type position. `type Ret<T> = T
// extends (...args: any[]) => infer R ? R : never` applied to `() => number`
// infers `R = number`, so the conditional resolves to `number`; a `"x"`
// initializer reports 2322.
// Verified against `cmd/tsgo --strict`: only line 4 (`const r2: Ret<...> = "x"`)
// reports `error TS2322: Type 'string' is not assignable to type 'number'.`
// (line 3 `const r: Ret<...> = 1` is accepted).
// Go: internal/checker/checker.go:Checker.getConditionalType (infer from signature return) + inferFromSignatures
#[test]
fn conditional_infer_return_type_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\n\
         type Ret<T> = T extends (...args: any[]) => infer R ? R : never;\n\
         const r: Ret<() => number> = 1;\n\
         const r2: Ret<() => number> = \"x\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// C-C2 slice 5: a conditional return type stays deferred until a generic
// function is instantiated. `function f<T>(x: T): T extends string ? 1 : 2` has
// a deferred conditional return type; `f<string>("a")` instantiates the
// signature with `T = string`, re-resolving the return type to `1`, so assigning
// the call result to `2` reports 2322.
// Verified against `cmd/tsgo --strict`: only the `const bad: 2 = a` line reports
// `error TS2322: Type '1' is not assignable to type '2'.`
// Go: internal/checker/checker.go:Checker.instantiateSignature + getConditionalTypeInstantiation
#[test]
fn conditional_deferred_through_generic_function_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T>(x: T): T extends string ? 1 : 2 { return null as any; }\n\
         const a = f<string>(\"a\");\n\
         const bad: 2 = a;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(diags[0].message, "Type '1' is not assignable to type '2'.");
}

// C-C3 slice 4: a template literal type with a concrete string-literal
// placeholder resolves to a string literal. `` `a${"x"}b` `` is `"axb"`, so
// assigning `"nope"` to it reports 2322 while `"axb"` is accepted.
// Verified against `cmd/tsgo --noEmit --strict`: only line 3 (`const u: T =
// "nope"`) reports `(3,7): error TS2322: Type '"nope"' is not assignable to
// type '"axb"'.` (line 2 `const t: T = "axb"` is accepted).
// Go: internal/checker/checker.go:Checker.getTemplateLiteralType (all-literal -> string literal)
#[test]
fn template_literal_concrete_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = `a${\"x\"}b`;\nconst t: T = \"axb\";\nconst u: T = \"nope\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '\"nope\"' is not assignable to type '\"axb\"'."
    );
}

// C-C3 slice 5: a template literal type with a union placeholder distributes
// over the union. `` `x${"a" | "b"}` `` is `"xa" | "xb"`, so assigning `"xc"`
// reports 2322 while `"xa"` is accepted.
// Verified against `cmd/tsgo --noEmit --strict`: only line 3 (`const u: T =
// "xc"`) reports `(3,7): error TS2322: Type '"xc"' is not assignable to type
// '"xa" | "xb"'.` (line 2 `const t: T = "xa"` is accepted).
// Go: internal/checker/checker.go:Checker.getTemplateLiteralType (union distribution)
#[test]
fn template_literal_union_distribution_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = `x${\"a\" | \"b\"}`;\nconst t: T = \"xa\";\nconst u: T = \"xc\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '\"xc\"' is not assignable to type '\"xa\" | \"xb\"'."
    );
}

// C-C3 slice 6: the intrinsic string-mapping type `Uppercase<S>`. Applied to a
// concrete string literal `"abc"`, it resolves to `"ABC"`, so assigning `"abc"`
// reports 2322 while `"ABC"` is accepted.
//
// Verified against `cmd/tsgo --noEmit --strict` (real lib `Uppercase`): only the
// `const v: U = "abc"` line reports `error TS2322: Type '"abc"' is not assignable
// to type '"ABC"'.` (the `const u: U = "ABC"` line is accepted).
//
// DIVERGENCE(port): Go declares `type Uppercase<S extends string> = intrinsic`
// in lib.es5.d.ts and dispatches in `getTypeAliasInstantiation` when the
// declared type is the intrinsic marker. The port's parser has no `intrinsic`
// keyword (it is `internal/parser`, out of scope), so the stand-in alias body is
// `= string` and the checker dispatches by the alias *name*. blocked-by: parser
// `intrinsic` keyword support.
// Go: internal/checker/checker.go:Checker.getStringMappingType / applyStringMapping
#[test]
fn intrinsic_uppercase_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Uppercase<S extends string> = string;\n\
         type U = Uppercase<\"abc\">;\n\
         const u: U = \"ABC\";\n\
         const v: U = \"abc\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '\"abc\"' is not assignable to type '\"ABC\"'."
    );
}

// C-C3 slice 1: a `Partial`-shaped homomorphic mapped type
// `{ [K in keyof T]?: T[K] }` over a concrete object makes every property
// optional. `Partial2<{ a: number }>` is `{ a?: number }`, so `{}` is assignable
// (a is optional) but `{ a: "x" }` reports 2322 (a is `number`).
// Verified against `cmd/tsgo --noEmit --strict`: only `const q: P = { a: "x" }`
// reports `(4,16): error TS2322: Type 'string' is not assignable to type
// 'number'.` (the `const p: P = {}` line is accepted).
// Go: internal/checker/checker.go:Checker.instantiateMappedType / resolveMappedTypeMembers
#[test]
fn mapped_partial_optional_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Partial2<T> = { [K in keyof T]?: T[K] };\n\
         type P = Partial2<{ a: number }>;\n\
         const p: P = {};\n\
         const q: P = { a: \"x\" };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// C-C3 slice 3: a `Required`-shaped mapped type `{ [K in keyof T]-?: T[K] }`
// strips optionality. `Required2<{ a?: number }>` is `{ a: number }`
// (required), so `const r: R = {}` reports a missing-property error.
//
// Verified against `cmd/tsgo --noEmit --strict`: `const r: R = {}` reports
// `(3,7): error TS2741: Property 'a' is missing in type '{}' but required in
// type 'Required2<{ a?: number | undefined; }>'.`
//
// DIVERGENCE(port): the target type prints as the resolved anonymous object
// `{ a: number; }` rather than Go's alias form `Required2<{ a?: number |
// undefined; }>` — the port does not do type-alias attribution on an
// instantiated type, and (strictNullChecks off in the stub) does not widen the
// source `a?` to `number | undefined`. The behavior (2741 for the now-required
// `a`) matches Go. blocked-by: alias attribution on instantiated types.
// Go: internal/checker/checker.go:Checker.resolveMappedTypeMembers (-? strips optional)
#[test]
fn mapped_required_strips_optional_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Required2<T> = { [K in keyof T]-?: T[K] };\n\
         type R = Required2<{ a?: number }>;\n\
         const r: R = {};",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2741, got {diags:?}");
    assert_eq!(diags[0].code, 2741);
    assert_eq!(
        diags[0].message,
        "Property 'a' is missing in type '{}' but required in type '{ a: number; }'."
    );
}

// C-C3 slice 7: key remapping via `as`. An identity remap `{ [K in keyof T as
// K]: T[K] }` exercises the `as` name-type path while keeping the keys, so
// `Id<{ a: number }>` is `{ a: number }` and `const bad: R = { a: "x" }`
// reports 2322. (A non-identity template-literal `as` remap is covered by the
// direct test `instantiate_mapped_type_as_remaps_keys`; the full template-`as`
// over `string & K` is DEFER — see that test.)
// Verified against `cmd/tsgo --noEmit --strict`: only `const bad: R = { a: "x" }`
// reports `(3,18): error TS2322: Type 'string' is not assignable to type
// 'number'.`
// Go: internal/checker/checker.go:Checker.resolveMappedTypeMembers (nameType remap)
#[test]
fn mapped_as_identity_remap_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Id<T> = { [K in keyof T as K]: T[K] };\n\
         type R = Id<{ a: number }>;\n\
         const bad: R = { a: \"x\" };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// ===========================================================================
// C-D1: strictNullChecks completeness (optional property / parameter) + enums.
// ===========================================================================

// C-D1 slice 1 (optional property `| undefined`): reading an optional property
// `{ a?: number }` yields `number | undefined` under strictNullChecks, so
// `const n: number = o.a;` reports 2322.
// Verified against `cmd/tsgo --noEmit --strict`: `(2,7): error TS2322: Type
// 'number | undefined' is not assignable to type 'number'.`
// Go: internal/checker/checker.go:Checker.getTypeForVariableLikeDeclaration (addOptionalityEx)
#[test]
fn optional_property_read_is_number_or_undefined_under_strict() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: { a?: number } = {};\nconst n: number = o.a;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    // The port sorts union constituents by type id, which allocates `undefined`
    // before `number`, so it prints `undefined | number` where cmd/tsgo prints
    // `number | undefined` (a pre-existing union member-ordering divergence; see
    // 4ay/4az). The CODE (2322) and the assignability decision match Go.
    assert_eq!(
        diags[0].message,
        "Type 'undefined | number' is not assignable to type 'number'."
    );
}

// C-D1 slice 1 guard: assigning `{}` to `{ a?: number }` is fine (the optional
// property need not be present). Verified against `cmd/tsgo --noEmit --strict`:
// 0 diagnostics.
// Go: internal/checker/relater.go (optional target property not required)
#[test]
fn assign_empty_object_to_optional_property_is_ok() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: { a?: number } = {};\no.a;\nconst x = o;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 0, "expected no diagnostics, got {diags:?}");
}

// C-D1 slice 1 (non-strict): without strictNullChecks an optional property reads
// as its bare type (`number`), so `const n: number = o.a;` is fine.
// Verified against `cmd/tsgo --noEmit --strictNullChecks false`: 0 diagnostics.
// Go: internal/checker/checker.go:Checker.addOptionalityEx (no-op when !strictNullChecks)
#[test]
fn optional_property_read_is_bare_type_without_strict_null_checks() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        strict_null_checks: Tristate::False,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "const o: { a?: number } = {};\nconst n: number = o.a;",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 0, "expected no diagnostics, got {diags:?}");
}

// C-D1 slice 2 (optional parameter `| undefined`): inside the body of
// `function f(x?: number)`, `x` is `number | undefined`, so `const n: number =
// x;` reports 2322.
// Verified against `cmd/tsgo --noEmit --strict`: `(1,32): error TS2322: Type
// 'number | undefined' is not assignable to type 'number'.`
// Go: internal/checker/checker.go:Checker.getTypeForVariableLikeDeclaration (addOptionalityEx, parameter)
#[test]
fn optional_parameter_is_number_or_undefined_in_body() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(x?: number) { const n: number = x; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    // Port union ordering prints `undefined | number` (see slice 1 note).
    assert_eq!(
        diags[0].message,
        "Type 'undefined | number' is not assignable to type 'number'."
    );
}

// C-D1 slice 2 (non-strict): without strictNullChecks an optional parameter is
// its bare type, so the body assignment is fine.
// Verified against `cmd/tsgo --noEmit --strictNullChecks false`: 0 diagnostics.
// Go: internal/checker/checker.go:Checker.addOptionalityEx (no-op when !strictNullChecks)
#[test]
fn optional_parameter_is_bare_type_without_strict_null_checks() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        strict_null_checks: Tristate::False,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "function f(x?: number) { const n: number = x; }",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 0, "expected no diagnostics, got {diags:?}");
}

// C-D1 slice 3 (numeric enum literal + union): `enum E { A, B }` gives `E.A`
// (value 0) / `E.B` (value 1); the enum type `E` is `E.A | E.B`. `E.A` is
// assignable to `E`, but a numeric literal `2` (matching no member) is not.
// Verified against `cmd/tsgo --noEmit --strict`: only `const c: E = 2;` reports
// `(3,7): error TS2322: Type '2' is not assignable to type 'E'.`
// Go: internal/checker/checker.go:Checker.getDeclaredTypeOfEnum / isSimpleTypeRelatedTo
#[test]
fn numeric_enum_member_and_union_assignability() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "enum E { A, B }\nconst a: E = E.A;\nconst c: E = 2;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(diags[0].message, "Type '2' is not assignable to type 'E'.");
}

// C-D1 slice 3 (matching numeric literal -> numeric enum is OK): a numeric
// literal whose value matches a member is assignable to the enum (bit-flag
// rule), so `const c0: E = 0;` and `const c1: E = 1;` are both fine.
// Verified against `cmd/tsgo --noEmit --strict`: 0 diagnostics.
// Go: internal/checker/relater.go:isSimpleTypeRelatedTo (numeric literal -> enum literal, matching value)
#[test]
fn matching_numeric_literal_is_assignable_to_enum() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "enum E { A, B }\nconst c0: E = 0;\nconst c1: E = 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 0, "expected no diagnostics, got {diags:?}");
}

// C-D1 slice 4 (enum auto-increment with explicit initializer): `enum E { A = 1,
// B }` gives `E.B` value 2, so `const b1: 2 = E.B;` is OK but `const b2: 1 =
// E.B;` reports 2322. The enum member literal prints `E.B` (multi-member enum).
// Verified against `cmd/tsgo --noEmit --strict`: only `const b2: 1 = E.B;`
// reports `(3,7): error TS2322: Type 'E.B' is not assignable to type '1'.`
// Go: internal/checker/checker.go:Checker.computeEnumMemberValue (auto-increment)
#[test]
fn enum_auto_increment_after_explicit_initializer() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "enum E { A = 1, B }\nconst b1: 2 = E.B;\nconst b2: 1 = E.B;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'E.B' is not assignable to type '1'."
    );
}

// C-D1 slice 5 (auto-increment with multiple explicit initializers): `enum E { A
// = 10, B, C = 20 }` gives `E.B` value 11, so `const b2: 12 = E.B;` reports
// 2322 ("Type 'E.B' is not assignable to type '12'.") while `const b1: 11 = E.B;`
// is OK.
// Verified against `cmd/tsgo --noEmit --strict`: only `const b2: 12 = E.B;`.
// Go: internal/checker/checker.go:Checker.computeEnumMemberValues (auto-increment continues)
#[test]
fn enum_auto_increment_between_explicit_initializers() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "enum E { A = 10, B, C = 20 }\nconst b1: 11 = E.B;\nconst b2: 12 = E.B;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'E.B' is not assignable to type '12'."
    );
}

// C-D1 slice 6 (string enum): `enum E { A = "x" }` member is a string enum
// literal; `E.A` is assignable to `string`, but a string literal `"x"` is not
// assignable to the enum `E` (single-member enums print as `E`).
// Verified against `cmd/tsgo --noEmit --strict`: only `const e: E = "x";`
// reports `(3,7): error TS2322: Type '"x"' is not assignable to type 'E'.`
// Go: internal/checker/checker.go:Checker.getEnumLiteralType (string) / isSimpleTypeRelatedTo
#[test]
fn string_enum_member_and_assignability() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "enum E { A = \"x\" }\nconst s: string = E.A;\nconst e: E = \"x\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '\"x\"' is not assignable to type 'E'."
    );
}

// C-D1 slice 7 (enum member reference as a value): `E.A` reads as its enum
// literal type; `E.A` is assignable to `number` and `E`, but not to a
// non-matching numeric literal type `5`.
// Verified against `cmd/tsgo --noEmit --strict`: only `const bad: 5 = E.A;`
// reports `(4,7): error TS2322: Type 'E.A' is not assignable to type '5'.`
// Go: internal/checker/checker.go:Checker.getTypeOfEnumMember
#[test]
fn enum_member_value_reference_assignability() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "enum E { A, B }\nconst n: number = E.A;\nconst e: E = E.B;\nconst bad: 5 = E.A;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'E.A' is not assignable to type '5'."
    );
}

// C-D1 slice 8 (const enum member typing): a `const enum` member is typed
// identically to a regular enum member (the emit-inlining is a transformer
// concern, DEFER). `const a: E = E.A;` is OK; `const bad: 5 = E.B;` reports
// 2322.
// Verified against `cmd/tsgo --noEmit --strict`: only `const bad: 5 = E.B;`
// reports `(3,7): error TS2322: Type 'E.B' is not assignable to type '5'.`
// Go: internal/checker/checker.go:Checker.getDeclaredTypeOfEnum (const enum)
#[test]
fn const_enum_member_typing() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const enum E { A, B }\nconst a: E = E.A;\nconst bad: 5 = E.B;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'E.B' is not assignable to type '5'."
    );
}

// C-D1 slice 9 (constant-expression enum members via the evaluator): `enum E { A
// = 1 << 1, B = A | 1 }` folds `A` to 2 and `B` to `2 | 1` = 3 (an entity
// reference to a prior member resolved through the evaluator), so `const b1: 3 =
// E.B;` is OK and `const b2: 4 = E.B;` reports 2322.
// Verified against `cmd/tsgo --noEmit --strict`: only `const b2: 4 = E.B;`
// reports `(3,7): error TS2322: Type 'E.B' is not assignable to type '4'.`
// Go: internal/checker/checker.go:Checker.computeConstantEnumMemberValue (evaluate)
#[test]
fn enum_constant_expression_members_via_evaluator() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "enum E { A = 1 << 1, B = A | 1 }\nconst b1: 3 = E.B;\nconst b2: 4 = E.B;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'E.B' is not assignable to type '4'."
    );
}

// ---------------------------------------------------------------------------
// C-D2: namespaces / this-type / abstract / overload elaboration / exhaustiveness
// ---------------------------------------------------------------------------

// C-D2 behavior A, slice 1 (namespace value member): a namespace with a value
// export is itself a value whose type carries the export as a member, so
// `N.x` reads its type. `const n: string = N.x;` reports 2322 because `N.x`
// is `number`.
// Verified against `cmd/tsgo --noEmit --strict`: only `const n: string = N.x;`
// reports `ns1.ts(2,7): error TS2322: Type 'number' is not assignable to type
// 'string'.`
// Go: internal/checker/checker.go:Checker.getTypeOfFuncClassEnumModuleWorker (module)
#[test]
fn namespace_value_member_read_is_number() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "namespace N { export const x = 1; }\nconst n: string = N.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// C-D2 behavior A, slice 1b (namespace value type prints `typeof N`): reading a
// non-existent member off a namespace value reports 2339 against the namespace
// value type, which the node builder prints as `typeof N`.
// Verified against `cmd/tsgo --noEmit --strict`:
// `ns1b.ts(2,13): error TS2339: Property 'y' does not exist on type 'typeof N'.`
// Go: internal/checker/nodebuilderimpl.go:typeToTypeNodeWorker (typeof query for a module value)
#[test]
fn namespace_value_missing_member_prints_typeof_n() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "namespace N { export const x = 1; }\nconst y = N.y;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2339, got {diags:?}");
    assert_eq!(diags[0].code, 2339);
    assert_eq!(
        diags[0].message,
        "Property 'y' does not exist on type 'typeof N'."
    );
}

// C-D2 behavior A, slice 2 (namespace type member): a qualified name `N.T` in
// type position resolves the exported type alias `T` of namespace `N`, so
// `const t: N.T = 1;` is OK and `const u: N.T = "x";` reports 2322.
// Verified against `cmd/tsgo --noEmit --strict`: only `const u: N.T = "x";`
// reports `ns2.ts(3,7): error TS2322: Type 'string' is not assignable to type
// 'number'.`
// Go: internal/checker/checker.go:Checker.resolveEntityName (qualified name) / getTypeFromTypeReference
#[test]
fn namespace_type_member_resolves_qualified_name() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "namespace N { export type T = number; }\nconst t: N.T = 1;\nconst u: N.T = \"x\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// C-D2 behavior A, slice 3 (nested namespace value member): an exported nested
// namespace `A.B` is itself a value member of `A`, so `A.B.x` reaches the inner
// export. `const n: string = A.B.x;` reports 2322 (`A.B.x` is `number`).
// Verified against `cmd/tsgo --noEmit --strict`:
// `ns3.ts(2,7): error TS2322: Type 'number' is not assignable to type 'string'.`
// Go: internal/checker/checker.go:Checker.getTypeOfFuncClassEnumModuleWorker (nested module)
#[test]
fn nested_namespace_value_member_read_is_number() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "namespace A { export namespace B { export const x = 1; } }\nconst n: string = A.B.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// C-D2 behavior A, slice 4 (namespace + namespace merge accumulates exports):
// two namespace declarations with the same name merge their exports into one
// symbol, so both `N.x` and `N.y` are reachable. Only `const c: string = N.x;`
// errors.
// Verified against `cmd/tsgo --noEmit --strict`:
// `nsmerge.ts(5,7): error TS2322: Type 'number' is not assignable to type
// 'string'.`
// Go: internal/checker/checker.go:Checker.getExportsOfSymbol (merged module symbol)
#[test]
fn merged_namespace_accumulates_exports() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "namespace N { export const x = 1; }\nnamespace N { export const y = 2; }\nconst a: number = N.x;\nconst b: number = N.y;\nconst c: string = N.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// C-D2 behavior B, slice 1 (`this` in a class method is the instance type):
// inside a non-static method, `this` resolves to the class instance type, so
// `this.x` reads property `x`. `const n: string = this.x;` reports 2322.
// Verified against `cmd/tsgo --noEmit --strict`:
// `this1.ts(1,30): error TS2322: Type 'number' is not assignable to type
// 'string'.`
// Go: internal/checker/checker.go:Checker.checkThisExpression / tryGetThisTypeAtEx
#[test]
fn this_in_method_is_class_instance_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { x = 1; m() { const n: string = this.x; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// C-D2 behavior B, slice 2 (polymorphic `this` return resolves to the class):
// a method declared `m(): this` returns the class instance type at a concrete
// call site, so `const d: string = c.m();` reports 2322 against `C`.
// Verified against `cmd/tsgo --noEmit --strict`:
// `this2.ts(3,7): error TS2322: Type 'C' is not assignable to type 'string'.`
// Go: internal/checker/checker.go:Checker.getThisType (this-type node)
#[test]
fn this_type_return_annotation_is_class_instance_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { x = 1; m(): this { return this; } }\nconst c = new C();\nconst d: string = c.m();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'C' is not assignable to type 'string'."
    );
}

// C-D2 behavior C, slice 1 (instantiating an abstract class reports 2511):
// `new A()` where `A` is an `abstract class` reports 2511 "Cannot create an
// instance of an abstract class.".
//
// NOTE: the source uses `declare abstract class A {}` rather than the bare
// `abstract class A {}` of the task's headline because the port's
// `parse_statement` is missing the leading-modifier keywords
// (`abstract`/`static`/`public`/...) in its declaration-dispatch guard, so the
// bare form is mis-parsed as a stray `abstract` identifier expression followed
// by a non-abstract `class A {}`. The `declare` form routes through the present
// `DeclareKeyword` guard (and keeps the file a script, so `A` stays in
// `locals`), so the `abstract` modifier is parsed onto the class. The checker
// behavior (2511) is identical and matches `cmd/tsgo`. Fixing `parse_statement`
// is out of scope (`internal/parser`).
// Verified against `cmd/tsgo --noEmit --strict` (`declare abstract class A {}`):
// `dabs.ts(2,1): error TS2511: Cannot create an instance of an abstract class.`
// Go: internal/checker/checker.go:Checker.resolveNewExpression (abstract construct)
#[test]
fn new_abstract_class_reports_2511() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare abstract class A {}\nnew A();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2511, got {diags:?}");
    assert_eq!(diags[0].code, 2511);
    assert_eq!(
        diags[0].message,
        "Cannot create an instance of an abstract class."
    );
}

// C-D2 behavior C, slice 1b (instantiating a concrete class is OK): `new C()`
// where `C` is a non-abstract class reports no diagnostic.
// Verified against `cmd/tsgo --noEmit --strict`: no diagnostics.
// Go: internal/checker/checker.go:Checker.resolveNewExpression (non-abstract)
#[test]
fn new_concrete_class_is_ok() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "class C {}\nnew C();"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// C-D2 behavior E, slice 1d (trailing code after an exhaustive switch is
// `never`): when every case of a `switch` over a discriminated union throws,
// the implicit no-match path is unreachable, so a reference to the discriminant
// after the switch has type `never` — assignable to anything. `const y: number
// = x;` after such a switch is therefore OK.
// Verified against `cmd/tsgo --noEmit --strict`: no diagnostics.
// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnDiscriminant (no-match complement)
#[test]
fn trailing_reference_after_exhaustive_throw_switch_is_never() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(x: \"a\" | \"b\"): void {\n  switch (x) {\n    case \"a\": throw 0;\n    case \"b\": throw 0;\n  }\n  const y: number = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// C-D2 behavior E, slice 1e (trailing code after a non-exhaustive throw switch
// keeps the unhandled member): when a case is missing, the unhandled member
// falls through, so the trailing reference is that member and the assignment
// `const y: number = x;` reports 2322 (the literal source generalized to its
// base for a non-unit target, matching Go).
// Verified against `cmd/tsgo --noEmit --strict`:
// `sw_throw2.ts(6,9): error TS2322: Type 'string' is not assignable to type
// 'number'.`
// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnDiscriminant (no-match complement)
#[test]
fn trailing_reference_after_non_exhaustive_throw_switch_is_leftover() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(x: \"a\" | \"b\" | \"c\"): void {\n  switch (x) {\n    case \"a\": throw 0;\n    case \"b\": throw 0;\n  }\n  const y: number = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// C-D2 behavior C, slice 2 (abstract method in a non-abstract class -> 1244):
// an `abstract` method modifier is only allowed in an `abstract class`, so
// `class B { abstract n(): void; }` reports 1244.
// Verified against `cmd/tsgo --noEmit --strict`:
// `abstract2.ts(2,11): error TS1244: Abstract methods can only appear within an
// abstract class.`
// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers (abstract)
#[test]
fn abstract_method_in_non_abstract_class_reports_1244() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B { abstract n(): void; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 1244, got {diags:?}");
    assert_eq!(diags[0].code, 1244);
    assert_eq!(
        diags[0].message,
        "Abstract methods can only appear within an abstract class."
    );
}

// C-D2 behavior D, slice 1 (overload-failure elaboration 2769): a call matching
// no overload of an overloaded function reports 2769 "No overload matches this
// call." with a message chain "The last overload gave the following error."
// (2770) wrapping the last overload's argument error (2345).
// Verified against `cmd/tsgo --noEmit --strict`:
// `overload1.ts(4,3): error TS2769: No overload matches this call.`
// `  The last overload gave the following error.`
// `    Argument of type 'boolean' is not assignable to parameter of type 'string'.`
//
// DIVERGENCE (pre-existing, documented since C-C2): the port models `boolean`
// as the union `false | true` and the node builder prints it as `false | true`
// rather than folding it back to `boolean`. The *semantics* match Go (the
// literal argument `true` is generalized to its boolean base type for the
// message); only the printed base-type name differs. The chain shape, codes
// (2769 / 2770 / 2345), and target text all match Go exactly.
// Go: internal/checker/checker.go:Checker.reportCallResolutionErrors
#[test]
fn overload_failure_reports_2769_with_elaboration_chain() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(x: number): void;\nfunction f(x: string): void;\nfunction f(x: any) {}\nf(true);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2769, got {diags:?}");
    assert_eq!(diags[0].code, 2769);
    assert_eq!(diags[0].message, "No overload matches this call.");
    let chain = &diags[0].message_chain;
    assert_eq!(chain.len(), 1, "expected one chain entry, got {chain:?}");
    assert_eq!(chain[0].code, 2770);
    assert_eq!(
        chain[0].message,
        "The last overload gave the following error."
    );
    assert_eq!(
        chain[0].next.len(),
        1,
        "expected leaf, got {:?}",
        chain[0].next
    );
    assert_eq!(chain[0].next[0].code, 2345);
    // Go prints `boolean`; the port prints the `false | true` union (documented
    // node-builder divergence above). The target/text are otherwise identical.
    assert_eq!(
        chain[0].next[0].message,
        "Argument of type 'false | true' is not assignable to parameter of type 'string'."
    );
}

// C-D2 behavior E, slice 1 (exhaustive switch narrows the discriminant to
// `never` in the default clause): in a `switch` over a discriminated union that
// covers every case, the `default` clause narrows the discriminant to `never`
// (the assert-never exhaustiveness pattern), so `const y: never = x;` is OK.
// Verified against `cmd/tsgo --noEmit --strict`: no diagnostics.
// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnDiscriminant (default complement)
#[test]
fn exhaustive_switch_default_narrows_discriminant_to_never() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(x: \"a\" | \"b\"): number {\n  switch (x) {\n    case \"a\": return 1;\n    case \"b\": return 2;\n    default: const y: never = x; return y;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// C-D2 behavior E, slice 1b (non-exhaustive switch leaves the leftover member in
// the default clause): when a case is missing, the `default` clause narrows the
// discriminant to the unhandled member, so `const y: never = x;` reports 2322
// against that member.
// Verified against `cmd/tsgo --noEmit --strict`:
// `sw_def_nonexhaustive.ts(5,20): error TS2322: Type '"c"' is not assignable to
// type 'never'.`
// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnDiscriminant (default complement)
#[test]
fn non_exhaustive_switch_default_narrows_to_leftover_member() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(x: \"a\" | \"b\" | \"c\"): number {\n  switch (x) {\n    case \"a\": return 1;\n    case \"b\": return 2;\n    default: const y: never = x; return y;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '\"c\"' is not assignable to type 'never'."
    );
}

// C-D2 behavior E, slice 1c (the task-headline exhaustive switch is OK): a
// function whose `switch` over a discriminated union covers every case and
// returns from each case has no trailing-return error, because the implicit
// fall-through is unreachable (the union has narrowed to `never`).
// Verified against `cmd/tsgo --noEmit --strict`: no diagnostics.
//
// DIVERGENCE (documented, blocked): for the *non-exhaustive* counterpart Go
// reports 2366 "Function lacks ending return statement ..."; the port does not,
// because the implicit-return analysis (`checkAllCodePathsInNonVoidFunctionReturn
// OrThrow` -> `functionHasImplicitReturn`) requires the function body's
// `EndFlowNode`, which the port's binder does not expose, and
// `is_reachable_flow_node` is `&self` (the switch-exhaustiveness refinement
// needs `&mut` to type the clause expressions; changing its signature is barred
// by the additive-only rule). The headline exhaustive case matches Go.
// Go: internal/checker/checker.go:Checker.checkAllCodePathsInNonVoidFunctionReturnOrThrow
#[test]
fn exhaustive_switch_without_trailing_return_is_ok() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(x: \"a\" | \"b\"): number {\n  switch (x) {\n    case \"a\": return 1;\n    case \"b\": return 2;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// CommonJS `require(...)`: a bare `require` that is the callee of a
// `require(...)` call in a JS file resolves to the synthetic `require` symbol
// (whose type is `any`), so `const a = require("./x")` does NOT report 2304 on
// the `require` identifier. This mirrors Go's `resolveName`, which returns
// `RequireSymbol` when the unresolved name's location is the callee of a
// require call in a JS file.
// Go: internal/binder/nameresolver.go:Resolve (RequireSymbol branch)
#[test]
fn require_call_in_js_file_resolves_no_cannot_find_name() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_js(
        "/a.js",
        "const a = require(\"./x\");",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2304),
        "require(...) callee in a JS file must resolve (no 2304): {diags:?}"
    );
}

// Guard: a bare `require` reference that is NOT the callee of a `require(...)`
// call is NOT resolved to the synthetic require symbol (that branch is gated on
// `IsRequireCall`), so it stays unresolved — and now reports the specialized
// node-typedefs hint TS2591 (Round 7's `getCannotFindNameDiagnosticForName`
// maps `require` to the node hint), NOT the silent `any` resolution. (Before
// Round 7 this asserted TS2304; the guard intent — `require` is not silently
// resolved when it is a bare reference — is unchanged.)
// Go: internal/binder/nameresolver.go:Resolve (RequireSymbol branch / IsRequireCall)
//     + internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName ("require")
#[test]
fn bare_require_reference_in_js_file_reports_2591_node_typedefs() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_js("/a.js", "require;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2591),
        "a bare `require` (not a call) is unresolved -> node-typedefs hint 2591: {diags:?}"
    );
    assert!(
        diags.iter().all(|d| d.code != 2304),
        "the bare `require` must NOT report the generic 2304: {diags:?}"
    );
}

// Guard: a `require(...)` call in a TS file does NOT get the JS-only require
// resolution, so the unresolved `require` still reports the specialized
// node-typedefs hint. `require` is in Go's node-builtin switch arm, so an
// unresolved `require` (here, in a TS file where the JS require-call resolution
// does not apply) reports TS2591 — not the generic TS2304. (Before Round 7 this
// asserted TS2304; Round 7 ports `getCannotFindNameDiagnosticForName`, which
// maps `require` to the node hint, matching tsc.)
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName ("require")
//     + internal/binder/nameresolver.go:Resolve (IsInJSFile gate)
#[test]
fn require_call_in_ts_file_still_reports_cannot_find_name_2591() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "require(\"./x\");"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2591),
        "require(...) in a TS file is unresolved -> node-typedefs hint 2591: {diags:?}"
    );
}

// ---------------------------------------------------------------------------
// Round 7 — getCannotFindNameDiagnosticForName: an unresolved identifier emits
// the SAME specialized "cannot find name" diagnostic code as tsc (dispatched on
// the name / parent kind / `UsesWildcardTypes`), instead of the generic TS2304.
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName
// ---------------------------------------------------------------------------

// Slice 1 (headline): an unresolved `module` reference reports the node
// type-definitions hint TS2591 ("...and then add 'node' to the types field..."
// variant, because the default options have `UsesWildcardTypes == false`), not
// the generic TS2304.
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName ("module")
#[test]
fn unresolved_module_reports_2591_node_typedefs() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "module;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].code, 2591);
    assert_eq!(
        diags[0].message,
        "Cannot find name 'module'. Do you need to install type definitions for node? Try `npm i --save-dev @types/node` and then add 'node' to the types field in your tsconfig."
    );
}

// Slice 3: the remaining Node ambient globals (`process`, `Buffer`, `NodeJS`)
// also report the node type-definitions hint TS2591.
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName
//     ("process", "Buffer", "NodeJS")
#[test]
fn unresolved_process_reports_2591_node_typedefs() {
    for name in ["process", "Buffer", "NodeJS"] {
        let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &format!("{name};")));
        let root = p.root();
        let mut c = Checker::new_checker(p);
        let diags = c.get_diagnostics(root);
        assert_eq!(diags.len(), 1, "{name}: got {diags:?}");
        assert_eq!(diags[0].code, 2591, "{name}: expected node hint 2591");
        assert_eq!(
            diags[0].message,
            format!("Cannot find name '{name}'. Do you need to install type definitions for node? Try `npm i --save-dev @types/node` and then add 'node' to the types field in your tsconfig.")
        );
    }
}

// Slice 4: with `types: ["*"]` (`UsesWildcardTypes` true), the Node-global hint
// is the WILDCARD variant TS2580 ("Try `npm i --save-dev @types/node`.") with no
// "...and then add 'node' to the types field..." tail — Go's `core.IfElse`
// selects the first arm when `UsesWildcardTypes()` holds. Proves the wildcard
// branch is gated on the `types` option.
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName (node arm)
//     + internal/core/compileroptions.go:UsesWildcardTypes
#[test]
fn unresolved_module_with_wildcard_types_reports_2580() {
    use tsgo_core::compileroptions::CompilerOptions;
    let options = CompilerOptions {
        types: vec!["*".to_string()],
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts", "module;", options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].code, 2580);
    assert_eq!(
        diags[0].message,
        "Cannot find name 'module'. Do you need to install type definitions for node? Try `npm i --save-dev @types/node`."
    );
}

// Slice 5: `document` / `console` point at the dom lib (TS2584). This arm is NOT
// gated on `UsesWildcardTypes` (Go returns the dom message unconditionally).
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName
//     ("document", "console")
#[test]
fn unresolved_document_console_reports_2584_dom() {
    for name in ["document", "console"] {
        let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &format!("{name};")));
        let root = p.root();
        let mut c = Checker::new_checker(p);
        let diags = c.get_diagnostics(root);
        assert_eq!(diags.len(), 1, "{name}: got {diags:?}");
        assert_eq!(diags[0].code, 2584, "{name}: expected dom hint 2584");
        assert_eq!(
            diags[0].message,
            format!("Cannot find name '{name}'. Do you need to change your target library? Try changing the 'lib' compiler option to include 'dom'.")
        );
    }
}

// Slice 6: a target-library name (`Map`/`Set`/`Promise`/`Symbol`/.../`BigInt`/
// `BigUint64Array`) reports TS2583 with the `{1}` argument filled by
// `getSuggestedLibForNonExistentName` (the first lib in the feature map for the
// name) — e.g. `Map` -> `es2015`, `SharedArrayBuffer` -> `es2017`,
// `AsyncGenerator` -> `es2018`, `BigInt` -> `es2020`. The `{1}` precision comes
// from Go's `onFailedToResolveSymbol`, which reports the missing-lib variant
// first when `getSuggestedLibForNonExistentName(name)` is non-empty.
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName (target-lib arm)
//     + Checker.getSuggestedLibForNonExistentName + Checker.onFailedToResolveSymbol
//     + internal/checker/utilities.go:getFeatureMap
#[test]
fn unresolved_target_lib_name_reports_2583_with_suggested_lib() {
    for (name, lib) in [
        ("Map", "es2015"),
        ("Set", "es2015"),
        ("Promise", "es2015"),
        ("Symbol", "es2015"),
        ("WeakMap", "es2015"),
        ("WeakSet", "es2015"),
        ("Iterator", "es2015"),
        ("AsyncIterator", "es2015"),
        ("Reflect", "es2015"),
        ("SharedArrayBuffer", "es2017"),
        ("Atomics", "es2017"),
        ("AsyncIterable", "es2018"),
        ("AsyncIterableIterator", "es2018"),
        ("AsyncGenerator", "es2018"),
        ("AsyncGeneratorFunction", "es2018"),
        ("BigInt", "es2020"),
        ("BigInt64Array", "es2020"),
        ("BigUint64Array", "es2020"),
    ] {
        let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &format!("{name};")));
        let root = p.root();
        let mut c = Checker::new_checker(p);
        let diags = c.get_diagnostics(root);
        assert_eq!(diags.len(), 1, "{name}: got {diags:?}");
        assert_eq!(diags[0].code, 2583, "{name}: expected target-lib hint 2583");
        assert_eq!(
            diags[0].message,
            format!("Cannot find name '{name}'. Do you need to change your target library? Try changing the 'lib' compiler option to '{lib}' or later.")
        );
    }
}

// Slice 7: an undefined shorthand property (`{ x }` where `x` is not in scope)
// reports TS18004 — the default arm checks `node.Parent.Kind ==
// ShorthandPropertyAssignment` before falling back to TS2304.
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName
//     (default arm, ShorthandPropertyAssignment)
#[test]
fn undefined_shorthand_property_reports_18004() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "const o = { x };"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].code, 18004);
    assert_eq!(
        diags[0].message,
        "No value exists in scope for the shorthand property 'x'. Either declare one or provide an initializer."
    );
}

// Slice 8 (guard): an ordinary undefined name (not in any specialized arm and
// not a shorthand property) STILL reports the generic TS2304 — the default arm
// is unchanged. Proves the dispatch did not blanket-replace TS2304.
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName (default -> Cannot_find_name_0)
#[test]
fn ordinary_undefined_name_still_reports_2304() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "foo;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'foo'.");
}

// Slice 9a: `$` points at `@types/jquery` (TS2592 default / TS2581 wildcard).
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName ("$")
#[test]
fn unresolved_dollar_reports_jquery_hint() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "$;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].code, 2592);
    assert_eq!(
        diags[0].message,
        "Cannot find name '$'. Do you need to install type definitions for jQuery? Try `npm i --save-dev @types/jquery` and then add 'jquery' to the types field in your tsconfig."
    );
}

// Slice 9a (wildcard): with `types: ["*"]`, `$` is the shorter TS2581 variant.
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName ("$")
#[test]
fn unresolved_dollar_with_wildcard_types_reports_2581() {
    use tsgo_core::compileroptions::CompilerOptions;
    let options = CompilerOptions {
        types: vec!["*".to_string()],
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts", "$;", options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].code, 2581);
    assert_eq!(
        diags[0].message,
        "Cannot find name '$'. Do you need to install type definitions for jQuery? Try `npm i --save-dev @types/jquery`."
    );
}

// Slice 9b: the test-runner globals point at `@types/jest`/`@types/mocha`
// (TS2593 default / TS2582 wildcard).
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName
//     ("beforeEach", "describe", "suite", "it", "test")
#[test]
fn unresolved_test_runner_names_report_2593() {
    for name in ["beforeEach", "describe", "suite", "it", "test"] {
        let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &format!("{name};")));
        let root = p.root();
        let mut c = Checker::new_checker(p);
        let diags = c.get_diagnostics(root);
        assert_eq!(diags.len(), 1, "{name}: got {diags:?}");
        assert_eq!(
            diags[0].code, 2593,
            "{name}: expected test-runner hint 2593"
        );
        assert_eq!(
            diags[0].message,
            format!("Cannot find name '{name}'. Do you need to install type definitions for a test runner? Try `npm i --save-dev @types/jest` or `npm i --save-dev @types/mocha` and then add 'jest' or 'mocha' to the types field in your tsconfig.")
        );
    }
}

// Slice 9c: `Bun` points at `@types/bun` (TS2868 default / TS2867 wildcard).
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName ("Bun")
#[test]
fn unresolved_bun_reports_2868() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "Bun;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "got {diags:?}");
    assert_eq!(diags[0].code, 2868);
    assert_eq!(
        diags[0].message,
        "Cannot find name 'Bun'. Do you need to install type definitions for Bun? Try `npm i --save-dev @types/bun` and then add 'bun' to the types field in your tsconfig."
    );
}

// Slice 9d: an unresolved `await` whose parent is a CallExpression
// (`await(0)` in a non-async script, where `await` is an identifier callee)
// reports TS2311 "Did you mean to write this in an async function?".
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName ("await")
#[test]
fn unresolved_await_callee_reports_2311() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "await(0);"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2311
            && d.message
                == "Cannot find name 'await'. Did you mean to write this in an async function?"),
        "expected TS2311 for the `await` callee: {diags:?}"
    );
}

// Slice 9d (fallthrough): an unresolved bare `await` that is NOT a call callee
// falls through Go's `case "await"` to the default arm -> generic TS2304.
// Go: internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName ("await" fallthrough)
#[test]
fn unresolved_bare_await_falls_through_to_2304() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "await;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags
            .iter()
            .any(|d| d.code == 2304 && d.message == "Cannot find name 'await'."),
        "a bare `await` (not a call callee) falls through to 2304: {diags:?}"
    );
}

// Direct unit test of the pure `getSuggestedLibForNonExistentName` port: every
// expected `name -> first-lib` (Go literals from `getFeatureMap`), spanning each
// distinct lib bucket, plus the empty result for an unknown name. Includes names
// that are NOT in the TS2583 switch arm (`Array`/`Math`/`Date`) to prove the
// function mirrors Go for ALL inputs (the lib arg is harmlessly ignored when the
// resolved message has no `{1}`).
// Go: internal/checker/checker.go:Checker.getSuggestedLibForNonExistentName
//     + internal/checker/utilities.go:getFeatureMap
#[test]
fn get_suggested_lib_for_non_existent_name_matches_feature_map() {
    use super::get_suggested_lib_for_non_existent_name as lib_of;
    assert_eq!(lib_of("Array"), "es2015");
    assert_eq!(lib_of("Map"), "es2015");
    assert_eq!(lib_of("Promise"), "es2015");
    assert_eq!(lib_of("Symbol"), "es2015");
    assert_eq!(lib_of("Reflect"), "es2015");
    assert_eq!(lib_of("Math"), "es2015");
    assert_eq!(lib_of("Atomics"), "es2017");
    assert_eq!(lib_of("SharedArrayBuffer"), "es2017");
    assert_eq!(lib_of("AsyncGenerator"), "es2018");
    assert_eq!(lib_of("Intl"), "es2018");
    assert_eq!(lib_of("BigInt"), "es2020");
    assert_eq!(lib_of("BigInt64Array"), "es2020");
    assert_eq!(lib_of("DataView"), "es2020");
    assert_eq!(lib_of("Int8Array"), "es2022");
    assert_eq!(lib_of("Error"), "es2022");
    assert_eq!(lib_of("MapConstructor"), "es2024");
    assert_eq!(lib_of("ArrayBuffer"), "es2024");
    assert_eq!(lib_of("Float16Array"), "es2025");
    assert_eq!(lib_of("RegExpConstructor"), "es2025");
    assert_eq!(lib_of("Date"), "esnext");
    assert_eq!(lib_of("DisposableStack"), "esnext");
    // Unknown / non-feature names -> empty (so the caller passes only `{0}`).
    assert_eq!(lib_of("module"), "");
    assert_eq!(lib_of("foo"), "");
    assert_eq!(lib_of(""), "");
}

// ---------------------------------------------------------------------------
// Round 8 — CommonJS module/exports resolution: once the binder sees a CommonJS
// module indicator in a JS file (`module.exports = X`, `exports.x = Y`, or a
// `require(...)` call), it declares `module`/`exports` as file locals, so they
// resolve through the normal scope walk and no longer report TS2304/TS2591.
// Their declared type is benign (`any`-like), so member access on them
// (`module.exports`, `exports.foo`) does not spuriously report TS2339.
// Go: internal/binder/binder.go:declareCommonJSVariable
//     + internal/checker/checker.go:Checker.getTypeOfSymbol (SymbolFlagsModuleExports)
// ---------------------------------------------------------------------------

// Headline: `module.exports = {};` in a JS file resolves `module` (no 2304/2591)
// and the `module.exports` member access does not report 2339.
#[test]
fn js_module_exports_assignment_resolves_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_js(
        "/a.js",
        "module.exports = {};",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2304 && d.code != 2591),
        "`module` must resolve via CommonJS binding (no 2304/2591): {diags:?}"
    );
    assert!(
        diags.iter().all(|d| d.code != 2339),
        "`module.exports` member access on the benign CJS type must not report 2339: {diags:?}"
    );
}

// `exports.foo = 1;` resolves `exports` (no 2304) and the `exports.foo` member
// access does not report 2339.
#[test]
fn js_exports_property_assignment_resolves_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_js("/a.js", "exports.foo = 1;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2304 && d.code != 2591),
        "`exports` must resolve via CommonJS binding (no 2304/2591): {diags:?}"
    );
    assert!(
        diags.iter().all(|d| d.code != 2339),
        "`exports.foo` member access on the benign CJS type must not report 2339: {diags:?}"
    );
}

// A bare `module` reference resolves once any CommonJS indicator is present,
// even one introduced by a `require(...)` call elsewhere in the file.
#[test]
fn js_require_call_makes_bare_module_resolve() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_js(
        "/a.js",
        "const x = require(\"./y\"); module; exports;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2304 && d.code != 2591),
        "a require(...) indicator makes `module`/`exports` resolve (no 2304/2591): {diags:?}"
    );
}

// GUARD: a JS file with NO CommonJS indicator keeps `module` unresolved → the
// Round 7 node-typedefs hint TS2591 is preserved (no over-resolution).
// Go: internal/binder/binder.go:setCommonJSModuleIndicator
#[test]
fn js_without_commonjs_indicator_module_still_reports_2591() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_js(
        "/a.js",
        "var y = 1; module;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2591),
        "a plain JS file with no CJS pattern keeps `module` unresolved (2591): {diags:?}"
    );
}

// Round 22 (RED->GREEN headline): a statement after `return` in a function body
// is unreachable, so with `allowUnreachableCode: false` the checker reports
// `TS7027 Unreachable code detected.` (category Error) on the unreachable
// statement, spanning exactly that statement (`GetTokenPosOfNode(stmt)..stmt
// .End()` — the name-less generic range that skips the leading trivia).
// Go: internal/checker/checker.go:Checker.checkSourceElementUnreachable(2374)
#[test]
fn unreachable_const_after_return_reports_ts7027() {
    let text = "function f() { return 1; const x = 2; }";
    let options = tsgo_core::compileroptions::CompilerOptions {
        allow_unreachable_code: tsgo_core::tristate::Tristate::False,
        ..Default::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts", text, options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts7027: Vec<_> = diags.iter().filter(|d| d.code == 7027).collect();
    assert_eq!(
        ts7027.len(),
        1,
        "expected exactly one TS7027, got {diags:?}"
    );
    let d = ts7027[0];
    assert_eq!(d.category, tsgo_diagnostics::Category::Error);
    assert_eq!(d.message, "Unreachable code detected.");
    let start = text.find("const").unwrap() as i32;
    assert_eq!(d.start, start, "span starts at the unreachable statement");
    assert_eq!(
        d.length,
        "const x = 2;".len() as i32,
        "span covers the unreachable statement"
    );
}

// Round 29 (RED->GREEN headline): a function declaration whose return-type
// annotation is a type predicate (`value is TypeA`) naming a parameter the
// declaration does NOT have reports `TS1225: Cannot find parameter 'value'.`,
// spanning exactly the predicate parameter-name identifier. Mirrors the corpus
// case `typePredicateParameterMismatch`.
// Go: internal/checker/checker.go:Checker.checkTypePredicate(3037)
#[test]
fn type_predicate_naming_unknown_parameter_reports_ts1225() {
    let text = "function isA(_value: object): value is object { return true; }";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts1225: Vec<_> = diags.iter().filter(|d| d.code == 1225).collect();
    assert_eq!(
        ts1225.len(),
        1,
        "expected exactly one TS1225, got {diags:?}"
    );
    let d = ts1225[0];
    assert_eq!(d.message, "Cannot find parameter 'value'.");
    let start = text.find("value is").unwrap() as i32;
    assert_eq!(
        d.start, start,
        "span starts at the predicate parameter name"
    );
    assert_eq!(d.length, "value".len() as i32, "span covers the name only");
}

// Round 29 (RED->GREEN): the `asserts <name>` predicate variant (no `is T`
// type) is checked the same way — an assertion predicate naming an unknown
// parameter reports `TS1225` at the asserted name. Mirrors the corpus case
// `assertsPredicateParameterMismatch`.
// Go: internal/checker/checker.go:Checker.checkTypePredicate(3037)
#[test]
fn asserts_predicate_naming_unknown_parameter_reports_ts1225() {
    let text = "function assertC(_condition: boolean): asserts condition { }";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts1225: Vec<_> = diags.iter().filter(|d| d.code == 1225).collect();
    assert_eq!(
        ts1225.len(),
        1,
        "expected exactly one TS1225, got {diags:?}"
    );
    let d = ts1225[0];
    assert_eq!(d.message, "Cannot find parameter 'condition'.");
    let start = text.find("condition {").unwrap() as i32;
    assert_eq!(d.start, start, "span starts at the asserted name");
    assert_eq!(
        d.length,
        "condition".len() as i32,
        "span covers the name only"
    );
}

// Round 29 (GUARD, no over-fire): a CORRECT predicate whose name matches a
// real parameter must NOT report `TS1225` — the parameter index is found.
// Go: internal/checker/checker.go:Checker.checkTypePredicate (parameterIndex >= 0)
#[test]
fn type_predicate_matching_parameter_reports_no_ts1225() {
    let text = "function isA(value: object): value is object { return true; }";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 1225),
        "a correct predicate must not report TS1225: {diags:?}"
    );
}

// Round 29 (GUARD, `this` predicate skipped): a `this`-typed predicate
// (`this is T`) has no parameter-name identifier to resolve, so the TS1225
// path is skipped (Go skips `TypePredicateKindThis`).
// Go: internal/checker/checker.go:Checker.checkTypePredicate (kind != This)
#[test]
fn this_type_predicate_reports_no_ts1225() {
    let text = "function f(): this is object { return true; }";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 1225),
        "a `this` predicate must not report TS1225: {diags:?}"
    );
}

// Round 29 (GUARD, binding-pattern element -> TS1230 not TS1225): when the
// predicate name is an element of a destructured (binding-pattern) parameter,
// Go reports `TS1230` ("cannot reference element ... in a binding pattern"),
// NOT `TS1225` — so the binding-pattern guard suppresses the cannot-find.
// Go: internal/checker/checker.go:Checker.checkIfTypePredicateVariableIsDeclaredInBindingPattern(3091)
#[test]
fn type_predicate_naming_binding_element_reports_ts1230_not_ts1225() {
    let text = "function isA({ value }: { value: object }): value is object { return true; }";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 1225),
        "a binding-pattern element predicate must not report TS1225: {diags:?}"
    );
    let ts1230: Vec<_> = diags.iter().filter(|d| d.code == 1230).collect();
    assert_eq!(
        ts1230.len(),
        1,
        "expected exactly one TS1230 instead, got {diags:?}"
    );
    assert_eq!(
        ts1230[0].message,
        "A type predicate cannot reference element 'value' in a binding pattern."
    );
}

// Round 31 (TS2309, export-assignment conflict — headline flip): a module that
// has an `export =` AND another *value* export reports `TS2309` on the
// `export =` statement (Go's `checkExternalModuleExports` ->
// `hasExportedMembersOfKind(moduleSymbol, Value)`). Mirrors the corpus
// `exportAssignmentMerging4` shape (`export const x` + `export = { ... }`).
// The span is the whole `export = { a: 1 };` statement (the export-assignment
// node, whose `end` includes the trailing `;`), trivia-skipped — matching
// `tsc`'s `a.ts(6,1)` baseline byte-for-byte.
// Go: internal/checker/checker.go:Checker.checkExternalModuleExports(5663)
#[test]
fn export_equals_with_value_export_reports_ts2309() {
    let text = "export const x = 42;\nexport = { a: 1 };";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts2309: Vec<_> = diags.iter().filter(|d| d.code == 2309).collect();
    assert_eq!(
        ts2309.len(),
        1,
        "expected exactly one TS2309, got {diags:?}"
    );
    assert_eq!(
        ts2309[0].message,
        "An export assignment cannot be used in a module with other exported elements."
    );
    // The reported span is the entire `export = { a: 1 };` statement (offset 21,
    // 18 bytes incl. the trailing `;`), trivia-skipped to the `export` keyword.
    assert_eq!(ts2309[0].start, 21, "span starts at the `export` keyword");
    assert_eq!(
        ts2309[0].length, 18,
        "span covers the whole export-assignment statement incl. the `;`"
    );
}

// Round 31 (a CLASS value export also conflicts): mirrors the corpus
// `exportAssignmentMerging10` shape (`export class Base` + `export = Foo`) —
// a class is a value (`SymbolFlags::CLASS`), so `hasExportedMembersOfKind`
// counts it and TS2309 fires.
// Go: internal/checker/checker.go:Checker.checkExternalModuleExports(5663)
#[test]
fn export_equals_with_class_export_reports_ts2309() {
    let text = "export class C {}\nexport = { a: 1 };";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts2309: Vec<_> = diags.iter().filter(|d| d.code == 2309).collect();
    assert_eq!(
        ts2309.len(),
        1,
        "a class value export conflicts with `export =`: {diags:?}"
    );
}

// Round 31 (faithfulness: an INSTANTIATED namespace counts as a value): a
// namespace that has a runtime form (`export const v`) is bound by Go as a
// `ValueModule` (Go's `declareModuleSymbol`: ValueModule iff
// `GetModuleInstanceState != NonInstantiated`), so it IS a value member and
// TS2309 fires. This proves the membership gate distinguishes an instantiated
// (value) namespace from a type-only one, rather than blanket-excluding modules.
// Go: internal/checker/checker.go:Checker.hasExportedMembersOfKind(5708)
#[test]
fn export_equals_with_instantiated_namespace_reports_ts2309() {
    let text = "export namespace N { export const v = 1; }\nexport = { a: 1 };";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts2309: Vec<_> = diags.iter().filter(|d| d.code == 2309).collect();
    assert_eq!(
        ts2309.len(),
        1,
        "an instantiated (value) namespace conflicts with `export =`: {diags:?}"
    );
}

// Round 31 (GUARD, sole export — NO over-fire): an `export =` that is the ONLY
// export reports NO TS2309 (Go's `hasExportedMembersOfKind` returns false — the
// only export is the export-equals symbol itself, which is skipped by name).
// Go: internal/checker/checker.go:Checker.hasExportedMembersOfKind (skip InternalSymbolNameExportEquals)
#[test]
fn export_equals_sole_export_reports_no_ts2309() {
    let text = "export = { a: 1 };";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2309),
        "a sole `export =` must not report TS2309: {diags:?}"
    );
}

// Round 31 (GUARD, type-only export — NO over-fire): `export type T` is type-only
// (`SymbolFlags::TYPE_ALIAS`, not `Value`), so it does NOT count as an "other
// exported element" — mirrors the corpus `exportAssignmentMerging8`
// (`export = Foo` + `export { SomeTypeAlias }`) which `tsc` does NOT flag.
// Go: internal/checker/checker.go:Checker.hasExportedMembersOfKind (kind = Value)
#[test]
fn export_equals_with_type_alias_only_reports_no_ts2309() {
    let text = "export type T = { x: number };\nexport = { a: 1 };";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2309),
        "a type-only export must not trigger TS2309: {diags:?}"
    );
}

// Round 31 (GUARD, interface-only export — NO over-fire): `export interface I`
// is type-only (`SymbolFlags::INTERFACE`), so it is not a value member.
// Go: internal/checker/checker.go:Checker.hasExportedMembersOfKind (kind = Value)
#[test]
fn export_equals_with_interface_only_reports_no_ts2309() {
    let text = "export interface I { x: number }\nexport = { a: 1 };";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2309),
        "an interface-only export must not trigger TS2309: {diags:?}"
    );
}

// Round 31 (GUARD, type-only NAMESPACE — the critical over-fire trap): a
// namespace that contains ONLY types (`export interface Baz`) is type-only.
// Go's binder gives it `NamespaceModule` (not a `Value`), so it does NOT count.
// The Rust binder over-assigns `ValueModule` to EVERY namespace (the
// instance-state split is DEFERRED there), so `hasExportedMembersOfKind` MUST
// re-derive the module instance state and exclude the non-instantiated
// namespace — otherwise it over-fires on the corpus `exportAssignmentMerging1`
// (`export type Foo` + `export namespace Bar { export interface Baz }` +
// `export = { ... }`), which `tsc` does NOT flag.
// Go: internal/checker/checker.go:Checker.hasExportedMembersOfKind +
//     internal/binder/binder.go:Binder.declareModuleSymbol (ValueModule iff instantiated)
#[test]
fn export_equals_with_type_only_namespace_reports_no_ts2309() {
    let text = "export namespace Bar { export interface Baz { y: number } }\nexport = { a: 1 };";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2309),
        "a type-only (non-instantiated) namespace must not trigger TS2309: {diags:?}"
    );
}

// -----------------------------------------------------------------------
// T1-C1: checkTypeForDuplicateIndexSignatures (TS2374)
// -----------------------------------------------------------------------

// An interface with two string index signatures must report TS2374 on each.
// Go: internal/checker/checker.go:Checker.checkTypeForDuplicateIndexSignatures
#[test]
fn duplicate_index_signatures_interface_reports_ts2374() {
    let text = "interface I {\n  [a: string]: number;\n  [b: string]: string;\n}";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts2374: Vec<_> = diags.iter().filter(|d| d.code == 2374).collect();
    assert_eq!(
        ts2374.len(),
        2,
        "expected two TS2374 diagnostics for duplicate string index signatures: {diags:?}"
    );
    assert!(
        ts2374[0].message.contains("string"),
        "message should mention 'string': {}",
        ts2374[0].message
    );
}

// A class with two number index signatures must report TS2374 on each.
// Go: internal/checker/checker.go:Checker.checkTypeForDuplicateIndexSignatures
#[test]
fn duplicate_index_signatures_class_reports_ts2374() {
    let text = "class C {\n  [a: number]: string;\n  [b: number]: number;\n}";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts2374: Vec<_> = diags.iter().filter(|d| d.code == 2374).collect();
    assert_eq!(
        ts2374.len(),
        2,
        "expected two TS2374 diagnostics for duplicate number index signatures: {diags:?}"
    );
    assert!(
        ts2374[0].message.contains("number"),
        "message should mention 'number': {}",
        ts2374[0].message
    );
}

// GUARD: a single index signature should NOT fire TS2374.
// Go: internal/checker/checker.go:Checker.checkTypeForDuplicateIndexSignatures
#[test]
fn single_index_signature_no_ts2374() {
    let text = "interface I {\n  [k: string]: number;\n}";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2374),
        "a single index signature must not trigger TS2374: {diags:?}"
    );
}

// GUARD: a string AND number index signature on the same interface is NOT a
// duplicate — they are different key types.
// Go: internal/checker/checker.go:Checker.checkTypeForDuplicateIndexSignatures
#[test]
fn different_index_signature_key_types_no_ts2374() {
    let text = "interface I {\n  [k: string]: number;\n  [k: number]: string;\n}";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2374),
        "different key-type index signatures must not trigger TS2374: {diags:?}"
    );
}

// -----------------------------------------------------------------------
// T1-C3: checkParameter — constructor parameter properties
// -----------------------------------------------------------------------

// A parameter property modifier (`public x: string`) on a parameter NOT in a
// constructor with a body must report TS2369.
// Go: internal/checker/checker.go:Checker.checkParameter (ParameterPropertyModifier arm)
#[test]
fn parameter_property_outside_constructor_body_reports_ts2369() {
    let text = "declare class C { constructor(public x: string); }";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts2369: Vec<_> = diags.iter().filter(|d| d.code == 2369).collect();
    assert_eq!(
        ts2369.len(),
        1,
        "expected TS2369 for parameter property outside constructor body: {diags:?}"
    );
}

// A parameter property named `constructor` must report TS2398.
// Go: internal/checker/checker.go:Checker.checkParameter (paramName == "constructor")
#[test]
fn parameter_property_named_constructor_reports_ts2398() {
    let text = "class C { constructor(public constructor: string) {} }";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts2398: Vec<_> = diags.iter().filter(|d| d.code == 2398).collect();
    assert_eq!(
        ts2398.len(),
        1,
        "expected TS2398 for parameter property named 'constructor': {diags:?}"
    );
}

// GUARD: a valid `constructor(public x: string) {}` must NOT report TS2369.
// Go: internal/checker/checker.go:Checker.checkParameter
#[test]
fn valid_parameter_property_in_constructor_no_ts2369() {
    let text = "class C { constructor(public x: string) {} }";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2369),
        "valid parameter property must not trigger TS2369: {diags:?}"
    );
}

// A `this` parameter that's not the first must report TS2680.
// Go: internal/checker/checker.go:Checker.checkParameter (this/new arm)
#[test]
fn this_parameter_not_first_reports_ts2680() {
    let text = "function f(x: number, this: string) {}";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts2680: Vec<_> = diags.iter().filter(|d| d.code == 2680).collect();
    assert_eq!(
        ts2680.len(),
        1,
        "expected TS2680 for non-first 'this' parameter: {diags:?}"
    );
}

// A `this` parameter in a constructor must report TS2681.
// Go: internal/checker/checker.go:Checker.checkParameter (constructor arm)
#[test]
fn this_parameter_in_constructor_reports_ts2681() {
    let text = "class C { constructor(this: C) {} }";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts2681: Vec<_> = diags.iter().filter(|d| d.code == 2681).collect();
    assert_eq!(
        ts2681.len(),
        1,
        "expected TS2681 for 'this' parameter in constructor: {diags:?}"
    );
}

// A `this` parameter in an arrow function must report TS2730.
// Go: internal/checker/checker.go:Checker.checkParameter (arrow function arm)
#[test]
fn this_parameter_in_arrow_reports_ts2730() {
    let text = "const f = (this: string) => {};";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", text));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let ts2730: Vec<_> = diags.iter().filter(|d| d.code == 2730).collect();
    assert_eq!(
        ts2730.len(),
        1,
        "expected TS2730 for 'this' parameter in arrow function: {diags:?}"
    );
}

// ---------- T1-C5: Unused locals/parameters (TS6133/TS6196) ----------

// Go: internal/checker/checker.go:checkUnusedLocalsAndParameters
// (noUnusedLocals -> TS6133 for unused variable in function body)
#[test]
fn unused_local_in_function_reports_6133() {
    let mut opts = CompilerOptions::default();
    opts.no_unused_locals = Tristate::True;
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "function f() { const x = 1; }",
        opts,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let unused = diags.iter().find(|d| d.code == 6133);
    assert!(
        unused.is_some(),
        "expected TS6133 for unused 'x'; got: {:?}",
        diags
    );
    assert!(
        unused.unwrap().message.contains("'x'"),
        "expected message to mention 'x'; got: {}",
        unused.unwrap().message
    );
}

// Go: internal/checker/checker.go:checkUnusedLocalsAndParameters
// (noUnusedParameters -> TS6133 for unused parameter)
#[test]
fn unused_parameter_reports_6133() {
    let mut opts = CompilerOptions::default();
    opts.no_unused_parameters = Tristate::True;
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "function f(x: number) {}",
        opts,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let unused = diags.iter().find(|d| d.code == 6133);
    assert!(
        unused.is_some(),
        "expected TS6133 for unused parameter 'x'; got: {:?}",
        diags
    );
}

// Go: internal/checker/checker.go:isUnreferencedVariableDeclaration
// (parameter starting with _ -> no unused report)
#[test]
fn underscore_parameter_suppresses_unused_report() {
    let mut opts = CompilerOptions::default();
    opts.no_unused_parameters = Tristate::True;
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "function f(_x: number) {}",
        opts,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let unused = diags.iter().find(|d| d.code == 6133);
    assert!(
        unused.is_none(),
        "parameter starting with _ should suppress TS6133; got: {:?}",
        diags
    );
}

// Go: internal/checker/checker.go:reportUnusedVariables
// (all variables unused -> TS6199 "All variables are unused.")
#[test]
fn all_variables_unused_reports_6199() {
    let mut opts = CompilerOptions::default();
    opts.no_unused_locals = Tristate::True;
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "function f() { const a = 1, b = 2; }",
        opts,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let all_unused = diags.iter().find(|d| d.code == 6199);
    assert!(
        all_unused.is_some(),
        "expected TS6199 for all unused variables; got: {:?}",
        diags
    );
}

// Go: internal/checker/checker.go:checkUnusedLocalsAndParameters
// (referenced variable -> no error)
#[test]
fn referenced_local_no_unused_error() {
    let mut opts = CompilerOptions::default();
    opts.no_unused_locals = Tristate::True;
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "function f() { const x = 1; return x; }",
        opts,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let unused = diags
        .iter()
        .find(|d| d.code == 6133 || d.code == 6196 || d.code == 6199);
    assert!(
        unused.is_none(),
        "referenced variable should not trigger unused; got: {:?}",
        diags
    );
}

// ========== T1-D5: checkSatisfiesExpression ==========

// Go: internal/checker/checker.go:checkSatisfiesExpression
// `satisfies` returns the expression type, not the target type.
#[test]
fn satisfies_expression_returns_expression_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: 'hello'; x satisfies string;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    let s = c.string_type();
    assert_ne!(
        t, s,
        "satisfies should return the narrow expression type, not the target string"
    );
}

// Go: internal/checker/checker.go:checkSatisfiesExpression
// `satisfies` reports TS1360 when the expression does not satisfy the target.
#[test]
fn satisfies_expression_incompatible_reports_1360() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "42 satisfies string;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1360);
    assert!(
        d.is_some(),
        "expected TS1360 (type does not satisfy); got: {:?}",
        diags
    );
}

// ========== T1-D8: checkTypeParameterListsIdentical ==========

// Go: internal/checker/checker.go:checkTypeParameterListsIdentical
// Identical interface type-parameter lists -> no error
#[test]
fn type_parameter_lists_identical_no_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Foo<T> { x: T; }\ninterface Foo<T> { y: T; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 2428);
    assert!(
        d.is_none(),
        "identical type parameter lists should produce no TS2428; got: {:?}",
        diags
    );
}

// Go: internal/checker/checker.go:checkTypeParameterListsIdentical
// Different type-parameter names -> TS2428
#[test]
fn type_parameter_lists_different_names_reports_2428() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Foo<T> { x: T; }\ninterface Foo<U> { y: U; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 2428);
    assert!(
        d.is_some(),
        "expected TS2428 (identical type parameters required); got: {:?}",
        diags
    );
}

// Go: internal/checker/checker.go:checkTypeParameterListsIdentical
// Different number of type parameters -> TS2428
#[test]
fn type_parameter_lists_different_count_reports_2428() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Foo<T> { x: T; }\ninterface Foo<T, U> { y: T; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 2428);
    assert!(
        d.is_some(),
        "expected TS2428 (identical type parameters required); got: {:?}",
        diags
    );
}

// ========== T1-D5b: checkMetaProperty / checkNewTargetMetaProperty ==========

// Go: internal/checker/checker.go:checkNewTargetMetaProperty
// `new.target` at top level (no function container) -> TS17013
#[test]
fn new_target_outside_function_reports_17013() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "new.target;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 17013);
    assert!(
        d.is_some(),
        "expected TS17013 (new.target only allowed in function body); got: {:?}",
        diags
    );
}

// Go: internal/checker/checker.go:checkNewTargetMetaProperty
// `new.target` inside a function -> no TS17013
#[test]
fn new_target_inside_function_no_17013() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f() { new.target; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 17013);
    assert!(
        d.is_none(),
        "new.target inside function should not trigger TS17013; got: {:?}",
        diags
    );
}

#[test]
fn satisfies_expression_compatible_no_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string; x satisfies string;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1360);
    assert!(
        d.is_none(),
        "compatible satisfies should produce no TS1360; got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// C6: Checker::resolve_entity_name
// ---------------------------------------------------------------------------

// Go: internal/checker/checker.go:Checker.resolveEntityName
// Resolve a locally declared variable through resolveEntityName (single identifier).
#[test]
fn resolve_entity_name_resolves_local_variable() {
    let p = StubProgram::parse_and_bind("/a.ts", "const x = 1;\nx;");
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let x_ref = match arena.data(stmts[1]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    assert_eq!(arena.kind(x_ref), tsgo_ast::Kind::Identifier);
    let mut c = Checker::new();
    let result = c.resolve_entity_name(
        &p,
        x_ref,
        tsgo_ast::SymbolFlags::VALUE,
        false, // ignore_errors
        false, // dont_resolve_alias
        None,  // location
    );
    assert!(result.is_some(), "should resolve local 'x'");
    assert_eq!(p.symbol(result.unwrap()).name, "x");
}

// Go: internal/checker/checker.go:Checker.resolveEntityName
// Resolve an identifier from an enclosing (outer) scope.
#[test]
fn resolve_entity_name_resolves_from_outer_scope() {
    let p = StubProgram::parse_and_bind("/a.ts", "const outer = 1;\nfunction f() { outer; }");
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    // stmts[1] = function f() { outer; }
    let body = match arena.data(stmts[1]) {
        NodeData::FunctionDeclaration(d) => d.body.expect("function body"),
        _ => panic!("function declaration"),
    };
    let body_stmts = match arena.data(body) {
        NodeData::Block(d) => d.list.nodes.clone(),
        _ => panic!("block"),
    };
    let outer_ref = match arena.data(body_stmts[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    assert_eq!(arena.kind(outer_ref), tsgo_ast::Kind::Identifier);
    let mut c = Checker::new();
    let result = c.resolve_entity_name(
        &p,
        outer_ref,
        tsgo_ast::SymbolFlags::VALUE,
        false,
        false,
        None,
    );
    assert!(
        result.is_some(),
        "should resolve 'outer' from enclosing scope"
    );
    assert_eq!(p.symbol(result.unwrap()).name, "outer");
}

// Go: internal/checker/checker.go:Checker.resolveEntityName
// An unresolved identifier with ignore_errors=false should report TS2304.
#[test]
fn resolve_entity_name_unresolved_reports_2304() {
    // Note: this test is separate from check_identifier 2304 tests — it verifies
    // the Checker::resolve_entity_name method's error-reporting path directly.
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "missing;"));
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let missing_ref = match arena.data(stmts[0]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    assert_eq!(arena.kind(missing_ref), tsgo_ast::Kind::Identifier);
    let mut c = Checker::new_checker(p.clone());
    let result = c.resolve_entity_name(
        p.as_ref(),
        missing_ref,
        tsgo_ast::SymbolFlags::VALUE,
        false, // ignore_errors = false -> should report
        false,
        None,
    );
    assert!(result.is_none(), "should not resolve 'missing'");
    let root = p.root();
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 2304);
    assert!(
        d.is_some(),
        "expected TS2304 from resolve_entity_name; got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// C7: checkExportSpecifier + checkExportAssignment
// ---------------------------------------------------------------------------

// Go: internal/checker/checker.go:Checker.checkExportSpecifier
// A valid export specifier referencing a locally declared name → no error.
#[test]
fn check_export_specifier_valid_no_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x = 1;\nexport { x };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 2304);
    assert!(
        d.is_none(),
        "valid export specifier should produce no TS2304; got: {:?}",
        diags
    );
}

// Go: internal/checker/checker.go:Checker.checkExportSpecifier
// Exporting a name that does not exist in the local scope → TS2304.
#[test]
fn check_export_specifier_nonexistent_name_reports_2304() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "export { missing };"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 2304);
    assert!(
        d.is_some(),
        "expected TS2304 for non-existent export specifier; got: {:?}",
        diags
    );
}

// Go: internal/checker/checker.go:Checker.checkExportAssignment
// `export =` inside a namespace → TS1063.
#[test]
fn check_export_assignment_in_namespace_reports_1063() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "namespace N {\n  export = 1;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1063);
    assert!(
        d.is_some(),
        "expected TS1063 (export= cannot be used in namespace); got: {:?}",
        diags
    );
}

// ============================================================================
// checkTypeOfExpression — `typeof x` returns string type
// ============================================================================

// Go: internal/checker/checker.go:Checker.checkTypeOfExpression(10577)
#[test]
fn check_typeof_expression_returns_string_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: number;\ntypeof x;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let s = c.string_type();
    assert_eq!(c.check_expression(&p, usage), s);
}

#[test]
fn check_typeof_expression_checks_inner_expression() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "typeof unknownVar;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "expected TS2304 (Cannot find name) for inner expression; got: {:?}",
        diags
    );
}

// ============================================================================
// checkVoidExpression — `void x` returns undefined type
// ============================================================================

// Go: internal/checker/checker.go:Checker.checkVoidExpression(10799)
#[test]
fn check_void_expression_returns_undefined_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const x: number;\nvoid x;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let u = c.undefined_type();
    assert_eq!(c.check_expression(&p, usage), u);
}

#[test]
fn check_void_zero_returns_undefined_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "void 0;");
    let usage = expr_stmt_expression(&p, 0);
    let mut c = Checker::new();
    let u = c.undefined_type();
    assert_eq!(c.check_expression(&p, usage), u);
}

// ============================================================================
// checkDeleteExpression — `delete obj.x` returns boolean; invalid operand
// reports TS2703
// ============================================================================

// Go: internal/checker/checker.go:Checker.checkDeleteExpression(10763)
#[test]
fn check_delete_expression_returns_boolean_type() {
    let p =
        StubProgram::parse_and_bind("/a.ts", "declare const obj: { x: number };\ndelete obj.x;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let b = c.boolean_type();
    assert_eq!(c.check_expression(&p, usage), b);
}

#[test]
fn check_delete_non_property_reports_2703() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: number;\ndelete x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2703),
        "expected TS2703 (operand of delete must be a property reference); got: {:?}",
        diags
    );
}

#[test]
fn check_delete_element_access_no_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const obj: { [k: string]: number };\ndelete obj[\"x\"];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2703),
        "delete on element access should not report 2703; got: {:?}",
        diags
    );
}

// ============================================================================
// checkWithStatement — reports TS2410 (with statement not supported)
// ============================================================================

// Go: internal/checker/checker.go:Checker.checkWithStatement(4129)
#[test]
fn check_with_statement_reports_2410() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const obj: any;\nwith (obj) { }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2410),
        "expected TS2410 (with statement not supported); got: {:?}",
        diags
    );
}

#[test]
fn check_with_statement_checks_expression() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "with (unknownObj) { }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "expected TS2304 (cannot find name) for with expression; got: {:?}",
        diags
    );
}

// ============================================================================
// checkLabeledStatement — reports TS1114 for duplicate labels
// ============================================================================

// Go: internal/checker/checker.go:Checker.checkLabeledStatement(4180)
#[test]
fn check_labeled_statement_reports_duplicate_label() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "foo: foo: 1;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 1114),
        "expected TS1114 (Duplicate label); got: {:?}",
        diags
    );
}

#[test]
fn check_labeled_statement_no_error_for_unique_labels() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "foo: bar: 1;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 1114),
        "unique labels should not report duplicate; got: {:?}",
        diags
    );
}

// ============================================================================
// checkThrowStatement — validates grammar (line-break check)
// ============================================================================

// Go: internal/checker/checker.go:Checker.checkThrowStatement(4198)
#[test]
fn check_throw_statement_checks_expression() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const err: any;\nthrow err;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2304),
        "throw with declared variable should not report cannot-find-name; got: {:?}",
        diags
    );
}

#[test]
fn check_throw_statement_validates_inner_expression() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "throw unknownError;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "expected TS2304 for undeclared throw expression; got: {:?}",
        diags
    );
}

// ============================================================================
// checkBreakOrContinueStatement — grammar validation (ambient context)
// ============================================================================

// Go: internal/checker/checker.go:Checker.checkBreakOrContinueStatement(4056)
#[test]
fn check_break_in_switch_no_checker_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: number;\nswitch (x) { case 1: break; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 1107 || d.code == 1104),
        "break in switch should not report jump errors; got: {:?}",
        diags
    );
}

#[test]
fn check_continue_in_loop_no_checker_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "while (true) { continue; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 1107 || d.code == 1104),
        "continue in loop should not report jump errors; got: {:?}",
        diags
    );
}

// ============================================================================
// checkReturnStatement — validates return type compatibility
// ============================================================================

// The return statement checking is already partially implemented (4q). These
// tests verify the existing behavior.
#[test]
fn check_return_statement_mismatch_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(): number { return 'hello'; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2322),
        "expected TS2322 (type not assignable) for wrong return type; got: {:?}",
        diags
    );
}

#[test]
fn check_return_statement_matching_type_no_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(): number { return 42; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2322),
        "correct return type should not report 2322; got: {:?}",
        diags
    );
}

// ============================================================================
// checkVariableDeclaration / checkVariableStatement — validates initializers
// ============================================================================

// These were previously tested (4m), but we add targeted tests for clarity.
#[test]
fn check_variable_declaration_type_mismatch_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x: number = 'hello';",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2322),
        "expected TS2322 for variable type mismatch; got: {:?}",
        diags
    );
}

#[test]
fn check_variable_declaration_matching_type_no_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x: number = 42;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2322),
        "matching types should not report 2322; got: {:?}",
        diags
    );
}

// ============================================================================
// checkIfStatement — validates condition expression is checked
// ============================================================================

#[test]
fn check_if_statement_checks_condition() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "if (unknownCond) { }"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "expected TS2304 for undeclared if condition; got: {:?}",
        diags
    );
}

#[test]
fn check_if_statement_checks_then_branch() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "if (true) { unknownInThen; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "expected TS2304 for undeclared identifier in then-branch; got: {:?}",
        diags
    );
}

// ============================================================================
// checkSwitchStatement — validates expression and case clauses
// ============================================================================

#[test]
fn check_switch_statement_checks_expression() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "switch (unknownExpr) { }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "expected TS2304 for undeclared switch expression; got: {:?}",
        diags
    );
}

#[test]
fn check_switch_case_clause_checks_expression() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: number;\nswitch (x) { case unknownCase: break; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "expected TS2304 for undeclared case expression; got: {:?}",
        diags
    );
}

// ============================================================================
// checkTryStatement — validates try/catch/finally blocks are descended
// ============================================================================

#[test]
fn check_try_statement_checks_try_block() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "try { unknownInTry; } catch (e) { }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "expected TS2304 for undeclared identifier in try block; got: {:?}",
        diags
    );
}

#[test]
fn check_try_statement_checks_catch_block() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "try { } catch (e) { unknownInCatch; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "expected TS2304 for undeclared identifier in catch block; got: {:?}",
        diags
    );
}

// ============================================================================
// checkWhileStatement / checkDoStatement — condition and body check
// ============================================================================

#[test]
fn check_while_statement_checks_condition() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "while (unknownCond) { }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "expected TS2304 for undeclared while condition; got: {:?}",
        diags
    );
}

#[test]
fn check_do_statement_checks_condition() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "do { } while (unknownCond);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "expected TS2304 for undeclared do-while condition; got: {:?}",
        diags
    );
}

// ============================================================================
// checkForStatement — validates initializer, condition, incrementor, body
// ============================================================================

#[test]
fn check_for_statement_checks_condition() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (let i = 0; unknownCond; i++) { }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "expected TS2304 for undeclared for condition; got: {:?}",
        diags
    );
}

#[test]
fn check_for_statement_checks_body() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (;;) { unknownInBody; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "expected TS2304 for undeclared identifier in for body; got: {:?}",
        diags
    );
}

// == T1-E batch 3 ==

#[test]
fn b3_class_declaration_no_name_reports_1211() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "class { }"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(c.get_diagnostics(root).iter().any(|d| d.code == 1211));
}

#[test]
fn b3_class_declaration_named_no_1211() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "class Foo { }"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(!c.get_diagnostics(root).iter().any(|d| d.code == 1211));
}

#[test]
fn b3_interface_reserved_name_reports_2427() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "interface string { }"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(c.get_diagnostics(root).iter().any(|d| d.code == 2427));
}

#[test]
fn b3_interface_non_reserved_no_2427() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Foo { x: number; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(!c.get_diagnostics(root).iter().any(|d| d.code == 2427));
}

#[test]
fn b3_type_alias_reserved_name_reports_2457() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type number = string;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(c.get_diagnostics(root).iter().any(|d| d.code == 2457));
}

#[test]
fn b3_type_alias_non_reserved_no_2457() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type MyType = string;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(!c.get_diagnostics(root).iter().any(|d| d.code == 2457));
}

#[test]
fn b3_enum_normal_members_no_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "enum E { X, Y, Z }"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(!c.get_diagnostics(root).iter().any(|d| d.code == 18024));
}

#[test]
fn b3_module_keyword_reports_1540() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "module Foo { }"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(c.get_diagnostics(root).iter().any(|d| d.code == 1540));
}

#[test]
fn b3_namespace_keyword_no_1540() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "namespace Foo { }"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(!c.get_diagnostics(root).iter().any(|d| d.code == 1540));
}

#[test]
fn b3_namespace_body_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "namespace N { unknownInNs; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(c.get_diagnostics(root).iter().any(|d| d.code == 2304));
}

#[test]
fn b3_function_body_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f() { unknownInFn; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(c.get_diagnostics(root).iter().any(|d| d.code == 2304));
}

#[test]
fn b3_method_body_is_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { foo() { unknownInMethod; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(c.get_diagnostics(root).iter().any(|d| d.code == 2304));
}

#[test]
fn b3_method_return_type_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { foo(): number { return 'x'; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(c.get_diagnostics(root).iter().any(|d| d.code == 2322));
}

#[test]
fn b3_constructor_body_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { constructor() { unknownInCtor; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(c.get_diagnostics(root).iter().any(|d| d.code == 2304));
}

#[test]
fn b3_accessor_params_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { set x(this: number) { } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(c.get_diagnostics(root).iter().any(|d| d.code == 2784));
}

#[test]
fn b3_accessor_body_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { get x() { unknownInGetter; return 1; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(c.get_diagnostics(root).iter().any(|d| d.code == 2304));
}

#[test]
fn b3_property_initializer_checked() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { x: number = unknownPropInit; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(c.get_diagnostics(root).iter().any(|d| d.code == 2304));
}

#[test]
fn b3_type_param_reserved_name_reports_2368() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Foo<string> = string;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(c.get_diagnostics(root).iter().any(|d| d.code == 2368));
}

#[test]
fn b3_type_param_non_reserved_no_2368() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "type Foo<T> = T;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    assert!(!c.get_diagnostics(root).iter().any(|d| d.code == 2368));
}

// ---- T1-E batch 4: type relation and inference functions ----

// Go: internal/checker/checker.go:isTupleType(23350)
#[test]
fn is_tuple_type_recognizes_tuple_objects() {
    let mut c = Checker::new();
    let s = c.string_type();
    let n = c.number_type();
    let tuple = c.create_tuple_type(vec![s, n]);
    assert!(c.is_tuple_type(tuple));
    assert!(!c.is_tuple_type(s));
    let obj = c.new_object_type(
        crate::core::types::ObjectFlags::INTERFACE,
        None,
        Default::default(),
    );
    assert!(!c.is_tuple_type(obj));
}

// Go: internal/checker/checker.go:Checker.isArrayType(23342)
#[test]
fn is_array_type_recognizes_array_references() {
    let mut c = Checker::new();
    let n = c.number_type();
    let arr = c.create_array_type(n);
    assert!(c.is_array_type(arr));
    assert!(!c.is_array_type(n));
    let tuple = c.create_tuple_type(vec![n]);
    assert!(!c.is_array_type(tuple));
}

// Go: internal/checker/checker.go:Checker.isReadonlyArrayType(23346)
#[test]
fn is_readonly_array_type_distinguishes_mutability() {
    let mut c = Checker::new();
    let n = c.number_type();
    let mutable_arr = c.create_array_type(n);
    let readonly_arr = c.create_array_type_ex(n, true);
    assert!(!c.is_readonly_array_type(mutable_arr));
    assert!(c.is_readonly_array_type(readonly_arr));
    assert!(c.is_array_type(readonly_arr));
}

// Go: internal/checker/checker.go:Checker.isArrayOrTupleType(23366)
#[test]
fn is_array_or_tuple_type_covers_both() {
    let mut c = Checker::new();
    let n = c.number_type();
    let arr = c.create_array_type(n);
    let tuple = c.create_tuple_type(vec![n]);
    assert!(c.is_array_or_tuple_type(arr));
    assert!(c.is_array_or_tuple_type(tuple));
    assert!(!c.is_array_or_tuple_type(n));
}

// Go: internal/checker/checker.go:Checker.createArrayType(24562)
#[test]
fn create_array_type_produces_reference_with_element_type() {
    let mut c = Checker::new();
    let s = c.string_type();
    let arr = c.create_array_type(s);
    let ty = c.get_type(arr);
    assert!(ty
        .object_flags()
        .contains(crate::core::types::ObjectFlags::REFERENCE));
    let obj = ty.as_object().expect("object");
    assert_eq!(obj.resolved_type_arguments, vec![s]);
}

// Go: internal/checker/checker.go:Checker.createArrayTypeEx(24566)
#[test]
fn create_array_type_ex_readonly_interns_separately() {
    let mut c = Checker::new();
    let n = c.number_type();
    let mutable_arr = c.create_array_type(n);
    let readonly_arr = c.create_array_type_ex(n, true);
    assert_ne!(mutable_arr, readonly_arr);
    let mutable_obj = c.get_type(mutable_arr).as_object().expect("obj");
    let readonly_obj = c.get_type(readonly_arr).as_object().expect("obj");
    assert_ne!(mutable_obj.target, readonly_obj.target);
}

// ---- T1-E batch 14: tuple + array type operations ----

// Go: internal/checker/checker.go:Checker.getElementTypeOfArrayType(23374)
#[test]
fn get_element_type_of_array_type_returns_first_type_argument() {
    let mut c = Checker::new();
    let s = c.string_type();
    let arr = c.create_array_type(s);
    assert_eq!(c.get_element_type_of_array_type(arr), Some(s));
    assert_eq!(c.get_element_type_of_array_type(s), None);
    let tuple = c.create_tuple_type(vec![s]);
    assert_eq!(c.get_element_type_of_array_type(tuple), None);
}

// Go: internal/checker/checker.go:Checker.getTupleElementType(23425)
#[test]
fn get_tuple_element_type_reads_positional_elements() {
    let mut c = Checker::new();
    let s = c.string_type();
    let n = c.number_type();
    let b = c.boolean_type();
    let tuple = c.create_tuple_type(vec![s, n, b]);
    assert_eq!(c.get_tuple_element_type(tuple, 0), Some(s));
    assert_eq!(c.get_tuple_element_type(tuple, 1), Some(n));
    assert_eq!(c.get_tuple_element_type(tuple, 2), Some(b));
    assert_eq!(c.get_tuple_element_type(n, 0), None);
}

// Go: internal/checker/checker.go:Checker.getTupleElementTypeOutOfStartCount(24716)
#[test]
fn get_tuple_element_type_out_of_range_yields_undefined() {
    let mut c = Checker::new();
    let s = c.string_type();
    let n = c.number_type();
    let tuple = c.create_tuple_type(vec![s, n]);
    assert_eq!(
        c.get_tuple_element_type(tuple, 2),
        Some(c.undefined_type()),
        "out-of-range index on a fixed tuple yields undefined"
    );
}

// Go: internal/checker/checker.go:Checker.getRestTypeOfTupleType(24712)
#[test]
fn get_rest_type_of_tuple_type_none_for_fixed_arity() {
    let mut c = Checker::new();
    let s = c.string_type();
    let n = c.number_type();
    let tuple = c.create_tuple_type(vec![s, n]);
    assert_eq!(
        c.get_rest_type_of_tuple_type(tuple),
        None,
        "fixed-arity tuples have no rest element"
    );
}

// Go: internal/checker/relater.go:Checker.sliceTupleType(1879)
#[test]
fn slice_tuple_type_extracts_subrange() {
    let mut c = Checker::new();
    let s = c.string_type();
    let n = c.number_type();
    let b = c.boolean_type();
    let tuple = c.create_tuple_type(vec![s, n, b]);
    let tail = c.slice_tuple_type(tuple, 1, 0);
    assert!(c.is_tuple_type(tail));
    assert_eq!(c.get_tuple_element_type(tail, 0), Some(n));
    assert_eq!(c.get_tuple_element_type(tail, 1), Some(b));
    assert_eq!(c.get_tuple_element_type(tail, 2), Some(c.undefined_type()));
}

// Go: internal/checker/relater.go:Checker.sliceTupleType(1879)
#[test]
fn slice_tuple_type_empty_when_index_at_or_past_end() {
    let mut c = Checker::new();
    let s = c.string_type();
    let n = c.number_type();
    let tuple = c.create_tuple_type(vec![s, n]);
    let empty = c.slice_tuple_type(tuple, 2, 0);
    assert!(c.is_tuple_type(empty));
    let obj = c.get_type(empty).as_object().expect("tuple object");
    assert!(obj.resolved_type_arguments.is_empty());
}

// Go: internal/checker/checker.go:Checker.getTypeFromArrayOrTupleTypeNode (tuple branch)
#[test]
fn get_type_from_tuple_type_node_produces_fixed_arity_tuple() {
    let p = StubProgram::parse_and_bind("/a.ts", "type T = [string, number];");
    let mut c = Checker::new();
    let type_alias = match p.arena().data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => panic!("source file"),
    };
    let tuple_node = match p.arena().data(type_alias) {
        NodeData::TypeAliasDeclaration(d) => d.type_node,
        _ => panic!("type alias"),
    };
    let ty = crate::core::declared_types::get_type_from_type_node(&mut c, &p, tuple_node, None);
    assert!(c.is_tuple_type(ty));
    assert_eq!(c.get_tuple_element_type(ty, 0), Some(c.string_type()));
    assert_eq!(c.get_tuple_element_type(ty, 1), Some(c.number_type()));
}

// Go: internal/checker/checker.go:Checker.getTypeFromArrayOrTupleTypeNode (`T[]`)
#[test]
fn get_type_from_array_type_node_produces_array_reference() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\ntype T = number[];",
    );
    let mut c = Checker::new();
    let type_alias = match p.arena().data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes[1],
        _ => panic!("source file"),
    };
    let array_node = match p.arena().data(type_alias) {
        NodeData::TypeAliasDeclaration(d) => d.type_node,
        _ => panic!("type alias"),
    };
    let ty = crate::core::declared_types::get_type_from_type_node(&mut c, &p, array_node, None);
    let obj = c.get_type(ty).as_object().expect("array type reference");
    assert!(obj.target.is_some());
    assert_eq!(obj.resolved_type_arguments, vec![c.number_type()]);
}

// Go: internal/checker/checker.go:Checker.getRegularTypeOfLiteralType(25132)
#[test]
fn regular_type_of_literal_type_returns_regular_form() {
    let mut c = Checker::new();
    let fresh = c.get_fresh_type_of_literal_type(c.regular_false_type());
    assert_ne!(fresh, c.regular_false_type());
    let regular = c.regular_type_of_literal_type(fresh);
    assert_eq!(regular, c.regular_false_type());
    let s = c.string_type();
    assert_eq!(c.regular_type_of_literal_type(s), s);
}

// Go: internal/checker/checker.go:Checker.checkTypeAssignableToAndOptionallyElaborate(12568)
#[test]
fn check_type_assignable_to_and_optionally_elaborate_reports_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x: number = 'hello';",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p.clone());
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(diags.iter().any(|d| d.code == 2322), "should report TS2322");
}

// Go: internal/checker/checker.go:Checker.getBaseTypeOfLiteralType(25293)
#[test]
fn get_base_type_of_literal_type_maps_literals_to_primitives() {
    let mut c = Checker::new();
    let str_lit = c.new_literal_type(
        crate::core::types::TypeFlags::STRING_LITERAL,
        crate::core::types::LiteralValue::String("x".into()),
        None,
    );
    assert_eq!(c.get_base_type_of_literal_type(str_lit), c.string_type());
    let num_lit = c.new_literal_type(
        crate::core::types::TypeFlags::NUMBER_LITERAL,
        crate::core::types::LiteralValue::Number(tsgo_jsnum::Number::from(42.0)),
        None,
    );
    assert_eq!(c.get_base_type_of_literal_type(num_lit), c.number_type());
    assert_eq!(
        c.get_base_type_of_literal_type(c.false_type()),
        c.boolean_type()
    );
    assert_eq!(
        c.get_base_type_of_literal_type(c.string_type()),
        c.string_type()
    );
}

// Go: internal/checker/checker.go:isLiteralType(25252)
#[test]
fn is_literal_type_recognizes_literals_and_unit_types() {
    let mut c = Checker::new();
    assert!(c.is_literal_type(c.false_type()));
    assert!(c.is_literal_type(c.null_type()));
    assert!(c.is_literal_type(c.undefined_type()));
    let str_lit = c.new_literal_type(
        crate::core::types::TypeFlags::STRING_LITERAL,
        crate::core::types::LiteralValue::String("x".into()),
        None,
    );
    assert!(c.is_literal_type(str_lit));
    assert!(!c.is_literal_type(c.string_type()));
    assert!(!c.is_literal_type(c.number_type()));
}

// --- T1-E batch 5: inference + contextual typing entry points (check.rs) ---

fn source_statements_batch5(p: &StubProgram) -> Vec<tsgo_ast::NodeId> {
    match p.arena().data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    }
}

fn var_decl_initializer_batch5(p: &StubProgram, stmt_idx: usize) -> tsgo_ast::NodeId {
    let arena = p.arena();
    let list = match arena.data(source_statements_batch5(p)[stmt_idx]) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => panic!("variable statement"),
    };
    let decl = match arena.data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes[0],
        _ => panic!("declaration list"),
    };
    match arena.data(decl) {
        NodeData::VariableDeclaration(d) => d.initializer.expect("initializer"),
        _ => panic!("variable declaration"),
    }
}

fn var_decl_type_annotation_batch5(p: &StubProgram, stmt_idx: usize) -> tsgo_ast::NodeId {
    let arena = p.arena();
    let list = match arena.data(source_statements_batch5(p)[stmt_idx]) {
        NodeData::VariableStatement(d) => d.declaration_list,
        _ => panic!("variable statement"),
    };
    let decl = match arena.data(list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes[0],
        _ => panic!("declaration list"),
    };
    match arena.data(decl) {
        NodeData::VariableDeclaration(d) => d.type_node.expect("type annotation"),
        _ => panic!("variable declaration"),
    }
}

fn object_literal_property_value_batch5(p: &StubProgram, member_idx: usize) -> tsgo_ast::NodeId {
    let arena = p.arena();
    let literal = var_decl_initializer_batch5(p, 0);
    let members = match arena.data(literal) {
        NodeData::ObjectLiteralExpression(d) => d.list.nodes.clone(),
        _ => panic!("object literal"),
    };
    match arena.data(members[member_idx]) {
        NodeData::PropertyAssignment(d) => d.initializer.expect("property value"),
        _ => panic!("property assignment"),
    }
}

// Go: internal/checker/checker.go:Checker.checkExpressionWithContextualType
#[test]
fn check_expression_with_contextual_type_preserves_string_literal() {
    let p = StubProgram::parse_and_bind("/a.ts", "const x: { k: \"a\" } = { k: \"a\" };");
    let value = object_literal_property_value_batch5(&p, 0);
    let mut c = Checker::new();
    let a_lit = c.get_string_literal_type("a");
    let lit = c.check_expression_with_contextual_type(&p, value, a_lit, None);
    assert_eq!(lit, a_lit);
}

// Go: internal/checker/checker.go:Checker.getContextualType (pushed stack)
#[test]
fn pushed_contextual_type_overrides_parent_walk() {
    use crate::core::contextual::ContextFlags;
    let p = StubProgram::parse_and_bind("/a.ts", "const x = 1;");
    let init = var_decl_initializer_batch5(&p, 0);
    let mut c = Checker::new();
    let string = c.string_type();
    c.push_contextual_type(init, string, false);
    assert_eq!(
        c.get_contextual_type(&p, init, ContextFlags::NONE),
        Some(string),
    );
    c.pop_contextual_type();
}

// Go: internal/checker/checker.go:Checker.getTypeOfNode / getRegularTypeOfExpression
#[test]
fn get_type_of_node_returns_expression_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "const x = 1;");
    let init = var_decl_initializer_batch5(&p, 0);
    let mut c = Checker::new();
    let num_lit = c.get_number_literal_type(tsgo_jsnum::Number::from(1.0));
    assert_eq!(c.get_type_of_node(&p, init, None), num_lit);
}

// Go: internal/checker/checker.go:Checker.getTypeFromTypeNode
#[test]
fn get_type_from_type_node_on_checker_resolves_primitives() {
    let p = StubProgram::parse_and_bind("/a.ts", "let x: string;");
    let ty = var_decl_type_annotation_batch5(&p, 0);
    let mut c = Checker::new();
    assert_eq!(c.get_type_from_type_node(&p, ty, None), c.string_type());
}

// Go: internal/checker/inference.go:Checker.getInferredType
#[test]
fn get_inferred_type_resolves_from_candidates() {
    use crate::core::inference::InferenceContext;
    let p = empty();
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let one = c.get_number_literal_type(tsgo_jsnum::Number::from(1.0));
    let two = c.get_number_literal_type(tsgo_jsnum::Number::from(2.0));
    let mut ctx = InferenceContext::new(&[tp]);
    ctx.inferences[0].candidates = vec![one, two];
    let union = c.get_union_type(&[one, two]);
    assert_eq!(c.get_inferred_type(&p, &mut ctx, 0), union);
}

// Go: internal/checker/inference.go:Checker.inferTypeArguments
#[test]
fn infer_type_arguments_on_checker_infers_from_argument() {
    let p = empty();
    let mut c = Checker::new();
    let tp = c.new_type_parameter(None);
    let num = c.number_type();
    assert_eq!(c.infer_type_arguments(&p, &[tp], &[num], &[tp]), vec![num]);
}

// Go: internal/checker/checker.go:Checker.getReturnTypeOfSignature
#[test]
fn get_return_type_of_signature_reads_resolved_return_type() {
    use crate::core::signatures::{Signature, SignatureFlags};
    let mut c = Checker::new();
    let ret = c.string_type();
    let mut sig = Signature::new(SignatureFlags::NONE);
    sig.resolved_return_type = Some(ret);
    let sid = c.new_signature(sig);
    assert_eq!(c.get_return_type_of_signature(sid), ret);
}

// Go: internal/checker/checker.go:Checker.checkTypeArguments
#[test]
fn check_type_arguments_reports_constraint_violation() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T extends number>(x: T) {}\nf<string>(\"a\");",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p.clone());
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2344),
        "expected TS2344 from checkTypeArguments path, got {diags:?}",
    );
}

// Go: internal/checker/checker.go:Checker.getPropertyTypeForIndexType
#[test]
fn get_property_type_for_index_type_resolves_literal_key() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const o: { a: number };");
    let mut c = Checker::new();
    let root = p.root();
    c.check_source_file(root);
    let o_sym = *p.locals(root).expect("locals").get("o").expect("symbol o");
    let o_ty = crate::core::declared_types::get_type_of_symbol(&mut c, &p, o_sym, None);
    let a_lit = c.get_string_literal_type("a");
    let prop = c
        .get_property_type_for_index_type(&p, o_ty, a_lit)
        .expect("property type for 'a'");
    assert_eq!(prop, c.number_type());
}

// ---- T1-E batch 10: class member body checks ----

fn diag_codes(src: &str) -> Vec<i32> {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.get_diagnostics(root)
        .into_iter()
        .map(|d| d.code)
        .collect()
}

// Go: internal/checker/checker.go:Checker.checkClassDeclaration / checkObjectTypeForDuplicateDeclarations
#[test]
fn class_duplicate_property_members_report_2300() {
    let codes = diag_codes("class C { x: number; x: string; }");
    assert!(
        codes.iter().filter(|&&c| c == 2300).count() >= 2,
        "expected TS2300 duplicates; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkClassForStaticPropertyNameConflicts
#[test]
fn class_static_name_property_conflicts_with_function_builtin_2699() {
    let options = CompilerOptions {
        use_define_for_class_fields: Tristate::False,
        ..CompilerOptions::default()
    };
    let codes = diag_codes_with_options("class C { static name = 1; }", options);
    assert!(
        codes.contains(&2699),
        "expected TS2699 for static `name` when useDefineForClassFields is off; got {codes:?}"
    );
}

// Go: internal/checker/grammar.rs:Checker.checkGrammarProperty (definite assignment)
#[test]
fn property_definite_assignment_with_initializer_reports_1263() {
    let codes = diag_codes("class C { x!: string = \"a\"; }");
    assert!(codes.contains(&1263), "expected TS1263; got {codes:?}");
}

// Go: internal/checker/checker.go:Checker.checkFunctionOrConstructorSymbol
#[test]
fn method_static_instance_overload_mismatch_reports_2387() {
    let codes =
        diag_codes("class C {\n  foo(): void;\n  static foo(): void;\n  static foo() {}\n}");
    assert!(
        codes.contains(&2387),
        "expected TS2387 static/instance overload mismatch; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkConstructorDeclaration
#[test]
fn derived_class_constructor_without_super_reports_2377() {
    let codes = diag_codes("class B {}\nclass D extends B { constructor() {} }");
    assert!(
        codes.contains(&2377),
        "expected TS2377 missing super call; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkFunctionOrConstructorSymbol (constructor overloads)
#[test]
fn constructor_overloads_without_implementation_reports_2390() {
    let codes = diag_codes("class C { constructor(x: number);\nconstructor(x: string); }");
    assert!(
        codes.contains(&2390),
        "expected TS2390 missing constructor implementation; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkAccessorDeclaration
#[test]
fn accessor_get_less_accessible_than_set_reports_2808() {
    let codes = diag_codes(
        "class E {\n  private get Baz(): number { return 0; }\n  public set Baz(n: number) {}\n}",
    );
    assert!(
        codes.iter().filter(|&&c| c == 2808).count() >= 2,
        "expected TS2808 on getter and setter; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers (abstract)
#[test]
fn check_abstract_declaration_method_in_plain_class_reports_1244() {
    let codes = diag_codes("class B { abstract n(): void; }");
    assert!(codes.contains(&1244), "expected TS1244; got {codes:?}");
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration (isValidBaseType)
#[test]
fn class_implements_primitive_reports_2422() {
    let codes = diag_codes("class C implements number {}");
    assert!(
        codes.contains(&2422),
        "expected TS2422 non-object implements; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarHeritageClause (type alias as value)
#[test]
fn class_implements_type_only_alias_reports_2693() {
    let codes = diag_codes("type T = { x: number };\nclass C implements T {}");
    assert!(
        codes.contains(&2693),
        "expected TS2693 type-alias-as-value; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkSuperExpression
#[test]
fn super_in_class_without_extends_reports_2335() {
    let codes = diag_codes("class C { m() { return super; } }");
    assert!(
        codes.contains(&2335),
        "expected TS2335 super without extends; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkSuperExpression (super call in method)
#[test]
fn super_call_outside_constructor_reports_2337() {
    let codes = diag_codes("class B {}\nclass D extends B { m() { super(); } }");
    assert!(
        codes.contains(&2337),
        "expected TS2337 super() outside constructor; got {codes:?}"
    );
}

#[test]
fn property_access_string_index_signature_yields_value_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Dict {\n  [k: string]: number;\n}\ndeclare const d: Dict;\nd.foo;",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, access), c.number_type());
}

#[test]
fn property_access_string_index_signature_reports_no_2339() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Dict {\n  [k: string]: number;\n}\ndeclare const d: Dict;\nd.foo;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2339),
        "index-signature property access must not report 2339, got {diags:?}"
    );
}

#[test]
fn private_property_access_outside_class_reports_2341() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { private x: number; }\nconst c = new C();\nc.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2341);
    assert_eq!(
        diags[0].message,
        "Property 'x' is private and only accessible within class 'C'."
    );
}

#[test]
fn private_property_access_inside_class_reports_nothing() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { private x: number; m() { this.x; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2341),
        "private member access inside declaring class must not report 2341, got {diags:?}"
    );
}

#[test]
fn is_valid_property_access_private_outside_class_is_false() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "class C { private x: number; }\nconst c = new C();\nc.x;",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    assert!(!c.is_valid_property_access(&p, access, "x"));
}

#[test]
fn is_valid_property_access_public_member_is_true() {
    let p =
        StubProgram::parse_and_bind("/a.ts", "class C { x: number; }\nconst c = new C();\nc.x;");
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    assert!(c.is_valid_property_access(&p, access, "x"));
}

// ---- T1-E batch 13: function body checking + return types ----

// Go: internal/checker/checker.go:Checker.getWidenedTypeForReturnExpression (per-expr arm)
#[test]
fn get_widened_type_for_return_expression_widens_string_literal() {
    let p = StubProgram::parse_and_bind("/a.ts", "const f = (x: number) => \"s\";");
    let arena = p.arena();
    let arrow = {
        let stmts = match arena.data(p.root()) {
            NodeData::SourceFile(d) => d.statements.nodes.clone(),
            _ => panic!("source file"),
        };
        let list = match arena.data(stmts[0]) {
            NodeData::VariableStatement(d) => d.declaration_list,
            _ => panic!("variable statement"),
        };
        let decl = match arena.data(list) {
            NodeData::VariableDeclarationList(d) => d.declarations.nodes[0],
            _ => panic!("declaration list"),
        };
        match arena.data(decl) {
            NodeData::VariableDeclaration(d) => d.initializer.expect("initializer"),
            _ => panic!("variable declaration"),
        }
    };
    let body = match arena.data(arrow) {
        NodeData::ArrowFunction(d) => d.body,
        _ => panic!("arrow"),
    };
    let mut c = Checker::new();
    assert_eq!(
        c.get_widened_type_for_return_expression(&p, body),
        c.string_type(),
        "fresh string literal widens to string"
    );
}

// Go: internal/checker/checker.go:Checker.getReturnTypeFromBody (block body)
#[test]
fn get_return_type_from_body_block_unions_widened_returns() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "function f() { if (true) { return 1; } else { return \"s\"; } }",
    );
    let arena = p.arena();
    let fn_decl = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => panic!("function decl"),
    };
    let mut c = Checker::new();
    let num = c.number_type();
    let str = c.string_type();
    let union = c.get_union_type(&[num, str]);
    assert_eq!(
        c.get_return_type_from_body(&p, fn_decl),
        union,
        "block with number|string returns should union-widen"
    );
}

// Go: internal/checker/checker.go:Checker.getReturnTypeFromAnnotation (declaration.Type())
#[test]
fn get_effective_return_type_node_finds_function_annotation() {
    let p = StubProgram::parse_and_bind("/a.ts", "function f(): number { return 1; }");
    let arena = p.arena();
    let fn_decl = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes[0],
        _ => panic!("function decl"),
    };
    let type_node = get_effective_return_type_node(&p, fn_decl).expect("return type node");
    assert_eq!(arena.kind(type_node), Kind::NumberKeyword);
}

// Go: internal/checker/relater.go:Checker.hasEffectiveRestParameter(1746)
#[test]
fn has_effective_rest_parameter_when_rest_flag_set() {
    let mut c = Checker::new();
    let mut sig = Signature::new(SignatureFlags::HAS_REST_PARAMETER);
    sig.parameters = vec![SymbolId(1)];
    let sig_id = c.new_signature(sig);
    assert!(c.has_effective_rest_parameter(sig_id));
    let plain = c.new_signature(Signature::new(SignatureFlags::NONE));
    assert!(!c.has_effective_rest_parameter(plain));
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarParameterList (TS1016)
#[test]
fn check_parameter_declaration_required_after_optional_reports_1016() {
    let codes = diag_codes("function f(a?: number, b: number) {}");
    assert!(
        codes.contains(&1016),
        "expected TS1016 required after optional; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarParameterList (TS1014)
#[test]
fn check_rest_parameter_not_last_reports_1014() {
    let codes = diag_codes("function f(...a: number[], b: number) {}");
    assert!(
        codes.contains(&1014),
        "expected TS1014 rest must be last; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkFunctionDeclaration / checkFunctionOrConstructorSymbolWorker
#[test]
fn check_function_declaration_duplicate_implementation_reports_2393() {
    let codes = diag_codes("function f() {}\nfunction f() {}");
    assert!(
        codes.iter().filter(|&&c| c == 2393).count() >= 2,
        "expected TS2393 on duplicate implementations; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkFunctionOrConstructorSymbolWorker (ambient overload)
#[test]
fn check_function_declaration_ambient_overload_mismatch_reports_2384() {
    let codes = diag_codes("declare function foo(x: number): void;\nfunction foo(x: string) {}");
    assert!(
        codes.contains(&2384),
        "expected TS2384 ambient/non-ambient overload mix; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkFunctionExpressionOrObjectLiteralMethod (widen return)
#[test]
fn check_function_expression_or_object_literal_method_widens_concise_return() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const f: () => string = () => \"s\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2322),
        "widened string literal should satisfy () => string; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkReturnStatement
#[test]
fn check_return_statement_void_annotation_rejects_value_reports_2322() {
    let codes = diag_codes("function f(): void { return 1; }");
    assert!(
        codes.contains(&2322),
        "expected TS2322 returning number to void; got {codes:?}"
    );
}

// ---- T1-E batch 15: prefix/postfix unary expressions ----

// Go: internal/checker/checker.go:Checker.checkPrefixUnaryExpression (numeric literal)
#[test]
fn prefix_minus_on_numeric_literal_yields_negated_fresh_literal() {
    let p = StubProgram::parse_and_bind("/a.ts", "-1;");
    let expr = expr_stmt_expression(&p, 0);
    let mut c = Checker::new();
    let ty = c.check_expression(&p, expr);
    let neg_one = c.get_number_literal_type(tsgo_jsnum::Number::from(-1.0));
    let fresh = c.get_fresh_type_of_literal_type(neg_one);
    assert_eq!(ty, fresh);
}

// Go: internal/checker/checker.go:Checker.checkPrefixUnaryExpression (numeric literal)
#[test]
fn prefix_plus_on_numeric_literal_yields_fresh_positive_literal() {
    let p = StubProgram::parse_and_bind("/a.ts", "+42;");
    let expr = expr_stmt_expression(&p, 0);
    let mut c = Checker::new();
    let ty = c.check_expression(&p, expr);
    let lit = c.get_number_literal_type(tsgo_jsnum::Number::from(42.0));
    assert_eq!(ty, c.get_fresh_type_of_literal_type(lit));
}

// Go: internal/checker/checker.go:Checker.checkPrefixUnaryExpression (ExclamationToken)
#[test]
fn prefix_bang_on_true_returns_false_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "!true;");
    let expr = expr_stmt_expression(&p, 0);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, expr), c.false_type());
}

// Go: internal/checker/checker.go:Checker.checkPrefixUnaryExpression (ExclamationToken)
#[test]
fn prefix_bang_on_false_returns_true_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "!false;");
    let expr = expr_stmt_expression(&p, 0);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, expr), c.true_type());
}

// Go: internal/checker/checker.go:Checker.checkPrefixUnaryExpression (ExclamationToken)
#[test]
fn prefix_bang_on_boolean_variable_returns_boolean() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const b: boolean;\n!b;");
    let expr = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, expr), c.boolean_type());
}

// Go: internal/checker/checker.go:Checker.checkPrefixUnaryExpression (MinusToken)
#[test]
fn prefix_minus_on_number_variable_returns_number() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const n: number;\n-n;");
    let expr = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, expr), c.number_type());
}

// Go: internal/checker/checker.go:Checker.checkPrefixUnaryExpression (PlusPlusToken)
#[test]
fn prefix_increment_on_number_variable_returns_number() {
    let p = StubProgram::parse_and_bind("/a.ts", "let n = 0;\n++n;");
    let expr = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, expr), c.number_type());
}

// Go: internal/checker/checker.go:Checker.checkReferenceExpression (2357)
#[test]
fn prefix_increment_on_non_reference_reports_2357() {
    let codes = diag_codes("++1;");
    assert!(
        codes.contains(&2357),
        "expected TS2357 on ++literal; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkPostfixUnaryExpression
#[test]
fn postfix_decrement_on_number_variable_returns_number() {
    let p = StubProgram::parse_and_bind("/a.ts", "let n = 0;\nn--;");
    let expr = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, expr), c.number_type());
}

// Go: internal/checker/checker.go:Checker.getUnaryResultType
#[test]
fn get_unary_result_type_bigint_operand_returns_bigint() {
    let c = Checker::new();
    assert_eq!(
        c.get_unary_result_type(c.bigint_type()),
        c.bigint_type(),
        "bigint operand yields bigint unary result"
    );
    assert_eq!(
        c.get_unary_result_type(c.number_type()),
        c.number_type(),
        "number operand yields number unary result"
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (2356)
#[test]
fn prefix_increment_on_string_operand_reports_2356() {
    let codes = diag_codes("let s = \"x\";\n++s;");
    assert!(
        codes.contains(&2356),
        "expected TS2356 on ++string; got {codes:?}"
    );
}

// ---- T1-E batch 16: comma operator ----

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (KindCommaToken)
#[test]
fn comma_operator_yields_right_operand_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: string;\ndeclare const b: number;\na, b;",
    );
    let comma = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, comma), c.number_type());
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (KindCommaToken)
#[test]
fn comma_operator_chained_yields_last_operand_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "1, 2, 3;");
    let comma = expr_stmt_expression(&p, 0);
    let mut c = Checker::new();
    // `1, 2, 3` parses as `(1, 2), 3`; the result is the last operand's type.
    let ty = c.check_expression(&p, comma);
    assert_eq!(c.type_to_string(ty), "3");
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (KindCommaToken, 2695)
#[test]
fn comma_operator_unused_side_effect_free_left_reports_2695() {
    let codes = diag_codes("1, 2;");
    assert!(
        codes.contains(&2695),
        "expected TS2695 on unused comma left; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (KindCommaToken)
#[test]
fn comma_operator_assignment_left_has_side_effects_no_2695() {
    let codes = diag_codes("let n = 0;\nn = 1, 2;");
    assert!(
        !codes.contains(&2695),
        "assignment left of comma has side effects; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (KindCommaToken, allowUnreachableCode)
#[test]
fn comma_operator_allow_unreachable_code_suppresses_2695() {
    let opts = CompilerOptions {
        allow_unreachable_code: Tristate::True,
        ..Default::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts", "1, 2;", opts,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2695),
        "allowUnreachableCode should suppress TS2695; got {diags:?}"
    );
}

// ---- T1-E batch 17: nullish coalesce operand diagnostics ----

// Go: internal/checker/checker.go:Checker.checkNullishCoalesceOperandLeft(12880)
#[test]
fn nullish_coalesce_null_left_reports_2871() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "null ?? \"x\";"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2871);
    assert_eq!(diags[0].message, "This expression is always nullish.");
}

// Go: internal/checker/checker.go:Checker.checkNullishCoalesceOperandLeft(12880)
#[test]
fn nullish_coalesce_undefined_left_reports_2871() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "undefined ?? \"x\";"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2871);
    assert_eq!(diags[0].message, "This expression is always nullish.");
}

// Go: internal/checker/checker.go:Checker.checkNullishCoalesceOperandLeft(12880)
#[test]
fn nullish_coalesce_never_nullish_string_literal_reports_2869() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "\"hello\" ?? \"x\";"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2869);
    assert_eq!(
        diags[0].message,
        "Right operand of ?? is unreachable because the left operand is never nullish."
    );
}

// Go: internal/checker/checker.go:Checker.checkNullishCoalesceOperandLeft(12880)
#[test]
fn nullish_coalesce_never_nullish_numeric_literal_reports_2869() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "1 ?? \"x\";"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2869);
}

// Go: internal/checker/checker.go:Checker.checkNullishCoalesceOperandLeft(12880)
#[test]
fn nullish_coalesce_variable_left_reports_nothing() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: number;\na ?? \"x\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkNullishCoalesceOperandLeft(12880)
#[test]
fn nullish_coalesce_parenthesized_null_left_reports_2871() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "(null) ?? \"x\";"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2871);
}

// Go: internal/checker/checker.go:Checker.getSyntacticNullishnessSemantics(12892)
#[test]
fn nullish_coalesce_conditional_both_nullish_branches_reports_2871() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "(true ? null : undefined) ?? \"x\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2871);
}

// Go: internal/checker/checker.go:Checker.checkNullishCoalesceOperands(12859) — `??=`
// does not run operand diagnostics (only the binary `??` form does).
#[test]
fn nullish_coalesce_assign_skips_operand_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare let x: string | undefined;\nnull ??= \"x\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2871 || d.code == 2869),
        "??= should not run nullish operand diagnostics; got {diags:?}"
    );
}

// ---- T1-E batch 18: boolean-bitwise operator suggestion (2447) ----

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12316)
#[test]
fn bitwise_or_on_boolean_literals_reports_2447() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "true | false;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2447);
    assert_eq!(
        diags[0].message,
        "The '|' operator is not allowed for boolean types. Consider using '||' instead."
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12316)
#[test]
fn bitwise_and_on_boolean_literals_reports_2447() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "true & false;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2447);
    assert_eq!(
        diags[0].message,
        "The '&' operator is not allowed for boolean types. Consider using '&&' instead."
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12316)
#[test]
fn bitwise_xor_on_boolean_literals_reports_2447() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "true ^ false;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2447);
    assert_eq!(
        diags[0].message,
        "The '^' operator is not allowed for boolean types. Consider using '!==' instead."
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12316)
#[test]
fn bitwise_or_compound_on_boolean_literals_reports_2447() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "var a = true;\na |= false;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2447);
    assert_eq!(
        diags[0].message,
        "The '|=' operator is not allowed for boolean types. Consider using '||' instead."
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12316)
#[test]
fn bitwise_and_mixed_boolean_number_reports_arithmetic_not_2447() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "true & 1;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2447),
        "mixed boolean/number operands should not report 2447; got {diags:?}"
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2362);
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12319)
#[test]
fn bitwise_or_on_boolean_literals_yields_number() {
    let p = StubProgram::parse_and_bind("/a.ts", "true | false;");
    let sub = expr_stmt_expression(&p, 0);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, sub), c.number_type());
}

// ---- T1-E batch 19: bigint arithmetic, shift simplification, array spread ----

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12330)
#[test]
fn bigint_bitwise_and_literals_yields_bigint() {
    let p = StubProgram::parse_and_bind("/a.ts", "1n & 2n;");
    let sub = expr_stmt_expression(&p, 0);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, sub), c.bigint_type());
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12330)
#[test]
fn bigint_bitwise_or_literals_yields_bigint() {
    let p = StubProgram::parse_and_bind("/a.ts", "1n | 2n;");
    let sub = expr_stmt_expression(&p, 0);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, sub), c.bigint_type());
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12341)
#[test]
fn bigint_mixed_with_number_bitwise_reports_2365() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "1n & 1;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2365);
    assert_eq!(
        diags[0].message,
        "Operator '&' cannot be applied to types '1n' and '1'."
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12333)
#[test]
fn bigint_unsigned_right_shift_reports_2365() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "1n >>> 1n;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2365);
    assert!(
        diags[0].message.contains(">>>"),
        "expected unsigned-shift operator in message, got {:?}",
        diags[0].message
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12351)
#[test]
fn enum_member_shift_by_32_reports_6807() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "enum E { A = 1 << 32 }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 6807);
    assert_eq!(
        diags[0].message,
        "This operation can be simplified. This shift is identical to `1 << 0`."
    );
}

// Go: internal/checker/checker.go:Checker.checkArrayLiteral(8003)
#[test]
fn array_literal_spread_nested_array_literal_yields_number_array() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\n[1, ...[2, 3]];",
    );
    let sub = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let result = c.check_expression(&p, sub);
    let number = c.number_type();
    let obj = c.get_type(result).as_object().expect("array reference");
    assert_eq!(
        obj.resolved_type_arguments,
        vec![number],
        "spread of a number array should yield Array<number>"
    );
}

// Go: internal/checker/checker.go:Checker.checkArrayLiteral(8032)
#[test]
fn array_literal_spread_number_array_variable_yields_number_array() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\ndeclare const a: number[];\n[0, ...a];",
    );
    let sub = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let result = c.check_expression(&p, sub);
    let obj = c.get_type(result).as_object().expect("array reference");
    assert_eq!(
        obj.resolved_type_arguments,
        vec![c.number_type()],
        "spread of number[] should yield Array<number>"
    );
}

// ---- T1-E batch 20: object spread, shift suggestions, prefix -1n ----

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12347) / errorOrSuggestion
#[test]
fn shift_by_32_outside_enum_reports_6807_as_suggestion() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "1 << 32;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let suggestions = c.get_suggestion_diagnostics(root);
    assert_eq!(
        suggestions.len(),
        1,
        "expected one suggestion, got {suggestions:?}"
    );
    assert_eq!(suggestions[0].code, 6807);
    assert_eq!(
        suggestions[0].category,
        tsgo_diagnostics::Category::Suggestion
    );
    assert_eq!(
        suggestions[0].message,
        "This operation can be simplified. This shift is identical to `1 << 0`."
    );
    let errors = c.peek_recorded_diagnostics(root);
    assert!(
        errors.is_empty(),
        "shift simplification outside enum must not be an error; got {errors:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkPrefixUnaryExpression(10828)
#[test]
fn prefix_minus_bigint_literal_yields_negated_fresh_bigint() {
    let p = StubProgram::parse_and_bind("/a.ts", "-1n;");
    let sub = expr_stmt_expression(&p, 0);
    let mut c = Checker::new();
    let neg_one = c.get_bigint_literal_type("-1n");
    let expected = c.get_fresh_type_of_literal_type(neg_one);
    assert_eq!(c.check_expression(&p, sub), expected);
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression(12330)
#[test]
fn bigint_left_shift_literals_yields_bigint() {
    let p = StubProgram::parse_and_bind("/a.ts", "1n << 2n;");
    let sub = expr_stmt_expression(&p, 0);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, sub), c.bigint_type());
}

// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13210)
#[test]
fn object_literal_spread_merges_spread_properties() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { a: 1, ...{ b: 2 } };\no;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ a: number; b: number; }"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13231)
#[test]
fn object_literal_spread_non_object_reports_2698() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "const o = { ...1 };"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2698);
    assert_eq!(
        diags[0].message,
        "Spread types may only be created from object types."
    );
}

// Go: internal/checker/checker.go:Checker.getSpreadType (right overrides left)
#[test]
fn object_literal_spread_overrides_same_named_property() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { a: \"x\", ...{ a: 1 } };\no.a;");
    let access = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, access), c.number_type());
}

// ---- T1-E batch 21: ES-symbol guard, union spread, await suggestion ----

// Go: internal/checker/checker.go:Checker.checkForDisallowedESSymbolOperand(12756)
#[test]
fn plus_with_symbol_operand_reports_2469() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: symbol;\ns + \"\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2469);
    assert_eq!(
        diags[0].message,
        "The '+' operator cannot be applied to type 'symbol'."
    );
}

// Go: internal/checker/checker.go:Checker.checkForDisallowedESSymbolOperand(12756)
#[test]
fn relational_with_symbol_operand_reports_2469() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: symbol;\ns < 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2469);
    assert_eq!(
        diags[0].message,
        "The '<' operator cannot be applied to type 'symbol'."
    );
}

// Go: internal/checker/checker.go:Checker.isValidSpreadType(13418)
#[test]
fn object_spread_union_of_objects_is_valid() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number } | { b: number };\nconst o = { ...x };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2698),
        "union of object types should be a valid spread; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.isValidSpreadType(13418)
#[test]
fn object_spread_union_with_non_object_reports_2698() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number } | number;\nconst o = { ...x };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2698);
}

// Go: internal/checker/checker.go:Checker.getSpreadType (union distribution)
#[test]
fn object_spread_only_union_yields_union_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number } | { b: number };\nconst o = { ...x };\no;",
    );
    let usage = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ a: number; } | { b: number; }"
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType(12743)
#[test]
fn arithmetic_on_promise_operand_suggests_await() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\ndeclare const p: Promise<number>;\np - 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let left_side = diags
        .iter()
        .find(|d| d.code == 2362)
        .expect("expected left-hand arithmetic operand error");
    assert_eq!(
        left_side.message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
    assert_eq!(left_side.related_information.len(), 1);
    assert_eq!(left_side.related_information[0].code, 2773);
    assert_eq!(
        left_side.related_information[0].message,
        "Did you forget to use 'await'?"
    );
}

// ---- T1-E batch 22: spread union distribution, base-type errors, thenable await ----

// Go: internal/checker/checker.go:Checker.getSpreadType (union distribution)
#[test]
fn object_spread_union_with_prefix_distributes() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number } | { b: number };\nconst o = { a: 1, ...x };\no;",
    );
    let usage = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ a: number; } | { a: number; b: number; }"
    );
}

// Go: internal/checker/checker.go:Checker.getBaseTypesIfUnrelated(12689)
#[test]
fn plus_incompatible_literals_reports_base_types_in_message() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "1 + true;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2365);
    assert_eq!(
        diags[0].message,
        "Operator '+' cannot be applied to types 'number' and 'false | true'."
    );
}

// Go: internal/checker/checker.go:Checker.getBaseTypesIfUnrelated(12689)
#[test]
fn equality_unrelated_literals_reports_base_types_in_message() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "\"a\" === 1;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2367);
    assert_eq!(
        diags[0].message,
        "This comparison appears to be unintentional because the types 'string' and 'number' have no overlap."
    );
}

// Go: internal/checker/checker.go:Checker.getPromisedTypeOfPromiseEx(28602)
#[test]
fn arithmetic_on_thenable_operand_suggests_await() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Thenable<T> { then(onfulfilled: (value: T) => void): void; }\ndeclare const t: Thenable<number>;\nt - 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let left_side = diags
        .iter()
        .find(|d| d.code == 2362)
        .expect("expected left-hand arithmetic operand error");
    assert_eq!(
        left_side.message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
    assert_eq!(left_side.related_information.len(), 1);
    assert_eq!(left_side.related_information[0].code, 2773);
    assert_eq!(
        left_side.related_information[0].message,
        "Did you forget to use 'await'?"
    );
}

// ---- T1-E batch 23: spread falsy filter, index merge, awaited unwrap ----

// Go: internal/checker/checker.go:Checker.isValidSpreadType(13418)
#[test]
fn object_spread_nullable_object_union_is_valid() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number } | null;\nconst o = { ...x };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2698),
        "null should be stripped before spread validation; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.isValidSpreadType(13418)
#[test]
fn object_spread_falsy_literal_union_is_valid() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number } | false;\nconst o = { ...x };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2698),
        "false should be stripped before spread validation; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getSpreadType(13301) / getUnionIndexInfos(13424)
#[test]
fn object_spread_merges_string_index_signatures() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A {\n  [k: string]: number;\n}\ninterface B {\n  [k: string]: string;\n}\ndeclare const a: A;\ndeclare const b: B;\nconst o = { ...a, ...b };\ndeclare const key: string;\no[key];",
    );
    let access = expr_stmt_expression(&p, 6);
    let mut c = Checker::new();
    let t = c.check_expression(&p, access);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "string | number"
    );
}

// Go: internal/checker/checker.go:Checker.getAwaitedTypeNoAlias(30941)
#[test]
fn arithmetic_on_nested_promise_operand_suggests_await() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\ndeclare const p: Promise<Promise<number>>;\np - 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let left_side = diags
        .iter()
        .find(|d| d.code == 2362)
        .expect("expected left-hand arithmetic operand error");
    assert_eq!(left_side.related_information.len(), 1);
    assert_eq!(left_side.related_information[0].code, 2773);
    assert_eq!(
        left_side.related_information[0].message,
        "Did you forget to use 'await'?"
    );
}

// Go: internal/checker/checker.go:Checker.isValidSpreadType(13418) / getSpreadType(13301)
#[test]
fn object_spread_after_falsy_removal_yields_object_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: null | { a: number };\nconst o = { ...x };\no.a;",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    assert_eq!(c.check_expression(&p, access), c.number_type());
}

// ---- T1-E batch 24: private/protected skip, optional merge, isSpreadableProperty ----

// Go: internal/checker/checker.go:Checker.getSpreadType(13362)
#[test]
fn object_spread_skips_private_class_member() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "class C { private x: number; public y: number; }\ndeclare const c: C;\nconst o = { ...c };\no;",
    );
    let usage = expr_stmt_expression(&p, 3);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ y: number; }"
    );
}

// Go: internal/checker/checker.go:Checker.getSpreadType(13362)
#[test]
fn object_spread_skips_protected_class_member() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "class C { protected x: number; public y: number; }\ndeclare const c: C;\nconst o = { ...c };\no;",
    );
    let usage = expr_stmt_expression(&p, 3);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ y: number; }"
    );
}

// Go: internal/checker/checker.go:Checker.getSpreadType(13370)
#[test]
fn object_spread_protected_right_skips_left_same_name() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "class C { protected foo: number; }\ndeclare const left: { foo: string };\ndeclare const c: C;\nconst o = { ...left, ...c };\no;",
    );
    let usage = expr_stmt_expression(&p, 4);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{}"
    );
}

// Go: internal/checker/checker.go:Checker.isSpreadableProperty(13494)
#[test]
fn object_spread_skips_class_method_member() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "class C { m(): void {} x: number; }\ndeclare const c: C;\nconst o = { ...c };\no;",
    );
    let usage = expr_stmt_expression(&p, 3);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ x: number; }"
    );
}

// Go: internal/checker/checker.go:Checker.isSpreadableProperty(13494)
#[test]
fn object_spread_includes_interface_method_member() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface I { m(): void; x: number; }\ndeclare const i: I;\nconst o = { ...i };\no;",
    );
    let usage = expr_stmt_expression(&p, 3);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ m: m; x: number; }"
    );
}

// Go: internal/checker/checker.go:Checker.getSpreadType(13376)
#[test]
fn object_spread_merges_optional_property_types() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: { sn: string };\ndeclare const b: { sn?: number };\nconst o = { ...a, ...b };\no.sn;",
    );
    let access = expr_stmt_expression(&p, 3);
    let mut c = Checker::new();
    let t = c.check_expression(&p, access);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "string | number"
    );
}

// ---- T1-E batch 25: getSpreadSymbol set-only, primitive skip, empty-object union merge ----

// Go: internal/checker/checker.go:Checker.getSpreadSymbol(13499)
#[test]
fn object_spread_setonly_accessor_yields_undefined_property() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface I { set x(v: number): void; }\ndeclare const i: I;\nconst o = { ...i };\no.x;",
    );
    let access = expr_stmt_expression(&p, 3);
    let mut c = Checker::new();
    let t = c.check_expression(&p, access);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "undefined"
    );
}

// Go: internal/checker/checker.go:Checker.getSpreadType(13332)
#[test]
fn object_spread_ignores_primitive_like_right_operand() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: { a: number };\ndeclare const s: string;\nconst o = { ...a, ...s };\no;",
    );
    let usage = expr_stmt_expression(&p, 3);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ a: number; }"
    );
}

// Go: internal/checker/checker.go:Checker.tryMergeUnionOfObjectTypeAndEmptyObject(13444)
#[test]
fn object_spread_union_with_empty_object_makes_properties_optional() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number } | {};\nconst o = { ...x };\no;",
    );
    let usage = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ a?: number; }"
    );
}

// Go: internal/checker/checker.go:Checker.getSpreadType(13301)
#[test]
fn object_spread_intersection_type_merges_all_properties() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: string } & { b: number };\nconst o = { ...x };\no;",
    );
    let usage = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ a: string; b: number; }"
    );
}

// ---- T1-E batch 26: spread_acc trailing merge, generic intersection, never/any ----

// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076) / getSpreadType(13301)
#[test]
fn object_spread_before_named_property_merges_spread_acc() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: { x: number };\nconst o = { ...a, b: 1 };\no;",
    );
    let usage = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ x: number; b: number; }"
    );
}

// Go: internal/checker/checker.go:Checker.getSpreadType(13335)
#[test]
fn object_spread_generic_intersection_left_merges_last_object_constituent() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T>(t: T & { a: string }) {\n  const o = { ...t, b: 1 };\n  o;\n}",
    );
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let body = match arena.data(stmts[0]) {
        NodeData::FunctionDeclaration(d) => d.body.expect("function body"),
        _ => panic!("function declaration"),
    };
    let body_stmts = match arena.data(body) {
        NodeData::Block(d) => d.list.nodes.clone(),
        _ => panic!("block"),
    };
    let usage = match arena.data(body_stmts[1]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "T & { a: string; b: number; }"
    );
}

// Go: internal/checker/checker.go:Checker.getSpreadType(13308)
#[test]
fn object_spread_never_right_operand_returns_left() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const left: { a: number };\ndeclare const n: never;\nconst o = { ...left, ...n };\no;",
    );
    let usage = expr_stmt_expression(&p, 3);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ a: number; }"
    );
}

// Go: internal/checker/checker.go:Checker.getSpreadType(13302)
#[test]
fn object_spread_any_operand_yields_any() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const a: any;\nconst o = { ...a };\no;");
    let usage = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "any"
    );
}

// ---- T1-E batch 27: unknown/never operands, const-context spread readonly ----

// Go: internal/checker/checker.go:Checker.tryMergeUnionOfObjectTypeAndEmptyObject(13444) / getSpreadType(13301)
#[test]
fn object_spread_const_context_union_empty_makes_readonly_optional_property() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number } | {};\nconst o = { ...x } as const;\no;",
    );
    let usage = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ readonly a?: number; }"
    );
}

// Go: internal/checker/checker.go:Checker.getSpreadType(13308)
#[test]
fn object_spread_never_left_operand_returns_right() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const n: never;\ndeclare const right: { a: number };\nconst o = { ...n, ...right };\no;",
    );
    let usage = expr_stmt_expression(&p, 3);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ a: number; }"
    );
}

// Go: internal/checker/checker.go:Checker.getSpreadType(13301) / getSpreadSymbol(13499)
#[test]
fn const_assertion_on_spread_object_literal_marks_spread_property_readonly() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: { x: number };\nconst o = { ...a, y: 1 } as const;\no;",
    );
    let usage = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    let x =
        crate::core::declared_types::get_property_of_type(&c, t, "x").expect("spread property `x`");
    assert!(
        c.synthesized_symbol_check_flags(x)
            .contains(tsgo_ast::CheckFlags::READONLY),
        "a const-context spread property must carry the Readonly check flag"
    );
}

// Go: internal/checker/checker.go:Checker.getSpreadType(13301) / getSpreadSymbol(13499)
#[test]
fn const_assertion_on_spread_object_literal_prints_readonly_members() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: { x: number };\nconst o = { ...a, y: 1 } as const;\no;",
    );
    let usage = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ readonly x: number; readonly y: 1; }"
    );
}

// ---- T1-E batch 28: const-context index readonly, generic spread alone ----

#[test]
fn non_generic_interface_is_not_generic_object_type() {
    use crate::core::declared_types::{get_declared_type_of_symbol, is_generic_object_type};
    use crate::core::symbols::resolve_name;
    let p = StubProgram::parse_and_bind("/a.ts", "interface A { x: number; }");
    let sym = resolve_name(
        &p,
        p.root(),
        "A",
        tsgo_ast::SymbolFlags::TYPE,
        false,
        p.globals(),
    )
    .expect("interface A");
    let mut c = Checker::new();
    let t = get_declared_type_of_symbol(&mut c, &p, sym, p.globals());
    assert!(
        !is_generic_object_type(&c, t),
        "a non-generic interface must not be a generic object type"
    );
}

// Go: internal/checker/checker.go:Checker.getIndexInfoWithReadonly(13411) / getSpreadType(13301)
#[test]
fn const_assertion_on_spread_object_literal_marks_index_signature_readonly() {
    use crate::core::types::TypeData;
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { [k: string]: number; }\ndeclare const a: A;\nconst o = { ...a } as const;\no;",
    );
    let usage = expr_stmt_expression(&p, 3);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    let index_infos = match &c.get_type(t).data {
        TypeData::Object(o) => o.index_infos.clone(),
        _ => panic!("spread result must be an object type"),
    };
    assert_eq!(index_infos.len(), 1, "expected one string index signature");
    assert!(
        c.index_info(index_infos[0]).is_readonly,
        "a const-context spread index signature must be readonly"
    );
}

// Go: internal/checker/checker.go:Checker.getSpreadType(13335)
#[test]
fn object_spread_generic_type_parameter_alone_yields_type_parameter() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T>(t: T) {\n  const o = { ...t };\n  o;\n}",
    );
    let arena = p.arena();
    let stmts = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let body = match arena.data(stmts[0]) {
        NodeData::FunctionDeclaration(d) => d.body.expect("function body"),
        _ => panic!("function declaration"),
    };
    let body_stmts = match arena.data(body) {
        NodeData::Block(d) => d.list.nodes.clone(),
        _ => panic!("block"),
    };
    let usage = match arena.data(body_stmts[1]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(crate::core::nodebuilder::type_to_string(&mut c, &p, t), "T");
}

// Go: internal/checker/checker.go:Checker.getSpreadType(13301)
#[test]
fn object_spread_result_carries_object_literal_flags() {
    use crate::core::types::TypeData;
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { x: number; }\ndeclare const a: A;\n({ ...a });",
    );
    let usage = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    let object_flags = match &c.get_type(t).data {
        TypeData::Object(_) => c.get_type(t).object_flags(),
        _ => panic!("spread result must be an object type"),
    };
    assert!(
        object_flags.contains(ObjectFlags::OBJECT_LITERAL),
        "spread result must carry ObjectFlagsObjectLiteral"
    );
    assert!(
        object_flags.contains(ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL),
        "spread result must carry ObjectFlagsContainsObjectOrArrayLiteral"
    );
}

// ---- T1-E batch 29: unknown spread, late-bound literal computed names ----

// Go: internal/checker/checker.go:Checker.isValidSpreadType(13418) / checkObjectLiteral
#[test]
fn object_spread_unknown_operand_reports_2698() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const u: unknown;\nconst o = { ...u };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2698),
        "spreading `unknown` is invalid; expected 2698, got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13165) / getPropertyNameFromType
#[test]
fn object_literal_string_literal_computed_name_creates_named_property() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { ['foo']: 1 };\no.foo;");
    let access = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let t = c.check_expression(&p, access);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "number"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13165) / getPropertyNameFromType
#[test]
fn object_literal_number_literal_computed_name_creates_named_property() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { [0]: 1 };\no;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "{ 0: number; }"
    );
    let prop = crate::core::declared_types::get_property_of_type(&c, t, "0")
        .expect("literal computed name `0` is a named property");
    let prop_type = crate::core::declared_types::get_type_of_symbol(&mut c, &p, prop, p.globals());
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, prop_type),
        "number"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13165) (CheckFlagsLate)
#[test]
fn object_literal_literal_computed_name_carries_late_check_flag() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { ['foo']: 1 };\no;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    let prop = crate::core::declared_types::get_property_of_type(&c, t, "foo")
        .expect("literal computed name `foo` is a named property");
    assert!(
        c.synthesized_symbol_check_flags(prop)
            .contains(tsgo_ast::CheckFlags::LATE),
        "a literal computed-name property must carry the Late check flag"
    );
}

// ---- T1-E batch 30: unique-symbol computed names ----

// Go: internal/checker/checker.go:Checker.getESSymbolLikeTypeForNode(22841)
#[test]
fn declare_const_unique_symbol_has_unique_es_symbol_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare const sym: unique symbol;\nsym;");
    let usage = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let t = c.check_expression(&p, usage);
    assert!(
        c.get_type(t)
            .flags()
            .contains(crate::core::types::TypeFlags::UNIQUE_ES_SYMBOL),
        "a `declare const sym: unique symbol` reference must be a unique symbol type"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13165) / getPropertyNameFromType
#[test]
fn object_literal_unique_symbol_computed_name_creates_named_property() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const sym: unique symbol;\nconst o = { [sym]: 1 };\no[sym];",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let t = c.check_expression(&p, access);
    assert_eq!(
        crate::core::nodebuilder::type_to_string(&mut c, &p, t),
        "number"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13165) (CheckFlagsLate)
#[test]
fn object_literal_unique_symbol_computed_name_carries_late_check_flag() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const sym: unique symbol;\nconst o = { [sym]: 1 };\no;",
    );
    let usage = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let o_type = c.check_expression(&p, usage);
    let sym_expr = match p.arena().data(source_statements(&p)[0]) {
        NodeData::VariableStatement(d) => {
            let list = match p.arena().data(d.declaration_list) {
                NodeData::VariableDeclarationList(dl) => dl.declarations.nodes[0],
                _ => panic!("declaration list"),
            };
            match p.arena().data(list) {
                NodeData::VariableDeclaration(vd) => vd.name,
                _ => panic!("variable declaration"),
            }
        }
        _ => panic!("variable statement"),
    };
    let sym_type = c.check_expression(&p, sym_expr);
    let prop_name = crate::core::late_binding::get_property_name_from_type(&c, sym_type);
    let prop = crate::core::declared_types::get_property_of_type(&c, o_type, &prop_name)
        .expect("unique-symbol computed name must be a named property");
    assert!(
        c.synthesized_symbol_check_flags(prop)
            .contains(tsgo_ast::CheckFlags::LATE),
        "a unique-symbol computed-name property must carry the Late check flag"
    );
}

// ---- T1-E batch 31: object-literal accessors/methods, computed-name contextual typing, getWidenedUniqueESSymbolType ----

// Go: internal/checker/checker.go:Checker.getWidenedLiteralLikeTypeForContextualType(25374)
#[test]
fn object_literal_unique_symbol_value_preserved_by_contextual_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const sym: unique symbol;\nconst o: { [sym]: typeof sym } = { [sym]: sym };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "a unique-symbol value in a matching contextual position must not widen to symbol; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getContextualTypeForObjectLiteralElement(29596)
#[test]
fn object_literal_computed_name_property_value_gets_contextual_type() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o: { [\"k\"]: number } = { [\"k\"]: 1 };");
    let literal = match p.arena().data(source_statements(&p)[0]) {
        NodeData::VariableStatement(d) => {
            let list = match p.arena().data(d.declaration_list) {
                NodeData::VariableDeclarationList(dl) => dl.declarations.nodes[0],
                _ => panic!("declaration list"),
            };
            match p.arena().data(list) {
                NodeData::VariableDeclaration(vd) => vd.initializer.expect("initializer"),
                _ => panic!("variable declaration"),
            }
        }
        _ => panic!("variable statement"),
    };
    let member = match p.arena().data(literal) {
        NodeData::ObjectLiteralExpression(d) => d.list.nodes[0],
        _ => panic!("object literal"),
    };
    let value = match p.arena().data(member) {
        NodeData::PropertyAssignment(d) => d.initializer.expect("property value"),
        _ => panic!("property assignment"),
    };
    let mut c = Checker::new();
    assert_eq!(
        c.get_contextual_type(&p, value, crate::core::contextual::ContextFlags::NONE),
        Some(c.number_type())
    );
}

fn source_statements(p: &StubProgram) -> Vec<tsgo_ast::NodeId> {
    match p.arena().data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    }
}

// ---- T1-E batch 32: object-literal method/accessor members, unique symbol in type literals ----

// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076)
#[test]
fn object_literal_method_member_is_callable_property() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { m() { return 1 } };\no.m;");
    let literal = var_decl_initializer_batch5(&p, 0);
    let access = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let o_type = c.check_expression(&p, literal);
    let m_type = c.check_expression(&p, access);
    let prop = crate::core::declared_types::get_property_of_type(&c, o_type, "m")
        .expect("object literal with a method must expose the method as a named property");
    let prop_type = crate::core::declared_types::get_type_of_symbol(&mut c, &p, prop, None);
    assert_eq!(m_type, prop_type);
    assert_eq!(c.get_signatures_of_type(prop_type).len(), 1);
}

// Go: internal/checker/checker.go:Checker.checkObjectLiteralMethod(13771)
#[test]
fn object_literal_method_contextual_return_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: { m(): number } = { m() { return 1 } };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "a contextually typed object-literal method body must satisfy the contextual return type; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076) (accessor arm)
#[test]
fn object_literal_get_accessor_member_is_readable_property() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { get x() { return 1 } };\no.x;");
    let literal = var_decl_initializer_batch5(&p, 0);
    let access = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let o_type = c.check_expression(&p, literal);
    let x_type = c.check_expression(&p, access);
    let prop = crate::core::declared_types::get_property_of_type(&c, o_type, "x")
        .expect("object literal with a getter must expose the accessor as a named property");
    let prop_type = crate::core::declared_types::get_type_of_symbol(&mut c, &p, prop, None);
    assert_eq!(x_type, prop_type);
}

// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076) (accessor arm)
#[test]
fn object_literal_get_accessor_contextual_return_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: { get x(): number } = { get x() { return 1 } };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "a contextually typed object-literal getter body must satisfy the contextual return type; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeFromTypeOperatorNode(22826)
#[test]
fn type_literal_unique_symbol_property_is_unique_es_symbol() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface I { readonly sym: unique symbol; }\ndeclare const i: I;\ni.sym;",
    );
    let access = expr_stmt_expression(&p, 2);
    let mut c = Checker::new();
    let sym_type = c.check_expression(&p, access);
    assert!(
        c.get_type(sym_type)
            .flags()
            .intersects(crate::core::types::TypeFlags::UNIQUE_ES_SYMBOL),
        "a `readonly sym: unique symbol` property must resolve to a unique symbol type"
    );
}

// ---- T1-E batch 33: getTypeOfAccessors, set-accessor member typing ----

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18370)
#[test]
fn object_literal_get_accessor_infers_return_type_from_body() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { get x() { return 1 } };\no.x;");
    let literal = var_decl_initializer_batch5(&p, 0);
    let access = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let _ = c.check_expression(&p, literal);
    let x_type = c.check_expression(&p, access);
    assert_eq!(x_type, c.number_type());
}

// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076) (accessor arm)
#[test]
fn object_literal_set_accessor_member_is_writable_property() {
    let p = StubProgram::parse_and_bind("/a.ts", "const o = { set x(v: number) { } };\no.x = 1;");
    let literal = var_decl_initializer_batch5(&p, 0);
    let assign = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let o_type = c.check_expression(&p, literal);
    let _ = c.check_expression(&p, assign);
    let prop = crate::core::declared_types::get_property_of_type(&c, o_type, "x")
        .expect("object literal with a setter must expose the accessor as a named property");
    let prop_type = crate::core::declared_types::get_type_of_symbol(&mut c, &p, prop, None);
    assert_eq!(prop_type, c.number_type());
}

// Go: internal/checker/checker.go:Checker.checkAccessorDeclaration(2912)
#[test]
fn object_literal_set_accessor_contextual_parameter_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: { set x(v: number) } = { set x(v) { v = \"\"; } };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.is_empty(),
        "a contextually typed object-literal setter parameter must be checked against the contextual type"
    );
}

// ---- T1-E batch 34: getWriteTypeOfAccessors, getter return checks ----

// Go: internal/checker/checker.go:Checker.getWriteTypeOfAccessors(18429)
#[test]
fn class_accessor_assignment_uses_setter_write_type() {
    let codes = diag_codes(
        "class C {\n  get x(): number { return 1; }\n  set x(v: string) { }\n}\nconst c = new C();\nc.x = 1;",
    );
    assert!(
        codes.contains(&2322),
        "assignment to an accessor must use the setter parameter type, not the getter return type; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getWriteTypeOfAccessors(18429)
#[test]
fn object_literal_accessor_assignment_uses_setter_write_type() {
    let codes =
        diag_codes("const o = { get x(): number { return 1; }, set x(v: string) { } };\no.x = 1;");
    assert!(
        codes.contains(&2322),
        "object-literal accessor assignment must use the setter parameter type; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkAccessorDeclaration(2923)
#[test]
fn getter_without_return_reports_2378() {
    let codes = diag_codes("class C { get x() { } }");
    assert!(
        codes.contains(&2378),
        "expected TS2378 a get accessor must return a value; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkAccessorDeclaration(2946)
#[test]
fn accessor_abstract_mismatch_reports_2676() {
    let codes =
        diag_codes("abstract class C {\n  abstract get x(): number;\n  set x(v: number) { }\n}");
    assert!(
        codes.iter().filter(|&&c| c == 2676).count() >= 2,
        "expected TS2676 on getter and setter when abstractness mismatches; got {codes:?}"
    );
}

// ---- T1-E batch 35: accessor computed names, implicit-any resolution ----

// Go: internal/checker/checker.go:Checker.checkAccessorDeclaration(2918)
#[test]
fn class_accessor_constructor_name_reports_1341() {
    let codes = diag_codes("class C { get constructor() { return 1; } }");
    assert!(
        codes.contains(&1341),
        "expected TS1341 class constructor may not be an accessor; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18401)
#[test]
fn set_only_accessor_without_param_type_reports_7032() {
    let codes = diag_codes("class C { set x(v) { } }");
    assert!(
        codes.contains(&7032),
        "expected TS7032 set accessor lacks parameter type annotation; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18403)
#[test]
fn abstract_getter_without_return_type_reports_7033() {
    let codes = diag_codes("abstract class C { abstract get x(); }");
    assert!(
        codes.contains(&7033),
        "expected TS7033 get accessor lacks return type annotation; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkAccessorDeclaration(2933)
#[test]
fn class_accessor_computed_name_checks_expression() {
    let codes = diag_codes("class C { get [x](): number { return 1; } }");
    assert!(
        codes.contains(&2304),
        "expected TS2304 on unresolved computed accessor name expression; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkAccessorDeclaration(2933) / checkComputedPropertyName
#[test]
fn class_accessor_computed_name_non_indexable_reports_2464() {
    let codes = diag_codes("const k: boolean = true;\nclass C { get [k](): number { return 1; } }");
    assert!(
        codes.contains(&2464),
        "expected TS2464 computed accessor name must be string/number/symbol; got {codes:?}"
    );
}

// ---- T1-E batch 36: accessor return paths, setter-annotated getter typing ----

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18370)
#[test]
fn set_only_accessor_read_type_from_setter_param() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { set x(v: number) { } }\nconst c = new C();\nconst n: number = c.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "set-only accessor read type must come from the setter parameter annotation; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18385) / checkReturnExpression
#[test]
fn getter_body_checked_against_setter_param_type() {
    let codes = diag_codes("class C { set x(v: number) { } get x() { return \"hi\"; } }");
    assert!(
        codes.contains(&2322),
        "getter body must be checked against the setter parameter type when the getter lacks an annotation; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkAllCodePathsInNonVoidFunctionReturnOrThrow(3704)
#[test]
fn getter_incomplete_return_paths_reports_2366() {
    let codes = diag_codes("let b = true;\nclass C { get x(): number { if (b) return 1; } }");
    assert!(
        codes.contains(&2366),
        "expected TS2366 when an annotated getter can fall through without returning; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkAllCodePathsInNonVoidFunctionReturnOrThrow(3704)
#[test]
fn getter_return_mismatch_with_explicit_annotation_reports_2322() {
    let codes = diag_codes("class C { get x(): number { return \"hi\"; } }");
    assert!(
        codes.contains(&2322),
        "getter return expression must match the explicit return type annotation; got {codes:?}"
    );
}

// ---- T1-E batch 37: accessor annotation priority, circularity guards ----

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18385)
#[test]
fn getter_annotation_takes_priority_over_setter_param_for_read_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { get x(): string { return \"\"; } set x(v: number) { } }\nconst c = new C();\nconst s: string = c.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "getter return-type annotation must win over setter parameter type for read type; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18385) / checkReturnExpression
#[test]
fn getter_body_checked_against_own_annotation_not_setter_param() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { get x(): number { return 1; } set x(v: string) { } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "getter body must be checked against its own annotation, not the setter parameter type; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkAccessorDeclaration(2960) / checkParameter
#[test]
fn class_set_accessor_body_parameter_assignability_reports_2322() {
    let codes = diag_codes("class C { set x(v: number) { v = \"\"; } }");
    assert!(
        codes.contains(&2322),
        "setter body must check parameter assignments against the annotated parameter type; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18370)
#[test]
fn object_literal_set_only_accessor_read_type_from_setter_param() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o = { set x(v: number) { } };\nconst n: number = o.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "object-literal set-only accessor read type must come from the setter parameter annotation; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18410)
#[test]
fn accessor_circular_type_annotation_reports_2502() {
    let codes = diag_codes("class C { get x(): C[\"x\"] { return this.x; } }");
    assert!(
        codes.contains(&2502),
        "expected TS2502 when an accessor type annotation is circular; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18417)
#[test]
fn getter_circular_body_inference_reports_7024() {
    let codes = diag_codes("class C { get x() { return this.x; } }");
    assert!(
        codes.contains(&7024),
        "expected TS7024 when getter return type is inferred circularly from its body; got {codes:?}"
    );
}

// ---- T1-E batch 38: object-literal accessor circularity, write-type guards ----

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18411)
#[test]
fn setter_circular_type_annotation_reports_2502() {
    let codes = diag_codes("class C { set x(v: C[\"x\"]) { } }");
    assert!(
        codes.contains(&2502),
        "expected TS2502 when a setter parameter type annotation is circular; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getWriteTypeOfAccessors(18433)
#[test]
fn set_only_accessor_write_type_resolution_is_reentrant_safe() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { set x(v: C[\"x\"]) { } }\nconst c = new C();\nc.x = c.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2502),
        "set-only accessor write/read uses must surface TS2502 for a circular setter annotation; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getWriteTypeOfAccessors(18429)
#[test]
fn set_only_accessor_write_type_falls_back_to_read_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { set x(v: number) { } }\nconst c = new C();\nc.x = 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "set-only accessor assignment must use read type when no explicit write annotation exists; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkAccessorDeclaration(2936)
#[test]
fn protected_getter_private_setter_is_accessible() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  protected get x(): number { return 1; }\n  private set x(v: number) { }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2808),
        "protected getter with private setter must not report TS2808; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkAccessorDeclaration(2936)
#[test]
fn abstract_setter_concrete_getter_reports_2676() {
    let codes =
        diag_codes("abstract class C {\n  abstract set x(v: number);\n  get x() { return 1; }\n}");
    assert!(
        codes.iter().filter(|&&c| c == 2676).count() >= 2,
        "expected TS2676 when abstractness mismatches on setter vs getter; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkAccessorDeclaration(2936)
#[test]
fn public_getter_private_setter_is_accessible() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  public get x(): number { return 1; }\n  private set x(v: number) { }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2808),
        "public getter with private setter must not report TS2808; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18417)
#[test]
fn object_literal_getter_circular_body_inference_reports_7024() {
    let codes = diag_codes("const o = { get x() { return o.x; } };");
    assert!(
        codes.contains(&7024),
        "expected TS7024 when an object-literal getter infers its return type circularly; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getWriteTypeOfAccessors(18454)
#[test]
fn get_only_accessor_assignment_uses_read_type_and_reports_2322() {
    let codes =
        diag_codes("class C { get x(): number { return 1; } }\nconst c = new C();\nc.x = \"\";");
    assert!(
        codes.contains(&2322),
        "get-only accessor assignment must use read type as write type and reject mismatched values; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectLiteral(13076) / checkReturnExpression
#[test]
fn object_literal_getter_contextual_return_type_mismatch_reports_2322() {
    let codes = diag_codes("const o: { get x(): string; } = { get x() { return 1; } };");
    assert!(
        codes.contains(&2322),
        "object-literal getter body must be checked against contextual return type; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18404)
#[test]
fn getter_without_annotation_infers_from_body_when_setter_annotated() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { get x() { return 1; } set x(v: number) { } }\nconst c = new C();\nconst n: number = c.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "getter without annotation must infer read type from body even when setter is annotated; got {diags:?}"
    );
}

// ---- T1-E batch 39: auto-accessor property read/write typing ----

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18370)
#[test]
fn auto_accessor_annotated_read_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { accessor x: number; }\nconst c = new C();\nconst n: number = c.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "annotated auto-accessor read type must resolve from the property annotation; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getWriteTypeOfAccessors(18429)
#[test]
fn auto_accessor_assignment_uses_annotation_as_write_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { accessor x: number; }\nconst c = new C();\nc.x = 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "auto-accessor assignment must use the property type annotation as write type; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getWriteTypeOfAccessors(18429)
#[test]
fn auto_accessor_assignment_mismatch_reports_2322() {
    let codes = diag_codes("class C { accessor x: number; }\nconst c = new C();\nc.x = \"\";");
    assert!(
        codes.contains(&2322),
        "auto-accessor assignment must reject values not assignable to the annotated write type; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18398)
#[test]
fn auto_accessor_infers_read_type_from_initializer() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { accessor x = 1; }\nconst c = new C();\nconst n: number = c.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "auto-accessor without annotation must infer read type from its initializer; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18405)
#[test]
fn auto_accessor_without_type_or_initializer_reports_7008() {
    let codes = diag_codes("class C { accessor x; }");
    assert!(
        codes.contains(&7008),
        "expected TS7008 when an auto-accessor lacks both type annotation and initializer; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18411)
#[test]
fn auto_accessor_circular_type_annotation_reports_2502() {
    let codes = diag_codes("class C { accessor x: C[\"x\"]; }");
    assert!(
        codes.contains(&2502),
        "expected TS2502 when an auto-accessor type annotation is circular; got {codes:?}"
    );
}

// ---- T1-E batch 40: auto-accessor grammar and initializer checks ----

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarProperty(1904)
#[test]
fn auto_accessor_optional_postfix_reports_1276() {
    let codes = diag_codes("class C { accessor x?: number; }");
    assert!(
        codes.contains(&1276),
        "expected TS1276 when an auto-accessor is declared optional; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarProperty(1904)
#[test]
fn auto_accessor_non_optional_postfix_has_no_1276() {
    let codes = diag_codes("class C { accessor x: number; }");
    assert!(
        !codes.contains(&1276),
        "annotated auto-accessor without ? must not report TS1276; got {codes:?}"
    );
}

// ---- T1-E batch 41: auto-accessor definite assignment and initializer checks ----

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarProperty(1938)
#[test]
fn auto_accessor_definite_assignment_with_initializer_reports_1263() {
    let codes = diag_codes("class C { accessor x!: string = \"a\"; }");
    assert!(
        codes.contains(&1263),
        "expected TS1263 when an auto-accessor has both ! and an initializer; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarProperty(1942)
#[test]
fn auto_accessor_definite_assignment_without_type_reports_1264() {
    let codes = diag_codes("class C { accessor x!; }");
    assert!(
        codes.contains(&1264),
        "expected TS1264 when an auto-accessor has ! without a type annotation; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarProperty(1938)
#[test]
fn auto_accessor_definite_assignment_with_type_has_no_grammar_error() {
    let codes = diag_codes("class C { accessor x!: number; }");
    assert!(
        !codes.contains(&1263) && !codes.contains(&1264),
        "annotated auto-accessor with ! must not report definite-assignment grammar errors; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkPropertyDeclaration / checkVariableLikeDeclaration(5760)
#[test]
fn auto_accessor_initializer_not_assignable_reports_2322() {
    let codes = diag_codes("class C { accessor x: number = \"s\"; }");
    assert!(
        codes.contains(&2322),
        "auto-accessor initializer must be checked against its type annotation; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkPropertyDeclaration / checkVariableLikeDeclaration(5760)
#[test]
fn auto_accessor_initializer_assignable_reports_no_2322() {
    let codes = diag_codes("class C { accessor x: number = 1; }");
    assert!(
        !codes.contains(&2322),
        "assignable auto-accessor initializer must not report TS2322; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarProperty(1944)
#[test]
fn static_auto_accessor_definite_assignment_reports_1255() {
    let codes = diag_codes("class C { static accessor x!: number; }");
    assert!(
        codes.contains(&1255),
        "expected TS1255 when a static auto-accessor uses definite assignment assertion; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkPropertyDeclaration (abstract initializer)
#[test]
fn abstract_auto_accessor_with_initializer_reports_1267() {
    let codes = diag_codes("abstract class C { abstract accessor x: number = 1; }");
    assert!(
        codes.contains(&1267),
        "expected TS1267 when an abstract auto-accessor has an initializer; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (assignment to inferred write type)
#[test]
fn auto_accessor_inferred_write_type_rejects_bad_assignment() {
    let codes = diag_codes("class C { accessor x = 1; }\nconst c = new C();\nc.x = \"\";");
    assert!(
        codes.contains(&2322),
        "assignment to inferred auto-accessor write type must reject incompatible values; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkPropertyAccessExpressionOrQualifiedName (private identifier)
#[test]
fn private_auto_accessor_read_type_resolves() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { accessor #x: number = 1; m(): number { return this.#x; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "private auto-accessor read type must resolve from annotation inside the class; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkPropertyAccessExpressionOrQualifiedName (private identifier write)
#[test]
fn private_auto_accessor_write_rejects_incompatible_assignment() {
    let codes = diag_codes("class C { accessor #x: number = 1; m() { this.#x = \"\"; } }");
    assert!(
        codes.contains(&2322),
        "private auto-accessor assignment must use write type; got {codes:?}"
    );
}

// ---- T1-E batch 42: auto-accessor modifier ordering and static typing ----

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(499)
#[test]
fn abstract_must_precede_accessor_modifier_reports_1029() {
    let codes = diag_codes("abstract class C { accessor abstract x: number; }");
    assert!(
        codes.contains(&1029),
        "expected TS1029 when accessor precedes abstract; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18370)
#[test]
fn static_auto_accessor_read_type_resolves() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { static accessor x: number = 1; }\nconst n: number = C.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "static auto-accessor read type must resolve from annotation; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getWriteTypeOfAccessors(18429)
#[test]
fn static_auto_accessor_assignment_rejects_incompatible_value() {
    let codes = diag_codes("class C { static accessor x: number = 1; }\nC.x = \"\";");
    assert!(
        codes.contains(&2322),
        "static auto-accessor assignment must reject incompatible values; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(369)
#[test]
fn static_must_precede_accessor_modifier_reports_1029() {
    let codes = diag_codes("class C { accessor static x: number = 1; }");
    assert!(
        codes.contains(&1029),
        "expected TS1029 when accessor precedes static; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(478)
#[test]
fn abstract_auto_accessor_outside_abstract_class_reports_1253() {
    let codes = diag_codes("class C { abstract accessor x: number; }");
    assert!(
        codes.contains(&1253),
        "expected TS1253 when abstract auto-accessor appears outside an abstract class; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeOfAccessors(18370)
#[test]
fn abstract_auto_accessor_without_initializer_typechecks() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "abstract class C { abstract accessor x: number; }\nclass D extends C { accessor x: number = 1; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "abstract auto-accessor declaration and concrete override must typecheck; got {diags:?}"
    );
}

// ---- T1-E batch 43: auto-accessor override and modifier grammar ----

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(503)
#[test]
fn abstract_auto_accessor_private_identifier_reports_18019() {
    let codes = diag_codes("abstract class C { abstract accessor #x: number; }");
    assert!(
        codes.contains(&18019),
        "expected TS18019 when abstract auto-accessor uses a private identifier; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(326)
#[test]
fn override_must_precede_accessor_modifier_reports_1029() {
    let codes = diag_codes("class C { accessor override x: number = 1; }");
    assert!(
        codes.contains(&1029),
        "expected TS1029 when accessor precedes override; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.issueMemberSpecificError(4467)
#[test]
fn auto_accessor_override_incompatible_type_reports_2416() {
    let codes = diag_codes(
        "class B { accessor x: number = 1; }\nclass D extends B { override accessor x: string = \"a\"; }",
    );
    assert!(
        codes.contains(&2416),
        "incompatible auto-accessor override must report TS2416; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.issueMemberSpecificError(4467)
#[test]
fn abstract_auto_accessor_override_incompatible_type_reports_2416() {
    let codes = diag_codes(
        "abstract class C { abstract accessor x: number; }\nclass D extends C { accessor x: string = \"a\"; }",
    );
    assert!(
        codes.contains(&2416),
        "incompatible concrete auto-accessor override of abstract accessor must report TS2416; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.issueMemberSpecificError(4467)
#[test]
fn compatible_auto_accessor_override_reports_no_extends_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B { accessor x: number = 1; }\nclass D extends B { override accessor x: number = 2; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "compatible auto-accessor override must not report extends errors; got {diags:?}"
    );
}

// ---- T1-E batch 44: auto-accessor accessibility ordering and duplicate members ----

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(344)
#[test]
fn public_must_precede_accessor_modifier_reports_1029() {
    let codes = diag_codes("class C { accessor public x: number = 1; }");
    assert!(
        codes.contains(&1029),
        "expected TS1029 when accessor precedes public; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(344)
#[test]
fn private_must_precede_accessor_modifier_reports_1029() {
    let codes = diag_codes("class C { accessor private x: number = 1; }");
    assert!(
        codes.contains(&1029),
        "expected TS1029 when accessor precedes private; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(344)
#[test]
fn protected_must_precede_accessor_modifier_reports_1029() {
    let codes = diag_codes("class C { accessor protected x: number = 1; }");
    assert!(
        codes.contains(&1029),
        "expected TS1029 when accessor precedes protected; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(358)
#[test]
fn public_auto_accessor_private_identifier_reports_18010() {
    let codes = diag_codes("class C { public accessor #x: number = 1; }");
    assert!(
        codes.contains(&18010),
        "expected TS18010 when a public accessibility modifier combines with a private identifier; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3122)
#[test]
fn class_property_and_auto_accessor_duplicate_reports_2300() {
    let count = duplicate_identifier_count("class C { x: number; accessor x: string = \"a\"; }");
    assert_eq!(
        count, 2,
        "property plus auto-accessor with the same name must report TS2300 twice"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(465)
#[test]
fn declare_accessor_conflict_reports_1243() {
    let codes = diag_codes("class C { declare accessor x: number; }");
    assert!(
        codes.contains(&1243),
        "expected TS1243 when declare combines with accessor; got {codes:?}"
    );
}

// ---- T1-E batch 45: auto-accessor modifier conflicts and duplicate members ----

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(383)
#[test]
fn duplicate_accessor_modifier_reports_1030() {
    let codes = diag_codes("class C { accessor accessor x: number = 1; }");
    assert!(
        codes.contains(&1030),
        "expected TS1030 when accessor modifier is duplicated; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(385)
#[test]
fn readonly_accessor_conflict_reports_1243() {
    let codes = diag_codes("class C { readonly accessor x: number = 1; }");
    assert!(
        codes.contains(&1243),
        "expected TS1243 when readonly combines with accessor; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(400)
#[test]
fn accessor_readonly_conflict_reports_1243() {
    let codes = diag_codes("class C { accessor readonly x: number = 1; }");
    assert!(
        codes.contains(&1243),
        "expected TS1243 when accessor precedes readonly; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(358)
#[test]
fn protected_auto_accessor_private_identifier_reports_18010() {
    let codes = diag_codes("class C { protected accessor #x: number = 1; }");
    assert!(
        codes.contains(&18010),
        "expected TS18010 when protected accessibility combines with a private identifier; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(358)
#[test]
fn private_auto_accessor_private_identifier_reports_18010() {
    let codes = diag_codes("class C { private accessor #x: number = 1; }");
    assert!(
        codes.contains(&18010),
        "expected TS18010 when private accessibility combines with a private identifier; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(443)
#[test]
fn abstract_static_auto_accessor_conflict_reports_1243() {
    let codes = diag_codes("abstract class C { static abstract accessor x: number; }");
    assert!(
        codes.contains(&1243),
        "expected TS1243 when static combines with abstract on an auto-accessor; got {codes:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModifiers(450)
#[test]
fn abstract_private_auto_accessor_conflict_reports_1243() {
    let codes = diag_codes("abstract class C { private abstract accessor x: number; }");
    assert!(
        codes.contains(&1243),
        "expected TS1243 when private combines with abstract on an auto-accessor; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3122)
#[test]
fn class_get_accessor_and_auto_accessor_duplicate_reports_2300() {
    let count =
        duplicate_identifier_count("class C { get x() { return 1; } accessor x: number = 1; }");
    assert_eq!(
        count, 2,
        "get accessor plus auto-accessor with the same name must report TS2300 twice"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3122)
#[test]
fn declare_class_get_accessor_auto_accessor_set_duplicate_reports_2300() {
    let count = duplicate_identifier_count(
        "declare class C { get x(): number; accessor x: number; set x(value: number); }",
    );
    assert_eq!(
        count, 3,
        "get, auto-accessor, and set with the same name must report TS2300 three times (C3)"
    );
}

// ---- T1-E batch 46: constructor parameter-property duplicate members ----

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3157)
#[test]
fn parameter_property_and_property_duplicate_reports_2300() {
    let count =
        duplicate_identifier_count("class C { constructor(public x: number) {} x: string; }");
    assert_eq!(
        count, 2,
        "parameter property plus property with the same name must report TS2300 twice"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3157)
#[test]
fn parameter_property_and_get_accessor_duplicate_reports_2300() {
    let count = duplicate_identifier_count(
        "class C { constructor(public x: number) {} get x() { return 1; } }",
    );
    assert_eq!(
        count, 2,
        "parameter property plus get accessor with the same name must report TS2300 twice"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3157)
#[test]
fn parameter_property_and_auto_accessor_duplicate_reports_2300() {
    let count = duplicate_identifier_count(
        "class C { constructor(public x: number) {} accessor x: string = \"a\"; }",
    );
    assert_eq!(
        count, 2,
        "parameter property plus auto-accessor with the same name must report TS2300 twice"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3157)
#[test]
fn parameter_property_and_set_accessor_duplicate_reports_2300() {
    let count = duplicate_identifier_count(
        "class C { constructor(public x: number) {} set x(value: number) {} }",
    );
    assert_eq!(
        count, 2,
        "parameter property plus set accessor with the same name must report TS2300 twice"
    );
}

// ---- T1-E batch 47: static prototype conflict + private-name static/instance ----

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3165)
#[test]
fn class_static_prototype_conflicts_with_function_builtin_2699() {
    let codes = diag_codes("class C { static prototype: number; }");
    assert!(
        codes.contains(&2699),
        "expected TS2699 when a class declares static `prototype`; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3165)
#[test]
fn declare_class_static_prototype_ambient_no_prototype_2699() {
    let codes = diag_codes("declare class C { static prototype: number; }");
    assert!(
        !codes.contains(&2699),
        "ambient classes skip the static `prototype` conflict check; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3177)
#[test]
fn class_static_and_instance_private_name_reports_2804() {
    let count = diag_codes("class C { #foo = 1; static #foo = 2; }")
        .into_iter()
        .filter(|c| *c == 2804)
        .count();
    assert_eq!(
        count, 2,
        "instance and static members sharing `#foo` must report TS2804 twice"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3177)
#[test]
fn class_static_and_instance_private_method_reports_2804() {
    let count = diag_codes("class C { #foo() {} static #foo() {} }")
        .into_iter()
        .filter(|c| *c == 2804)
        .count();
    assert_eq!(
        count, 2,
        "instance and static private methods sharing `#foo` must report TS2804 twice"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3177)
#[test]
fn class_instance_only_private_name_no_2804() {
    let codes = diag_codes("class C { #foo = 1; }");
    assert!(
        !codes.iter().any(|&c| c == 2804),
        "a lone instance private field must not report TS2804; got {codes:?}"
    );
}

// ---- T1-E batch 48: class-expression duplicate declarations ----

// Go: internal/checker/checker.go:Checker.checkClassExpression(10007)
#[test]
fn class_expression_duplicate_properties_reports_2300() {
    let count = duplicate_identifier_count("const C = class { x: number; x: string; };");
    assert_eq!(
        count, 2,
        "duplicate properties in a class expression must report TS2300 twice"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3165)
#[test]
fn class_expression_static_prototype_conflicts_with_function_builtin_2699() {
    let codes = diag_codes("const C = class { static prototype: number; };");
    assert!(
        codes.contains(&2699),
        "expected TS2699 when a class expression declares static `prototype`; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3177)
#[test]
fn class_expression_static_and_instance_private_name_reports_2804() {
    let count = diag_codes("const C = class { #foo = 1; static #foo = 2; };")
        .into_iter()
        .filter(|c| *c == 2804)
        .count();
    assert_eq!(
        count, 2,
        "instance and static private fields sharing `#foo` in a class expression must report TS2804 twice"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3157)
#[test]
fn class_expression_parameter_property_and_property_duplicate_reports_2300() {
    let count = duplicate_identifier_count(
        "const C = class { constructor(public x: number) {} x: string; };",
    );
    assert_eq!(
        count, 2,
        "parameter property plus property in a class expression must report TS2300 twice"
    );
}

// ---- T1-E batch 49: class-expression method overload static consistency ----

// Go: internal/checker/checker.go:Checker.checkClassExpression(10007) +
// Checker.checkFunctionOrConstructorSymbolWorker (reportImplementationExpectedError)
#[test]
fn class_expression_method_static_instance_overload_mismatch_reports_2387() {
    let codes = diag_codes(
        "const C = class {\n  foo(): void;\n  static foo(): void;\n  static foo() {}\n};",
    );
    assert!(
        codes.contains(&2387),
        "expected TS2387 static/instance overload mismatch in a class expression; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3157)
#[test]
fn class_expression_parameter_property_and_get_accessor_duplicate_reports_2300() {
    let count = duplicate_identifier_count(
        "const C = class { constructor(public x: number) {} get x() { return 1; } };",
    );
    assert_eq!(
        count, 2,
        "parameter property plus get accessor in a class expression must report TS2300 twice"
    );
}

// Go: internal/checker/checker.go:Checker.checkObjectTypeForDuplicateDeclarations(3177)
#[test]
fn class_expression_static_and_instance_private_method_reports_2804() {
    let count = diag_codes("const C = class { #foo() {} static #foo() {} };")
        .into_iter()
        .filter(|c| *c == 2804)
        .count();
    assert_eq!(
        count, 2,
        "instance and static private methods sharing `#foo` in a class expression must report TS2804 twice"
    );
}

// ---- T1-E batch 50: useDefineForClassFields static-name conflict guard ----

fn diag_codes_with_options(src: &str, options: CompilerOptions) -> Vec<i32> {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts", src, options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.get_diagnostics(root)
        .into_iter()
        .map(|d| d.code)
        .collect()
}

// Go: internal/checker/checker.go:Checker.checkClassForStaticPropertyNameConflicts(4367)
#[test]
fn class_static_name_no_2699_when_use_define_for_class_fields_enabled() {
    let codes = diag_codes("class C { static name = 1; }");
    assert!(
        !codes.contains(&2699),
        "with useDefineForClassFields (default), static `name` must not report TS2699; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkClassForStaticPropertyNameConflicts(4367)
#[test]
fn class_static_name_reports_2699_when_use_define_for_class_fields_disabled() {
    let options = CompilerOptions {
        use_define_for_class_fields: Tristate::False,
        ..CompilerOptions::default()
    };
    let codes = diag_codes_with_options("class C { static name = 1; }", options);
    assert!(
        codes.contains(&2699),
        "without useDefineForClassFields, static `name` must report TS2699; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkClassForStaticPropertyNameConflicts(4367)
#[test]
fn class_expression_static_name_no_2699_when_use_define_for_class_fields_enabled() {
    let codes = diag_codes("const C = class { static name = 1; };");
    assert!(
        !codes.contains(&2699),
        "with useDefineForClassFields (default), a class expression static `name` must not report TS2699; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkClassForStaticPropertyNameConflicts(4367)
#[test]
fn class_expression_static_name_reports_2699_when_use_define_for_class_fields_disabled() {
    let options = CompilerOptions {
        use_define_for_class_fields: Tristate::False,
        ..CompilerOptions::default()
    };
    let codes = diag_codes_with_options("const C = class { static name = 1; };", options);
    assert!(
        codes.contains(&2699),
        "without useDefineForClassFields, a class expression static `name` must report TS2699; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkClassExpression(10007) +
// Checker.checkFunctionOrConstructorSymbolWorker (reportImplementationExpectedError)
#[test]
fn class_expression_method_instance_static_overload_mismatch_reports_2388() {
    let codes =
        diag_codes("const C = class {\n  static foo(): void;\n  foo(): void;\n  foo() {}\n};");
    assert!(
        codes.contains(&2388),
        "expected TS2388 instance/static overload mismatch in a class expression; got {codes:?}"
    );
}

// ---- T1-E batch 51: effective property names for static builtin conflicts ----

// Go: internal/checker/checker.go:Checker.checkClassForStaticPropertyNameConflicts(4366) +
// getEffectivePropertyNameForPropertyNameNode(18566)
#[test]
fn class_static_computed_string_literal_name_reports_2699_when_use_define_for_class_fields_disabled(
) {
    let options = CompilerOptions {
        use_define_for_class_fields: Tristate::False,
        ..CompilerOptions::default()
    };
    let codes = diag_codes_with_options("class C { static [\"name\"] = 1; }", options);
    assert!(
        codes.contains(&2699),
        "computed static `[\"name\"]` must report TS2699 when udfcf is off; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkClassForStaticPropertyNameConflicts(4366) +
// getEffectivePropertyNameForPropertyNameNode(18566)
#[test]
fn class_expression_static_computed_string_literal_name_reports_2699_when_use_define_for_class_fields_disabled(
) {
    let options = CompilerOptions {
        use_define_for_class_fields: Tristate::False,
        ..CompilerOptions::default()
    };
    let codes = diag_codes_with_options("const C = class { static [\"length\"] = 1; };", options);
    assert!(
        codes.contains(&2699),
        "computed static `[\"length\"]` in a class expression must report TS2699 when udfcf is off; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkClassForStaticPropertyNameConflicts(4366) +
// getEffectivePropertyNameForPropertyNameNode(18566)
#[test]
fn class_static_computed_const_name_reports_2699_when_use_define_for_class_fields_disabled() {
    let options = CompilerOptions {
        use_define_for_class_fields: Tristate::False,
        ..CompilerOptions::default()
    };
    let codes = diag_codes_with_options(
        "const key = \"caller\" as const;\nclass C { static [key] = 1; }",
        options,
    );
    assert!(
        codes.contains(&2699),
        "computed static `[key]` with a const string-literal type must report TS2699 when udfcf is off; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkFunctionOrConstructorSymbolWorker (reportImplementationExpectedError)
#[test]
fn method_instance_static_overload_mismatch_reports_2388() {
    let codes = diag_codes("class C {\n  static foo(): void;\n  foo(): void;\n  foo() {}\n}");
    assert!(
        codes.contains(&2388),
        "expected TS2388 instance/static overload mismatch; got {codes:?}"
    );
}

// ---- T1-E batch 52: implements-a-class hint + entity-name implements guard ----

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4348)
#[test]
fn class_incorrectly_implements_class_reports_2720() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B {\n  x: number = 1;\n}\nclass C implements B {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(
        diags[0].code, 2720,
        "implementing a class must report TS2720, not TS2420; got {diags:?}"
    );
    assert_eq!(
        diags[0].message,
        "Class 'C' incorrectly implements class 'B'. Did you mean to extend 'B' and inherit its members as a subclass?"
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4340)
#[test]
fn class_implements_non_entity_name_expression_reports_2500() {
    let codes = diag_codes("class C implements (1 as any) {}");
    assert!(
        codes.contains(&2500),
        "non-entity-name implements expression must report TS2500; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4348)
#[test]
fn class_expression_incorrectly_implements_class_reports_2720() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B {\n  x: number = 1;\n}\nconst C = class implements B {};",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 2720, "got {diags:?}");
}

// ---- T1-E batch 53: class index constraint checks ----

// Go: internal/checker/checker.go:Checker.checkIndexConstraints(4360)
#[test]
fn class_property_not_assignable_to_string_index_reports_2411() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  [x: string]: string;\n  a: number;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 2411);
    assert_eq!(
        diags[0].message,
        "Property 'a' of type 'number' is not assignable to 'string' index type 'string'."
    );
}

// Go: internal/checker/checker.go:Checker.checkIndexConstraints(4360)
#[test]
fn class_property_assignable_to_string_index_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  [x: string]: string;\n  a: string;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "compatible property and index signature must not report"
    );
}

// Go: internal/checker/checker.go:Checker.checkIndexConstraintForProperty(4787)
#[test]
fn class_without_index_signature_skips_index_constraint_check() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  a: number;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "class without index signature must not report index constraint errors"
    );
}

// ---- T1-E batch 54: cross-index-signature compatibility (2413) ----

// Go: internal/checker/checker.go:Checker.checkIndexConstraintForIndexSignature(4833)
#[test]
fn class_incompatible_index_signatures_reports_2413() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  [n: number]: string;\n  [s: string]: number;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 2413);
    assert_eq!(
        diags[0].message,
        "'number' index type 'string' is not assignable to 'string' index type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkIndexConstraintForIndexSignature(4833)
#[test]
fn class_compatible_index_signatures_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  [n: number]: number;\n  [s: string]: number;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "compatible index signatures must not report cross-index errors"
    );
}

// ---- T1-E batch 55: static-side index constraints ----

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4361)
#[test]
fn class_static_property_not_assignable_to_string_index_reports_2411() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  static [x: string]: string;\n  static a: number;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 2411);
    assert_eq!(
        diags[0].message,
        "Property 'a' of type 'number' is not assignable to 'string' index type 'string'."
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4361)
#[test]
fn class_static_property_assignable_to_string_index_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  static [x: string]: string;\n  static a: string;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "compatible static property and index signature must not report"
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4361)
#[test]
fn class_static_incompatible_index_signatures_reports_2413() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  static [n: number]: string;\n  static [s: string]: number;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 2413);
    assert_eq!(
        diags[0].message,
        "'number' index type 'string' is not assignable to 'string' index type 'number'."
    );
}

// ---- T1-E batch 56: static-side extends check (2417) ----

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4310)
#[test]
fn class_static_side_incorrectly_extends_base_reports_2417() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class A {\n  static x: string = \"a\";\n}\nclass B extends A {\n  static x: number = 1;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 2417);
    assert_eq!(
        diags[0].message,
        "Class static side 'typeof B' incorrectly extends base class static side 'typeof A'."
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4310)
#[test]
fn class_static_side_compatible_extends_base_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class A {\n  static x: string = \"a\";\n}\nclass B extends A {\n  static x: string = \"b\";\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "compatible static override must not report static-side extends error"
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4309)
#[test]
fn class_instance_extends_failure_skips_static_side_check() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class A {\n  x: number = 0;\n  static x: string = \"a\";\n}\nclass B extends A {\n  x: string = \"s\";\n  static x: number = 1;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(
        diags.len(),
        1,
        "expected only instance extends error; got {diags:?}"
    );
    assert_eq!(
        diags[0].code, 2416,
        "static-side 2417 must be skipped when instance extends fails"
    );
}

// ---- T1-E batch 57: override modifiers, base accessibility, abstract members, property init ----

// Go: internal/checker/checker.go:Checker.checkBaseTypeAccessibility(4454)
#[test]
fn class_extending_private_constructor_class_reports_2675() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class Base {\n  private constructor() {}\n}\nclass Derived extends Base {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 2675);
    assert_eq!(
        diags[0].message,
        "Cannot extend a class 'Base'. Class constructor is marked as private."
    );
}

// Go: internal/checker/checker.go:Checker.checkMemberForOverrideModifier(4706)
#[test]
fn override_modifier_without_extends_reports_4112() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  override x = 1;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 4112);
    assert_eq!(
        diags[0].message,
        "This member cannot have an 'override' modifier because its containing class 'C' does not extend another class."
    );
}

// Go: internal/checker/checker.go:Checker.checkMemberForOverrideModifier(4732)
#[test]
fn override_modifier_for_nonexistent_base_member_reports_4113() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B {\n  x = 1;\n}\nclass D extends B {\n  override y = 2;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 4113);
    assert_eq!(
        diags[0].message,
        "This member cannot have an 'override' modifier because it is not declared in the base class 'B'."
    );
}

// Go: internal/checker/checker.go:Checker.checkMemberForOverrideModifier(4741)
#[test]
fn missing_override_modifier_with_no_implicit_override_reports_4114() {
    let options = CompilerOptions {
        no_implicit_override: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "class B {\n  x = 1;\n}\nclass D extends B {\n  x = 2;\n}",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 4114);
    assert_eq!(
        diags[0].message,
        "This member must have an 'override' modifier because it overrides a member in the base class 'B'."
    );
}

// Go: internal/checker/checker.go:Checker.checkKindsOfPropertyMemberOverrides(4645)
#[test]
fn non_abstract_class_missing_abstract_member_reports_2515() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "abstract class B {\n  abstract m(): void;\n}\nclass D extends B {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 2515);
    assert_eq!(
        diags[0].message,
        "Non-abstract class 'D' does not implement inherited abstract member m from class 'B'."
    );
}

// Go: internal/checker/checker.go:Checker.checkPropertyInitialization(4907)
#[test]
fn uninitialized_property_with_strict_property_initialization_reports_2564() {
    let options = CompilerOptions {
        strict_null_checks: Tristate::True,
        strict_property_initialization: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "class C {\n  x: string;\n}",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 2564);
    assert_eq!(
        diags[0].message,
        "Property 'x' has no initializer and is not definitely assigned in the constructor."
    );
}

// Go: internal/checker/checker.go:Checker.checkMemberForOverrideModifier (valid override -> ok)
#[test]
fn valid_override_modifier_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B {\n  x = 1;\n}\nclass D extends B {\n  override x = 2;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert!(
        c.get_diagnostics(root).is_empty(),
        "valid override must not report override-modifier errors"
    );
}

// ---- T1-E batch 58: mixin constructors, property-kind overrides, override spelling ----

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4314)
#[test]
fn mixin_class_without_rest_constructor_reports_2545() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Ctor = new (...args: any[]) => object;\n\
         function f<T extends Ctor>(B: T) {\n\
           class M extends B {}\n\
           return M;\n\
         }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2545),
        "expected mixin 2545; got {diags:?}"
    );
    let d = diags.iter().find(|d| d.code == 2545).unwrap();
    assert_eq!(
        d.message,
        "A mixin class must have a constructor with a single rest parameter of type 'any[]'."
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4320)
#[test]
fn mixin_class_extending_abstract_type_variable_without_abstract_reports_2797() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T extends abstract new (...args: any[]) => object>(B: T) {\n\
           class M extends B {\n\
             constructor(...args: any[]) { super(...args); }\n\
           }\n\
           return M;\n\
         }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2797),
        "expected mixin abstract 2797; got {diags:?}"
    );
    let d = diags.iter().find(|d| d.code == 2797).unwrap();
    assert_eq!(
        d.message,
        "A mixin class that extends from a type variable containing an abstract construct signature must also be declared 'abstract'."
    );
}

// Go: internal/checker/checker.go:Checker.checkKindsOfPropertyMemberOverrides(4633)
#[test]
fn derived_method_overriding_base_property_reports_2425() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B { x = 1; }\nclass D extends B { x() {} }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2425)
        .unwrap_or_else(|| panic!("expected 2425; got {diags:?}"));
    assert_eq!(
        d.message,
        "Class 'B' defines instance member property 'x', but extended class 'D' defines it as instance member function."
    );
}

// Go: internal/checker/checker.go:Checker.checkKindsOfPropertyMemberOverrides(4628)
#[test]
fn derived_accessor_overriding_base_method_reports_2423() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B { x() {} }\nclass D extends B { get x() { return 1; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2423)
        .unwrap_or_else(|| panic!("expected 2423; got {diags:?}"));
    assert_eq!(
        d.message,
        "Class 'B' defines instance member function 'x', but extended class 'D' defines it as instance member accessor."
    );
}

// Go: internal/checker/checker.go:Checker.checkKindsOfPropertyMemberOverrides(4631)
#[test]
fn derived_method_overriding_base_accessor_reports_2426() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B { get x() { return 1; } }\nclass D extends B { x() {} }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2426)
        .unwrap_or_else(|| panic!("expected 2426; got {diags:?}"));
    assert_eq!(
        d.message,
        "Class 'B' defines instance member accessor 'x', but extended class 'D' defines it as instance member function."
    );
}

// Go: internal/checker/checker.go:Checker.checkMemberForOverrideModifier(4733)
#[test]
fn override_modifier_misspelling_suggests_base_member_reports_4117() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class A { doSomething() {} }\nclass B extends A { override doSomethang() {} }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 4117);
    assert_eq!(
        diags[0].message,
        "This member cannot have an 'override' modifier because it is not declared in the base class 'A'. Did you mean 'doSomething'?"
    );
}

// Go: internal/checker/checker.go:Checker.checkMemberForOverrideModifier(4745)
#[test]
fn parameter_property_missing_override_reports_4115() {
    let options = CompilerOptions {
        no_implicit_override: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "class B { a: string }\nclass D extends B {\n  constructor(public a: string) { super(); }\n}",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 4115);
    assert_eq!(
        diags[0].message,
        "This parameter property must have an 'override' modifier because it overrides a member in base class 'B'."
    );
}

// ---- T1-E batch 59: base constructor type, extends constraints, 2510 ----

// Go: internal/checker/checker.go:Checker.getBaseConstructorTypeOfClass(16843)
#[test]
fn class_extends_non_constructor_expression_reports_2507() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class D extends 42 {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 2507);
    assert_eq!(
        diags[0].message,
        "Type '42' is not a constructor function type."
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4300)
#[test]
fn class_extends_generic_with_invalid_type_argument_reports_2344() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class Base<T extends string> {}\nclass D extends Base<number> {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2344)
        .unwrap_or_else(|| panic!("expected 2344; got {diags:?}"));
    assert_eq!(
        d.message,
        "Type 'number' does not satisfy the constraint 'string'."
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4331)
#[test]
fn class_extends_intersection_constructors_with_mismatched_returns_reports_2510() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { a: number }\n\
         interface B { b: string }\n\
         type CTor = (new () => A) & (new (x: number) => B);\n\
         declare const Base: CTor;\n\
         class D extends Base {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2510)
        .unwrap_or_else(|| panic!("expected 2510; got {diags:?}"));
    assert_eq!(
        d.message,
        "Base constructors must all have the same return type."
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration(4328)
#[test]
fn class_extends_constructor_with_uniform_returns_reports_no_2510() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { a: number }\n\
         declare const Base: new () => A;\n\
         class D extends Base {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2510),
        "uniform constructor return types must not report 2510; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkMemberForOverrideModifier(4751)
#[test]
fn abstract_member_missing_override_reports_4116() {
    let options = CompilerOptions {
        no_implicit_override: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "abstract class B { abstract m(): void; }\nabstract class D extends B { abstract m(): void; }",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 4116);
    assert_eq!(
        diags[0].message,
        "This member must have an 'override' modifier because it overrides an abstract method that is declared in the base class 'B'."
    );
}

// ---- T1-E batch 60: interface extends checks, index constraints, enum member ----

// Go: internal/checker/checker.go:Checker.checkInterfaceDeclaration(4985)
#[test]
fn interface_incorrectly_extends_interface_reports_2430() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface J { x: number; }\ninterface I extends J { x: string; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2430)
        .unwrap_or_else(|| panic!("expected 2430; got {diags:?}"));
    assert_eq!(
        d.message,
        "Interface 'I' incorrectly extends interface 'J'."
    );
}

// Go: internal/checker/checker.go:Checker.checkInheritedPropertiesAreIdentical(5032)
#[test]
fn interface_cannot_simultaneously_extend_conflicting_bases_reports_2320() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { x: number; }\ninterface B { x: string; }\ninterface I extends A, B {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2320)
        .unwrap_or_else(|| panic!("expected 2320; got {diags:?}"));
    assert_eq!(
        d.message,
        "Interface 'I' cannot simultaneously extend types 'A' and 'B'."
    );
    assert_eq!(d.related_information.len(), 1);
    assert_eq!(d.related_information[0].code, 2319);
    assert_eq!(
        d.related_information[0].message,
        "Named property 'x' of types 'A' and 'B' are not identical."
    );
}

// Go: internal/checker/checker.go:Checker.checkInterfaceDeclaration(4994)
#[test]
fn interface_extends_non_entity_name_expression_reports_2499() {
    let codes = diag_codes("interface I extends (1 as any) {}");
    assert!(
        codes.contains(&2499),
        "non-entity-name extends expression must report TS2499; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.resolveBaseTypesOfInterface(19367)
#[test]
fn interface_extends_primitive_type_reports_2312() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I extends number {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2312)
        .unwrap_or_else(|| panic!("expected 2312; got {diags:?}"));
    assert_eq!(
        d.message,
        "An interface can only extend an object type or intersection of object types with statically known members."
    );
}

// Go: internal/checker/checker.go:Checker.checkInterfaceDeclaration(4987)
#[test]
fn interface_property_not_assignable_to_string_index_reports_2411() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I {\n  [x: string]: string;\n  a: number;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 2411);
    assert_eq!(
        diags[0].message,
        "Property 'a' of type 'number' is not assignable to 'string' index type 'string'."
    );
}

// Go: internal/checker/checker.go:Checker.checkEnumMember(5096)
#[test]
fn enum_member_private_identifier_reports_18024() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "enum E { #x }"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 18024);
    assert_eq!(
        diags[0].message,
        "An enum member cannot be named with a private identifier."
    );
}

// ---- T1-E batch 61: enum member names, initializers, string-valued members ----

// Go: internal/checker/checker.go:Checker.computeEnumMemberValue(23842)
#[test]
fn enum_member_must_have_initializer_reports_1061() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "enum E { A = \"a\", B }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 1061);
    assert_eq!(diags[0].message, "Enum member must have initializer.");
}

// Go: internal/checker/checker.go:Checker.computeEnumMemberValue(23822)
#[test]
fn enum_bigint_literal_name_reports_2452() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "enum E { 1n = 0 }"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic; got {diags:?}");
    assert_eq!(diags[0].code, 2452);
    assert_eq!(
        diags[0].message,
        "An enum member cannot have a numeric name."
    );
}

// Go: internal/checker/checker.go:Checker.computeEnumMemberValues (string-valued members)
#[test]
fn enum_computed_value_with_string_member_reports_2553() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "enum E { A = \"a\", B = 1 + 1 }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2553)
        .unwrap_or_else(|| panic!("expected 2553; got {diags:?}"));
    assert_eq!(
        d.message,
        "Computed values are not permitted in an enum with string valued members."
    );
}

// Go: internal/checker/checker.go:Checker.computeConstantEnumMemberValue(23880)
#[test]
fn enum_non_numeric_initializer_reports_18033() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "enum E { A = true }"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 18033)
        .unwrap_or_else(|| panic!("expected 18033; got {diags:?}"));
    assert_eq!(
        d.message,
        "Type 'boolean' is not assignable to type 'number' as required for computed enum member values."
    );
}

// ---- T1-E batch 62: enum expressions, const enum, isolatedModules ----

// Go: internal/checker/checker.go:Checker.checkEnumMember(5100)
#[test]
fn enum_member_initializer_expression_is_checked_reports_2304() {
    let codes = diag_codes("enum E { A = foo }");
    assert!(
        codes.contains(&2304),
        "enum member initializer must be expression-checked; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.computeConstantEnumMemberValue(23876)
#[test]
fn const_enum_non_constant_initializer_reports_2474() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const enum E { A = E.B, B = 1 }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2474)
        .unwrap_or_else(|| panic!("expected 2474; got {diags:?}"));
    assert_eq!(
        d.message,
        "const enum member initializers must be constant expressions."
    );
}

// Go: internal/checker/checker.go:Checker.computeConstantEnumMemberValue(23878)
#[test]
fn ambient_enum_non_constant_initializer_reports_1066() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare enum E { A = E.B, B = 1 }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 1066)
        .unwrap_or_else(|| panic!("expected 1066; got {diags:?}"));
    assert_eq!(
        d.message,
        "In ambient enum declarations member initializer must be constant expression."
    );
}

// Go: internal/checker/checker.go:Checker.computeEnumMemberValue(23850)
#[test]
fn isolated_modules_enum_following_entity_initializer_reports_18056() {
    let options = CompilerOptions {
        isolated_modules: Tristate::True,
        ..CompilerOptions::default()
    };
    let codes = diag_codes_with_options("const n = 1;\nenum E { A = n, B }", options);
    assert!(
        codes.contains(&18056),
        "expected TS18056 when isolatedModules and a member auto-increments after a non-literal numeric initializer; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkEnumDeclaration(5086)
#[test]
fn merged_enum_duplicate_omit_initializer_reports_2432_via_get_diagnostics() {
    let codes = diag_codes("enum E { A }\nenum E { B }");
    assert!(
        codes.contains(&2432),
        "expected TS2432 for merged enum with two omit-first-initializer declarations; got {codes:?}"
    );
}

// ---- T1-E batch 63: merged const enum, non-finite const values, isolatedModules strings ----

// Go: internal/checker/checker.go:Checker.checkEnumDeclaration(5069)
#[test]
fn merged_enum_const_and_non_const_reports_2473() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const enum E { A }\nenum E { B }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2473)
        .unwrap_or_else(|| panic!("expected 2473; got {diags:?}"));
    assert_eq!(
        d.message,
        "Enum declarations must all be const or non-const."
    );
}

// Go: internal/checker/checker.go:Checker.computeConstantEnumMemberValue(23863)
#[test]
fn const_enum_infinity_initializer_reports_2477() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const enum E { A = Infinity }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2477)
        .unwrap_or_else(|| panic!("expected 2477; got {diags:?}"));
    assert_eq!(
        d.message,
        "'const' enum member initializer was evaluated to a non-finite value."
    );
}

// Go: internal/checker/checker.go:Checker.computeConstantEnumMemberValue(23864)
#[test]
fn const_enum_nan_initializer_reports_2478() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const enum E { A = NaN }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2478)
        .unwrap_or_else(|| panic!("expected 2478; got {diags:?}"));
    assert_eq!(
        d.message,
        "'const' enum member initializer was evaluated to disallowed value 'NaN'."
    );
}

// Go: internal/checker/checker.go:Checker.computeConstantEnumMemberValue(23869)
#[test]
fn isolated_modules_non_syntactic_string_enum_initializer_reports_18055() {
    let options = CompilerOptions {
        isolated_modules: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "enum E { A = \"a\", B = 2 + A }",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 18055)
        .unwrap_or_else(|| panic!("expected 18055; got {diags:?}"));
    assert_eq!(
        d.message,
        "'E.B' has a string type, but must have syntactically recognizable string syntax when 'isolatedModules' is enabled."
    );
}

// ---- T1-E batch 64: namespace/module declaration checks ----

// Go: internal/checker/checker.go:Checker.checkEnumDeclaration(5050)
#[test]
fn erasable_syntax_only_non_ambient_enum_reports_1294() {
    let options = CompilerOptions {
        erasable_syntax_only: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "enum E { A }",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 1294)
        .unwrap_or_else(|| panic!("expected 1294; got {diags:?}"));
    assert_eq!(
        d.message,
        "This syntax is not allowed when 'erasableSyntaxOnly' is enabled."
    );
}

// Go: internal/checker/checker.go:Checker.checkEnumDeclaration(5050)
#[test]
fn erasable_syntax_only_declare_enum_no_1294() {
    let options = CompilerOptions {
        erasable_syntax_only: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "declare enum E { A }",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 1294),
        "ambient enum must not report 1294 under erasableSyntaxOnly; got {diags:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModuleElementContext
#[test]
fn namespace_inside_function_reports_1235() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f() { namespace N { } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 1235)
        .unwrap_or_else(|| panic!("expected 1235; got {diags:?}"));
    assert_eq!(
        d.message,
        "A namespace declaration is only allowed at the top level of a namespace or module."
    );
}

// Go: internal/checker/checker.go:Checker.checkModuleDeclaration(5142)
#[test]
fn isolated_modules_instantiated_namespace_in_script_reports_1280() {
    let options = CompilerOptions {
        isolated_modules: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "namespace N { export const x = 1; }",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 1280)
        .unwrap_or_else(|| panic!("expected 1280; got {diags:?}"));
    assert_eq!(
        d.message,
        "Namespaces are not allowed in global script files when 'isolatedModules' is enabled. If this file is not intended to be a global script, set 'moduleDetection' to 'force' or add an empty 'export {}' statement."
    );
}

// Go: internal/checker/checker.go:Checker.checkModuleDeclaration(5142)
#[test]
fn isolated_modules_namespace_in_external_module_no_1280() {
    let options = CompilerOptions {
        isolated_modules: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "export {};\nnamespace N { export const x = 1; }",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 1280),
        "namespace in external module must not report 1280; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkModuleDeclaration(5153)
#[test]
fn namespace_before_merged_class_reports_2434() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "namespace N { export const x = 1; }\nclass N {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2434)
        .unwrap_or_else(|| panic!("expected 2434; got {diags:?}"));
    assert_eq!(
        d.message,
        "A namespace declaration cannot be located prior to a class or function with which it is merged."
    );
}

// ---- T1-E batch 65: global augmentation + namespace merge (2433/2670) ----

// Go: internal/checker/checker.go:Checker.checkModuleDeclaration(5113)
#[test]
fn global_augmentation_without_declare_in_module_reports_2670() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "export {};\nglobal {\n  interface X {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2670)
        .unwrap_or_else(|| panic!("expected 2670; got {diags:?}"));
    assert_eq!(
        d.message,
        "Augmentations for the global scope should have 'declare' modifier unless they appear in already ambient context."
    );
}

// Go: internal/checker/checker.go:Checker.checkModuleDeclaration(5113)
#[test]
fn declare_global_augmentation_in_module_no_2670() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "export {};\ndeclare global {\n  interface X {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| d.code == 2670),
        "declare global must not report 2670; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkModuleDeclaration(5177)
#[test]
fn global_augmentation_in_script_reports_2669() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "global {\n  interface X {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2669)
        .unwrap_or_else(|| panic!("expected 2669; got {diags:?}"));
    assert_eq!(
        d.message,
        "Augmentations for the global scope can only be directly nested in external modules or ambient module declarations."
    );
}

#[test]
fn multi_file_view_delegates_source_files() {
    let p = MultiFileProgram::build(&[
        ("/class.ts", "class D {}"),
        ("/module.ts", "namespace D { export const y = 1; }"),
    ]);
    let view = p.file_view(p.source_files()[1]).unwrap();
    assert_eq!(view.source_files().len(), 2);
}

// Go: internal/checker/checker.go:Checker.checkModuleDeclaration(5151)
#[test]
fn namespace_different_file_from_merged_class_reports_2433() {
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/class.ts", "class D {}"),
        ("/module.ts", "namespace D { export const y = 1; }"),
    ]));
    let file_module = p.source_files()[1];
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(file_module);
    let d = diags
        .iter()
        .find(|d| d.code == 2433)
        .unwrap_or_else(|| panic!("expected 2433; got {diags:?}"));
    assert_eq!(
        d.message,
        "A namespace declaration cannot be in a different file from a class or function with which it is merged."
    );
}

// Go: internal/checker/checker.go:Checker.checkModuleDeclaration(5188)
#[test]
fn ambient_module_nested_in_namespace_reports_2435() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "namespace N { declare module \"m\" {} }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2435)
        .unwrap_or_else(|| panic!("expected 2435; got {diags:?}"));
    assert_eq!(
        d.message,
        "Ambient modules cannot be nested in other modules or namespaces."
    );
}

// ---- T1-E batch 66: constructor super() SPI flow (2376/2401/17005) ----

fn legacy_class_field_options() -> CompilerOptions {
    CompilerOptions {
        use_define_for_class_fields: Tristate::False,
        ..CompilerOptions::default()
    }
}

// Go: internal/checker/checker.go:Checker.checkConstructorDeclaration(2843)
#[test]
fn super_call_in_block_with_parameter_property_reports_2401() {
    let codes = diag_codes_with_options(
        "class B {}\nclass D extends B {\n  constructor(public y: string) {\n    if (true) { super(); }\n  }\n}",
        legacy_class_field_options(),
    );
    assert!(
        codes.contains(&2401),
        "expected TS2401 for nested super() with parameter property; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkConstructorDeclaration(2861)
#[test]
fn super_after_this_with_initialized_property_reports_2376() {
    let codes = diag_codes_with_options(
        "class B {}\nclass D extends B {\n  a = 1;\n  constructor() {\n    this.a = 3;\n    super();\n  }\n}",
        legacy_class_field_options(),
    );
    assert!(
        codes.contains(&2376),
        "expected TS2376 when this precedes super with initialized property; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkConstructorDeclaration(2831)
#[test]
fn super_call_when_extends_null_reports_17005() {
    let codes = diag_codes("class C extends null { constructor() { super(); } }");
    assert!(
        codes.contains(&17005),
        "expected TS17005 for super() when class extends null; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkConstructorDeclaration(2866)
#[test]
fn derived_extends_null_without_super_no_2377() {
    let codes = diag_codes("class C extends null { constructor() {} }");
    assert!(
        !codes.contains(&2377),
        "extends null without super must not report TS2377; got {codes:?}"
    );
}

// ---- T1-E batch 67: constructor SPI private identifiers + new 2351 ----

// Go: internal/checker/checker.go:Checker.checkConstructorDeclaration(2843)
#[test]
fn super_call_in_block_with_private_field_reports_2401() {
    let codes = diag_codes_with_options(
        "class B {}\nclass D extends B {\n  #x = 1;\n  constructor() {\n    if (true) { super(); }\n  }\n}",
        legacy_class_field_options(),
    );
    assert!(
        codes.contains(&2401),
        "expected TS2401 for nested super() with private field; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkConstructorDeclaration(2861)
#[test]
fn super_after_this_with_private_field_reports_2376() {
    let codes = diag_codes_with_options(
        "class B {}\nclass D extends B {\n  #x = 1;\n  constructor() {\n    this;\n    super();\n  }\n}",
        legacy_class_field_options(),
    );
    assert!(
        codes.contains(&2376),
        "expected TS2376 when this precedes super with private field; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkConstructorDeclaration(2838)
#[test]
fn nested_super_allowed_with_emit_standard_class_fields() {
    let codes = diag_codes(
        "class B {}\nclass D extends B {\n  constructor(public y: string) {\n    if (true) { super(); }\n  }\n}",
    );
    assert!(
        !codes.contains(&2401),
        "emit-standard class fields must not require root-level super(); got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.resolveNewExpression(8611)
#[test]
fn new_numeric_literal_reports_2351() {
    let codes = diag_codes("new 1();");
    assert!(
        codes.contains(&2351),
        "expected TS2351 for `new` on a number literal; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.resolveNewExpression(8611)
#[test]
fn new_non_class_identifier_reports_2351() {
    let codes = diag_codes("const n = 1;\nnew n();");
    assert!(
        codes.contains(&2351),
        "expected TS2351 for `new` on a non-class value; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.resolveNewExpression(8571)
#[test]
fn new_valid_class_no_2351() {
    let codes = diag_codes("class C {}\nnew C();");
    assert!(
        !codes.contains(&2351),
        "constructing a concrete class must not report TS2351; got {codes:?}"
    );
}

// ---- T1-E batch 68: new-expression constructor accessibility (2673/2674) ----

// Go: internal/checker/checker.go:Checker.isConstructorAccessible(8615)
#[test]
fn new_private_constructor_outside_class_reports_2673() {
    let codes = diag_codes("class C { private constructor() {} }\nnew C();");
    assert!(
        codes.contains(&2673),
        "expected TS2673 for `new` on a private constructor outside the class; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.isConstructorAccessible(8615)
#[test]
fn new_protected_constructor_outside_class_reports_2674() {
    let codes = diag_codes("class C { protected constructor() {} }\nnew C();");
    assert!(
        codes.contains(&2674),
        "expected TS2674 for `new` on a protected constructor outside the class; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.isConstructorAccessible(8615)
#[test]
fn new_private_constructor_inside_class_no_2673() {
    let codes = diag_codes(
        "class C {\n  private constructor() {}\n  static create() { return new C(); }\n}",
    );
    assert!(
        !codes.contains(&2673),
        "private constructor must be constructible inside the declaring class; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.isConstructorAccessible(8628)
#[test]
fn new_protected_base_constructor_in_subclass_no_2674() {
    let codes = diag_codes(
        "class B { protected constructor() {} }\nclass D extends B {\n  m() { new B(); }\n}",
    );
    assert!(
        !codes.contains(&2674),
        "protected base constructor must be accessible from a subclass; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.isConstructorAccessible(8639)
#[test]
fn new_protected_constructor_in_unrelated_function_reports_2674() {
    let codes = diag_codes("class C { protected constructor() {} }\nfunction f() { new C(); }");
    assert!(
        codes.contains(&2674),
        "expected TS2674 for protected constructor outside class hierarchy; got {codes:?}"
    );
}

// ---- T1-E batch 69: new-expression construct signature resolution ----

// Go: internal/checker/checker.go:Checker.resolveNewExpression -> resolveCall (2345)
#[test]
fn new_wrong_constructor_argument_reports_2345() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { constructor(x: number) {} }\nnew C(\"s\");",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2345, got {diags:?}");
    assert_eq!(diags[0].code, 2345);
    assert_eq!(
        diags[0].message,
        "Argument of type 'string' is not assignable to parameter of type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.resolveNewExpression -> resolveCall (well-typed)
#[test]
fn new_matching_constructor_argument_no_diagnostic() {
    let codes = diag_codes("class C { constructor(x: number) {} }\nnew C(1);");
    assert!(
        !codes.contains(&2345),
        "matching constructor argument must not report TS2345; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.resolveNewExpression -> resolveCall (2554)
#[test]
fn new_wrong_constructor_arity_reports_2554() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { constructor(x: number) {} }\nnew C();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2554, got {diags:?}");
    assert_eq!(diags[0].code, 2554);
    assert_eq!(diags[0].message, "Expected 1 arguments, but got 0.");
}

// Go: internal/checker/checker.go:Checker.resolveNewExpression -> chooseOverload (2769)
#[test]
fn new_overloaded_constructor_no_match_reports_2769() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  constructor(x: number);\n  constructor(x: string);\n  constructor(x: any) {}\n}\nnew C(true);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2769, got {diags:?}");
    assert_eq!(diags[0].code, 2769);
    assert_eq!(diags[0].message, "No overload matches this call.");
}

// Go: internal/checker/checker.go:Checker.resolveNewExpression -> chooseOverload (ok)
#[test]
fn new_overloaded_constructor_matching_overload_no_diagnostic() {
    let codes = diag_codes(
        "class C {\n  constructor(x: number);\n  constructor(x: string);\n  constructor(x: any) {}\n}\nnew C(\"s\");",
    );
    assert!(
        codes.is_empty(),
        "matching overloaded constructor must not report diagnostics; got {codes:?}"
    );
}

// ---- T1-E batch 70: tagged template expressions ----

// Go: internal/checker/checker.go:Checker.checkTaggedTemplateExpression(9994)
#[test]
fn tagged_template_result_type_is_signature_return_type() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "function tag(l: any): string { return \"\"; }\ntag`hello`;",
    );
    let call = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let string = c.string_type();
    assert_eq!(c.check_expression(&p, call), string);
}

// Go: internal/checker/checker.go:Checker.resolveTaggedTemplateExpression -> resolveCall (well-typed)
#[test]
fn tagged_template_matching_substitution_no_diagnostic() {
    let codes = diag_codes("function tag(l: any, x: number): void {}\ntag`a${1}b`;");
    assert!(
        codes.is_empty(),
        "matching tagged-template substitution must not report diagnostics; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.resolveTaggedTemplateExpression -> resolveCall (2345)
#[test]
fn tagged_template_substitution_not_assignable_reports_2345() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function tag(l: any, x: number): void {}\ntag`a${\"s\"}b`;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2345, got {diags:?}");
    assert_eq!(diags[0].code, 2345);
    assert_eq!(
        diags[0].message,
        "Argument of type 'string' is not assignable to parameter of type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.resolveTaggedTemplateExpression -> resolveCall (2554)
#[test]
fn tagged_template_wrong_arity_reports_2554() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function tag(): void {}\ntag`hello`;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2554, got {diags:?}");
    assert_eq!(diags[0].code, 2554);
    assert_eq!(diags[0].message, "Expected 0 arguments, but got 1.");
}

// Go: internal/checker/checker.go:Checker.resolveTaggedTemplateExpression -> invocationError (2349)
#[test]
fn tagged_template_non_callable_tag_reports_2349() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x = 1;\nx`hello`;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    assert_eq!(diags[0].message, "This expression is not callable.");
}

// Go: internal/checker/checker.go:Checker.resolveTaggedTemplateExpression -> resolveCall (2769)
#[test]
fn tagged_template_overloaded_no_match_reports_2769() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function tag(l: any, x: number): void;\nfunction tag(l: any, x: string): void;\nfunction tag(l: any, x: any): void {}\ntag`a${true}b`;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2769, got {diags:?}");
    assert_eq!(diags[0].code, 2769);
    assert_eq!(diags[0].message, "No overload matches this call.");
}

// ---- T1-E batch 71: call-expression invocation errors (2349/2348) ----

// Go: internal/checker/checker.go:Checker.resolveCallExpression -> invocationError (2349)
#[test]
fn call_non_callable_value_reports_2349() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "const x = 1;\nx();"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    assert_eq!(diags[0].message, "This expression is not callable.");
}

// Go: internal/checker/checker.go:Checker.resolveCallExpression (2348)
#[test]
fn call_class_value_without_new_reports_2348() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "class C {}\nC();"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2348, got {diags:?}");
    assert_eq!(diags[0].code, 2348);
    assert_eq!(
        diags[0].message,
        "Value of type 'typeof C' is not callable. Did you mean to include 'new'?"
    );
}

// Go: internal/checker/checker.go:Checker.resolveCallExpression -> invocationError (well-typed call)
#[test]
fn call_on_function_value_no_invocation_error() {
    let codes = diag_codes("function f(): void {}\nf();");
    assert!(
        !codes.contains(&2349) && !codes.contains(&2348),
        "a callable function must not report invocation errors; got {codes:?}"
    );
}

// ---- T1-E batch 72: element access 7053 + union invocation ----

// Go: internal/checker/checker.go:Checker.getPropertyTypeForIndexType (7053/7054 chain)
#[test]
fn element_access_string_index_no_signature_reports_7053() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O {\n  a: number;\n}\ndeclare const o: O;\ndeclare const k: string;\no[k];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 7053, got {diags:?}");
    assert_eq!(diags[0].code, 7053);
    assert_eq!(
        diags[0].message,
        "Element implicitly has an 'any' type because expression of type 'string' can't be used to index type 'O'."
    );
    assert_eq!(diags[0].message_chain.len(), 1);
    assert_eq!(diags[0].message_chain[0].code, 7054);
    assert_eq!(
        diags[0].message_chain[0].message,
        "No index signature with a parameter of type 'string' was found on type 'O'."
    );
}

// Go: internal/checker/checker.go:Checker.invocationErrorDetails (2755 + 2349)
#[test]
fn call_union_no_callable_constituent_reports_2349_with_2755_chain() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: number | string;\nx();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    assert_eq!(diags[0].message, "This expression is not callable.");
    assert_eq!(diags[0].message_chain.len(), 1);
    assert_eq!(diags[0].message_chain[0].code, 2755);
    assert_eq!(
        diags[0].message_chain[0].message,
        "No constituent of type 'string | number' is callable."
    );
}

// Go: internal/checker/checker.go:Checker.invocationErrorDetails (2756/2757 + 2349)
#[test]
fn call_union_mixed_callable_constituent_reports_2349_with_2756_chain() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const f: (() => void) | number;\nf();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    assert_eq!(diags[0].message, "This expression is not callable.");
    assert_eq!(diags[0].message_chain.len(), 1);
    assert_eq!(diags[0].message_chain[0].code, 2756);
    assert_eq!(
        diags[0].message_chain[0].message,
        "Not all constituents of type 'number | () => void' are callable."
    );
    assert_eq!(diags[0].message_chain[0].next.len(), 1);
    assert_eq!(diags[0].message_chain[0].next[0].code, 2757);
    assert_eq!(
        diags[0].message_chain[0].next[0].message,
        "Type 'number' has no call signatures."
    );
}

// ---- T1-E batch 73: literal element-access 2339 + union 2758 ----

// Go: internal/checker/checker.go:Checker.getPropertyTypeForIndexType (2339 literal)
#[test]
fn element_access_string_literal_missing_property_reports_2339() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O {\n  a: number;\n}\ndeclare const o: O;\no[\"missing\"];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2339, got {diags:?}");
    assert_eq!(diags[0].code, 2339);
    assert_eq!(
        diags[0].message,
        "Property 'missing' does not exist on type 'O'."
    );
}

// Go: internal/checker/checker.go:Checker.getPropertyTypeForIndexType (2339 literal)
#[test]
fn element_access_number_literal_missing_property_reports_2339() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O {\n  a: number;\n}\ndeclare const o: O;\no[0];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2339, got {diags:?}");
    assert_eq!(diags[0].code, 2339);
    assert_eq!(diags[0].message, "Property '0' does not exist on type 'O'.");
}

// Go: internal/checker/checker.go:Checker.invocationErrorDetails (2758 + 2349)
#[test]
fn call_union_incompatible_signatures_reports_2349_with_2758_chain() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const f: ((x: number) => void) | ((x: string) => void);\nf();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    assert_eq!(diags[0].message, "This expression is not callable.");
    assert_eq!(diags[0].message_chain.len(), 1);
    assert_eq!(diags[0].message_chain[0].code, 2758);
    assert_eq!(
        diags[0].message_chain[0].message,
        "Each member of the union type '(x: number) => void | (x: string) => void' has signatures, but none of those signatures are compatible with each other."
    );
}

// ---- T1-E batch 74: property suggestions (2551) + construct 2350 ----

// Go: internal/checker/checker.go:Checker.reportNonexistentProperty (2551)
#[test]
fn property_access_misspelling_reports_2551_with_suggestion() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O {\n  prop1: number;\n}\ndeclare const o: O;\no.prop;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2551, got {diags:?}");
    assert_eq!(diags[0].code, 2551);
    assert_eq!(
        diags[0].message,
        "Property 'prop' does not exist on type 'O'. Did you mean 'prop1'?"
    );
    assert_eq!(diags[0].related_information.len(), 1);
    assert_eq!(diags[0].related_information[0].code, 2728);
    assert_eq!(
        diags[0].related_information[0].message,
        "'prop1' is declared here."
    );
}

// Go: internal/checker/checker.go:Checker.getPropertyTypeForIndexType (2551 literal)
#[test]
fn element_access_string_literal_misspelling_reports_2551() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O {\n  prop1: number;\n}\ndeclare const o: O;\no[\"prop\"];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2551, got {diags:?}");
    assert_eq!(diags[0].code, 2551);
    assert_eq!(
        diags[0].message,
        "Property 'prop' does not exist on type 'O'. Did you mean 'prop1'?"
    );
}

// Go: internal/checker/checker.go:Checker.reportNonexistentProperty (2339 when no suggestion)
#[test]
fn property_access_no_close_suggestion_still_reports_2339() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O {\n  prop1: number;\n}\ndeclare const o: O;\no.zzzzz;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2339, got {diags:?}");
    assert_eq!(diags[0].code, 2339);
    assert_eq!(
        diags[0].message,
        "Property 'zzzzz' does not exist on type 'O'."
    );
}

// Go: internal/checker/checker.go:Checker.resolveNewExpression (2350, !noImplicitAny)
#[test]
fn new_non_void_function_reports_2350_when_no_implicit_any_disabled() {
    let options = CompilerOptions {
        no_implicit_any: Tristate::False,
        ..CompilerOptions::default()
    };
    let codes = diag_codes_with_options(
        "function fnNumber(): number { return 32; }\nnew fnNumber();",
        options,
    );
    assert!(
        codes.contains(&2350),
        "expected TS2350 for `new` on a non-void function when noImplicitAny is off; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.resolveNewExpression (2350)
#[test]
fn new_non_void_function_reports_2350_message() {
    let options = CompilerOptions {
        no_implicit_any: Tristate::False,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "function fnNumber(): number { return 32; }\nnew fnNumber();",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| {
            d.code == 2350
                && d.message == "Only a void function can be called with the 'new' keyword."
        }),
        "expected TS2350 with canonical message; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.resolveNewExpression (no 2350 for void return)
#[test]
fn new_void_function_no_2350_when_no_implicit_any_disabled() {
    let options = CompilerOptions {
        no_implicit_any: Tristate::False,
        ..CompilerOptions::default()
    };
    let codes = diag_codes_with_options("function fnVoid(): void {}\nnew fnVoid();", options);
    assert!(
        !codes.contains(&2350),
        "void-returning function must not report TS2350; got {codes:?}"
    );
}

// ---- T1-E batch 75: new void-this (2679) + missing construct signature (7009) ----

// Go: internal/checker/checker.go:Checker.resolveNewExpression (2679, !noImplicitAny)
#[test]
fn new_void_this_function_reports_2679_when_no_implicit_any_disabled() {
    let options = CompilerOptions {
        no_implicit_any: Tristate::False,
        ..CompilerOptions::default()
    };
    let codes = diag_codes_with_options(
        "function VoidThis(this: void): void {}\nnew VoidThis();",
        options,
    );
    assert!(
        codes.contains(&2679),
        "expected TS2679 for `new` on a void-`this` function when noImplicitAny is off; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.resolveNewExpression (2679)
#[test]
fn new_void_this_function_reports_2679_message() {
    let options = CompilerOptions {
        no_implicit_any: Tristate::False,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "function VoidThis(this: void): void {}\nnew VoidThis();",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| {
            d.code == 2679
                && d.message
                    == "A function that is called with the 'new' keyword cannot have a 'this' type that is 'void'."
        }),
        "expected TS2679 with canonical message; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkCallExpression (7009, noImplicitAny)
#[test]
fn new_function_without_construct_signature_reports_7009() {
    let codes = diag_codes("function fn() {}\nnew fn();");
    assert!(
        codes.contains(&7009),
        "expected TS7009 for `new` on a function without a construct signature; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkCallExpression (7009)
#[test]
fn new_function_without_construct_signature_reports_7009_message() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function fn() {}\nnew fn();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| {
            d.code == 7009
                && d.message
                    == "'new' expression, whose target lacks a construct signature, implicitly has an 'any' type."
        }),
        "expected TS7009 with canonical message; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkCallExpression (no 7009 when !noImplicitAny)
#[test]
fn new_function_no_7009_when_no_implicit_any_disabled() {
    let options = CompilerOptions {
        no_implicit_any: Tristate::False,
        ..CompilerOptions::default()
    };
    let codes = diag_codes_with_options("function fn() {}\nnew fn();", options);
    assert!(
        !codes.contains(&7009),
        "TS7009 must not report when noImplicitAny is off; got {codes:?}"
    );
}

// ---- T1-E batch 76: union property chains + static member hints (2576) ----

// Go: internal/checker/checker.go:Checker.reportNonexistentProperty (union chain)
#[test]
fn union_property_missing_on_constituent_reports_2339_with_chain() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { a: number }\ninterface C { b: string }\n\
         type U2 = A | C;\ndeclare const u2: U2;\nu2.a;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2339, got {diags:?}");
    assert_eq!(diags[0].code, 2339);
    assert_eq!(
        diags[0].message,
        "Property 'a' does not exist on type 'A | C'."
    );
    assert_eq!(diags[0].message_chain.len(), 1);
    assert_eq!(diags[0].message_chain[0].code, 2339);
    assert_eq!(
        diags[0].message_chain[0].message,
        "Property 'a' does not exist on type 'C'."
    );
}

// Go: internal/checker/checker.go:Checker.reportNonexistentProperty (2576)
#[test]
fn instance_property_access_static_member_reports_2576() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class A {\n  static y = 1;\n}\ndeclare const a: A;\na.y;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2576, got {diags:?}");
    assert_eq!(diags[0].code, 2576);
    assert_eq!(
        diags[0].message,
        "Property 'y' does not exist on type 'A'. Did you mean to access the static member 'A.y' instead?"
    );
}

// Go: internal/checker/checker.go:Checker.reportNonexistentProperty (2576, `this` receiver)
#[test]
fn instance_method_accesses_static_member_on_this_reports_2576() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class List {\n  m() {\n    this.Foo();\n  }\n  static Foo() {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| {
            d.code == 2576
                && d.message
                    == "Property 'Foo' does not exist on type 'List'. Did you mean to access the static member 'List.Foo' instead?"
        }),
        "expected TS2576 for `this.Foo` on instance, got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getPropertyTypeForIndexType (2576 bracket form)
#[test]
fn element_access_static_member_reports_2576_with_bracket_suggestion() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class A {\n  static y = 1;\n}\ndeclare const a: A;\na[\"y\"];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2576, got {diags:?}");
    assert_eq!(diags[0].code, 2576);
    assert_eq!(
        diags[0].message,
        "Property 'y' does not exist on type 'A'. Did you mean to access the static member 'A[\"y\"]' instead?"
    );
}

// ---- T1-E batch 77: await suggestions on property access and comparisons ----

// Go: internal/checker/checker.go:Checker.reportNonexistentProperty (GetPromisedTypeOfPromise)
#[test]
fn property_access_on_promise_missing_property_suggests_await() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         declare const p: Promise<{ x: number }>;\np.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2339, got {diags:?}");
    assert_eq!(diags[0].code, 2339);
    assert_eq!(
        diags[0].message,
        "Property 'x' does not exist on type 'Promise<{ x: number; }>'."
    );
    assert_eq!(diags[0].related_information.len(), 1);
    assert_eq!(diags[0].related_information[0].code, 2773);
    assert_eq!(
        diags[0].related_information[0].message,
        "Did you forget to use 'await'?"
    );
}

// Go: internal/checker/checker.go:Checker.reportNonexistentProperty (GetPromisedTypeOfPromise)
#[test]
fn element_access_on_promise_missing_property_suggests_await() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         declare const p: Promise<{ x: number }>;\np[\"x\"];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2339, got {diags:?}");
    assert_eq!(diags[0].code, 2339);
    assert_eq!(diags[0].related_information.len(), 1);
    assert_eq!(diags[0].related_information[0].code, 2773);
}

// Go: internal/checker/checker.go:Checker.reportOperatorError (relational await)
#[test]
fn relational_comparison_on_promise_operand_suggests_await() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         declare const p: Promise<number>;\np < 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2365, got {diags:?}");
    assert_eq!(diags[0].code, 2365);
    assert_eq!(
        diags[0].message,
        "Operator '<' cannot be applied to types 'Promise<number>' and 'number'."
    );
    assert_eq!(diags[0].related_information.len(), 1);
    assert_eq!(diags[0].related_information[0].code, 2773);
    assert_eq!(
        diags[0].related_information[0].message,
        "Did you forget to use 'await'?"
    );
}

// Go: internal/checker/checker.go:Checker.reportOperatorError (equality await)
#[test]
fn equality_comparison_on_promise_operand_suggests_await() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         declare const p: Promise<number>;\ndeclare const n: number;\np === n;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2367, got {diags:?}");
    assert_eq!(diags[0].code, 2367);
    assert_eq!(
        diags[0].message,
        "This comparison appears to be unintentional because the types 'Promise<number>' and 'number' have no overlap."
    );
    assert_eq!(diags[0].related_information.len(), 1);
    assert_eq!(diags[0].related_information[0].code, 2773);
}

// Go: internal/checker/checker.go:Checker.reportOperatorError (+ await)
#[test]
fn plus_on_two_promise_operands_suggests_await() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         declare const a: Promise<number>;\ndeclare const b: Promise<number>;\na + b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2365, got {diags:?}");
    assert_eq!(diags[0].code, 2365);
    assert_eq!(
        diags[0].message,
        "Operator '+' cannot be applied to types 'Promise<number>' and 'Promise<number>'."
    );
    assert_eq!(diags[0].related_information.len(), 1);
    assert_eq!(diags[0].related_information[0].code, 2773);
}

// ---- T1-E batch 78: invocation await suggestions (2349 + 2773) ----

// Go: internal/checker/checker.go:Checker.invocationErrorDetails (await suggestion)
#[test]
fn call_on_promise_callable_operand_suggests_await() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         declare const p: Promise<() => void>;\np();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    assert_eq!(diags[0].message, "This expression is not callable.");
    assert_eq!(diags[0].related_information.len(), 1);
    assert_eq!(diags[0].related_information[0].code, 2773);
    assert_eq!(
        diags[0].related_information[0].message,
        "Did you forget to use 'await'?"
    );
}

// Go: internal/checker/checker.go:Checker.invocationErrorDetails (no await when awaited type is not callable)
#[test]
fn call_on_promise_non_callable_operand_no_await_suggestion() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         declare const p: Promise<number>;\np();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    assert_eq!(
        diags[0].related_information.len(),
        0,
        "non-callable awaited type must not suggest await, got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.resolveTaggedTemplateExpression -> invocationError (await)
#[test]
fn tagged_template_on_promise_callable_tag_suggests_await() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         declare const p: Promise<(strings: TemplateStringsArray) => void>;\np`x`;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    assert_eq!(diags[0].related_information.len(), 1);
    assert_eq!(diags[0].related_information[0].code, 2773);
}

// Go: internal/checker/checker.go:Checker.invocationErrorDetails (property-access callee)
#[test]
fn call_property_on_promise_callable_suggests_await() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         declare const o: { f: Promise<() => void> };\no.f();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    assert_eq!(diags[0].related_information.len(), 1);
    assert_eq!(diags[0].related_information[0].code, 2773);
}

// ---- T1-E batch 79: getter-as-function hint (6234) + namespace import recovery ----

// Go: internal/checker/checker.go:Checker.invocationErrorDetails (get accessor)
#[test]
fn call_get_accessor_with_empty_parens_reports_6234() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { get prop(): number { return 1; } }\n\
         declare const c: C;\nc.prop();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 6234);
    assert_eq!(
        diags[0].message,
        "This expression is not callable because it is a 'get' accessor. Did you mean to use it without '()'?"
    );
    assert_eq!(diags[0].message_chain.len(), 1);
    assert_eq!(diags[0].message_chain[0].code, 2757);
}

// Go: internal/checker/checker.go:Checker.invocationErrorDetails (non-getter)
#[test]
fn call_non_getter_property_still_reports_2349_head() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const o: { x: number };\no.x();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    assert_eq!(diags[0].message, "This expression is not callable.");
}

// Go: internal/checker/checker.go:Checker.invocationErrorDetails (getter with args)
#[test]
fn call_get_accessor_with_arguments_keeps_2349_head() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { get prop(): number { return 1; } }\n\
         declare const c: C;\nc.prop(1);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    assert_eq!(diags[0].message, "This expression is not callable.");
}

// Go: internal/checker/checker.go:Checker.resolveESModuleSymbol
#[test]
fn namespace_import_resolve_alias_targets_wrapped_clone() {
    use crate::core::declared_types::resolve_alias;
    use crate::core::is_es_module_symbol;
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/foo.ts", "function foo(): void {}\nexport = foo;"),
        ("/index.ts", "import * as ns from \"./foo\";"),
    ]));
    let index = p.source_files()[1];
    let view = p.file_view(index).unwrap();
    let mut c = Checker::new_checker(std::rc::Rc::clone(&p) as std::rc::Rc<dyn BoundProgram>);
    let locals = view.locals(view.root()).expect("module locals");
    let alias = *locals.get("ns").expect("import binding ns");
    let target = resolve_alias(&mut c, view.as_ref(), alias).expect("alias target");
    assert!(
        is_es_module_symbol(target),
        "namespace import of export= callable must resolve to a wrapped clone"
    );
    assert!(
        c.resolved_symbol_flags(view.as_ref(), target)
            .intersects(tsgo_ast::SymbolFlags::VALUE),
        "wrapped clone flags={:?}",
        c.resolved_symbol_flags(view.as_ref(), target)
    );
}

// Go: internal/checker/checker.go:Checker.checkCallExpression
#[test]
fn namespace_import_call_expression_type_carries_export_type_links() {
    use crate::core::is_es_module_symbol;
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/foo.ts", "function foo(): void {}\nexport = foo;"),
        ("/index.ts", "import * as ns from \"./foo\";\nns();"),
    ]));
    let index = p.source_files()[1];
    let view = p.file_view(index).unwrap();
    let mut c = Checker::new_checker(std::rc::Rc::clone(&p) as std::rc::Rc<dyn BoundProgram>);
    let stmts = match view.arena().data(view.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let call_expr = match view.arena().data(stmts[1]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    let callee = match view.arena().data(call_expr) {
        NodeData::CallExpression(d) => d.expression,
        _ => panic!("call expression"),
    };
    let ty = c.check_expression(view.as_ref(), callee);
    let sym = c.get_type(ty).symbol.expect("wrapped type symbol");
    assert!(is_es_module_symbol(sym));
    assert!(
        c.export_type_originating_import(sym).is_some(),
        "wrapped namespace import must record originating import"
    );
}

// Go: internal/checker/checker.go:Checker.invocationErrorRecovery(9965)
#[test]
fn namespace_import_callable_export_reports_7038_related_info() {
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/foo.ts", "function foo(): void {}\nexport = foo;"),
        ("/index.ts", "import * as ns from \"./foo\";\nns();"),
    ]));
    let index = p.source_files()[1];
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(index);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    assert_eq!(diags[0].related_information.len(), 1);
    assert_eq!(diags[0].related_information[0].code, 7038);
    assert_eq!(
        diags[0].related_information[0].message,
        "Type originates at this import. A namespace-style import cannot be called or constructed, and will cause a failure at runtime. Consider using a default import or import require here instead."
    );
}

// ---- T1-E batch 80: assignability chains, excess 2561, invocation recovery, operators ----

// Go: internal/checker/relater.go:Relater.reportError (dotted-name collapse, depth 3)
#[test]
fn assignability_chain_triple_nested_property_collapses_to_dotted_abc() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: { a: { b: { c: string } } };\n\
         const o: { a: { b: { c: number } } } = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    let dotted = &d.message_chain[0];
    assert_eq!(dotted.code, 2200);
    assert_eq!(
        dotted.message,
        "The types of 'a.b.c' are incompatible between these types."
    );
    assert_eq!(dotted.next.len(), 1);
    assert_eq!(dotted.next[0].code, 2322);
    assert_eq!(
        dotted.next[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/relater.go:Relater.hasExcessProperties (2561 suggestion)
#[test]
fn object_literal_excess_property_misspelling_reports_2561() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O { prop1: number; }\nconst o: O = { prop: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2561);
    assert_eq!(
        diags[0].message,
        "Object literal may only specify known properties, but 'prop' does not exist in type 'O'. Did you mean to write 'prop1'?"
    );
}

// Go: internal/checker/relater.go:Relater.hasExcessProperties (2353 when no suggestion)
#[test]
fn object_literal_excess_property_no_suggestion_still_reports_2353() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O { prop1: number; }\nconst o: O = { zzzzz: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2353);
    assert_eq!(
        diags[0].message,
        "Object literal may only specify known properties, and 'zzzzz' does not exist in type 'O'."
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (+ void, 2365)
#[test]
fn plus_void_operand_reports_2365() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\nf() + 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2365, got {diags:?}");
    assert_eq!(diags[0].code, 2365);
    assert_eq!(
        diags[0].message,
        "Operator '+' cannot be applied to types 'void' and 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.invocationErrorDetails (construct, 2761 chain)
#[test]
fn new_non_constructable_reports_2351_with_2761_chain() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: number;\nnew x();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2351, got {diags:?}");
    assert_eq!(diags[0].code, 2351);
    assert_eq!(diags[0].message, "This expression is not constructable.");
    assert_eq!(diags[0].message_chain.len(), 1);
    assert_eq!(diags[0].message_chain[0].code, 2761);
    assert_eq!(
        diags[0].message_chain[0].message,
        "Type 'number' has no construct signatures."
    );
}

// Go: internal/checker/checker.go:Checker.invocationErrorRecovery (construct + export=)
#[test]
fn namespace_import_callable_export_new_skips_7038_without_construct_sigs() {
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/foo.ts", "function foo(): void {}\nexport = foo;"),
        ("/index.ts", "import * as ns from \"./foo\";\nnew ns();"),
    ]));
    let index = p.source_files()[1];
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(index);
    assert_eq!(diags.len(), 1, "expected one 2351, got {diags:?}");
    assert_eq!(diags[0].code, 2351);
    assert!(
        diags[0].related_information.is_empty(),
        "function export= has no construct signatures, so 7038 must not attach: {diags:?}"
    );
}

// ---- T1-E batch 81: union construct chains, void equality, invocation spans, + literals ----

// Go: internal/checker/checker.go:Checker.invocationErrorDetails (construct union, 2759)
#[test]
fn new_union_no_constructable_constituent_reports_2351_with_2759_chain() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: number | string;\nnew x();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2351, got {diags:?}");
    assert_eq!(diags[0].code, 2351);
    assert_eq!(diags[0].message, "This expression is not constructable.");
    assert_eq!(diags[0].message_chain.len(), 1);
    assert_eq!(diags[0].message_chain[0].code, 2759);
    assert_eq!(
        diags[0].message_chain[0].message,
        "No constituent of type 'string | number' is constructable."
    );
}

// Go: internal/checker/checker.go:Checker.invocationErrorDetails (construct union, 2760)
#[test]
fn new_union_mixed_constructable_constituent_reports_2351_with_2760_chain() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const f: (new () => void) | number;\nnew f();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2351, got {diags:?}");
    assert_eq!(diags[0].code, 2351);
    assert_eq!(diags[0].message_chain.len(), 1);
    assert_eq!(diags[0].message_chain[0].code, 2760);
    assert_eq!(
        diags[0].message_chain[0].message,
        "Not all constituents of type 'number | new () => void' are constructable."
    );
    assert_eq!(diags[0].message_chain[0].next.len(), 1);
    assert_eq!(diags[0].message_chain[0].next[0].code, 2761);
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (void equality, 2367)
#[test]
fn void_equality_with_number_reports_2367() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\nf() === 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2367, got {diags:?}");
    assert_eq!(diags[0].code, 2367);
    assert_eq!(
        diags[0].message,
        "This comparison appears to be unintentional because the types 'void' and 'number' have no overlap."
    );
}

// Go: internal/checker/checker.go:Checker.invocationErrorDetails (property-access target)
#[test]
fn invocation_error_on_property_access_targets_name_node() {
    let src = "declare const o: { m: number };\no.m();";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    let m_pos = src.find("m()").expect("m() in source") as i32;
    assert_eq!(
        diags[0].start, m_pos,
        "diagnostic must target the property name, not the whole access; got start={} src={src:?}",
        diags[0].start
    );
}

// Go: internal/checker/checker.go:Checker.invocationErrorDetails (construct union, 2762)
#[test]
fn new_union_incompatible_construct_signatures_reports_2351_with_2762_chain() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const f: (new (x: number) => void) | (new (x: string) => void);\nnew f();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2351, got {diags:?}");
    assert_eq!(diags[0].code, 2351);
    assert_eq!(diags[0].message_chain.len(), 1);
    assert_eq!(diags[0].message_chain[0].code, 2762);
    assert!(
        diags[0].message_chain[0]
            .message
            .contains("has construct signatures, but none of those signatures are compatible"),
        "got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void left, 2362)
#[test]
fn multiply_void_operand_reports_2362_on_left() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\nf() * 2;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2362, got {diags:?}");
    assert_eq!(diags[0].code, 2362);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/relater.go:Relater.hasExcessProperties (2561 on union discriminant)
#[test]
fn object_literal_excess_on_union_discriminant_misspelling_reports_2561() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = { kind: \"a\"; prop1: number } | { kind: \"b\" };\n\
         const t: T = { kind: \"a\", prop: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2561);
    assert_eq!(
        diags[0].message,
        "Object literal may only specify known properties, but 'prop' does not exist in type '{ prop1: number; kind: \"a\"; }'. Did you mean to write 'prop1'?"
    );
}

// ---- T1-E batch 82: MultiFileProgram ES-module symbol routing, operators, relations ----

// Go: internal/checker/checker.go:Checker.invocationErrorRecovery (class export=, 7038)
#[test]
fn namespace_import_class_export_construct_reports_7038() {
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/foo.ts", "class Foo {}\nexport = Foo;"),
        ("/index.ts", "import * as ns from \"./foo\";\nnew ns();"),
    ]));
    let index = p.source_files()[1];
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(index);
    assert_eq!(diags.len(), 1, "expected one 2351, got {diags:?}");
    assert_eq!(diags[0].code, 2351);
    assert_eq!(diags[0].related_information.len(), 1);
    assert_eq!(diags[0].related_information[0].code, 7038);
    assert_eq!(
        diags[0].related_information[0].message,
        "Type originates at this import. A namespace-style import cannot be called or constructed, and will cause a failure at runtime. Consider using a default import or import require here instead."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void right, 2363)
#[test]
fn multiply_void_right_operand_reports_2363() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\n2 * f();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2363, got {diags:?}");
    assert_eq!(diags[0].code, 2363);
    assert_eq!(
        diags[0].message,
        "The right-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/relater.go:Relater.hasExcessProperties (error node on property name)
#[test]
fn object_literal_excess_property_targets_property_name_span() {
    let src = "interface O { prop1: number; }\nconst o: O = { prop: 1 };";
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2561);
    let prop_pos = src.find("prop:").expect("prop: in source") as i32;
    assert_eq!(
        diags[0].start, prop_pos,
        "excess-property diagnostic must start at the property name, not leading trivia; got start={} src={src:?}",
        diags[0].start
    );
}

// ---- T1-E batch 83: void relational, arithmetic operands, ES-module namespace import ----

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (void relational, 2365)
#[test]
fn void_relational_with_number_reports_2365() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\nf() < 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2365, got {diags:?}");
    assert_eq!(diags[0].code, 2365);
    assert_eq!(
        diags[0].message,
        "Operator '<' cannot be applied to types 'void' and 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void right, 2363)
#[test]
fn divide_void_right_operand_reports_2363() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\n2 / f();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2363, got {diags:?}");
    assert_eq!(diags[0].code, 2363);
    assert_eq!(
        diags[0].message,
        "The right-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void left, 2362)
#[test]
fn modulo_void_left_operand_reports_2362() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\nf() % 2;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2362, got {diags:?}");
    assert_eq!(diags[0].code, 2362);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.resolveESModuleSymbol (default export wrap)
#[test]
fn namespace_import_default_export_wraps_and_call_reports_2349() {
    use crate::core::is_es_module_symbol;
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/foo.ts", "export default function foo(): void {}\n"),
        ("/index.ts", "import * as ns from \"./foo\";\nns();"),
    ]));
    let index = p.source_files()[1];
    let view = p.file_view(index).unwrap();
    let mut c = Checker::new_checker(std::rc::Rc::clone(&p) as std::rc::Rc<dyn BoundProgram>);
    let stmts = match view.arena().data(view.root()) {
        NodeData::SourceFile(d) => d.statements.nodes.clone(),
        _ => panic!("source file"),
    };
    let call_expr = match view.arena().data(stmts[1]) {
        NodeData::ExpressionStatement(d) => d.expression,
        _ => panic!("expression statement"),
    };
    let callee = match view.arena().data(call_expr) {
        NodeData::CallExpression(d) => d.expression,
        _ => panic!("call expression"),
    };
    let ty = c.check_expression(view.as_ref(), callee);
    let sym = c.get_type(ty).symbol.expect("wrapped type symbol");
    assert!(is_es_module_symbol(sym));
    assert!(
        c.export_type_originating_import(sym).is_some(),
        "default-export namespace import must record originating import"
    );
    let diags = c.get_diagnostics(index);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    assert_eq!(diags[0].message, "This expression is not callable.");
}

// Go: internal/checker/relater.go:Relater.reportError (array element assignability chain)
#[test]
fn assignability_chain_nested_array_element_collapses_to_dotted_message() {
    let src = format!(
        "{ARRAY_LIB}declare const src: {{ items: {{ value: string }}[] }};\n\
         const o: {{ items: {{ value: number }}[] }} = src;"
    );
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", &src));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    let items = &d.message_chain[0];
    assert_eq!(items.code, 2326);
    assert_eq!(items.message, "Types of property 'items' are incompatible.");
    fn chain_contains_leaf(chain: &DiagnosticMessageChain, code: i32, text: &str) -> bool {
        if chain.code == code && chain.message == text {
            return true;
        }
        chain
            .next
            .iter()
            .any(|n| chain_contains_leaf(n, code, text))
    }
    assert!(
        chain_contains_leaf(
            items,
            2322,
            "Type 'string' is not assignable to type 'number'."
        ),
        "expected nested element mismatch chain, got {items:?}"
    );
}

// Go: internal/checker/relater.go:Relater.hasExcessProperties (intersection target, 2561)
#[test]
fn object_literal_excess_on_intersection_misspelling_reports_2561() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { prop1: number; }\ninterface B { prop2: number; }\n\
         const o: A & B = { prop: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, 2561);
    assert_eq!(
        diags[0].message,
        "Object literal may only specify known properties, but 'prop' does not exist in type 'A & B'. Did you mean to write 'prop1'?"
    );
}

// ---- T1-E batch 84: bitwise/shift void operands, minus void, tuple assignability ----

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void left, 2362)
#[test]
fn bitwise_void_left_operand_reports_2362() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\nf() | 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2362, got {diags:?}");
    assert_eq!(diags[0].code, 2362);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void right, 2363)
#[test]
fn exponent_void_right_operand_reports_2363() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\n2 ** f();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2363, got {diags:?}");
    assert_eq!(diags[0].code, 2363);
    assert_eq!(
        diags[0].message,
        "The right-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void right, 2363)
#[test]
fn minus_void_right_operand_reports_2363() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\n1 - f();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2363, got {diags:?}");
    assert_eq!(diags[0].code, 2363);
    assert_eq!(
        diags[0].message,
        "The right-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void left, 2362)
#[test]
fn shift_void_left_operand_reports_2362() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\nf() >> 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2362, got {diags:?}");
    assert_eq!(diags[0].code, 2362);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/relater.go:Relater.reportError (tuple element assignability chain)
#[test]
fn assignability_chain_tuple_element_mismatch_reports_2326() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: [string, number];\nconst o: [number, number] = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2626);
    assert_eq!(
        d.message_chain[0].message,
        "Type at position 0 in source is not compatible with type at position 0 in target."
    );
    assert_eq!(d.message_chain[0].next.len(), 1);
    assert_eq!(d.message_chain[0].next[0].code, 2322);
    assert_eq!(
        d.message_chain[0].next[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.getTargetOfNamespaceImport (named exports only)
#[test]
fn namespace_import_named_exports_only_resolves_module_symbol() {
    use crate::core::declared_types::resolve_alias;
    use crate::core::is_es_module_symbol;
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/foo.ts", "export const x = 1;\n"),
        ("/index.ts", "import * as ns from \"./foo\";\nns.x;"),
    ]));
    let index = p.source_files()[1];
    let view = p.file_view(index).unwrap();
    let mut c = Checker::new_checker(std::rc::Rc::clone(&p) as std::rc::Rc<dyn BoundProgram>);
    let locals = view.locals(view.root()).expect("module locals");
    let alias = *locals.get("ns").expect("import binding ns");
    let target = resolve_alias(&mut c, view.as_ref(), alias).expect("alias target");
    assert!(
        !is_es_module_symbol(target),
        "namespace import of named-export-only module must resolve to the module symbol, not a wrapped clone"
    );
    let diags = c.get_diagnostics(index);
    assert!(
        diags.is_empty(),
        "ns.x on a named-export module must type-check without diagnostics, got {diags:?}"
    );
}

// ---- T1-E batch 85: tuple arity mismatch, single-element tuple assignability ----

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (tuple arm, 2619)
#[test]
fn assignability_chain_tuple_source_too_long_reports_2619() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: [number, number];\nconst o: [number] = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2619);
    assert_eq!(
        d.message_chain[0].message,
        "Source has 2 element(s) but target allows only 1."
    );
}

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (tuple arm, 2618)
#[test]
fn assignability_chain_tuple_source_too_short_reports_2618() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: [number];\nconst o: [number, number] = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2618);
    assert_eq!(
        d.message_chain[0].message,
        "Source has 1 element(s) but target requires 2."
    );
}

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (tuple arm, no 2626 for arity 1)
#[test]
fn assignability_chain_single_element_tuple_mismatch_skips_2626() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: [string];\nconst o: [number] = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2322);
    assert_eq!(
        d.message_chain[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void left, 2362)
#[test]
fn exponent_void_left_operand_reports_2362() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\nf() ** 2;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2362, got {diags:?}");
    assert_eq!(diags[0].code, 2362);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// ---- T1-E batch 86: void shift/mod/div operands, readonly tuple assignability ----

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void right, 2363)
#[test]
fn shift_void_right_operand_reports_2363() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\n1 << f();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2363, got {diags:?}");
    assert_eq!(diags[0].code, 2363);
    assert_eq!(
        diags[0].message,
        "The right-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void right, 2363)
#[test]
fn modulo_void_right_operand_reports_2363() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\n1 % f();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2363, got {diags:?}");
    assert_eq!(diags[0].code, 2363);
    assert_eq!(
        diags[0].message,
        "The right-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void left, 2362)
#[test]
fn divide_void_left_operand_reports_2362() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\nf() / 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2362, got {diags:?}");
    assert_eq!(diags[0].code, 2362);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void left, 2362)
#[test]
fn unsigned_shift_void_left_operand_reports_2362() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\nf() >>> 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2362, got {diags:?}");
    assert_eq!(diags[0].code, 2362);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/relater.go:Relater.tryElaborateArrayLikeErrors (readonly tuple, 4104)
#[test]
fn assignability_readonly_as_const_tuple_to_mutable_variable_reports_4104() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const o: [number, number] = [3, 4] as const;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 4104, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 4104);
    assert_eq!(
        d.message,
        "The type 'readonly [3, 4]' is 'readonly' and cannot be assigned to the mutable type '[number, number]'."
    );
    assert!(d.message_chain.is_empty());
}

// Go: internal/checker/relater.go:Relater.tryElaborateArrayLikeErrors (readonly tuple call, 4104)
#[test]
fn assignability_readonly_as_const_tuple_to_mutable_param_reports_4104() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function distance([x, y]: [number, number]) { return x + y; }\n\
         const point = [3, 4] as const;\n\
         distance(point);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 4104);
    assert_eq!(
        d.message,
        "The type 'readonly [3, 4]' is 'readonly' and cannot be assigned to the mutable type '[number, number]'."
    );
    assert!(d.message_chain.is_empty());
}

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (tuple arm, 2626)
#[test]
fn assignability_chain_readonly_tuple_second_element_mismatch_reports_2626() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const src = [1, \"x\"] as const;\nconst o: [number, number] = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2626);
    assert_eq!(
        d.message_chain[0].message,
        "Type at position 1 in source is not compatible with type at position 1 in target."
    );
    assert_eq!(d.message_chain[0].next.len(), 1);
    assert_eq!(d.message_chain[0].next[0].code, 2322);
    assert_eq!(
        d.message_chain[0].next[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// ---- T1-E batch 87: readonly array 4104, remaining void operands ----

const BATCH_87_ARRAY_STUBS: &str = "interface Array<T> { [n: number]: T; length: number; }\n\
interface ReadonlyArray<T> { readonly [n: number]: T; readonly length: number; }\n";

// Go: internal/checker/relater.go:Relater.tryElaborateArrayLikeErrors (readonly array, 4104)
#[test]
fn assignability_readonly_array_to_mutable_variable_reports_4104() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{BATCH_87_ARRAY_STUBS}declare const src: readonly number[];\nconst o: number[] = src;"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 4104, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 4104);
    assert_eq!(
        d.message,
        "The type 'ReadonlyArray<number>' is 'readonly' and cannot be assigned to the mutable type 'Array<number>'."
    );
    assert!(d.message_chain.is_empty());
}

// Go: internal/checker/relater.go:Relater.tryElaborateArrayLikeErrors (readonly array call, 4104)
#[test]
fn assignability_readonly_array_to_mutable_param_reports_4104() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{BATCH_87_ARRAY_STUBS}function sum(xs: number[]) {{ return xs.length; }}\n\
             declare const src: readonly number[];\n\
             sum(src);"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 4104);
    assert_eq!(
        d.message,
        "The type 'ReadonlyArray<number>' is 'readonly' and cannot be assigned to the mutable type 'Array<number>'."
    );
    assert!(d.message_chain.is_empty());
}

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (tuple arm, readonly array source)
#[test]
fn assignability_readonly_array_to_mutable_tuple_reports_4104() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{BATCH_87_ARRAY_STUBS}declare const src: readonly number[];\nconst o: [number, number] = src;"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 4104, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 4104);
    assert_eq!(
        d.message,
        "The type 'ReadonlyArray<number>' is 'readonly' and cannot be assigned to the mutable type '[number, number]'."
    );
    assert!(d.message_chain.is_empty());
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void left, 2362)
#[test]
fn minus_void_left_operand_reports_2362() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\nf() - 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2362, got {diags:?}");
    assert_eq!(diags[0].code, 2362);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void right, 2363)
#[test]
fn bitwise_and_void_right_operand_reports_2363() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\n1 & f();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2363, got {diags:?}");
    assert_eq!(diags[0].code, 2363);
    assert_eq!(
        diags[0].message,
        "The right-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void right, 2363)
#[test]
fn unsigned_shift_void_right_operand_reports_2363() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\n1 >>> f();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2363, got {diags:?}");
    assert_eq!(diags[0].code, 2363);
    assert_eq!(
        diags[0].message,
        "The right-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// ---- T1-E batch 88: tuple-array assignability ----

const BATCH_88_ARRAY_STUBS: &str = "interface Array<T> { [n: number]: T; length: number; }\n\
interface ReadonlyArray<T> { readonly [n: number]: T; readonly length: number; }\n";

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (tuple arm, 2620)
#[test]
fn assignability_chain_array_to_fixed_tuple_reports_2620() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{BATCH_88_ARRAY_STUBS}declare const src: number[];\nconst o: [number, number] = src;"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2620);
    assert_eq!(
        d.message_chain[0].message,
        "Target requires 2 element(s) but source may have fewer."
    );
}

// Go: internal/checker/relater.go:Relater.structuredTypeRelatedToWorker (array target, index types)
#[test]
fn assignability_tuple_to_mutable_array_is_allowed() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{BATCH_88_ARRAY_STUBS}declare const src: [number, number];\nconst o: number[] = src;"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "mutable tuple to number[] must assign without diagnostics, got {diags:?}"
    );
}

// Go: internal/checker/relater.go:Relater.structuredTypeRelatedToWorker (array target, index types)
#[test]
fn assignability_chain_tuple_to_array_element_mismatch_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{BATCH_88_ARRAY_STUBS}declare const src: [number, string];\nconst o: number[] = src;"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2322);
    assert_eq!(
        d.message_chain[0].message,
        "Type 'string | number' is not assignable to type 'number'."
    );
}

// Go: internal/checker/relater.go:Relater.tryElaborateArrayLikeErrors (readonly tuple to mutable array)
#[test]
fn assignability_readonly_tuple_to_mutable_array_reports_4104() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!("{BATCH_88_ARRAY_STUBS}const src = [1, 2] as const;\nconst o: number[] = src;"),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 4104, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 4104);
    assert_eq!(
        d.message,
        "The type 'readonly [1, 2]' is 'readonly' and cannot be assigned to the mutable type 'Array<number>'."
    );
    assert!(d.message_chain.is_empty());
}

// ---- T1-E batch 89: empty-tuple 2621, remaining void operands, readonly tuple-array ----

const BATCH_89_ARRAY_STUBS: &str = "interface Array<T> { [n: number]: T; length: number; }\n\
interface ReadonlyArray<T> { readonly [n: number]: T; readonly length: number; }\n";

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (tuple arm, 2621)
#[test]
fn assignability_chain_array_to_empty_tuple_reports_2621() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!("{BATCH_89_ARRAY_STUBS}declare const src: number[];\nconst o: [] = src;"),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2621);
    assert_eq!(
        d.message_chain[0].message,
        "Target allows only 0 element(s) but source may have more."
    );
}

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (tuple arm, 2620)
#[test]
fn assignability_chain_array_to_single_element_tuple_reports_2620() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!("{BATCH_89_ARRAY_STUBS}declare const src: number[];\nconst o: [number] = src;"),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2620);
    assert_eq!(
        d.message_chain[0].message,
        "Target requires 1 element(s) but source may have fewer."
    );
}

// Go: internal/checker/relater.go:Relater.structuredTypeRelatedToWorker (readonly tuple to readonly array)
#[test]
fn assignability_readonly_tuple_to_readonly_array_is_allowed() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{BATCH_89_ARRAY_STUBS}declare const src: readonly [number, number];\n\
             const o: readonly number[] = src;"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "readonly tuple to readonly number[] must assign without diagnostics, got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void left, 2362)
#[test]
fn bitwise_xor_void_left_operand_reports_2362() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\nf() ^ 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2362, got {diags:?}");
    assert_eq!(diags[0].code, 2362);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void right, 2363)
#[test]
fn bitwise_xor_void_right_operand_reports_2363() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\n1 ^ f();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2363, got {diags:?}");
    assert_eq!(diags[0].code, 2363);
    assert_eq!(
        diags[0].message,
        "The right-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void right, 2363)
#[test]
fn bitwise_or_void_right_operand_reports_2363() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\n1 | f();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2363, got {diags:?}");
    assert_eq!(diags[0].code, 2363);
    assert_eq!(
        diags[0].message,
        "The right-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (tuple arm, 2620)
#[test]
fn assignability_chain_readonly_array_to_readonly_tuple_reports_2620() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{BATCH_89_ARRAY_STUBS}declare const src: readonly number[];\n\
             const o: readonly [number, number] = src;"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2620);
    assert_eq!(
        d.message_chain[0].message,
        "Target requires 2 element(s) but source may have fewer."
    );
}

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (rest tuple arm)
#[test]
fn assignability_rest_tuple_accepts_matching_source() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: [string, number, number];\n\
         const o: [string, ...number[]] = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "fixed+rest tuple target must accept a matching source tuple, got {diags:?}"
    );
}

// ---- T1-E batch 90: named tuple grammar, optional-tuple 2623, void operands ----

// Go: internal/checker/checker.go:Checker.checkNamedTupleMember (5086)
#[test]
fn named_tuple_optional_after_type_reports_5086() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = [label: string?];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 5086, got {diags:?}");
    assert_eq!(diags[0].code, 5086);
    assert_eq!(
        diags[0].message,
        "A labeled tuple element is declared as optional with a question mark after the name and before the colon, rather than after the type."
    );
}

// Go: internal/checker/checker.go:Checker.checkNamedTupleMember (5087)
#[test]
fn named_tuple_rest_after_colon_reports_5087() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = [label: ...number[]];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 5087, got {diags:?}");
    assert_eq!(diags[0].code, 5087);
    assert_eq!(
        diags[0].message,
        "A labeled tuple element is declared as rest with a '...' before the name, rather than before the type."
    );
}

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (2623)
#[test]
fn assignability_chain_optional_source_to_required_target_reports_2623() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: [number, string?];\nconst o: [number, string] = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2623);
    assert_eq!(
        d.message_chain[0].message,
        "Source provides no match for required element at position 1 in target."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (void left, 2362)
#[test]
fn bitwise_and_void_left_operand_reports_2362() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\nf() & 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2362, got {diags:?}");
    assert_eq!(diags[0].code, 2362);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (rest tuple span mismatch, 2627)
#[test]
fn assignability_rest_tuple_element_mismatch_reports_2627() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: [string, string, string];\n\
         const o: [string, ...number[]] = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2627);
    assert_eq!(
        d.message_chain[0].message,
        "Type at positions 1 through 2 in source is not compatible with type at position 1 in target."
    );
    assert_eq!(d.message_chain[0].next.len(), 1);
    assert_eq!(d.message_chain[0].next[0].code, 2322);
    assert_eq!(
        d.message_chain[0].next[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// ---- T1-E batch 91: labeled optional 2623, 5085, optional-tuple 2620, void ++ ----

// Go: internal/checker/checker.go:Checker.checkNamedTupleMember (5085)
#[test]
fn named_tuple_both_optional_and_rest_reports_5085() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = [...label?: number[]];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 5085, got {diags:?}");
    assert_eq!(diags[0].code, 5085);
    assert_eq!(
        diags[0].message,
        "A tuple member cannot be both optional and rest."
    );
}

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (2623, labeled tuple)
#[test]
fn assignability_chain_labeled_optional_to_required_reports_2623() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: [a: number, b?: string];\n\
         const o: [a: number, b: string] = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2623);
    assert_eq!(
        d.message_chain[0].message,
        "Source provides no match for required element at position 1 in target."
    );
}

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (tuple arm, 2620, optional target)
#[test]
fn assignability_chain_array_to_optional_tuple_target_reports_2620() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{BATCH_89_ARRAY_STUBS}declare const src: number[];\n\
             const o: [number, number?] = src;"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2620);
    assert_eq!(
        d.message_chain[0].message,
        "Target requires 1 element(s) but source may have fewer."
    );
}

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (2623, optional first element)
#[test]
fn assignability_chain_optional_first_element_reports_2623() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: [number?];\nconst o: [number] = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2623);
    assert_eq!(
        d.message_chain[0].message,
        "Source provides no match for required element at position 0 in target."
    );
}

// Go: internal/checker/checker.go:Checker.checkIdentifier (assignment to enum, 2628)
#[test]
fn assign_to_enum_identifier_reports_2628() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "enum E { A }\nE = 1;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2628, got {diags:?}");
    assert_eq!(diags[0].code, 2628);
    assert_eq!(
        diags[0].message,
        "Cannot assign to 'E' because it is an enum."
    );
}

// Go: internal/checker/checker.go:Checker.checkIdentifier (assignment to class, 2629)
#[test]
fn assign_to_class_identifier_reports_2629() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "class C {}\nC = 1;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2629, got {diags:?}");
    assert_eq!(diags[0].code, 2629);
    assert_eq!(
        diags[0].message,
        "Cannot assign to 'C' because it is a class."
    );
}

// ---- T1-E batch 92: variadic tuple 2624-2627, namespace/function assign 2630+, void postfix 2356 ----

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (2624, variadic target)
#[test]
fn assignability_variadic_target_missing_variadic_source_reports_2624() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type VariadicTarget<T extends unknown[]> = [string, ...T];\n\
         declare const src: [string, ...number[]];\n\
         const o: VariadicTarget<[number]> = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2624);
    assert_eq!(
        d.message_chain[0].message,
        "Source provides no match for variadic element at position 1 in target."
    );
}

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (2625, variadic source)
#[test]
fn assignability_variadic_source_to_fixed_target_reports_2625() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type VariadicSource<T extends unknown[]> = [string, ...T];\n\
         declare const src: VariadicSource<[number]>;\n\
         const o: [string, number] = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2625);
    assert_eq!(
        d.message_chain[0].message,
        "Variadic element at position 1 in source does not match element at position 1 in target."
    );
}

// Go: internal/checker/relater.go:Relater.propertiesRelatedTo (2627, rest tail mismatch)
#[test]
fn assignability_rest_tuple_span_mismatch_reports_2627() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: [number, number, string];\n\
         const o: [number, ...number[]] = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, 2322);
    assert_eq!(d.message_chain.len(), 1);
    assert_eq!(d.message_chain[0].code, 2627);
    assert_eq!(
        d.message_chain[0].message,
        "Type at positions 1 through 2 in source is not compatible with type at position 1 in target."
    );
    assert_eq!(d.message_chain[0].next.len(), 1);
    assert_eq!(d.message_chain[0].next[0].code, 2322);
    assert_eq!(
        d.message_chain[0].next[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkIdentifier (assignment to namespace, 2631)
#[test]
fn assign_to_namespace_identifier_reports_2631() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "namespace M {}\nM = 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2631, got {diags:?}");
    assert_eq!(diags[0].code, 2631);
    assert_eq!(
        diags[0].message,
        "Cannot assign to 'M' because it is a namespace."
    );
}

// Go: internal/checker/checker.go:Checker.checkIdentifier (assignment to function, 2630)
#[test]
fn assign_to_function_identifier_reports_2630() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f() {}\nf = 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2630, got {diags:?}");
    assert_eq!(diags[0].code, 2630);
    assert_eq!(
        diags[0].message,
        "Cannot assign to 'f' because it is a function."
    );
}

// Go: internal/checker/checker.go:Checker.checkPostfixUnaryExpression (2356)
#[test]
fn postfix_increment_on_void_operand_reports_2356() {
    let codes = diag_codes("declare function f(): void;\nlet x = f()++;");
    assert!(
        codes.contains(&2356),
        "expected TS2356 on void++; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkPrefixUnaryExpression (2356)
#[test]
fn prefix_increment_on_void_operand_reports_2356() {
    let codes = diag_codes("declare function f(): void;\n++f();");
    assert!(
        codes.contains(&2356),
        "expected TS2356 on ++void; got {codes:?}"
    );
}

// ---- T1-E batch 93: import/const assignment, enum increment, void decrement ----

// Go: internal/checker/checker.go:Checker.checkIdentifier (assignment to import, 2632)
#[test]
fn assign_to_named_import_binding_reports_2632() {
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/a.ts", "export let x = 1;"),
        ("/b.ts", "import { x } from \"./a\";\nx = 2;"),
    ]));
    let index = p.source_files()[1];
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(index);
    assert_eq!(diags.len(), 1, "expected one 2632, got {diags:?}");
    assert_eq!(diags[0].code, 2632);
    assert_eq!(
        diags[0].message,
        "Cannot assign to 'x' because it is an import."
    );
}

// Go: internal/checker/checker.go:Checker.checkIdentifier (assignment to const, 2588)
#[test]
fn assign_to_const_identifier_reports_2588() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "const x = 1;\nx = 2;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2588, got {diags:?}");
    assert_eq!(diags[0].code, 2588);
    assert_eq!(
        diags[0].message,
        "Cannot assign to 'x' because it is a constant."
    );
}

// Go: internal/checker/checker.go:Checker.checkIdentifier (compound assignment to enum, 2628)
#[test]
fn prefix_increment_on_enum_identifier_reports_2628() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "enum E { A }\n++E;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2628, got {diags:?}");
    assert_eq!(diags[0].code, 2628);
    assert_eq!(
        diags[0].message,
        "Cannot assign to 'E' because it is an enum."
    );
}

// Go: internal/checker/checker.go:Checker.checkPostfixUnaryExpression (2356)
#[test]
fn postfix_decrement_on_void_operand_reports_2356() {
    let codes = diag_codes("declare function f(): void;\nf()--;");
    assert!(
        codes.contains(&2356),
        "expected TS2356 on void--; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkPrefixUnaryExpression (2356)
#[test]
fn prefix_decrement_on_void_operand_reports_2356() {
    let codes = diag_codes("declare function f(): void;\n--f();");
    assert!(
        codes.contains(&2356),
        "expected TS2356 on --void; got {codes:?}"
    );
}

// ---- T1-E batch 94: readonly property writes, postfix enum, default import assign ----

// Go: internal/checker/checker.go:Checker.isAssignmentToReadonlyEntity (2540)
#[test]
fn assign_to_readonly_property_reports_2540() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O { readonly x: number; }\ndeclare const o: O;\no.x = 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2540, got {diags:?}");
    assert_eq!(diags[0].code, 2540);
    assert_eq!(
        diags[0].message,
        "Cannot assign to 'x' because it is a read-only property."
    );
}

// Go: internal/checker/checker.go:Checker.checkIdentifier (assignment to enum, 2628)
#[test]
fn postfix_increment_on_enum_identifier_reports_2628() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "enum E { A }\nE++;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2628, got {diags:?}");
    assert_eq!(diags[0].code, 2628);
    assert_eq!(
        diags[0].message,
        "Cannot assign to 'E' because it is an enum."
    );
}

// Go: internal/checker/checker.go:Checker.checkIdentifier (assignment to import, 2632)
#[test]
fn assign_to_default_import_binding_reports_2632() {
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/a.ts", "export default 1;"),
        ("/b.ts", "import d from \"./a\";\nd = 2;"),
    ]));
    let index = p.source_files()[1];
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(index);
    assert_eq!(diags.len(), 1, "expected one 2632, got {diags:?}");
    assert_eq!(diags[0].code, 2632);
    assert_eq!(
        diags[0].message,
        "Cannot assign to 'd' because it is an import."
    );
}

// Go: internal/checker/checker.go:Checker.checkIdentifier (assignment to function, 2630)
#[test]
fn compound_assignment_to_function_identifier_reports_2630() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f() {}\nf += 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2630, got {diags:?}");
    assert_eq!(diags[0].code, 2630);
    assert_eq!(
        diags[0].message,
        "Cannot assign to 'f' because it is a function."
    );
}

// Go: internal/checker/checker.go:Checker.isAssignmentToReadonlyEntity (2540, class property)
#[test]
fn assign_to_readonly_class_property_outside_constructor_reports_2540() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { readonly x = 1; }\nconst c = new C();\nc.x = 2;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2540, got {diags:?}");
    assert_eq!(diags[0].code, 2540);
    assert_eq!(
        diags[0].message,
        "Cannot assign to 'x' because it is a read-only property."
    );
}

// ---- T1-E batch 95: constructor readonly exception, index signature 2542, namespace import writes ----

// Go: internal/checker/checker.go:Checker.isAssignmentToReadonlyEntity (constructor exception)
#[test]
fn assign_to_readonly_class_property_in_constructor_is_allowed() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { readonly x = 1; constructor() { this.x = 2; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "constructor may assign readonly property; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.isAssignmentToReadonlyEntity (uninitialized readonly)
#[test]
fn assign_to_uninitialized_readonly_property_in_constructor_is_allowed() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { readonly x: number; constructor() { this.x = 1; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "constructor may assign uninitialized readonly property; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkPropertyAccessExpressionOrQualifiedName (2542)
#[test]
fn assign_to_readonly_index_signature_via_property_access_reports_2542() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O { readonly [x: string]: number; }\ndeclare const o: O;\no.foo = 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2542, got {diags:?}");
    assert_eq!(diags[0].code, 2542);
    assert_eq!(
        diags[0].message,
        "Index signature in type 'O' only permits reading."
    );
}

// Go: internal/checker/checker.go:Checker.checkIndexedAccess (2542)
#[test]
fn assign_to_readonly_index_signature_via_element_access_reports_2542() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O { readonly [x: string]: number; }\ndeclare const o: O;\no[\"foo\"] = 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2542, got {diags:?}");
    assert_eq!(diags[0].code, 2542);
    assert_eq!(
        diags[0].message,
        "Index signature in type 'O' only permits reading."
    );
}

// Go: internal/checker/checker.go:Checker.isAssignmentToReadonlyEntity (namespace import receiver)
#[test]
fn assign_to_namespace_import_property_reports_2540() {
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/a.ts", "export const x = 1;"),
        ("/b.ts", "import * as ns from \"./a\";\nns.x = 2;"),
    ]));
    let index = p.source_files()[1];
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(index);
    assert_eq!(diags.len(), 1, "expected one 2540, got {diags:?}");
    assert_eq!(diags[0].code, 2540);
    assert_eq!(
        diags[0].message,
        "Cannot assign to 'x' because it is a read-only property."
    );
}

// ---- T1-E batch 96: delete readonly, parameter-property ctor, delete optional ----

// Go: internal/checker/checker.go:Checker.checkDeleteExpression (2704)
#[test]
fn delete_readonly_property_reports_2704() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O { readonly x: number; }\ndeclare const o: O;\ndelete o.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2704, got {diags:?}");
    assert_eq!(diags[0].code, 2704);
    assert_eq!(
        diags[0].message,
        "The operand of a 'delete' operator cannot be a read-only property."
    );
}

// Go: internal/checker/checker.go:Checker.errorIfWritingToReadonlyIndex (2542, delete)
#[test]
fn delete_readonly_index_signature_via_property_access_reports_2542() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O { readonly [x: string]: number; }\ndeclare const o: O;\ndelete o.foo;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2542, got {diags:?}");
    assert_eq!(diags[0].code, 2542);
    assert_eq!(
        diags[0].message,
        "Index signature in type 'O' only permits reading."
    );
}

// Go: internal/checker/checker.go:Checker.errorIfWritingToReadonlyIndex (2542, delete)
#[test]
fn delete_readonly_index_signature_via_element_access_reports_2542() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface O { readonly [x: string]: number; }\ndeclare const o: O;\ndelete o[\"foo\"];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2542, got {diags:?}");
    assert_eq!(diags[0].code, 2542);
    assert_eq!(
        diags[0].message,
        "Index signature in type 'O' only permits reading."
    );
}

// Go: internal/checker/checker.go:Checker.isAssignmentToReadonlyEntity (parameter property)
#[test]
fn assign_to_readonly_parameter_property_in_constructor_is_allowed() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { constructor(public readonly x: number) { this.x = 1; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "constructor may assign readonly parameter property; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkDeleteExpression (18011)
#[test]
fn delete_private_identifier_operand_reports_18011() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { #x = 1; m() { delete this.#x; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 18011, got {diags:?}");
    assert_eq!(diags[0].code, 18011);
    assert_eq!(
        diags[0].message,
        "The operand of a 'delete' operator cannot be a private identifier."
    );
}

// Go: internal/checker/checker.go:Checker.checkDeleteExpressionMustBeOptional (2790)
#[test]
fn delete_required_property_with_strict_null_checks_reports_2790() {
    let mut options = CompilerOptions::default();
    options.strict_null_checks = Tristate::True;
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "interface O { x: number; }\ndeclare const o: O;\ndelete o.x;",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2790, got {diags:?}");
    assert_eq!(diags[0].code, 2790);
    assert_eq!(
        diags[0].message,
        "The operand of a 'delete' operator must be optional."
    );
}

// ---- T1-E batch 97: assertion comparability, invalid as const, angle bracket ----

// Go: internal/checker/checker.go:Checker.checkAssertionDeferred (2352)
#[test]
fn type_assertion_incompatible_types_reports_2352() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const n: number;\nconst x = n as string;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2352, got {diags:?}");
    assert_eq!(diags[0].code, 2352);
    assert_eq!(
        diags[0].message,
        "Conversion of type 'number' to type 'string' may be a mistake because neither type sufficiently overlaps with the other. If this was intentional, convert the expression to 'unknown' first."
    );
}

// Go: internal/checker/checker.go:Checker.checkAssertionDeferred (comparable -> ok)
#[test]
fn type_assertion_compatible_types_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const n: number;\nconst x = n as number;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkAssertion (1355)
#[test]
fn const_assertion_on_variable_reports_1355() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const v: number;\nconst x = v as const;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 1355, got {diags:?}");
    assert_eq!(diags[0].code, 1355);
    assert_eq!(
        diags[0].message,
        "A 'const' assertion can only be applied to references to enum members, or string, number, boolean, array, or object literals."
    );
}

// Go: internal/checker/checker.go:Checker.isValidConstAssertionArgument
#[test]
fn const_assertion_on_enum_member_is_allowed() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "enum E { A }\nconst x = E.A as const;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkAssertion (TypeAssertionExpression)
#[test]
fn angle_bracket_type_assertion_takes_asserted_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let x = <string>1;\nconst y: number = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 2322)
        .expect("expected TS2322 from asserted string assigned to number");
    assert_eq!(
        d.message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkAssertionDeferred (2352)
#[test]
fn angle_bracket_type_assertion_incompatible_reports_2352() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const n: number;\nconst x = <string>n;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2352, got {diags:?}");
    assert_eq!(diags[0].code, 2352);
}

// Go: internal/checker/checker.go:Checker.checkIdentifier (compound assignment to namespace, 2631)
#[test]
fn compound_assignment_to_namespace_identifier_reports_2631() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "namespace N {}\nN += 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2631, got {diags:?}");
    assert_eq!(diags[0].code, 2631);
    assert_eq!(
        diags[0].message,
        "Cannot assign to 'N' because it is a namespace."
    );
}

// ---- T1-E batch 98: assignment refs, compound ops, in/private ----

// Go: internal/checker/checker.go:Checker.checkAssignmentOperator (2364)
#[test]
fn assignment_to_literal_reports_2364() {
    let codes = diag_codes("1 = 2;");
    assert!(
        codes.contains(&2364),
        "expected TS2364 on literal assignment target; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkAssignmentOperator (2364)
#[test]
fn logical_and_equals_to_literal_reports_2364() {
    let codes = diag_codes("true &&= false;");
    assert!(
        codes.contains(&2364),
        "expected TS2364 on non-reference &&= target; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkAssignmentOperator (2364)
#[test]
fn compound_assignment_to_literal_reports_2364() {
    let codes = diag_codes("1 += 1;");
    assert!(
        codes.contains(&2364),
        "expected TS2364 on non-reference += target; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkPostfixUnaryExpression (2357)
#[test]
fn postfix_increment_on_non_reference_reports_2357() {
    let codes = diag_codes("1++;");
    assert!(
        codes.contains(&2357),
        "expected TS2357 on postfix ++literal; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkAssignmentOperator (`-=` widened result, 2322)
#[test]
fn minus_equals_widens_literal_type_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "let x: 1;\nx -= 1;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type '1'."
    );
}

// Go: internal/checker/checker.go:Checker.checkArithmeticOperandType (2362)
#[test]
fn bitwise_and_on_string_operands_reports_2362() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: string;\ndeclare const b: string;\na & b;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2362),
        "expected TS2362 on string & string left operand; got {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.code == 2363),
        "expected TS2363 on string & string right operand; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkInExpression (private identifier left)
#[test]
fn in_expression_private_identifier_left_reports_no_left_operand_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  #x: number;\n  m(o: object) {\n    #x in o;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        !diags.iter().any(|d| {
            d.code == 2322
                && d.message.contains("string | number | symbol")
        }),
        "private identifier left operand must not be checked as string|number|symbol; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkInExpression (valid private identifier left)
#[test]
fn in_expression_private_identifier_on_object_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  #x: number;\n  m(o: object) {\n    #x in o;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// ---- T1-E batch 99: for-in, unary symbol, exponentiation, static block, import type ----

// Go: internal/checker/checker.go:Checker.checkForInStatement (2407)
#[test]
fn for_in_rhs_number_reports_2407() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const n: number;\nfor (const x in n) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2407, got {diags:?}");
    assert_eq!(diags[0].code, 2407);
    assert_eq!(
        diags[0].message,
        "The right-hand side of a 'for...in' statement must be of type 'any', an object type or a type parameter, but here has type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkForInStatement (2405)
#[test]
fn for_in_expr_lhs_number_variable_reports_2405() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let x: number;\nfor (x in { a: 1 }) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2405, got {diags:?}");
    assert_eq!(diags[0].code, 2405);
    assert_eq!(
        diags[0].message,
        "The left-hand side of a 'for...in' statement must be of type 'string' or 'any'."
    );
}

// Go: internal/checker/checker.go:Checker.checkForInStatement (valid object RHS)
#[test]
fn for_in_object_rhs_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (const x in { a: 1 }) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkForInStatement (2491)
#[test]
fn for_in_destructuring_declaration_reports_2491() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (const { a } in { a: 1 }) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2491, got {diags:?}");
    assert_eq!(diags[0].code, 2491);
    assert_eq!(
        diags[0].message,
        "The left-hand side of a 'for...in' statement cannot be a destructuring pattern."
    );
}

// Go: internal/checker/checker.go:Checker.checkPrefixUnaryExpression (2469)
#[test]
fn prefix_minus_on_symbol_operand_reports_2469() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: symbol;\n-s;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2469, got {diags:?}");
    assert_eq!(diags[0].code, 2469);
    assert_eq!(
        diags[0].message,
        "The '-' operator cannot be applied to type 'symbol'."
    );
}

// Go: internal/checker/checker.go:Checker.checkPrefixUnaryExpression (2469)
#[test]
fn prefix_tilde_on_symbol_operand_reports_2469() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: symbol;\n~s;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2469, got {diags:?}");
    assert_eq!(diags[0].code, 2469);
    assert_eq!(
        diags[0].message,
        "The '~' operator cannot be applied to type 'symbol'."
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (** bigint, 2791)
#[test]
fn bigint_exponentiation_below_es2016_target_reports_2791() {
    use tsgo_core::compileroptions::ScriptTarget;
    let options = CompilerOptions {
        target: ScriptTarget::Es5,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "1n ** 2n;",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2791, got {diags:?}");
    assert_eq!(diags[0].code, 2791);
    assert_eq!(
        diags[0].message,
        "Exponentiation cannot be performed on 'bigint' values unless the 'target' option is set to 'es2016' or later."
    );
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (** string, 2362)
#[test]
fn exponentiation_string_left_operand_reports_2362() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const s: string;\ns ** 2;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2362, got {diags:?}");
    assert_eq!(diags[0].code, 2362);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type."
    );
}

// Go: internal/checker/checker.go:Checker.checkReturnStatement (18041)
#[test]
fn return_in_class_static_block_reports_18041() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { static { return; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 18041, got {diags:?}");
    assert_eq!(diags[0].code, 18041);
    assert_eq!(
        diags[0].message,
        "A 'return' statement cannot be used inside a class static block."
    );
}

// Go: internal/ast/utilities.go:IsValidTypeOnlyAliasUseSite
#[test]
fn import_type_value_use_reports_1361() {
    let p = std::rc::Rc::new(MultiFileProgram::build(&[
        ("/m.ts", "export type T = number;"),
        ("/a.ts", "import type { T } from \"./m\";\nT;"),
    ]));
    let file_a = p.source_files()[1];
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(file_a);
    assert!(
        diags.iter().any(|d| d.code == 1361),
        "expected TS1361: {diags:?}",
    );
}

// Go: internal/checker/checker.go:Checker.checkPropertyDeclaration (static initializer, 2322)
#[test]
fn static_property_initializer_not_assignable_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { static x: number = \"s\"; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkArrayLiteral (spread non-array, 2495)
#[test]
fn array_literal_spread_non_array_reports_2495() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number };\nconst a = [...x];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2495, got {diags:?}");
    assert_eq!(diags[0].code, 2495);
    assert_eq!(
        diags[0].message,
        "Type '{ a: number; }' is not an array type or a string type."
    );
}

// ---- T1-E batch 100: for-of LHS, await/yield/conditional, null access, super/static, enum/module aug ----

// Go: internal/checker/checker.go:Checker.checkForOfStatement (expression LHS assignability)
#[test]
fn for_of_expression_lhs_not_assignable_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\n\
         declare let x: string;\ndeclare const a: number[];\nfor (x of a) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// Go: internal/checker/checker.go:Checker.checkForOfStatement (optional property access LHS, 2781)
#[test]
fn for_of_optional_property_access_lhs_reports_2781() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const o: { x?: number };\nfor (o?.x of [1]) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2781, got {diags:?}");
    assert_eq!(diags[0].code, 2781);
    assert_eq!(
        diags[0].message,
        "The left-hand side of a 'for...of' statement may not be an optional property access."
    );
}

// Go: internal/checker/checker.go:Checker.checkForOfStatement (valid expression LHS)
#[test]
fn for_of_expression_lhs_assignable_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> {\n  [n: number]: T;\n  length: number;\n}\n\
         declare let x: number;\ndeclare const a: number[];\nfor (x of a) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkAwaitExpression (1320)
#[test]
fn await_invalid_thenable_operand_reports_1320() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "async function f() {\n  await { then(): void {} };\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 1320, got {diags:?}");
    assert_eq!(diags[0].code, 1320);
    assert_eq!(
        diags[0].message,
        "Type of 'await' operand must either be a valid promise or must not contain a callable 'then' member."
    );
}

// Go: internal/checker/checker.go:Checker.checkAwaitExpression (Promise unwrap)
#[test]
fn await_promise_operand_resolves_without_1320() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         async function f(p: Promise<number>) {\n  await p;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 1320),
        "valid Promise await must not report 1320; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkYieldExpression (1163)
#[test]
fn yield_outside_generator_reports_1163() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f() { yield 1; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 1163, got {diags:?}");
    assert_eq!(diags[0].code, 1163);
    assert_eq!(
        diags[0].message,
        "A 'yield' expression is only allowed in a generator body."
    );
}

// Go: internal/checker/checker.go:Checker.checkYieldExpression (annotated return assignability)
#[test]
fn yield_value_not_assignable_to_generator_return_type_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function* g(): number {\n  yield \"s\";\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkConditionalExpression
#[test]
fn conditional_expression_yields_union_of_branch_types() {
    use crate::core::nodebuilder::type_to_string;
    let p = StubProgram::parse_and_bind("/a.ts", "declare const b: boolean;\n(b ? 1 : \"s\");");
    let expr = expr_stmt_expression(&p, 1);
    let mut c = Checker::new();
    let ty = c.check_expression(&p, expr);
    let printed = type_to_string(&mut c, &p, ty);
    assert!(
        printed.contains('|') && printed.contains('1') && printed.contains("\"s\""),
        "expected literal union of branch types, got {printed}"
    );
}

// Go: internal/checker/checker.go:Checker.checkConditionalExpression (assignability via variable decl)
#[test]
fn conditional_expression_branch_not_assignable_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const b: boolean;\nconst x: number = b ? 1 : \"s\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '1 | \"s\"' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkTaggedTemplateExpression (first-arg assignability)
#[test]
fn tagged_template_first_parameter_not_assignable_reports_2345() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function tag(l: never): void {}\ntag`hello`;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2345, got {diags:?}");
    assert_eq!(diags[0].code, 2345);
    assert_eq!(
        diags[0].message,
        "Argument of type 'any' is not assignable to parameter of type 'never'."
    );
}

// Go: internal/checker/relater.go:Relater.hasExcessProperties (call argument object literal)
#[test]
fn call_object_literal_excess_property_reports_2353() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(x: { a: number }): void;\nf({ a: 1, b: 2 });",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2353, got {diags:?}");
    assert_eq!(diags[0].code, 2353);
    assert_eq!(
        diags[0].message,
        "Object literal may only specify known properties, and 'b' does not exist in type '{ a: number; }'."
    );
}

// Go: internal/checker/checker.go:Checker.checkAssignmentOperator (2779)
#[test]
fn assignment_to_optional_property_access_reports_2779() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const o: { x?: number };\no?.x = 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2779, got {diags:?}");
    assert_eq!(diags[0].code, 2779);
    assert_eq!(
        diags[0].message,
        "The left-hand side of an assignment expression may not be an optional property access."
    );
}

// Go: internal/checker/checker.go:Checker.checkIndexedAccess / checkNonNullType (18047)
#[test]
fn element_access_on_possibly_null_reports_18047() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number } | null;\nx[\"a\"];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 18047, got {diags:?}");
    assert_eq!(diags[0].code, 18047);
    assert_eq!(diags[0].message, "'x' is possibly 'null'.");
}

// Go: internal/checker/checker.go:Checker.checkSuperExpression (2337 in static method)
#[test]
fn super_call_in_static_method_reports_2337() {
    let codes = diag_codes("class B {}\nclass D extends B { static m() { super(); } }");
    assert!(
        codes.contains(&2337),
        "expected TS2337 super() in static method; got {codes:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getThisType (2526)
#[test]
fn this_type_in_static_method_reports_2526() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { static m(): this { return this; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2526, got {diags:?}");
    assert_eq!(diags[0].code, 2526);
    assert_eq!(
        diags[0].message,
        "A 'this' type is available only in a non-static member of a class or interface."
    );
}

// Go: internal/checker/checker.go:Checker.computeEnumMemberValues (1164)
#[test]
fn enum_computed_member_name_reports_1164() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "enum E { [\"a\" + \"b\"] = 1 }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 1164, got {diags:?}");
    assert_eq!(diags[0].code, 1164);
    assert_eq!(
        diags[0].message,
        "Computed property names are not allowed in enums."
    );
}

// Go: internal/checker/checker.go:Checker.checkModuleAugmentationElement
#[test]
fn module_augmentation_export_assignment_reports_2666() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "export {};\ndeclare module \"m\" { export = 1; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2666, got {diags:?}");
    assert_eq!(diags[0].code, 2666);
    assert_eq!(
        diags[0].message,
        "Exports and export assignments are not permitted in module augmentations."
    );
}

// ---- T1-E batch 101: async/generator returns, call signatures, mapped/indexed/typequery, destructuring ----

// Go: internal/checker/checker.go:Checker.getReturnTypeFromAnnotation
#[test]
fn async_function_return_type_annotation_node_is_number_keyword() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         async function f(): number { return 1; }",
    );
    let arena = p.arena();
    let fn_decl = match arena.data(p.root()) {
        NodeData::SourceFile(d) => d.statements.nodes[1],
        _ => panic!("function decl"),
    };
    let type_node = get_effective_return_type_node(&p, fn_decl).expect("return type node");
    assert_eq!(
        arena.kind(type_node),
        Kind::NumberKeyword,
        "async return annotation should be NumberKeyword, got {:?}",
        arena.kind(type_node)
    );
}

// Go: internal/checker/checker.go:Checker.checkAsyncFunctionReturnType (1064)
#[test]
fn async_function_non_promise_return_type_annotation_reports_1064() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         async function f(): number { return 1; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 1064, got {diags:?}");
    assert_eq!(diags[0].code, 1064);
    assert_eq!(
        diags[0].message,
        "The return type of an async function or method must be the global Promise<T> type. Did you mean to write 'Promise<number>'?"
    );
}

// Go: internal/checker/checker.go:Checker.checkReturnExpression (async Promise unwrap)
#[test]
fn async_function_return_not_assignable_to_promised_type_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         async function f(): Promise<number> { return \"s\"; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkReturnExpression (async Promise unwrap)
#[test]
fn async_function_return_assignable_to_promised_type_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         async function f(): Promise<number> { return 1; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkReturnExpression (async concise body)
#[test]
fn async_arrow_return_value_assignable_to_promised_return_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Promise<T> { then(onfulfilled: (value: T) => void): void; }\n\
         const f = async (): Promise<number> => 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2322),
        "async arrow concise body must assign to promised return type; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkYieldExpression (valid yield)
#[test]
fn generator_yield_assignable_to_annotated_yield_type_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function* g(): number {\n  yield 1;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.getIterationTypeOfGeneratorFunctionReturnType (yield)
#[test]
fn generator_yield_not_assignable_to_generator_yield_type_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Generator<T, TReturn, TNext> { next(): { value: T }; }\n\
         function* g(): Generator<number, void, unknown> { yield \"s\"; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkCallExpression (interface call signature)
#[test]
fn interface_call_signature_invocation_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I { (): void; }\ndeclare const i: I;\ni();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.resolveCallExpression (interface call return type)
#[test]
fn interface_call_signature_invocation_assigns_return_type() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I { (x: number): string; }\ndeclare const i: I;\nconst s: string = i(1);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.resolveCallExpression (argument assignability)
#[test]
fn interface_call_signature_wrong_argument_reports_2345() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I { (x: number): string; }\ndeclare const i: I;\ni(\"s\");",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2345, got {diags:?}");
    assert_eq!(diags[0].code, 2345);
    assert_eq!(
        diags[0].message,
        "Argument of type 'string' is not assignable to parameter of type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.resolveCallExpression -> invocationError (2349)
#[test]
fn interface_without_call_signature_invocation_reports_2349() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I { x: number; }\ndeclare const i: I;\ni();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2349, got {diags:?}");
    assert_eq!(diags[0].code, 2349);
    assert_eq!(diags[0].message, "This expression is not callable.");
}

// Go: internal/checker/checker.go:Checker.checkMappedType
#[test]
fn mapped_type_invalid_constraint_type_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type M = { [K in boolean]: number };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert!(
        diags[0]
            .message
            .contains("not assignable to type 'string | number | symbol'"),
        "expected mapped constraint 2322, got {:?}",
        diags[0].message
    );
}

// Go: internal/checker/checker.go:Checker.checkIndexedAccessIndexType (2538)
#[test]
fn indexed_access_type_boolean_index_reports_2538() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\n\
         type IB = Array<number>[boolean];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2538, got {diags:?}");
    assert_eq!(diags[0].code, 2538);
    assert!(
        diags[0].message.contains("cannot be used as an index type"),
        "expected 2538 index-type message, got {:?}",
        diags[0].message
    );
}

// Go: internal/checker/checker.go:Checker.getTypeFromTypeQueryNode
#[test]
fn type_query_on_declared_value_resolves_without_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: number;\ntype T = typeof x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.getTypeFromTypeQueryNode / checkExpression
#[test]
fn type_query_on_undeclared_identifier_reports_2304() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = typeof missing;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2304, got {diags:?}");
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'missing'.");
}

// Go: internal/checker/checker.go:Checker.checkImportMetaProperty (supported module)
#[test]
fn import_meta_esnext_module_reports_no_1343() {
    use tsgo_core::compileroptions::ModuleKind;
    let options = CompilerOptions {
        module: ModuleKind::EsNext,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "import.meta;",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 1343),
        "import.meta with module ESNext must not report 1343; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkImportMetaProperty (1343)
#[test]
fn import_meta_with_commonjs_module_reports_1343() {
    use tsgo_core::compileroptions::ModuleKind;
    let options = CompilerOptions {
        module: ModuleKind::CommonJs,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "import.meta;",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 1343, got {diags:?}");
    assert_eq!(diags[0].code, 1343);
    assert_eq!(
        diags[0].message,
        "The 'import.meta' meta-property is only allowed when the '--module' option is 'es2020', 'es2022', 'esnext', 'system', 'node16', 'node18', 'node20', or 'nodenext'."
    );
}

// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration (binding pattern)
#[test]
fn object_destructuring_pattern_initializer_not_assignable_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const { a }: { a: number } = { a: \"s\" };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration (rest in destructuring)
#[test]
fn object_destructuring_rest_excess_property_reports_2353() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const { a, ...rest }: { a: number } = { a: 1, b: 2 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2353, got {diags:?}");
    assert_eq!(diags[0].code, 2353);
    assert_eq!(
        diags[0].message,
        "Object literal may only specify known properties, and 'b' does not exist in type '{ a: number; }'."
    );
}

// Go: internal/checker/checker.go:Checker.checkVariableLikeDeclaration (array binding pattern)
#[test]
fn array_destructuring_pattern_initializer_not_assignable_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const src: [string];\nconst [a]: [number] = src;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message_chain[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration (implements call signature)
#[test]
fn class_missing_interface_call_signature_reports_2420() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I { (): void; }\nclass C implements I { }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2420, got {diags:?}");
    assert_eq!(diags[0].code, 2420);
    assert_eq!(
        diags[0].message,
        "Class 'C' incorrectly implements interface 'I'."
    );
}

// ---- T1-E batch 102: conditional/infer, template literal, union/intersection, type predicate, recursive alias, implements ----

// Go: internal/checker/checker.go:Checker.checkInferType (1338)
#[test]
fn infer_outside_conditional_extends_reports_1338() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "type Bad = infer U;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 1338, got {diags:?}");
    assert_eq!(diags[0].code, 1338);
    assert_eq!(
        diags[0].message,
        "'infer' declarations are only permitted in the 'extends' clause of a conditional type."
    );
}

// Go: internal/checker/checker.go:Checker.checkInferType (valid infer in extends)
#[test]
fn infer_in_conditional_extends_clause_reports_no_1338() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Element<T> = T extends (infer U)[] ? U : never;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 1338),
        "infer in extends must not report 1338; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkConditionalType
#[test]
fn infer_in_conditional_check_type_reports_1338() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Bad = (infer U) extends string ? U : never;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 1338),
        "expected 1338 for infer in check type; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkTemplateLiteralType
#[test]
fn template_literal_type_invalid_span_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = `a${object}b`;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert!(
        diags[0]
            .message
            .contains("Type 'object' is not assignable to type '")
            && diags[0].message.contains("string")
            && diags[0].message.contains("undefined"),
        "expected template constraint 2322, got {:?}",
        diags[0].message
    );
}

// Go: internal/checker/checker.go:Checker.checkTemplateLiteralType
#[test]
fn template_literal_type_valid_span_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = `a${string}b`;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkUnionOrIntersectionType
#[test]
fn union_type_with_missing_member_reports_2304() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type U = number | Missing;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2304, got {diags:?}");
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'Missing'.");
}

// Go: internal/checker/checker.go:Checker.checkTypePredicate (1229)
#[test]
fn type_predicate_referencing_rest_parameter_reports_1229() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(...args: unknown[]): args is unknown[] { return true; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 1229, got {diags:?}");
    assert_eq!(diags[0].code, 1229);
    assert_eq!(
        diags[0].message,
        "A type predicate cannot reference a rest parameter."
    );
}

// Go: internal/checker/checker.go:Checker.checkTypePredicate (2677)
#[test]
fn type_predicate_type_not_assignable_to_parameter_reports_2677() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function g(x: string): x is number { return true; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2677, got {diags:?}");
    assert_eq!(diags[0].code, 2677);
    assert_eq!(
        diags[0].message,
        "A type predicate's type must be assignable to its parameter's type."
    );
    assert_eq!(diags[0].message_chain.len(), 1);
    assert_eq!(
        diags[0].message_chain[0].message,
        "Type 'number' is not assignable to type 'string'."
    );
}

// Go: internal/checker/checker.go:Checker.checkTypePredicate (parameterIndex >= 0)
#[test]
fn type_predicate_type_assignable_to_parameter_reports_no_2677() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function isStr(x: string | number): x is string { return typeof x === \"string\"; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2677),
        "valid predicate type must not report 2677; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkClassLikeDeclaration (implements satisfaction, 2420)
#[test]
fn class_missing_interface_property_reports_2420() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I { x: number; }\nclass C implements I {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2420, got {diags:?}");
    assert_eq!(diags[0].code, 2420);
    assert_eq!(
        diags[0].message,
        "Class 'C' incorrectly implements interface 'I'."
    );
}

// Go: internal/checker/checker.go:Checker.getDeclaredTypeOfTypeAlias (circularity)
#[test]
fn circular_type_alias_resolves_without_crash() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "type A = A;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2589),
        "direct self-reference is broken by cycle guard, not depth limit; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkNewExpression (2511)
#[test]
fn new_expression_on_abstract_class_reports_2511() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare abstract class A {}\nnew A();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2511, got {diags:?}");
    assert_eq!(diags[0].code, 2511);
    assert_eq!(
        diags[0].message,
        "Cannot create an instance of an abstract class."
    );
}

// Go: internal/checker/flow.go:Checker.narrowTypeByDiscriminantProperty
#[test]
fn discriminated_union_narrowing_in_if_branch_assigns_member() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { kind: \"a\"; x: number };\n\
         type B = { kind: \"b\"; y: string };\n\
         declare const v: A | B;\n\
         if (v.kind === \"a\") {\n  const n: number = v.x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.getIndexType (keyof lookup)
#[test]
fn keyof_lookup_type_assignability_e2e() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Keys = keyof { a: number; b: string };\nconst k: Keys = \"a\";\nconst bad: Keys = \"c\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '\"c\"' is not assignable to type '\"a\" | \"b\"'."
    );
}

// ---- T1-E batch 103: narrowing, switch, catch unknown, optional chain, spread, namespace, export, decorator ----

// Go: internal/checker/flow.go:Checker.narrowTypeByInKeyword
#[test]
fn in_guard_narrows_union_member_access_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { a: number };\n\
         type B = { b: string };\n\
         declare const v: A | B;\n\
         if (\"a\" in v) {\n  const n: number = v.a;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality
#[test]
fn literal_equality_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: \"a\" | \"b\";\nif (x === \"a\") {\n  const s: \"a\" = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkSwitchStatement (7029)
#[test]
fn switch_fallthrough_reports_7029() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        no_fallthrough_cases_in_switch: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "switch (1) {\n  case 1: const x = 1;\n  case 2: break;\n}",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 7029, got {diags:?}");
    assert_eq!(diags[0].code, 7029);
    assert_eq!(diags[0].message, "Fallthrough case in switch.");
}

// Go: internal/checker/checker.go:Checker.checkAllCodePathsInNonVoidFunctionReturnOrThrow (2366)
#[test]
fn non_exhaustive_switch_union_function_reports_2366() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(x: \"a\" | \"b\"): number {\n  switch (x) {\n    case \"a\": return 1;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2366),
        "expected TS2366 for non-exhaustive switch; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkNonNullTypeWithReporter (18046)
#[test]
fn catch_variable_unknown_member_access_reports_18046() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        use_unknown_in_catch_variables: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "try {} catch (e) { e.toString(); }",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 18046, got {diags:?}");
    assert_eq!(diags[0].code, 18046);
    assert_eq!(diags[0].message, "'e' is of type 'unknown'.");
}

// Go: internal/checker/checker.go:Checker.checkReferenceExpression (2777)
#[test]
fn increment_on_optional_property_access_reports_2777() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const o: { x?: number };\no?.x++;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2777),
        "expected TS2777 for optional-chain increment; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkNonNullAssertion (unnecessary on non-null)
#[test]
fn non_null_assertion_on_non_nullable_operand_ok() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string;\nconst y: string = x!;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "unnecessary ! on string is OK; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getArgumentArityError (2556)
#[test]
fn spread_non_tuple_to_fixed_parameter_reports_2556() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(a: number): void;\n\
         declare const o: { a: number; b: number };\n\
         f(...o);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2556),
        "expected TS2556 for spread of non-tuple; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.getExportsOfSymbol (function+namespace merge)
#[test]
fn function_namespace_merge_exports_member() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function Foo() {}\nnamespace Foo { export const x = 1; }\nconst a: number = Foo.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "merged Foo.x should be number; got {diags:?}"
    );
}

// Go: internal/checker/grammarchecks.go:Checker.checkGrammarModuleElementContext (1231)
#[test]
fn export_assignment_in_function_reports_1231() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f() { export = 1; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 1231),
        "expected TS1231 for nested export=; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.resolveDecorator (1329)
#[test]
fn decorator_uncalled_zero_arg_reports_1329() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        experimental_decorators: Tristate::True,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "function dec() { return 1; }\n@dec class C {}",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 1329),
        "expected TS1329 for uncalled decorator factory; got {diags:?}"
    );
    let d = diags.iter().find(|d| d.code == 1329).unwrap();
    assert_eq!(
        d.message,
        "'dec' accepts too few arguments to be used as a decorator here. Did you mean to call it first and write '@dec()'?"
    );
}

// Go: internal/checker/checker.go:Checker.checkNamedTupleMember (5086)
#[test]
fn named_tuple_optional_mark_after_name_reports_5086() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = [label: string?];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 5086, got {diags:?}");
    assert_eq!(diags[0].code, 5086);
}

// ---- T1-E batch 104: typeof/truthiness/switch/assignment flow, void, prefix +bigint, intersection excess ----

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeof
#[test]
fn typeof_bigint_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: bigint | string;\nif (typeof x === \"bigint\") {\n  const b: bigint = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeof
#[test]
fn typeof_boolean_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: boolean | number;\nif (typeof x === \"boolean\") {\n  const b: boolean = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (nullable branch)
#[test]
fn null_equality_guard_narrows_else_branch_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | null;\nif (x === null) {\n  const n: null = x;\n} else {\n  const s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (nullable branch)
#[test]
fn undefined_inequality_guard_narrows_else_branch_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | undefined;\nif (x !== undefined) {\n  const s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.getTypeAtSwitchClause
#[test]
fn switch_literal_case_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: \"a\" | \"b\";\nswitch (x) {\n  case \"a\": {\n    const s: \"a\" = x;\n    break;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.getTypeAtFlowAssignment
#[test]
fn assignment_flow_narrows_union_variable_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "let x: string | number;\nx = \"s\";\nconst y: string = x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkConditionalExpression / checkTruthinessOfType (1345)
#[test]
fn void_function_result_in_conditional_reports_1345() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(): void;\nconst x = f() ? 1 : 2;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 1345, got {diags:?}");
    assert_eq!(diags[0].code, 1345);
    assert_eq!(
        diags[0].message,
        "An expression of type 'void' cannot be tested for truthiness."
    );
}

// Go: internal/checker/checker.go:Checker.checkConditionalExpression / checkTruthinessOfType (2872)
#[test]
fn always_truthy_array_literal_in_conditional_reports_2872() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x = [] ? 1 : 2;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2872, got {diags:?}");
    assert_eq!(diags[0].code, 2872);
    assert_eq!(
        diags[0].message,
        "This kind of expression is always truthy."
    );
}

// Go: internal/checker/checker.go:Checker.checkConditionalExpression / checkTruthinessOfType (2873)
#[test]
fn always_falsy_empty_string_in_conditional_reports_2873() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x = \"\" ? 1 : 2;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2873, got {diags:?}");
    assert_eq!(diags[0].code, 2873);
    assert_eq!(diags[0].message, "This kind of expression is always falsy.");
}

// Go: internal/checker/checker.go:Checker.checkVoidExpression
#[test]
fn void_expression_yields_undefined_assignable() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: number;\nconst u: undefined = void x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "void expr should be undefined; got {diags:?}"
    );
}

// Go: internal/checker/checker.go:Checker.checkPrefixUnaryExpression (2736)
#[test]
fn prefix_plus_on_bigint_literal_reports_2736() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "+1n;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2736, got {diags:?}");
    assert_eq!(diags[0].code, 2736);
    assert_eq!(
        diags[0].message,
        "Operator '+' cannot be applied to type 'bigint'."
    );
}

// Go: internal/checker/relater.go:Relater.hasExcessProperties (intersection target)
#[test]
fn call_intersection_parameter_excess_property_reports_2353() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f(x: { a: number } & { b: string }): void;\nf({ a: 1, b: \"s\", c: 2 });",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2353, got {diags:?}");
    assert_eq!(diags[0].code, 2353);
    assert_eq!(
        diags[0].message,
        "Object literal may only specify known properties, and 'c' does not exist in type '{ a: number; } & { b: string; }'."
    );
}

// ---- T1-E batch 105: switch typeof, negated flow, primitive falsy &&, typeof guards, negative literal type ----

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeof
#[test]
fn typeof_number_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: number | string;\nif (typeof x === \"number\") {\n  const n: number = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeof
#[test]
fn typeof_symbol_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: symbol | string;\nif (typeof x === \"symbol\") {\n  const s: symbol = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeof
#[test]
fn typeof_object_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: object | string;\nif (typeof x === \"object\") {\n  const o: object = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeof
#[test]
fn typeof_undefined_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | undefined;\nif (typeof x === \"undefined\") {\n  const u: undefined = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTypeOf
#[test]
fn switch_typeof_string_case_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | number;\nswitch (typeof x) {\n  case \"string\": {\n    const s: string = x;\n    break;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTypeOf
#[test]
fn switch_typeof_default_narrows_complement_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | number | boolean;\nswitch (typeof x) {\n  case \"string\":\n    break;\n  case \"number\":\n    break;\n  default: {\n    const b: boolean = x;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowType (prefix `!`)
#[test]
fn negated_truthiness_guard_narrows_else_branch_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | null | undefined;\nif (!x) {\n} else {\n  const s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (`&&` truthy left)
#[test]
fn logical_and_number_primitive_includes_zero_falsy_part() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: number;\ndeclare const b: number;\na && b;",
    );
    let root = p.root();
    let mut c = Checker::new();
    let and = match p.arena().data(root) {
        NodeData::SourceFile(d) => match p.arena().data(d.statements.nodes[2]) {
            NodeData::ExpressionStatement(d) => d.expression,
            _ => panic!("expression statement"),
        },
        _ => panic!("source file"),
    };
    let t = c.check_expression(&p, and);
    assert_eq!(
        c.type_to_string(t),
        "number | 0",
        "number && number should union the zero falsy part of the right base with the right operand"
    );
}

// Go: internal/checker/checker.go:Checker.getTypeFromLiteralTypeNode
#[test]
fn negative_numeric_literal_type_assignable_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type N = -1;\nconst x: N = -1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (loose `!= null`)
#[test]
fn loose_null_inequality_guard_narrows_else_branch_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | null | undefined;\nif (x != null) {\n  const s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// ---- T1-E batch 106: in 2638, negated in, switch typeof number, loose == null, type predicate flow ----

// Go: internal/checker/checker.go:Checker.checkInExpression (hasEmptyObjectIntersection, 2638)
#[test]
fn in_expression_unknown_narrowed_right_reports_2638() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const k: string;\n\
         declare const y: unknown;\n\
         if (y) {\n  k in y;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2638, got {diags:?}");
    assert_eq!(diags[0].code, 2638);
    assert_eq!(
        diags[0].message,
        "Type '{}' may represent a primitive value, which is not permitted as the right operand of the 'in' operator."
    );
}

// Go: internal/checker/flow.go:Checker.narrowTypeByInKeyword (negated `in`)
#[test]
fn negated_in_guard_narrows_else_branch_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { a: number };\n\
         type B = { b: string };\n\
         declare const v: A | B;\n\
         if (!(\"a\" in v)) {\n  const s: string = v.b;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTypeOf
#[test]
fn switch_typeof_number_case_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: number | string;\n\
         switch (typeof x) {\n  case \"number\": {\n    const n: number = x;\n    break;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (loose `== null`)
#[test]
fn loose_null_equality_guard_narrows_branches_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | null | undefined;\n\
         if (x == null) {\n  const n: null | undefined = x;\n} else {\n  const s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTypePredicate
#[test]
fn type_predicate_call_narrows_union_in_then_branch_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare let x: string | number;\n\
         declare function isString(v: unknown): v is string;\n\
         if (isString(x)) {\n  const s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByDiscriminantProperty
#[test]
fn discriminant_property_switch_case_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { kind: \"a\"; a: number };\n\
         type B = { kind: \"b\"; b: string };\n\
         declare const v: A | B;\n\
         switch (v.kind) {\n  case \"a\": {\n    const n: number = v.a;\n    break;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (`||` nullable left)
#[test]
fn logical_or_nullable_left_yields_union_with_right() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: string | null;\ndeclare const b: number;\na || b;",
    );
    let root = p.root();
    let mut c = Checker::new();
    let or = match p.arena().data(root) {
        NodeData::SourceFile(d) => match p.arena().data(d.statements.nodes[2]) {
            NodeData::ExpressionStatement(d) => d.expression,
            _ => panic!("expression statement"),
        },
        _ => panic!("source file"),
    };
    let t = c.check_expression(&p, or);
    assert_eq!(
        c.type_to_string(t),
        "string | number",
        "nullable string || number should union truthy string with number"
    );
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (strict `=== undefined`)
#[test]
fn strict_undefined_equality_guard_narrows_then_branch_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | undefined;\nif (x === undefined) {\n  const u: undefined = x;\n} else {\n  const s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByInKeyword (`"prop" in ref` reversed operand order)
#[test]
fn in_guard_reversed_operand_order_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { a: number };\n\
         type B = { b: string };\n\
         declare const v: A | B;\n\
         if (v && \"a\" in v) {\n  const n: number = v.a;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.getTypeFromLiteralTypeNode (negative bigint literal type)
#[test]
fn negative_bigint_literal_type_assignable_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type N = -1n;\nconst x: N = -1n;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// ---- T1-E batch 107: boolean literal equality, negated predicate, typeof inequality, ?? union ----

// Go: internal/checker/flow.go:Checker.narrowTypeByTypePredicate (negated call)
#[test]
fn negated_type_predicate_else_branch_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare let x: string | number;\n\
         declare function isString(v: unknown): v is string;\n\
         if (!isString(x)) {\n  const n: number = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeof / narrowTypeByLiteralExpression
#[test]
fn typeof_inequality_guard_narrows_else_branch_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | number;\n\
         if (typeof x !== \"string\") {\n  const n: number = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByBooleanComparison
#[test]
fn boolean_true_equality_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: boolean | string;\n\
         if (x === true) {\n  const b: true = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByBooleanComparison
#[test]
fn boolean_false_inequality_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: boolean | string;\n\
         if (x !== false) {\n  const s: string | true = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeof (discriminant property access)
#[test]
fn typeof_on_discriminant_property_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { kind: \"a\"; a: number };\n\
         type B = { kind: \"b\"; b: string };\n\
         declare const v: A | B;\n\
         if (typeof v.kind === \"string\") {\n  const k: \"a\" | \"b\" = v.kind;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnDiscriminantProperty
#[test]
fn switch_on_discriminant_property_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { kind: \"a\"; a: number };\n\
         type B = { kind: \"b\"; b: string };\n\
         declare const v: A | B;\n\
         switch (v.kind) {\n  case \"a\": {\n    const n: number = v.a;\n    break;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (literal inequality)
#[test]
fn literal_inequality_guard_narrows_else_branch_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: \"a\" | \"b\";\n\
         if (x !== \"a\") {\n  const s: \"b\" = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTypePredicate (parameter index 0)
#[test]
fn type_predicate_else_branch_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare let x: string | number;\n\
         declare function isString(v: unknown): v is string;\n\
         if (isString(x)) {\n} else {\n  const n: number = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (`??` nullable left)
#[test]
fn nullish_coalesce_nullable_left_yields_union_with_right() {
    let p = StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: string | null;\ndeclare const b: number;\na ?? b;",
    );
    let root = p.root();
    let mut c = Checker::new();
    let coalesce = match p.arena().data(root) {
        NodeData::SourceFile(d) => match p.arena().data(d.statements.nodes[2]) {
            NodeData::ExpressionStatement(d) => d.expression,
            _ => panic!("expression statement"),
        },
        _ => panic!("source file"),
    };
    let t = c.check_expression(&p, coalesce);
    assert_eq!(
        c.type_to_string(t),
        "string | number",
        "nullable string ?? number should union the non-null string with number"
    );
}

// ---- T1-E batch 108: comma flow, || subtype reduce, const alias, &&/|| flow ----

// Go: internal/checker/flow.go:Checker.narrowTypeByBinaryExpression (CommaToken)
#[test]
fn comma_operator_condition_narrows_right_operand_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare let x: string | number;\n\
         let a = 0;\n\
         if ((a = 1, typeof x === \"string\")) {\n  const s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkBinaryLikeExpression (`||` subtype reduction)
#[test]
fn logical_or_result_is_subtype_reduced() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: \"a\" | null;\ndeclare const y: string;\nconst n: number = x || y;",
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

// Go: internal/checker/flow.go:Checker.narrowType (const-alias inlining)
#[test]
fn const_alias_typeof_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare let x: string | number;\n\
         const isStr = typeof x === \"string\";\n\
         if (isStr) {\n  const s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByBinaryExpression (AmpersandAmpersandToken)
#[test]
fn logical_and_condition_narrows_both_operands_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare let x: string | number | null;\n\
         declare const ok: boolean;\n\
         const guard = ok && typeof x === \"string\";\n\
         if (guard) {\n  const s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.getDiscriminantPropertyAccess (element access)
#[test]
fn element_access_discriminant_equality_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { kind: \"a\"; a: number };\n\
         type B = { kind: \"b\"; b: string };\n\
         declare const v: A | B;\n\
         if (v[\"kind\"] === \"a\") {\n  const n: number = v.a;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (numeric literal)
#[test]
fn numeric_literal_equality_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: 0 | 1 | 2;\nif (x === 0) {\n  const z: 0 = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// ---- T1-E batch 109: typeof function, instanceof flow, construct RHS ----

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeof ("function")
#[test]
fn typeof_function_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: (() => void) | string;\n\
         if (typeof x === \"function\") {\n  const f: () => void = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByInstanceof
#[test]
fn instanceof_class_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {}\n\
         declare let x: C | string;\n\
         if (x instanceof C) {\n  const c: C = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByInstanceof (negated)
#[test]
fn negated_instanceof_guard_narrows_else_branch_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {}\n\
         declare let x: C | string;\n\
         if (!(x instanceof C)) {\n  const s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeof ("function", negated)
#[test]
fn typeof_function_inequality_guard_narrows_else_branch_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: (() => void) | string;\n\
         if (typeof x !== \"function\") {\n  const s: string = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.resolveInstanceofExpression (class constructor RHS)
#[test]
fn instanceof_class_constructor_right_operand_reports_no_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {}\n\
         declare const o: object;\n\
         o instanceof C;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByInstanceof (derived class)
#[test]
fn instanceof_derived_class_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class A {}\n\
         class B extends A {}\n\
         declare let x: A | string;\n\
         if (x instanceof B) {\n  const b: B = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// ---- T1-E batch 110: switch(true), discriminant chains, equality, relations, contextual ----

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTrue
#[test]
fn switch_true_typeof_case_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare let x: string | number;\n\
         switch (true) {\n  case typeof x === \"string\": {\n    const s: string = x;\n    break;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTrue
#[test]
fn switch_true_literal_equality_case_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: \"a\" | \"b\";\n\
         switch (true) {\n  case x === \"a\": {\n    const s: \"a\" = x;\n    break;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTrue
#[test]
fn switch_true_discriminant_property_case_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { kind: \"a\"; a: number };\n\
         type B = { kind: \"b\"; b: string };\n\
         declare const v: A | B;\n\
         switch (true) {\n  case v.kind === \"a\": {\n    const n: number = v.a;\n    break;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTrue (default complement)
#[test]
fn switch_true_default_case_narrows_complement_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: \"a\" | \"b\";\n\
         switch (true) {\n  case x === \"a\": break;\n  default: {\n    const s: \"b\" = x;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTrue (sequential cases)
#[test]
fn switch_true_second_case_narrows_after_first_fails_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare let x: string | number;\n\
         switch (true) {\n  case typeof x === \"string\": break;\n  case typeof x === \"number\": {\n    const n: number = x;\n    break;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTrue
#[test]
fn switch_true_instanceof_case_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {}\n\
         declare let x: C | string;\n\
         switch (true) {\n  case x instanceof C: {\n    const c: C = x;\n    break;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTrue
#[test]
fn switch_true_in_guard_case_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { a: number };\n\
         type B = { b: string };\n\
         declare const v: A | B;\n\
         switch (true) {\n  case \"a\" in v: {\n    const n: number = v.a;\n    break;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTrue
#[test]
fn switch_true_type_predicate_case_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare let x: string | number;\n\
         declare function isString(v: unknown): v is string;\n\
         switch (true) {\n  case isString(x): {\n    const s: string = x;\n    break;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality
#[test]
fn number_literal_equality_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: 1 | 2;\nif (x === 1) {\n  const n: 1 = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality
#[test]
fn number_literal_inequality_guard_narrows_else_branch_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: 1 | 2;\nif (x !== 1) {\n  const n: 2 = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality
#[test]
fn widened_number_equality_to_literal_narrows_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: number | \"a\";\nif (x === 1) {\n  const n: number = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (enum member)
#[test]
fn enum_member_equality_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "enum E { A, B }\ndeclare const x: E;\nif (x === E.A) {\n  const e: E.A = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByDiscriminantProperty
#[test]
fn chained_if_discriminant_narrows_both_branches_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { kind: \"a\"; a: number };\n\
         type B = { kind: \"b\"; b: string };\n\
         declare const v: A | B;\n\
         if (v.kind === \"a\") {\n  const n: number = v.a;\n} else if (v.kind === \"b\") {\n  const s: string = v.b;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnDiscriminantProperty
#[test]
fn switch_two_discriminant_cases_narrow_both_branches_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { kind: \"a\"; a: number };\n\
         type B = { kind: \"b\"; b: string };\n\
         declare const v: A | B;\n\
         switch (v.kind) {\n  case \"a\": {\n    const n: number = v.a;\n    break;\n  }\n  case \"b\": {\n    const s: string = v.b;\n    break;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByDiscriminantProperty
#[test]
fn nested_discriminant_if_then_switch_narrows_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { kind: \"a\"; tag: 1 | 2; a: number };\n\
         type B = { kind: \"b\"; b: string };\n\
         declare const v: A | B;\n\
         if (v.kind === \"a\") {\n  switch (v.tag) {\n    case 1: {\n      const n: number = v.a;\n      break;\n    }\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByDiscriminantProperty
#[test]
fn property_number_equality_narrows_discriminated_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type A = { tag: 1; a: number };\n\
         type B = { tag: 2; b: string };\n\
         declare const v: A | B;\n\
         if (v.tag === 1) {\n  const n: number = v.a;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByEquality (loose double-equals)
#[test]
fn loose_number_equality_guard_narrows_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: 1 | 2;\nif (x == 1) {\n  const n: 1 = x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/relater.go:structuredTypeRelatedTo (source union, target intersection)
#[test]
fn union_source_not_assignable_to_intersection_target_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { x: number; }\ninterface B { y: string; }\ndeclare const u: A | B;\nconst v: A & B = u;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type 'A | B' is not assignable to type 'A & B'."
    );
}

// Go: internal/checker/relater.go:structuredTypeRelatedTo (source union, target union)
#[test]
fn wider_union_not_assignable_to_narrower_union_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const a: 1 | 2 | 3;\nconst b: 1 | 2 = a;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
    assert_eq!(
        diags[0].message,
        "Type '1 | 2 | 3' is not assignable to type '1 | 2'."
    );
}

// Go: internal/checker/checker.go:Checker.getContextualTypeForElementExpression
#[test]
fn tuple_contextual_array_literal_assignable_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const t: [number, string] = [1, \"a\"];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/relater.go:Relater.typeRelatedToSomeType (wrong member on matched constituent)
#[test]
fn object_literal_union_contextual_wrong_member_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type U = { k: \"a\"; x: number } | { k: \"b\"; y: string };\nconst u: U = { k: \"a\", x: \"oops\" };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
}

// Go: internal/checker/relater.go:Relater.hasExcessProperties (parameter contextual union)
#[test]
fn function_parameter_contextual_union_excess_property_reports_2353() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type U = { k: \"a\"; x: number } | { k: \"b\"; y: string };\n\
         declare function f(u: U): void;\nf({ k: \"a\", x: 1, extra: true });",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2353),
        "expected TS2353 excess property; got {diags:?}"
    );
}

// Go: internal/checker/relater.go:structuredTypeRelatedTo (intersection target, missing member)
#[test]
fn object_missing_intersection_member_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { x: number; }\ninterface B { y: string; }\nconst v: A & B = { x: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
}

// Go: internal/checker/relater.go:Relater.hasExcessProperties (intersection target)
#[test]
fn object_literal_excess_on_intersection_target_reports_2353() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface A { x: number; }\ninterface B { y: string; }\nconst v: A & B = { x: 1, y: \"s\", z: true };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().any(|d| d.code == 2353),
        "expected TS2353 excess property on intersection; got {diags:?}"
    );
}

// Go: internal/checker/relater.go:Relater.typeRelatedToSomeType (fresh literal to union)
#[test]
fn fresh_object_literal_assignable_to_wider_discriminated_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type U = { kind: \"a\"; a: number } | { kind: \"b\"; b: string } | { kind: \"c\"; c: boolean };\n\
         const u: U = { kind: \"a\", a: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTrue (negated prior case)
#[test]
fn switch_true_negated_prior_case_narrows_else_branch_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: \"a\" | \"b\";\n\
         switch (true) {\n  case x !== \"a\": {\n    const s: \"b\" = x;\n    break;\n  }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// ---- T1-E batch 111: typeof/switch narrowing, satisfies, keyof, lookup, spread ----

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeOf
#[test]
fn typeof_object_includes_null_branch_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: { a: number } | null;\n\
         if (typeof x === \"object\") { const o: { a: number } | null = x; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeOf (negated)
#[test]
fn typeof_symbol_negated_narrows_to_string_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const y: symbol | string;\n\
         if (typeof y !== \"symbol\") { const s: string = y; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeOf (negated)
#[test]
fn typeof_undefined_negated_narrows_to_string_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: string | undefined;\n\
         if (typeof x !== \"undefined\") { const s: string = x; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeByTypeOf (negated)
#[test]
fn typeof_object_negated_narrows_to_string_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: object | string;\n\
         if (typeof x !== \"object\") { const s: string = x; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/flow.go:Checker.narrowTypeBySwitchOnTypeOf
#[test]
fn switch_typeof_symbol_case_narrows_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare const x: symbol | string;\n\
         switch (typeof x) { case \"symbol\": { const s: symbol = x; break; } }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.resolveCall (tuple spread to rest tuple)
#[test]
fn spread_tuple_to_rest_tuple_parameter_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function h(...args: [number, string]): void;\n\
         const t: [number, string] = [1, \"a\"];\n\
         h(...t);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.resolveCall (array spread to rest array)
#[test]
fn spread_array_to_rest_array_parameter_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface Array<T> { [n: number]: T; length: number; }\n\
         function f(...args: number[]): void {}\n\
         const a: number[] = [1, 2];\n\
         f(...a);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.getTypeFromIndexedAccessTypeNode (union index)
#[test]
fn indexed_access_union_key_resolves_to_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = { a: number; b: string };\n\
         type U = T[\"a\" | \"b\"];\n\
         const v: U = 1;\n\
         const w: U = \"s\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkSatisfiesExpression
#[test]
fn satisfies_compatible_object_literal_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x = { a: 1 } satisfies { a: number };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkSatisfiesExpression + hasExcessProperties
#[test]
fn satisfies_excess_property_reports_2353() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const obj = { a: 1, b: \"x\" } satisfies { a: number };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2353, got {diags:?}");
    assert_eq!(diags[0].code, 2353);
}

// Go: internal/checker/checker.go:Checker.getIndexType
#[test]
fn keyof_array_type_includes_length_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type K = keyof string[];\nconst k: K = \"length\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkIndexedAccessType
#[test]
fn indexed_access_missing_property_reports_2339() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = { a: number };\ntype U = T[\"b\"];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2339, got {diags:?}");
    assert_eq!(diags[0].code, 2339);
}

// Go: internal/checker/checker.go:Checker.getArgumentArityError (tuple spread arity)
#[test]
fn spread_tuple_wrong_arity_reports_2554() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function h(a: number, b: string): void;\n\
         const t: [number, string, boolean] = [1, \"a\", true];\n\
         h(...t);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2554, got {diags:?}");
    assert_eq!(diags[0].code, 2554);
}

// Go: internal/checker/types.go:template literal type resolution
#[test]
fn template_literal_type_mismatch_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Prefix = \"a\" | \"b\";\n\
         type TL = `${Prefix}-end`;\n\
         const tl: TL = \"a-end\";\n\
         const bad: TL = \"c-end\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
}

// Go: internal/checker/checker.go:Checker.checkPropertyAccessExpression (static member)
#[test]
fn static_member_on_instance_reports_2576() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { static x = 1; }\nconst c = new C();\nconst n = c.x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2576, got {diags:?}");
    assert_eq!(diags[0].code, 2576);
}

// Go: internal/checker/checker.go:Checker.resolveNewExpression -> reportCallResolutionErrors
#[test]
fn construct_overload_no_match_reports_2769_with_chain() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  constructor(x: number);\n  constructor(x: string);\n  constructor(x: any) {}\n}\nnew C(true);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2769, got {diags:?}");
    assert_eq!(diags[0].code, 2769);
    assert!(!diags[0].message_chain.is_empty());
}

// Go: internal/checker/checker.go:Checker.resolveCall (overload with rest)
#[test]
fn overload_with_rest_parameter_resolves_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function g(x: number): void;\n\
         function g(x: string, ...rest: number[]): void;\n\
         function g(x: any, ...rest: any[]) {}\n\
         g(\"s\", 1, 2);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// ---- T1-E batch 112: utility mapped types, generic defaults, conditional assignability ----

const UTILITY_TYPES: &str = "\
type Pick<T, K extends keyof T> = { [P in K]: T[P] };\n\
type Omit<T, K extends keyof T> = { [P in keyof T as P extends K ? never : P]: T[P] };\n\
type Partial<T> = { [P in keyof T]?: T[P] };\n\
type Required<T> = { [P in keyof T]-?: T[P] };\n\
type Readonly<T> = { readonly [P in keyof T]: T[P] };\n\
type Record<K extends string | number | symbol, T> = { [P in K]: T };\n";

// Go: internal/checker/checker.go:Checker.instantiateMappedType (Pick)
#[test]
fn pick_type_selects_keys_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{UTILITY_TYPES}\
             type Src = {{ a: number; b: string }};\n\
             type P = Pick<Src, \"a\">;\n\
             const p: P = {{ a: 1 }};"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.instantiateMappedType (Pick excess)
#[test]
fn pick_type_excess_property_reports_2353() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{UTILITY_TYPES}\
             type Src = {{ a: number; b: string }};\n\
             type P = Pick<Src, \"a\">;\n\
             const p: P = {{ a: 1, b: \"x\" }};"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2353, got {diags:?}");
    assert_eq!(diags[0].code, 2353);
}

// Go: internal/checker/checker.go:Checker.instantiateMappedType (Omit)
#[test]
fn omit_type_excludes_keys_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{UTILITY_TYPES}\
             type Src = {{ a: number; b: string }};\n\
             type O = Omit<Src, \"b\">;\n\
             const o: O = {{ a: 1 }};"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.getTypeOfPropertyOfType (Omit / as-never)
#[test]
fn omit_type_omitted_property_access_reports_2339() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{UTILITY_TYPES}\
             type Src = {{ a: number; b: string }};\n\
             type O = Omit<Src, \"b\">;\n\
             declare const o: O;\n\
             const x = o.b;"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2339, got {diags:?}");
    assert_eq!(diags[0].code, 2339);
}

// Go: internal/checker/checker.go:Checker.resolveMappedTypeMembers (Partial)
#[test]
fn partial_type_makes_properties_optional_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{UTILITY_TYPES}\
             type Src = {{ a: number; b: string }};\n\
             type P = Partial<Src>;\n\
             const p: P = {{ a: 1 }};"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.resolveMappedTypeMembers (Partial assignability)
#[test]
fn partial_type_wrong_property_type_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{UTILITY_TYPES}\
             type Src = {{ a: number }};\n\
             type P = Partial<Src>;\n\
             const p: P = {{ a: \"s\" }};"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
}

// Go: internal/checker/checker.go:Checker.resolveMappedTypeMembers (Required -?)
#[test]
fn required_type_strips_optional_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{UTILITY_TYPES}\
             type Src = {{ a?: number }};\n\
             type R = Required<Src>;\n\
             const r: R = {{ a: 1 }};"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.resolveMappedTypeMembers (Required missing)
#[test]
fn required_type_missing_property_reports_2741() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{UTILITY_TYPES}\
             type Src = {{ a?: number }};\n\
             type R = Required<Src>;\n\
             const r: R = {{ }};"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2741, got {diags:?}");
    assert_eq!(diags[0].code, 2741);
}

// Go: internal/checker/checker.go:Checker.resolveMappedTypeMembers (Readonly)
#[test]
fn readonly_type_property_write_reports_2540() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{UTILITY_TYPES}\
             type Src = {{ a: number }};\n\
             type R = Readonly<Src>;\n\
             declare let r: R;\n\
             r.a = 2;"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2540, got {diags:?}");
    assert_eq!(diags[0].code, 2540);
}

// Go: internal/checker/checker.go:Checker.instantiateMappedType (Record)
#[test]
fn record_type_maps_keys_to_value_type_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{UTILITY_TYPES}\
             type R = Record<\"a\" | \"b\", number>;\n\
             const r: R = {{ a: 1, b: 2 }};"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.instantiateMappedType (Record wrong value)
#[test]
fn record_type_wrong_value_type_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        &format!(
            "{UTILITY_TYPES}\
             type R = Record<\"a\", number>;\n\
             const r: R = {{ a: \"s\" }};"
        ),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
}

// Go: internal/checker/checker.go:Checker.resolveCall (inferred type from first argument)
#[test]
fn generic_call_second_argument_mismatch_reports_2345() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T = number>(x: T, y: T): void {}\nf(1, \"oops\");",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2345, got {diags:?}");
    assert_eq!(diags[0].code, 2345);
}

// Go: internal/checker/checker.go:Checker.getDefaultFromTypeParameter
#[test]
fn generic_call_with_matching_default_type_argument_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T = number>(x: T): T { return x; }\nconst n: number = f(1);",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.checkTypeParameters (2706)
#[test]
fn required_type_parameter_after_default_reports_2706() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T = number, U>(x: T, y: U): void {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2706, got {diags:?}");
    assert_eq!(diags[0].code, 2706);
}

// Go: internal/checker/checker.go:Checker.checkTypeParametersNotReferenced (2744)
#[test]
fn type_parameter_default_references_later_parameter_reports_2744() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type F<T = U, U = number> = T;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2744, got {diags:?}");
    assert_eq!(diags[0].code, 2744);
}

// Go: internal/checker/checker.go:Checker.getDefaultFromTypeParameter (alias instantiation)
#[test]
fn type_alias_default_type_parameter_applied_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Box<T = number> = { value: T };\ntype B = Box;\nconst b: B = { value: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.getDefaultFromTypeParameter (alias mismatch)
#[test]
fn type_alias_default_type_parameter_mismatch_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Box<T = number> = { value: T };\ntype B = Box;\nconst b: B = { value: \"s\" };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
}

// Go: internal/checker/checker.go:Checker.getTypeFromConditionalTypeNode (true branch)
#[test]
fn conditional_type_true_branch_resolves_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type IsStr<T> = T extends string ? \"yes\" : \"no\";\n\
         type A = IsStr<string>;\n\
         const a: A = \"yes\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.getTypeFromConditionalTypeNode (false branch)
#[test]
fn conditional_type_false_branch_resolves_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type IsStr<T> = T extends string ? \"yes\" : \"no\";\n\
         type A = IsStr<number>;\n\
         const a: A = \"no\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.getTypeFromConditionalTypeNode (mismatch)
#[test]
fn conditional_type_resolved_branch_mismatch_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type IsStr<T> = T extends string ? \"yes\" : \"no\";\n\
         type A = IsStr<string>;\n\
         const a: A = \"no\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
}

// Go: internal/checker/checker.go:Checker.distributeConditionalType (union check type)
#[test]
fn conditional_type_distributes_over_union_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type F<T> = T extends string ? T : never;\n\
         type A = F<string | number>;\n\
         const a: A = \"s\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.distributeConditionalType (union mismatch)
#[test]
fn conditional_type_distributed_union_mismatch_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type F<T> = T extends string ? T : never;\n\
         type A = F<string | number>;\n\
         const a: A = 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
}

// Go: internal/checker/checker.go:Checker.getTypeFromConditionalTypeNode (infer)
#[test]
fn conditional_type_with_infer_resolves_element_type_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Element<T> = T extends (infer U)[] ? U : never;\n\
         type E = Element<string[]>;\n\
         const e: E = \"s\";",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.getTypeFromConditionalTypeNode (infer mismatch)
#[test]
fn conditional_type_with_infer_mismatch_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Element<T> = T extends (infer U)[] ? U : never;\n\
         type E = Element<string[]>;\n\
         const e: E = 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
}

// Go: internal/checker/checker.go:Checker.getTypeFromConditionalTypeNode (nested)
#[test]
fn nested_conditional_type_resolves_no_diagnostics() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type Outer<T> = T extends string ? (T extends \"a\" ? 1 : 2) : 3;\n\
         type A = Outer<\"a\">;\n\
         const a: A = 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// Go: internal/checker/checker.go:Checker.getTypeFromConditionalTypeNode (never branch)
#[test]
fn conditional_type_never_branch_is_uninhabited_reports_2322() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type F<T> = T extends string ? T : never;\n\
         type A = F<number>;\n\
         const a: A = 1;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected one 2322, got {diags:?}");
    assert_eq!(diags[0].code, 2322);
}
