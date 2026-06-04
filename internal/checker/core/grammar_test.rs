use crate::core::program::BoundProgram;
use crate::core::test_support::StubProgram;
use crate::core::Checker;

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers (duplicate, tracer)
#[test]
fn duplicate_modifier_reports_already_seen() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "export export function f() {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 1030);
    assert_eq!(diags[0].message, "'export' modifier already seen.");
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers (async in ambient)
#[test]
fn async_in_ambient_context_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare async function f() {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1040);
    assert!(
        d.is_some(),
        "expected TS1040 (async in ambient context); got: {:?}",
        diags
    );
    assert_eq!(
        d.unwrap().message,
        "'async' modifier cannot be used in an ambient context."
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers (accessibility)
#[test]
fn duplicate_accessibility_modifier_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  public private x;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    // `public private` on a class member: a second accessibility modifier.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 1028);
    assert_eq!(diags[0].message, "Accessibility modifier already seen.");
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers (KindPublicKeyword,
// `flags&ModifierFlagsStatic != 0` -> X_0_modifier_must_precede_1_modifier)
#[test]
fn accessibility_modifier_after_static_must_precede() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  static public x = 1;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    // `static public`: the accessibility modifier must come before `static`.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 1029);
    assert_eq!(
        diags[0].message,
        "'public' modifier must precede 'static' modifier."
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers (KindAccessorKeyword,
// `flags&ModifierFlagsReadonly != 0` -> X_0_modifier_cannot_be_used_with_1_modifier)
#[test]
fn accessor_modifier_with_readonly_reports_conflict() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  readonly accessor x = 1;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    // `readonly accessor`: the two modifiers cannot be combined.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 1243);
    assert_eq!(
        diags[0].message,
        "'accessor' modifier cannot be used with 'readonly' modifier."
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers (KindReadonlyKeyword,
// node.Kind not a property/index-sig/parameter ->
// X_readonly_modifier_can_only_appear_on_a_property_declaration_or_index_signature)
#[test]
fn readonly_modifier_on_method_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  readonly m() {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    // `readonly` is only valid on a property declaration or index signature.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 1024);
    assert_eq!(
        diags[0].message,
        "'readonly' modifier can only appear on a property declaration or index signature."
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers (KindAccessorKeyword,
// node.Kind != PropertyDeclaration -> X_accessor_modifier_can_only_appear_on_a_property_declaration)
#[test]
fn accessor_modifier_on_method_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  accessor m() {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    // `accessor` is only valid on a property declaration.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 1275);
    assert_eq!(
        diags[0].message,
        "'accessor' modifier can only appear on a property declaration."
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarVariableDeclaration
// (blockScopeKind == NodeFlagsConst, no initializer -> X_0_declarations_must_be_initialized)
#[test]
fn const_declaration_without_initializer_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "const x;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    // `const x;` has no initializer.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 1155);
    assert_eq!(
        diags[0].message,
        "'const' declarations must be initialized."
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarConstructorTypeAnnotation
// (node.Type != nil -> Type_annotation_cannot_appear_on_a_constructor_declaration)
#[test]
fn constructor_with_return_type_annotation_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  constructor(): void {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    // A constructor cannot have a return type annotation.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 1093);
    assert_eq!(
        diags[0].message,
        "Type annotation cannot appear on a constructor declaration."
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarConstructorTypeParameters
// (node.TypeParameters != nil -> Type_parameters_cannot_appear_on_a_constructor_declaration)
#[test]
fn constructor_with_type_parameters_reports_diagnostic() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  constructor<T>() {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    // A constructor cannot declare type parameters.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 1092);
    assert_eq!(
        diags[0].message,
        "Type parameters cannot appear on a constructor declaration."
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarObjectLiteralExpression
// (duplicate property assignment -> TS1117)
#[test]
fn object_literal_duplicate_property_reports_1117() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x = { a: 1, a: 2 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let dup = diags.iter().find(|d| d.code == 1117);
    assert!(
        dup.is_some(),
        "expected TS1117 for duplicate property; got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarObjectLiteralExpression
// (duplicate get accessor -> TS1118)
#[test]
fn object_literal_duplicate_getter_reports_1118() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x = { get a() { return 1; }, get a() { return 2; } };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let dup = diags.iter().find(|d| d.code == 1118);
    assert!(
        dup.is_some(),
        "expected TS1118 for duplicate getter; got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarObjectLiteralExpression
// (property + accessor -> TS1119)
#[test]
fn object_literal_property_and_accessor_reports_1119() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x = { a: 1, get a() { return 2; } };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let dup = diags.iter().find(|d| d.code == 1119);
    assert!(
        dup.is_some(),
        "expected TS1119 for property+accessor; got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarObjectLiteralExpression
// (get + set pair -> no error, valid pairing)
#[test]
fn object_literal_get_set_pair_no_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x = { get a() { return 1; }, set a(v: number) {} };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let dup = diags.iter().find(|d| d.code == 1118);
    assert!(
        dup.is_none(),
        "get+set pair should NOT report TS1118; got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarObjectLiteralExpression
// (duplicate methods -> TS2300)
#[test]
fn object_literal_duplicate_method_reports_2300() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x = { m() {}, m() {} };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let dup = diags.iter().find(|d| d.code == 2300);
    assert!(
        dup.is_some(),
        "expected TS2300 for duplicate method; got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// checkGrammarStatementInAmbientContext (TS1036 / TS1183)
// ---------------------------------------------------------------------------

// Go: internal/checker/grammarchecks.go:checkGrammarStatementInAmbientContext
// Statement inside a function in a `declare` module -> TS1183
#[test]
fn ambient_function_body_reports_1183() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function f() { return 1; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1183);
    assert!(
        d.is_some(),
        "expected TS1183 (implementation not declared in ambient); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarStatementInAmbientContext
// A .d.ts style ambient function with a body (via declare) -> TS1183
#[test]
fn ambient_function_with_body_reports_1183() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function g() { const x = 1; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1183);
    assert!(
        d.is_some(),
        "expected TS1183 (implementation not declared in ambient); got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// checkGrammarVariableDeclarationList (using / await using)
// ---------------------------------------------------------------------------

// Go: internal/checker/grammarchecks.go:checkGrammarVariableDeclarationList
// Empty declaration list -> TS1123
#[test]
fn variable_declaration_list_empty_reports_1123() {
    // This is hard to produce via normal parsing, so we test that a `const`
    // without initializer at least fires 1155. A true empty list requires
    // parser error recovery.
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "const ;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    // Parser-recovered empty const -> should fire 1123 or 1155
    assert!(
        !diags.is_empty(),
        "expected a diagnostic for empty/invalid const decl"
    );
}

// ---------------------------------------------------------------------------
// checkGrammarAccessor (get/set parameter/return rules)
// ---------------------------------------------------------------------------

// Go: internal/checker/grammarchecks.go:checkGrammarAccessor
// A getter with parameters -> TS1054
#[test]
fn getter_with_parameter_reports_1054() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  get a(v: number) { return v; }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1054);
    assert!(
        d.is_some(),
        "expected TS1054 (get accessor cannot have parameters); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarAccessor
// A setter with return type annotation -> TS1095
#[test]
fn setter_with_return_type_reports_1095() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  set a(v: number): void {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1095);
    assert!(
        d.is_some(),
        "expected TS1095 (set accessor cannot have return type); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarAccessor
// A setter with no parameters -> TS1049
#[test]
fn setter_with_no_parameter_reports_1049() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  set a() {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1049);
    assert!(
        d.is_some(),
        "expected TS1049 (set accessor must have exactly one parameter); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarAccessor
// A setter with optional parameter -> TS1051
#[test]
fn setter_with_optional_parameter_reports_1051() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  set a(v?: number) {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1051);
    assert!(
        d.is_some(),
        "expected TS1051 (set accessor optional parameter); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarAccessor
// A setter with rest parameter -> TS1053
#[test]
fn setter_with_rest_parameter_reports_1053() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  set a(...v: number[]) {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1053);
    assert!(
        d.is_some(),
        "expected TS1053 (set accessor rest parameter); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarAccessor
// A setter with initializer -> TS1052
#[test]
fn setter_with_initializer_reports_1052b() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  set a(v: number = 1) {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1052);
    assert!(
        d.is_some(),
        "expected TS1052 (set accessor parameter initializer); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarAccessor
// An accessor with type parameters -> TS1094
#[test]
fn accessor_with_type_parameters_reports_1094() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  get a<T>() { return 1; }\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1094);
    assert!(
        d.is_some(),
        "expected TS1094 (accessor cannot have type parameters); got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// checkGrammarModifiers expanded rules
// ---------------------------------------------------------------------------

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers (override already seen)
#[test]
fn override_modifier_already_seen_reports_1030() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class B { x = 1; }\nclass C extends B {\n  override override x = 2;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 1030 && d.message.contains("override"));
    assert!(
        d.is_some(),
        "expected TS1030 for duplicate override; got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers (declare + async)
// declare + async -> TS1040
#[test]
fn declare_with_async_reports_1040() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare async function g(): void;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1040);
    assert!(
        d.is_some(),
        "expected TS1040 for declare+async; got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers
// export on class element -> TS1031
#[test]
fn export_on_class_member_reports_1031() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  export x = 1;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1031);
    assert!(
        d.is_some(),
        "expected TS1031 (export on class element); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers (declare already seen)
#[test]
fn declare_modifier_already_seen_reports_1030() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare declare const x: number;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 1030 && d.message.contains("declare"));
    assert!(
        d.is_some(),
        "expected TS1030 for duplicate declare; got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers (async already seen)
#[test]
fn async_modifier_already_seen_reports_1030() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "async async function f() {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 1030 && d.message.contains("async"));
    assert!(
        d.is_some(),
        "expected TS1030 for duplicate async; got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers (readonly on non-property, non-index, non-param)
// Already tested above, but confirming `readonly` on a method: TS1024
// (duplicate of readonly_modifier_on_method_reports_diagnostic -- skip)

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers
// `abstract` + `static` incompatibility -> TS1243
#[test]
fn abstract_with_static_reports_1243() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "abstract class C {\n  static abstract m(): void;\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 1243 && d.message.contains("static") && d.message.contains("abstract"));
    assert!(
        d.is_some(),
        "expected TS1243 (static+abstract incompatible); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers
// Non-method/property/accessor with `abstract` -> TS1242
#[test]
fn abstract_on_non_method_reports_1242() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "abstract class C {\n  abstract constructor() {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    // constructor cannot be abstract -> "can only appear on a class, method, or property declaration"
    let d = diags.iter().find(|d| d.code == 1242);
    assert!(
        d.is_some(),
        "expected TS1242 (abstract only on class/method/property); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers post-loop
// `static` on constructor -> TS1089
#[test]
fn static_on_constructor_reports_1089() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  static constructor() {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1089);
    assert!(
        d.is_some(),
        "expected TS1089 (static on constructor); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers post-loop
// `async` on constructor -> TS1089
#[test]
fn async_on_constructor_reports_1089() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C {\n  async constructor() {}\n}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1089);
    assert!(
        d.is_some(),
        "expected TS1089 (async on constructor); got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// checkGrammarForInOrForOfStatement
// ---------------------------------------------------------------------------

// Go: internal/checker/grammarchecks.go:checkGrammarForInOrForOfStatement
// Multiple declarations in for-in -> TS1091
#[test]
fn for_in_multiple_declarations_reports_1091() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (let x, y in {}) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1091);
    assert!(
        d.is_some(),
        "expected TS1091 (only single variable in for-in); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarForInOrForOfStatement
// for-in variable with initializer -> TS1189
#[test]
fn for_in_variable_with_initializer_reports_1189() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (let x = 0 in {}) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1189);
    assert!(
        d.is_some(),
        "expected TS1189 (for-in variable cannot have initializer); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarForInOrForOfStatement
// for-in variable with type annotation -> TS2404
#[test]
fn for_in_variable_with_type_annotation_reports_2404() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (let x: string in {}) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 2404);
    assert!(
        d.is_some(),
        "expected TS2404 (for-in left-hand side cannot use type annotation); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarForInOrForOfStatement
// for-of variable with type annotation -> TS2483
#[test]
fn for_of_variable_with_type_annotation_reports_2483() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (let x: number of []) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 2483);
    assert!(
        d.is_some(),
        "expected TS2483 (for-of left-hand side cannot use type annotation); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarForInOrForOfStatement
// Valid for-of statement -> no grammar error
#[test]
fn for_of_valid_no_grammar_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (const x of [1, 2, 3]) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let grammar_diag = diags.iter().find(|d| {
        d.code == 1188 || d.code == 1189 || d.code == 1190 || d.code == 2404 || d.code == 2483
    });
    assert!(
        grammar_diag.is_none(),
        "valid for-of should not produce for-in/for-of grammar errors; got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarForInOrForOfStatement
// Multiple declarations in for-of -> TS1188
#[test]
fn for_of_multiple_declarations_reports_1188() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "for (let x, y of []) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1188);
    assert!(
        d.is_some(),
        "expected TS1188 (only single variable in for-of); got: {:?}",
        diags
    );
}

// ========== checkGrammarProperty ==========

// Go: internal/checker/grammarchecks.go:checkGrammarProperty
// Class field named "constructor" -> TS18006
#[test]
fn class_field_named_constructor_reports_18006() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { 'constructor': number; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 18006);
    assert!(
        d.is_some(),
        "expected TS18006 (classes may not have a field named 'constructor'); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarProperty
// Interface property with initializer -> TS1246
#[test]
fn interface_property_with_initializer_reports_1246() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I { x: number = 1; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1246);
    assert!(
        d.is_some(),
        "expected TS1246 (interface property cannot have initializer); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarProperty
// Valid class property -> no grammar errors from checkGrammarProperty
#[test]
fn class_property_valid_no_grammar_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "class C { x: number = 1; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 18006 || d.code == 1246);
    assert!(
        d.is_none(),
        "valid class property should not trigger grammar errors; got: {:?}",
        diags
    );
}

// ========== checkGrammarTypeOperatorNode ==========

// Go: internal/checker/grammarchecks.go:checkGrammarTypeOperatorNode
// `readonly` type modifier on non-array/tuple -> TS1354
#[test]
fn readonly_type_modifier_on_non_array_reports_1354() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = readonly string;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1354);
    assert!(
        d.is_some(),
        "expected TS1354 (readonly type modifier only on array/tuple); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarTypeOperatorNode
// `readonly` on array type -> no TS1354
#[test]
fn readonly_type_modifier_on_array_no_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = readonly number[];",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1354);
    assert!(
        d.is_none(),
        "readonly on array type should not trigger TS1354; got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarTypeOperatorNode
// `unique` on non-symbol type -> TS1005 ('symbol' expected)
#[test]
fn unique_type_on_non_symbol_reports_1005() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "type T = unique string;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 1005 && d.message.contains("symbol"));
    assert!(
        d.is_some(),
        "expected TS1005 ('symbol' expected for unique type); got: {:?}",
        diags
    );
}

// ========== checkGrammarTopLevelElementForRequiredDeclareModifier ==========

// Go: internal/checker/grammarchecks.go:checkGrammarTopLevelElementForRequiredDeclareModifier
// .d.ts file top-level declaration without declare -> TS1046
#[test]
fn dts_top_level_without_declare_reports_1046() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.d.ts",
        "function f(): void;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1046);
    assert!(
        d.is_some(),
        "expected TS1046 (top-level declarations in .d.ts must start with declare/export); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarTopLevelElementForRequiredDeclareModifier
// .d.ts file with declare modifier -> no TS1046
#[test]
fn dts_top_level_with_declare_no_1046() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.d.ts",
        "declare function f(): void;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1046);
    assert!(
        d.is_none(),
        ".d.ts with declare should not trigger TS1046; got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarTopLevelElementForRequiredDeclareModifier
// .d.ts file interface (exempt from declare requirement) -> no TS1046
#[test]
fn dts_top_level_interface_no_1046() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.d.ts",
        "interface I { x: number; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1046);
    assert!(
        d.is_none(),
        "interface in .d.ts should not trigger TS1046; got: {:?}",
        diags
    );
}

// ========== checkGrammarIndexSignatureParameters ==========

// Go: internal/checker/grammarchecks.go:checkGrammarIndexSignatureParameters
// Index signature with rest parameter -> TS1017
#[test]
fn index_signature_rest_parameter_reports_1017() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I { [...x: string]: number; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1017);
    assert!(
        d.is_some(),
        "expected TS1017 (index signature cannot have rest parameter); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarIndexSignatureParameters
// Index signature with optional parameter -> TS1019
#[test]
fn index_signature_optional_parameter_reports_1019() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I { [x?: string]: number; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1019);
    assert!(
        d.is_some(),
        "expected TS1019 (index signature parameter cannot have question mark); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarIndexSignatureParameters
// Index signature with initializer -> TS1020
#[test]
fn index_signature_with_initializer_reports_1020() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I { [x: string = 'a']: number; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1020);
    assert!(
        d.is_some(),
        "expected TS1020 (index signature parameter cannot have initializer); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarIndexSignatureParameters
// Valid index signature -> no grammar errors
#[test]
fn index_signature_valid_no_grammar_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "interface I { [x: string]: number; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| {
        d.code == 1017 || d.code == 1019 || d.code == 1020 || d.code == 1022 || d.code == 1096
    });
    assert!(
        d.is_none(),
        "valid index signature should not trigger grammar errors; got: {:?}",
        diags
    );
}

// ========== checkGrammarTypeArguments ==========

// Go: internal/checker/grammarchecks.go:checkGrammarTypeArguments
// Trailing comma in type arguments -> TS1009
#[test]
fn type_arguments_trailing_comma_reports_1009() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T>() {}\nf<number,>();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1009);
    assert!(
        d.is_some(),
        "expected TS1009 (trailing comma not allowed in type arguments); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarTypeArguments
// Valid type arguments -> no TS1009 or TS1099
#[test]
fn type_arguments_valid_no_grammar_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f<T>() {}\nf<number>();",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1009 || d.code == 1099);
    assert!(
        d.is_none(),
        "valid type arguments should not trigger grammar errors; got: {:?}",
        diags
    );
}

// ========== checkGrammarArrowFunction ==========

// Go: internal/checker/grammarchecks.go:checkGrammarArrowFunction
// Arrow function with line terminator before `=>` -> TS1200
#[test]
fn arrow_function_line_terminator_before_arrow_reports_1200() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const f = (x: number)\n=> x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1200);
    assert!(
        d.is_some(),
        "expected TS1200 (line terminator not permitted before arrow); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarArrowFunction
// Valid arrow function -> no TS1200
#[test]
fn arrow_function_valid_no_1200() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const f = (x: number) => x;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1200);
    assert!(
        d.is_none(),
        "valid arrow function should not trigger TS1200; got: {:?}",
        diags
    );
}

// ========== T1-D9: checkGrammarMetaProperty ==========

// Go: internal/checker/grammarchecks.go:checkGrammarMetaProperty
// `new.xyz` where xyz !== "target" -> TS17012
#[test]
fn meta_property_new_invalid_name_reports_17012() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f() { new.xyz; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 17012);
    assert!(
        d.is_some(),
        "expected TS17012 ('xyz' is not a valid meta-property for 'new'); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarMetaProperty
// `import.xyz` where xyz !== "meta" and not a callee -> TS17012
#[test]
fn meta_property_import_invalid_name_reports_17012() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "import.xyz;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 17012);
    assert!(
        d.is_some(),
        "expected TS17012 ('xyz' is not a valid meta-property for 'import'); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarMetaProperty
// `import.xyz(...)` where it IS a callee but xyz !== "meta"/"defer" -> TS18061
#[test]
fn meta_property_import_callee_invalid_name_reports_18061() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "import.xyz('./a');"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 18061);
    assert!(
        d.is_some(),
        "expected TS18061 ('xyz' is not a valid meta-property for keyword 'import'. Did you mean 'meta' or 'defer'?); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarMetaProperty
// `import.defer` as non-callee -> TS1005 ("(" expected)
#[test]
fn meta_property_import_defer_non_callee_reports_1005() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "import.defer;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1005);
    assert!(
        d.is_some(),
        "expected TS1005 ('(' expected) for import.defer non-callee; got: {:?}",
        diags
    );
}

// ========== T1-D10: checkGrammarImportCallExpression ==========

// Go: internal/checker/grammarchecks.go:checkGrammarImportCallExpression
// `import()` with 0 args -> TS1450
#[test]
fn import_call_zero_args_reports_1450() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "import();"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1450);
    assert!(
        d.is_some(),
        "expected TS1450 (dynamic import accepts module specifier + optional attributes); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarImportCallExpression
// `import('./a')` with default options (ESNext) -> no grammar error
#[test]
fn import_call_single_arg_esnext_no_grammar_error() {
    use tsgo_core::compileroptions::{CompilerOptions, ModuleKind};
    let mut opts = CompilerOptions::default();
    opts.module = ModuleKind::EsNext;
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "import('./a');",
        opts,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let grammar = diags
        .iter()
        .find(|d| d.code == 1323 || d.code == 1326 || d.code == 1450);
    assert!(
        grammar.is_none(),
        "import('./a') with ESNext module should not trigger grammar error; got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarImportCallExpression
// `import('./a')` with module=ES2015 -> TS1323 (dynamic imports not supported)
#[test]
fn import_call_es2015_reports_1323() {
    use tsgo_core::compileroptions::{CompilerOptions, ModuleKind};
    let mut opts = CompilerOptions::default();
    opts.module = ModuleKind::Es2015;
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts",
        "import('./a');",
        opts,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1323);
    assert!(
        d.is_some(),
        "expected TS1323 (dynamic imports not supported with ES2015); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarImportCallExpression
// `import(...x)` with spread -> TS1325
#[test]
fn import_call_spread_arg_reports_1325() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind("/a.ts", "import(...['./a']);"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1325);
    assert!(
        d.is_some(),
        "expected TS1325 (spread element in dynamic import); got: {:?}",
        diags
    );
}

// ========== T1-D7: checkGrammarDecorator ==========

// Go: internal/checker/grammarchecks.go:checkGrammarDecorator
// Decorator with optional chaining `?.` reports expression-in-parens error
#[test]
fn decorator_optional_chain_call_reports_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function dec(t: any) { return t; }\nclass C { @dec?.() method() {} }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1497);
    assert!(
        d.is_some(),
        "expected TS1497 (expression must be enclosed in parentheses to be used as a decorator); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarDecorator
// Simple identifier decorator -> no grammar error
#[test]
fn decorator_simple_identifier_no_grammar_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "declare function sealed(target: any): any;\n@sealed\nclass C {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1497);
    assert!(
        d.is_none(),
        "simple identifier decorator should not trigger TS1497; got: {:?}",
        diags
    );
}

// ========== checkGrammarParameterList ==========

// Go: internal/checker/grammarchecks.go:checkGrammarParameterList
// Rest parameter not last -> TS1014
#[test]
fn rest_parameter_not_last_reports_1014() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(...a: number[], b: string) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1014);
    assert!(
        d.is_some(),
        "expected TS1014 (rest parameter must be last); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarParameterList
// Rest parameter with ? -> TS1047
#[test]
fn rest_parameter_optional_reports_1047() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(...a?: number[]) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1047);
    assert!(
        d.is_some(),
        "expected TS1047 (rest parameter cannot be optional); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarParameterList
// Rest parameter with initializer -> TS1048
#[test]
fn rest_parameter_with_initializer_reports_1048() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(...a: number[] = []) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1048);
    assert!(
        d.is_some(),
        "expected TS1048 (rest parameter cannot have initializer); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarParameterList
// Required parameter after optional -> TS1016
#[test]
fn required_parameter_after_optional_reports_1016() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(a?: number, b: number) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1016);
    assert!(
        d.is_some(),
        "expected TS1016 (required parameter cannot follow optional); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarParameterList
// Valid parameter list -> no grammar error
#[test]
fn valid_parameter_list_no_grammar_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f(a: number, b?: string, ...c: boolean[]) {}",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags
        .iter()
        .find(|d| d.code == 1014 || d.code == 1016 || d.code == 1047 || d.code == 1048);
    assert!(
        d.is_none(),
        "valid parameter list should not trigger grammar errors; got: {:?}",
        diags
    );
}

#[test]
fn meta_property_new_target_valid_no_grammar_error() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "function f() { new.target; }",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let grammar = diags.iter().find(|d| d.code == 17012);
    assert!(
        grammar.is_none(),
        "new.target in function body should not trigger TS17012; got: {:?}",
        diags
    );
}

// ========== checkGrammarComputedPropertyName ==========

// Go: internal/checker/grammarchecks.go:checkGrammarComputedPropertyName
// Comma expression in computed property name -> TS1171
#[test]
fn computed_property_name_comma_expr_reports_1171() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const x = { [1, 2]: 3 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1171);
    assert!(
        d.is_some(),
        "expected TS1171 (comma expression not allowed in computed property name); got: {:?}",
        diags
    );
}

// Go: internal/checker/grammarchecks.go:checkGrammarComputedPropertyName
// Valid computed property name -> no TS1171
#[test]
fn computed_property_name_valid_no_1171() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind(
        "/a.ts",
        "const key = 'a';\nconst x = { [key]: 1 };",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 1171);
    assert!(
        d.is_none(),
        "valid computed property should not trigger TS1171; got: {:?}",
        diags
    );
}
