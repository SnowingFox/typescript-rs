use crate::core::declared_types::get_declared_type_of_symbol;
use crate::core::program::BoundProgram;
use crate::core::test_support::StubProgram;
use crate::core::Checker;
use std::rc::Rc;
use tsgo_ast::{NodeData, NodeId, SymbolId};
use tsgo_core::compileroptions::{CompilerOptions, JsxEmit};
use tsgo_core::tristate::Tristate;

/// All-defaults options with classic `jsx: react` emit (the only mode that
/// requires the JSX factory namespace in scope, i.e. reports TS2874).
fn react_options() -> CompilerOptions {
    CompilerOptions {
        jsx: JsxEmit::React,
        ..CompilerOptions::default()
    }
}

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
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_tsx("/a.tsx", "<Foo/>;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'Foo'.");
}

// Go: internal/checker/jsx.go:Checker.checkJsxElement / checkJsxChildren
#[test]
fn element_children_are_typed() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_tsx(
        "/a.tsx",
        "interface IntrinsicElements {\n  div: number;\n}\n<div>{y}</div>;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(std::rc::Rc::clone(&p) as std::rc::Rc<dyn BoundProgram>);
    let it = get_declared_type_of_symbol(&mut c, &*p, sym(&p, "IntrinsicElements"), None);
    c.set_jsx_intrinsic_elements(it);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    // The child expression `{y}` references an undefined name.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

// Go: internal/checker/jsx.go:Checker.getIntrinsicTagSymbol (TS7026, self-closing)
#[test]
fn self_closing_intrinsic_without_jsx_intrinsic_elements_reports_one_ts7026() {
    // No `JSX.IntrinsicElements` in scope + `noImplicitAny` (default) -> the
    // intrinsic `<div/>` is implicitly `any` and reports TS7026 exactly once, on
    // the (self-closing) element node.
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_tsx("/a.tsx", "<div/>;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 7026);
    assert_eq!(
        diags[0].message,
        "JSX element implicitly has type 'any' because no interface 'JSX.IntrinsicElements' exists."
    );
}

// Go: internal/checker/jsx.go:Checker.checkJsxElementDeferred (TS7026, open + close)
#[test]
fn paired_intrinsic_element_without_jsx_intrinsic_elements_reports_two_ts7026() {
    // A paired intrinsic `<div></div>` with no `JSX.IntrinsicElements` reports
    // TS7026 TWICE under `noImplicitAny`: once on the opening element (via the
    // opening-like check) and once on the closing element (via the deferred
    // closing-tag resolution), matching Go's per-element span/count.
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_tsx("/a.tsx", "<div></div>;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 2);
    for d in diags {
        assert_eq!(d.code, 7026);
        assert_eq!(
            d.message,
            "JSX element implicitly has type 'any' because no interface 'JSX.IntrinsicElements' exists."
        );
    }
    // The two diagnostics fall on DISTINCT spans (opening vs closing element).
    assert_ne!(diags[0].start, diags[1].start);
}

// Go: internal/scanner/scanner.go:GetErrorRangeForNode — the TS7026 span starts
// at the element's first non-trivia character (the `<`), NOT the node's
// full-start (which includes the leading whitespace before `<`). Drives the
// `error_skipping_leading_trivia` emit so the byte offset matches `tsc`.
#[test]
fn self_closing_intrinsic_ts7026_span_skips_leading_trivia() {
    // Two leading spaces before `<div/>`; the diagnostic must start at the `<`
    // (byte offset 2), not the element node's full-start (byte 0).
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_tsx("/a.tsx", "  <div/>;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 7026);
    assert_eq!(
        diags[0].start, 2,
        "span starts at `<`, skipping leading trivia"
    );
    // `<div/>` is 6 characters wide (the element node end is byte 8).
    assert_eq!(diags[0].length, 6);
}

// GUARD — Go: internal/checker/jsx.go:Checker.getJsxType (JSX.IntrinsicElements
// resolves -> no TS7026). A `declare namespace JSX { interface IntrinsicElements
// { div } }` in scope makes `getJsxType(IntrinsicElements)` resolve to a real
// (non-error) type, so the known intrinsic `<div/>` reports NO TS7026.
#[test]
fn intrinsic_element_with_declared_jsx_intrinsic_elements_reports_no_ts7026() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_tsx(
        "/a.tsx",
        "declare namespace JSX {\n  interface IntrinsicElements {\n    div: any;\n  }\n}\n<div/>;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 7026),
        "declared JSX.IntrinsicElements must suppress TS7026; got {diags:?}"
    );
}

// GUARD — Go: internal/checker/jsx.go:isJsxIntrinsicTagName (value tags are NOT
// intrinsic). A capitalized, resolved component `<Foo/>` is value-based, so the
// intrinsic-only TS7026 never fires (and `Foo` is declared, so no TS2304).
#[test]
fn value_element_reports_no_ts7026() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_tsx(
        "/a.tsx",
        "declare const Foo: any;\n<Foo/>;",
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.is_empty(),
        "a resolved value element emits no JSX implicit-any/cannot-find-name; got {diags:?}"
    );
}

// GUARD — Go: internal/checker/jsx.go:Checker.getIntrinsicTagSymbol (TS7026 is
// gated on `c.noImplicitAny`). With `strict: false` AND `noImplicitAny: false`,
// the intrinsic `<div/>` with no `JSX.IntrinsicElements` stays implicitly `any`
// SILENTLY (no TS7026).
#[test]
fn intrinsic_element_without_no_implicit_any_reports_no_ts7026() {
    let options = CompilerOptions {
        strict: Tristate::False,
        no_implicit_any: Tristate::False,
        ..CompilerOptions::default()
    };
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_tsx_with_options(
        "/a.tsx", "<div/>;", options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 7026),
        "noImplicitAny disabled must suppress TS7026; got {diags:?}"
    );
}

// RED->GREEN (TS2874 tracer) — Go: internal/checker/checker.go:Checker.markJsxAliasReferenced.
// Under classic `jsx: react` emit, a JSX element whose factory namespace
// (`React`) is NOT in scope reports TS2874 on the tag name. A resolved value
// component `<C/>` (so no TS2304 / TS7026) leaves TS2874 as the sole diagnostic.
#[test]
fn classic_react_value_element_without_react_reports_ts2874() {
    let p = Rc::new(StubProgram::parse_and_bind_tsx_with_options(
        "/a.tsx",
        "declare const C: any;\n<C/>;",
        react_options(),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert_eq!(diags.len(), 1, "expected only TS2874; got {diags:?}");
    assert_eq!(diags[0].code, 2874);
    assert_eq!(
        diags[0].message,
        "This JSX tag requires 'React' to be in scope, but it could not be found."
    );
}

// The TS2874 span is the TAG NAME (Go's `jsxFactoryLocation = node.TagName()`),
// not the whole element: `<Component/>` underlines just `Component`.
#[test]
fn ts2874_span_is_the_tag_name() {
    let src = "declare const Component: any;\n<Component/>;";
    let p = Rc::new(StubProgram::parse_and_bind_tsx_with_options(
        "/a.tsx",
        src,
        react_options(),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 2874).expect("TS2874");
    let start = src.find("Component/>").expect("tag") as i32;
    assert_eq!(d.start, start, "span starts at the tag name");
    assert_eq!(d.length, "Component".len() as i32, "span is the tag name");
}

// GUARD — Go: `resolveName(..., namespace, Value, ...)` succeeds. A `declare var
// React` in scope makes the factory namespace resolvable, so NO TS2874.
#[test]
fn classic_react_with_react_var_in_scope_reports_no_ts2874() {
    let p = Rc::new(StubProgram::parse_and_bind_tsx_with_options(
        "/a.tsx",
        "declare var React: any;\ndeclare const C: any;\n<C/>;",
        react_options(),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2874),
        "React in scope must suppress TS2874; got {diags:?}"
    );
}

// GUARD — Go: a namespace-import alias (`import * as React`) counts as the
// factory namespace being in scope (the port resolves with `VALUE | ALIAS`), so
// NO TS2874 even though the alias target module is unresolved here.
#[test]
fn classic_react_with_namespace_import_alias_reports_no_ts2874() {
    let p = Rc::new(StubProgram::parse_and_bind_tsx_with_options(
        "/a.tsx",
        "import * as React from \"react\";\ndeclare const C: any;\n<C/>;",
        react_options(),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2874),
        "an `import * as React` alias must suppress TS2874; got {diags:?}"
    );
}

// GUARD — Go: `jsxFactoryRefErr := IfElse(Jsx == JsxEmitReact, ...)`. Under
// `jsx: preserve` (and react-native / the automatic runtime) the classic factory
// namespace is not required, so a missing `React` does NOT report TS2874.
#[test]
fn preserve_mode_value_element_reports_no_ts2874() {
    let options = CompilerOptions {
        jsx: JsxEmit::Preserve,
        ..CompilerOptions::default()
    };
    let p = Rc::new(StubProgram::parse_and_bind_tsx_with_options(
        "/a.tsx",
        "declare const C: any;\n<C/>;",
        options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2874),
        "non-classic-react emit must not report TS2874; got {diags:?}"
    );
}

// GUARD — Go: `markJsxAliasReferenced` runs for intrinsic tags too (the factory
// `React.createElement("div", ...)` still needs `React` in scope). A classic
// `<div/>` with no `React` reports BOTH TS2874 (on the `div` tag) and the
// existing TS7026 (no `JSX.IntrinsicElements`).
#[test]
fn classic_react_intrinsic_element_without_react_reports_ts2874_and_ts7026() {
    let p = Rc::new(StubProgram::parse_and_bind_tsx_with_options(
        "/a.tsx",
        "<div/>;",
        react_options(),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let ts2874: Vec<_> = diags.iter().filter(|d| d.code == 2874).collect();
    assert_eq!(
        ts2874.len(),
        1,
        "one TS2874 on the intrinsic tag; got {diags:?}"
    );
    assert_eq!(
        ts2874[0].message,
        "This JSX tag requires 'React' to be in scope, but it could not be found."
    );
    assert!(
        diags.iter().any(|d| d.code == 7026),
        "the intrinsic still reports TS7026; got {diags:?}"
    );
}

// GUARD — Go: `getLocalJsxNamespace` reads the per-file `@jsx <factory>` pragma.
// A `/** @jsx h */` pragma sets the factory namespace to `h`; with `declare var
// h` in scope the namespace resolves, so NO TS2874 (even though `React` is
// absent).
#[test]
fn jsx_pragma_overrides_factory_namespace_no_ts2874() {
    let p = Rc::new(StubProgram::parse_and_bind_tsx_with_options(
        "/a.tsx",
        "/** @jsx h */\ndeclare var h: any;\ndeclare const C: any;\n<C/>;",
        react_options(),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    assert!(
        diags.iter().all(|d| d.code != 2874),
        "the `@jsx h` pragma factory `h` resolves; no TS2874; got {diags:?}"
    );
}

// GUARD — the `@jsx <factory>` pragma is reflected in the TS2874 ARGUMENT: with
// `@jsx h` but no `h` in scope, the error names `h` (not `React`).
#[test]
fn jsx_pragma_factory_missing_reports_ts2874_with_pragma_name() {
    let p = Rc::new(StubProgram::parse_and_bind_tsx_with_options(
        "/a.tsx",
        "/** @jsx h */\ndeclare const C: any;\n<C/>;",
        react_options(),
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    let d = diags.iter().find(|d| d.code == 2874).expect("TS2874");
    assert_eq!(
        d.message,
        "This JSX tag requires 'h' to be in scope, but it could not be found."
    );
}

// Go: internal/checker/jsx.go:Checker.checkJsxFragment
#[test]
fn fragment_children_are_typed() {
    let p = std::rc::Rc::new(StubProgram::parse_and_bind_tsx("/a.tsx", "<>{z}</>;"));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.check_source_file(root);
    let diags = c.get_diagnostics(root);
    // A fragment types its `{z}` child, reporting the undefined name.
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'z'.");
}
