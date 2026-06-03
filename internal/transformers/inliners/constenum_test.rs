use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;
use tsgo_evaluator::EvalValue;
use tsgo_jsnum::Number;

// Go: internal/transformers/inliners/constenum.go:safeMultiLineComment
#[test]
fn safe_comment_no_close() {
    assert_eq!(safe_multi_line_comment("hello"), " hello ");
}

#[test]
fn safe_comment_escapes_close() {
    assert_eq!(safe_multi_line_comment("a*/b"), " a*_/b ");
}

#[test]
fn safe_comment_multiple_closes() {
    assert_eq!(safe_multi_line_comment("a*/b*/c"), " a*_/b*_/c ");
}

#[test]
fn safe_comment_empty() {
    assert_eq!(safe_multi_line_comment(""), "  ");
}

// Go: internal/transformers/inliners/constenum.go:NewConstEnumInliningTransformer
// With a no-op resolver, the transform is a pass-through.
#[test]
fn no_resolver_is_passthrough() {
    let (ec, source_file) = parse_shared("var x = Direction.Up;");
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let mut tx = new_const_enum_inlining_transformer(&opts, None);
    let result = tx.transform_source_file(source_file);
    assert_eq!(
        emit(&ec, result, "var x = Direction.Up;"),
        "var x = Direction.Up;"
    );
}

/// A test resolver that returns a numeric value for `Direction.Up`.
struct StubResolver {
    value: EvalValue,
    #[allow(dead_code)]
    target_kind: Kind,
}

impl ConstantValueResolver for StubResolver {
    fn get_constant_value(&self, _node: NodeId) -> EvalValue {
        self.value.clone()
    }
}

// Go: internal/transformers/inliners/constenum.go:visit (numeric inlining)
// A property access whose constant value is a number is replaced with the
// numeric literal.
#[test]
fn numeric_constant_replaces_property_access() {
    let (ec, source_file) = parse_shared("var x = Direction.Up;");
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let resolver = StubResolver {
        value: EvalValue::Num(Number::from(0.0)),
        target_kind: Kind::PropertyAccessExpression,
    };
    let mut tx = new_const_enum_inlining_transformer(&opts, Some(Box::new(resolver)));
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, "var x = Direction.Up;"), "var x = 0;");
}

// Go: internal/transformers/inliners/constenum.go:visit (string inlining)
#[test]
fn string_constant_replaces_property_access() {
    let (ec, source_file) = parse_shared("var x = Direction.Up;");
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let resolver = StubResolver {
        value: EvalValue::Str("UP".to_string()),
        target_kind: Kind::PropertyAccessExpression,
    };
    let mut tx = new_const_enum_inlining_transformer(&opts, Some(Box::new(resolver)));
    let result = tx.transform_source_file(source_file);
    assert_eq!(
        emit(&ec, result, "var x = Direction.Up;"),
        "var x = \"UP\";"
    );
}

// Go: internal/transformers/inliners/constenum.go:visit (negative number)
#[test]
fn negative_numeric_constant_uses_prefix_unary() {
    let (ec, source_file) = parse_shared("var x = Nums.Neg;");
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let resolver = StubResolver {
        value: EvalValue::Num(Number::from(-5.0)),
        target_kind: Kind::PropertyAccessExpression,
    };
    let mut tx = new_const_enum_inlining_transformer(&opts, Some(Box::new(resolver)));
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, "var x = Nums.Neg;"), "var x = -5;");
}

// Go: internal/transformers/inliners/constenum.go:visit (no constant = pass through)
#[test]
fn non_constant_access_is_unchanged() {
    let (ec, source_file) = parse_shared("var x = obj.prop;");
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let resolver = StubResolver {
        value: EvalValue::None,
        target_kind: Kind::PropertyAccessExpression,
    };
    let mut tx = new_const_enum_inlining_transformer(&opts, Some(Box::new(resolver)));
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, "var x = obj.prop;"), "var x = obj.prop;");
}
