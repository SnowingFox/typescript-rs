use crate::core::declared_types::get_declared_type_of_symbol;
use crate::core::program::BoundProgram;
use crate::core::test_support::StubProgram;
use crate::core::Checker;
use tsgo_ast::{NodeData, NodeId, SymbolId};

fn sym(p: &StubProgram, name: &str) -> SymbolId {
    *p.locals(p.root())
        .expect("locals")
        .get(name)
        .unwrap_or_else(|| panic!("missing {name}"))
}

// Returns the expression of the `idx`-th top-level expression statement.
fn expr_stmt_expression(p: &StubProgram, idx: usize) -> NodeId {
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

// Go: internal/checker/jsx.go:Checker.checkJsxSelfClosingElement (intrinsic, tracer)
#[test]
fn check_intrinsic_self_closing_element_resolves() {
    let p = StubProgram::parse_and_bind_tsx(
        "/a.tsx",
        "interface IntrinsicElements {\n  div: number;\n}\n<div/>;",
    );
    let mut c = Checker::new();
    let it = get_declared_type_of_symbol(&mut c, &p, sym(&p, "IntrinsicElements"), None);
    c.set_jsx_intrinsic_elements(it);
    let jsx = expr_stmt_expression(&p, 1);
    let any = c.any_type();
    // A known intrinsic tag resolves with no diagnostics; the element type is `any`
    // until `JSX.Element` lib globals land (P6).
    assert_eq!(c.check_jsx_self_closing_element(&p, jsx), any);
    assert!(c.get_diagnostics(p.root()).is_empty());
}

// Go: internal/checker/jsx.go:Checker.getIntrinsicTagSymbol (unknown tag)
#[test]
fn unknown_intrinsic_tag_reports_diagnostic() {
    let p = StubProgram::parse_and_bind_tsx(
        "/a.tsx",
        "interface IntrinsicElements {\n  div: number;\n}\n<span/>;",
    );
    let mut c = Checker::new();
    let it = get_declared_type_of_symbol(&mut c, &p, sym(&p, "IntrinsicElements"), None);
    c.set_jsx_intrinsic_elements(it);
    let jsx = expr_stmt_expression(&p, 1);
    c.check_jsx_self_closing_element(&p, jsx);
    let diags = c.get_diagnostics(p.root());
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2339);
    assert_eq!(
        diags[0].message,
        "Property 'span' does not exist on type 'JSX.IntrinsicElements'."
    );
}

// Go: internal/checker/jsx.go:Checker.checkJsxAttribute (prop type mismatch)
#[test]
fn attribute_type_mismatch_reports_diagnostic() {
    use crate::core::declared_types::get_property_of_type;
    let p = StubProgram::parse_and_bind_tsx(
        "/a.tsx",
        "interface Attrs {\n  id: string;\n}\ninterface IntrinsicElements {\n  div: number;\n}\n<div id={1}/>;",
    );
    let mut c = Checker::new();
    let it = get_declared_type_of_symbol(&mut c, &p, sym(&p, "IntrinsicElements"), None);
    let attrs = get_declared_type_of_symbol(&mut c, &p, sym(&p, "Attrs"), None);
    // Make `IntrinsicElements.div` carry the `Attrs` props type.
    let div_sym = get_property_of_type(&c, it, "div").expect("div");
    c.value_symbol_links.get(div_sym).resolved_type = Some(attrs);
    c.set_jsx_intrinsic_elements(it);
    let jsx = expr_stmt_expression(&p, 2);
    c.check_jsx_self_closing_element(&p, jsx);
    let diags = c.get_diagnostics(p.root());
    // `id={1}` (number) is not assignable to the declared `id: string`.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2322);
    assert!(diags[0]
        .message
        .ends_with("is not assignable to type 'string'."));
}

// Go: internal/checker/jsx.go:Checker.checkJsxOpeningLikeElement (value element)
#[test]
fn value_element_unresolved_reports_cannot_find_name() {
    // Driven through `check_source_file` so the JSX dispatch in `check_expression`
    // is exercised too.
    let p = StubProgram::parse_and_bind_tsx("/a.tsx", "<Foo/>;");
    let mut c = Checker::new();
    c.check_source_file(&p, p.root());
    let diags = c.get_diagnostics(p.root());
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'Foo'.");
}

// Go: internal/checker/jsx.go:Checker.checkJsxElement / checkJsxChildren
#[test]
fn element_children_are_typed() {
    let p = StubProgram::parse_and_bind_tsx(
        "/a.tsx",
        "interface IntrinsicElements {\n  div: number;\n}\n<div>{y}</div>;",
    );
    let mut c = Checker::new();
    let it = get_declared_type_of_symbol(&mut c, &p, sym(&p, "IntrinsicElements"), None);
    c.set_jsx_intrinsic_elements(it);
    c.check_source_file(&p, p.root());
    let diags = c.get_diagnostics(p.root());
    // The child expression `{y}` references an undefined name.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/jsx.go:Checker.checkJsxFragment
#[test]
fn fragment_children_are_typed() {
    let p = StubProgram::parse_and_bind_tsx("/a.tsx", "<>{z}</>;");
    let mut c = Checker::new();
    c.check_source_file(&p, p.root());
    let diags = c.get_diagnostics(p.root());
    // A fragment types its `{z}` child, reporting the undefined name.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'z'.");
}
