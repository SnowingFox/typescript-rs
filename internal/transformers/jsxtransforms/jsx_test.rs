use super::*;
use crate::test_support::{emit, parse_shared_tsx};
use std::rc::Rc;

// Runs the JSX transformer over `input` (parsed as `.tsx`) and asserts the emit.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared_tsx(input);
    let mut tx = new_jsx_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "downlevel({input:?})");
}

// Go: internal/transformers/jsxtransforms/jsx.go:visitJsxOpeningLikeElementCreateElement
// Tracer bullet: an intrinsic (lowercase) self-closing element lowers to a
// `React.createElement("div", null)` classic-runtime call.
#[test]
fn intrinsic_self_closing_element_lowers_to_create_element() {
    check_downlevel("<div/>;", "React.createElement(\"div\", null);");
}

// Go: internal/transformers/jsxtransforms/jsx.go:getJsxFactoryCallee (automatic runtime)
// Under `jsx: react-jsx` (track-2 `compiler_options.jsx`), `<div/>` lowers to the
// automatic-runtime `jsx("div", {})` call (props is `{}`, not `null`). The
// implicit `react/jsx-runtime` import injection is DEFER'd.
#[test]
fn automatic_runtime_self_closing_element_lowers_to_jsx_call() {
    let (ec, source_file) = parse_shared_tsx("<div/>;");
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.jsx = tsgo_core::compileroptions::JsxEmit::ReactJsx;
    let mut tx = new_jsx_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, "<div/>;"), "jsx(\"div\", {});");
}

// Go: internal/transformers/jsxtransforms/jsx.go:getTagName (component identifier)
// A capitalized tag is a component reference: the identifier is used directly.
#[test]
fn component_self_closing_element_uses_identifier_tag() {
    check_downlevel("<Foo/>;", "React.createElement(Foo, null);");
}

// Go: internal/transformers/jsxtransforms/jsx.go:transformJsxAttributeToObjectLiteralElement
// A string-valued attribute becomes a `{ name: "value" }` props object.
#[test]
fn string_attribute_becomes_props_object() {
    check_downlevel(
        "<div id=\"x\"/>;",
        "React.createElement(\"div\", { id: \"x\" });",
    );
}

// Go: internal/transformers/jsxtransforms/jsx.go:transformJsxAttributeInitializer (JsxExpression)
// An expression-valued attribute uses the inner expression as the prop value.
#[test]
fn expression_attribute_uses_inner_expression() {
    check_downlevel("<div id={y}/>;", "React.createElement(\"div\", { id: y });");
}

// Go: internal/transformers/jsxtransforms/jsx.go:visitJsxOpeningLikeElementCreateElement (children)
// An expression child becomes a trailing `createElement` argument.
#[test]
fn expression_child_becomes_trailing_argument() {
    check_downlevel("<div>{x}</div>;", "React.createElement(\"div\", null, x);");
}

// Go: internal/transformers/jsxtransforms/jsx.go:visitJsxText (fixupWhitespaceAndDecodeEntities)
// JSX text becomes a string-literal child argument.
#[test]
fn text_child_becomes_string_literal() {
    check_downlevel(
        "<div>hi</div>;",
        "React.createElement(\"div\", null, \"hi\");",
    );
}

// Go: internal/transformers/jsxtransforms/jsx.go:transformJsxChildToExpression (nested element)
// A nested element child is lowered recursively.
#[test]
fn nested_element_child_is_lowered() {
    check_downlevel(
        "<div><span/></div>;",
        "React.createElement(\"div\", null, React.createElement(\"span\", null));",
    );
}

// Go: internal/transformers/jsxtransforms/jsx.go:visitJsxOpeningFragmentCreateElement
// A fragment lowers to `React.createElement(React.Fragment, null, ...children)`.
#[test]
fn fragment_lowers_to_react_fragment_create_element() {
    check_downlevel("<>{x}</>;", "React.createElement(React.Fragment, null, x);");
}

// ───────────────────────────────────────────────────────────────────────
// T2-8 integration tests: JSX transform verification
// ───────────────────────────────────────────────────────────────────────

// Go: internal/transformers/jsxtransforms/jsx.go:visitJsxText + visitJsxOpeningLikeElementCreateElement
// A classic-runtime element with a text child "hello" produces the expected
// `React.createElement("div", null, "hello")` call — verifies that the text
// normalisation path handles multi-char strings.
#[test]
fn classic_div_with_text_hello() {
    check_downlevel(
        "<div>hello</div>;",
        "React.createElement(\"div\", null, \"hello\");",
    );
}

// Go: internal/transformers/jsxtransforms/jsx.go:getTagName + transformJsxAttributeInitializer
// A component (uppercase tag) with an expression-valued attribute produces a
// props object containing the expression as a value.
#[test]
fn component_with_expression_prop() {
    check_downlevel(
        "<Comp prop={x}/>;",
        "React.createElement(Comp, { prop: x });",
    );
}

// Go: internal/transformers/jsxtransforms/jsx.go:transformJsxChildToExpression (recursive)
// A nested structure — an outer element whose expression child contains a
// nested element — lowers both elements recursively.
#[test]
fn nested_element_inside_expression_child() {
    check_downlevel(
        "<div>{<span>{y}</span>}</div>;",
        "React.createElement(\"div\", null, React.createElement(\"span\", null, y));",
    );
}

// Go: internal/transformers/jsxtransforms/jsx.go:visitJsxOpeningFragmentCreateElement + visitJsxText
// A fragment wrapping a plain text child lowers to
// `React.createElement(React.Fragment, null, "text")`.
#[test]
fn fragment_with_text_child() {
    check_downlevel(
        "<>text</>;",
        "React.createElement(React.Fragment, null, \"text\");",
    );
}

// Go: internal/transformers/jsxtransforms/jsx.go:visitJsxOpeningLikeElementCreateElement
// An element with multiple props produces a single props object whose
// properties appear in source order.
#[test]
fn element_with_multiple_props() {
    check_downlevel(
        "<div id=\"a\" className={b}/>;",
        "React.createElement(\"div\", { id: \"a\", className: b });",
    );
}

// Go: internal/transformers/jsxtransforms/jsx.go:transformJsxChildToExpression
// An element with multiple children of different kinds (text + expression +
// nested element) produces three trailing arguments, each lowered to its
// appropriate form.
#[test]
fn element_with_mixed_children() {
    check_downlevel(
        "<div>a{x}<span/></div>;",
        "React.createElement(\"div\", null, \"a\", x, React.createElement(\"span\", null));",
    );
}
