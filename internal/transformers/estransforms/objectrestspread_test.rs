use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// Runs the object-rest-spread transformer over `input` and asserts the emitted JS.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_object_rest_spread_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "downlevel({input:?})");
}

// The `__rest` helper definition emitted in the module prologue once a rest
// binding requests it. Verbatim from `tsgo_printer::emithelpers::REST_HELPER`
// (the Go `restHelper` literal) — used to build the expected emit text.
const REST_PROLOGUE: &str = r#"var __rest = (this && this.__rest) || function (s, e) {
    var t = {};
    for (var p in s) if (Object.prototype.hasOwnProperty.call(s, p) && e.indexOf(p) < 0)
        t[p] = s[p];
    if (s != null && typeof Object.getOwnPropertySymbols === "function")
        for (var i = 0, p = Object.getOwnPropertySymbols(s); i < p.length; i++) {
            if (e.indexOf(p[i]) < 0 && Object.prototype.propertyIsEnumerable.call(s, p[i]))
                t[p[i]] = s[p[i]];
        }
    return t;
};"#;

// Go: internal/transformers/estransforms/objectrestspread.go:visitObjectLiteralExpression
// Tracer bullet: a lone object spread `{ ...x }` lowers to `Object.assign({}, x)`.
#[test]
fn object_spread_only_lowers_to_assign() {
    check_downlevel("const o = { ...x };", "const o = Object.assign({}, x);");
}

// Go: internal/transformers/estransforms/objectrestspread.go:chunkObjectLiteralElements
// A spread followed by a property chunks into `Object.assign(Object.assign({}, x), { y })`.
#[test]
fn spread_then_property_chunks_pairwise() {
    check_downlevel(
        "const o = { ...x, y };",
        "const o = Object.assign(Object.assign({}, x), { y });",
    );
}

// Go: internal/transformers/estransforms/objectrestspread.go:chunkObjectLiteralElements
// A leading property chunk is the assign target (no synthetic `{}` prepended):
// `{ a, ...x }` -> `Object.assign({ a }, x)`.
#[test]
fn property_then_spread_uses_chunk_as_target() {
    check_downlevel(
        "const o = { a, ...x };",
        "const o = Object.assign({ a }, x);",
    );
}

// Go: internal/transformers/destructuring.go:flattenObjectBindingOrAssignmentPattern (rest arm)
// Tracer bullet for object **rest** binding: a lone rest in a variable
// declaration lowers to a `__rest` call, and the helper definition is emitted
// once in the module prologue. `var { ...rest } = o;` -> `var rest = __rest(o, []);`.
#[test]
fn object_rest_binding_lowers_to_rest_helper() {
    check_downlevel(
        "var { ...rest } = o;",
        &format!("{REST_PROLOGUE}\nvar rest = __rest(o, []);"),
    );
}

// Go: internal/transformers/destructuring.go:flattenObjectBindingOrAssignmentPattern
// A leading shorthand binding is kept as an object binding pattern declaration,
// and its key is excluded from `__rest`:
// `var { a, ...rest } = o;` -> `var { a } = o, rest = __rest(o, ["a"]);`.
#[test]
fn leading_binding_is_excluded_from_rest_keys() {
    check_downlevel(
        "var { a, ...rest } = o;",
        &format!("{REST_PROLOGUE}\nvar {{ a }} = o, rest = __rest(o, [\"a\"]);"),
    );
}

// Go: internal/printer/factory.go:NodeFactory.NewRestHelper (multiple excluded keys)
// Each leading binding contributes one excluded key, in source order.
#[test]
fn multiple_leading_bindings_list_all_rest_keys() {
    check_downlevel(
        "var { a, b, ...rest } = o;",
        &format!("{REST_PROLOGUE}\nvar {{ a, b }} = o, rest = __rest(o, [\"a\", \"b\"]);"),
    );
}

// Go: internal/transformers/destructuring.go:flattenDestructuringBinding (declaration-list flags)
// The `const`/`let` kind of the declaration list is preserved on the rebuilt list.
#[test]
fn const_declaration_kind_is_preserved() {
    check_downlevel(
        "const { x, ...rest } = o;",
        &format!("{REST_PROLOGUE}\nconst {{ x }} = o, rest = __rest(o, [\"x\"]);"),
    );
}

// Go: internal/ast/utilities.go:TryGetPropertyNameOfBindingOrAssignmentElement (rename)
// A renamed binding `{ a: b }` excludes the *property* key `a` (not the local
// binding `b`) and keeps the rename in the leading pattern.
#[test]
fn renamed_leading_binding_excludes_property_key() {
    check_downlevel(
        "var { a: b, ...rest } = o;",
        &format!("{REST_PROLOGUE}\nvar {{ a: b }} = o, rest = __rest(o, [\"a\"]);"),
    );
}

// Go: internal/transformers/destructuring.go:ensureIdentifier (hoist DEFER)
// A non-simple initializer with leading bindings would need a hoisted temp (the
// value is referenced twice); that is outside the reachable subset, so the
// statement is left unchanged (DEFER) rather than emitting wrong code.
#[test]
fn non_simple_initializer_with_leading_binding_is_left_unchanged() {
    check_downlevel("var { a, ...rest } = f();", "var { a, ...rest } = f();");
}
