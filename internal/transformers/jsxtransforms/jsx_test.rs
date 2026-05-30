use super::*;
use crate::test_support::{emit, parse_shared_tsx};
use std::rc::Rc;

// Runs the JSX transformer over `input` (parsed as `.tsx`) and asserts the emit.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared_tsx(input);
    let mut tx = new_jsx_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
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
