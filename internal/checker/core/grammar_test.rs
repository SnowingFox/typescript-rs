use crate::core::program::BoundProgram;
use crate::core::test_support::StubProgram;
use crate::core::Checker;

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers (duplicate, tracer)
#[test]
fn duplicate_modifier_reports_already_seen() {
    let p = StubProgram::parse_and_bind("/a.ts", "export export function f() {}");
    let mut c = Checker::new();
    c.check_source_file(&p, p.root());
    let diags = c.get_diagnostics(p.root());
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 1030);
    assert_eq!(diags[0].message, "'export' modifier already seen.");
}

// Go: internal/checker/grammarchecks.go:checkGrammarModifiers (async in ambient)
#[test]
fn async_in_ambient_context_reports_diagnostic() {
    let p = StubProgram::parse_and_bind("/a.ts", "declare async function f() {}");
    let mut c = Checker::new();
    c.check_source_file(&p, p.root());
    let diags = c.get_diagnostics(p.root());
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
    let p = StubProgram::parse_and_bind("/a.ts", "class C {\n  public private x;\n}");
    let mut c = Checker::new();
    c.check_source_file(&p, p.root());
    let diags = c.get_diagnostics(p.root());
    // `public private` on a class member: a second accessibility modifier.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 1028);
    assert_eq!(diags[0].message, "Accessibility modifier already seen.");
}
