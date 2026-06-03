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
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 1040);
    assert_eq!(
        diags[0].message,
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
