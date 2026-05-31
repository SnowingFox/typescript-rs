use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// Runs the destructuring transformer over `input` and asserts the emitted JS.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_destructuring_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "downlevel({input:?})");
}

// Go: internal/transformers/destructuring.go:flattenArrayBindingOrAssignmentPattern
// Tracer bullet: a simple array binding pattern over a simple (identifier)
// initializer decomposes into element-access declarations.
// `var [a, b] = arr;` -> `var a = arr[0], b = arr[1];`.
#[test]
fn array_binding_decomposes_to_element_accesses() {
    check_downlevel("var [a, b] = arr;", "var a = arr[0], b = arr[1];");
}

// Go: internal/transformers/destructuring.go:flattenObjectBindingOrAssignmentPattern
// An object binding pattern decomposes into property-access declarations.
// `var { a, b } = o;` -> `var a = o.a, b = o.b;`.
#[test]
fn object_binding_decomposes_to_property_accesses() {
    check_downlevel("var { a, b } = o;", "var a = o.a, b = o.b;");
}

// Go: internal/transformers/destructuring.go:createDefaultValueCheck
// A default in an array element guards the read with `=== void 0`, hoisting the
// element access into a temp so it is read once.
// `var [a = 1] = arr;` -> `var _a = arr[0], a = _a === void 0 ? 1 : _a;`.
#[test]
fn array_default_guards_with_void_zero() {
    check_downlevel(
        "var [a = 1] = arr;",
        "var _a = arr[0], a = _a === void 0 ? 1 : _a;",
    );
}

// Go: internal/transformers/destructuring.go:createDefaultValueCheck
// A default in an object element guards the property read with `=== void 0`.
// `var { a = 1 } = o;` -> `var _a = o.a, a = _a === void 0 ? 1 : _a;`.
#[test]
fn object_default_guards_with_void_zero() {
    check_downlevel(
        "var { a = 1 } = o;",
        "var _a = o.a, a = _a === void 0 ? 1 : _a;",
    );
}

// Go: internal/transformers/destructuring.go:flattenArrayBindingOrAssignmentPattern (recursion)
// A nested array pattern recurses, composing element accesses.
// `var [[a]] = x;` -> `var a = x[0][0];`.
#[test]
fn nested_array_pattern_composes_element_accesses() {
    check_downlevel("var [[a]] = x;", "var a = x[0][0];");
}

// Go: internal/transformers/destructuring.go:flattenObjectBindingOrAssignmentPattern (recursion)
// A nested object pattern recurses, composing property accesses.
// `var { a: { b } } = o;` -> `var b = o.a.b;`.
#[test]
fn nested_object_pattern_composes_property_accesses() {
    check_downlevel("var { a: { b } } = o;", "var b = o.a.b;");
}

// Go: internal/transformers/destructuring.go:flattenArrayBindingOrAssignmentPattern (rest arm)
// An array rest element lowers to an `array.slice(i)` call for the remaining
// elements. `var [a, ...r] = arr;` -> `var a = arr[0], r = arr.slice(1);`.
#[test]
fn array_rest_lowers_to_slice() {
    check_downlevel("var [a, ...r] = arr;", "var a = arr[0], r = arr.slice(1);");
}

// Go: internal/transformers/destructuring.go:createDestructuringPropertyAccess (computed) +
//     flattenDestructuringBinding (BindingOrAssignmentElementContainsNonLiteralComputedName)
// A non-literal computed key forces the source object into a temp first (so it
// is evaluated before the key), then the key into its own temp, preserving the
// `o` then `k` evaluation order.
// `var { [k]: a } = o;` -> `var _a = o, _b = k, a = _a[_b];`.
#[test]
fn computed_key_captures_object_then_key_into_temps() {
    check_downlevel("var { [k]: a } = o;", "var _a = o, _b = k, a = _a[_b];");
}

// Go: internal/transformers/destructuring.go:ensureIdentifier (hoistTempVariables = false)
// A non-simple (non-identifier) initializer is referenced once per element, so
// it is captured in a temp. With `hoistTempVariables = false` (the non-exported
// variable-statement path, which the driver uses) that temp materializes as a
// same-statement declaration (matching tsc's ES5 output) rather than a hoisted
// `var`. `var [a, b] = f();` -> `var _a = f(), a = _a[0], b = _a[1];`.
#[test]
fn non_simple_initializer_is_captured_in_a_temp_declaration() {
    check_downlevel("var [a, b] = f();", "var _a = f(), a = _a[0], b = _a[1];");
}

// ---- Assignment-target mode (round 6k) ----------------------------------
// `FlattenDestructuringAssignment`: a destructuring **assignment** whose value
// is unused (statement context, `needsValue = false`) decomposes into a comma
// sequence of individual element/property assignments. Temps are *hoisted*
// (`hoistTempVariables = true`, the assignment-mode default) into a leading
// `var`, unlike the binding-mode same-statement declarations above.

// Go: internal/transformers/destructuring.go:FlattenDestructuringAssignment / emitAssignment
// Tracer bullet: an array-literal assignment over a simple (identifier) value
// decomposes into element-access assignments.
// `[a, b] = arr;` -> `a = arr[0], b = arr[1];`.
#[test]
fn array_assignment_decomposes_to_element_access_assignments() {
    check_downlevel("[a, b] = arr;", "a = arr[0], b = arr[1];");
}

// Go: internal/transformers/destructuring.go:ensureIdentifier (hoistTempVariables = true)
// A non-simple (non-identifier) value is referenced once per element, so it is
// captured in a hoisted temp `var _a;` with the capture folded into the comma
// sequence (`_a = arr()`). `[a, b] = f();` -> `var _a;` + `_a = f(), a = _a[0], b = _a[1];`.
#[test]
fn array_assignment_non_simple_value_hoists_temp() {
    check_downlevel("[a, b] = f();", "var _a;\n_a = f(), a = _a[0], b = _a[1];");
}

// Go: internal/transformers/destructuring.go:flattenObjectBindingOrAssignmentPattern (assignment mode)
// An object-literal assignment decomposes into property-access assignments. The
// statement-leading `(` parentheses (needed only so the parser doesn't read `{`
// as a block) are dropped once the pattern is lowered.
// `({ a, b } = o);` -> `a = o.a, b = o.b;`.
#[test]
fn object_assignment_decomposes_to_property_access_assignments() {
    check_downlevel("({ a, b } = o);", "a = o.a, b = o.b;");
}

// Go: internal/transformers/destructuring.go:createDefaultValueCheck (assignment mode)
// A default in an array-assignment element guards the read with `=== void 0`,
// hoisting the element access into a temp so it is read once. The temp is
// hoisted into a leading `var` (assignment mode).
// `[a = 1] = arr;` -> `var _a;` + `_a = arr[0], a = _a === void 0 ? 1 : _a;`.
#[test]
fn array_assignment_default_guards_with_void_zero() {
    check_downlevel(
        "[a = 1] = arr;",
        "var _a;\n_a = arr[0], a = _a === void 0 ? 1 : _a;",
    );
}

// Go: internal/transformers/destructuring.go:flattenArrayBindingOrAssignmentPattern (recursion, assignment mode)
// A nested array-assignment pattern recurses, composing element accesses.
// `[[a]] = x;` -> `a = x[0][0];`.
#[test]
fn nested_array_assignment_composes_element_accesses() {
    check_downlevel("[[a]] = x;", "a = x[0][0];");
}

// Go: internal/transformers/destructuring.go:flattenObjectBindingOrAssignmentPattern (recursion, assignment mode)
// A nested object-assignment pattern recurses, composing property accesses.
// `({ a: { b } } = o);` -> `b = o.a.b;`.
#[test]
fn nested_object_assignment_composes_property_accesses() {
    check_downlevel("({ a: { b } } = o);", "b = o.a.b;");
}

// Go: internal/transformers/destructuring.go:flattenArrayBindingOrAssignmentPattern (rest arm, assignment mode)
// An array-assignment rest element lowers to an `array.slice(i)` call for the
// remaining elements. `[a, ...r] = arr;` -> `a = arr[0], r = arr.slice(1);`.
#[test]
fn array_assignment_rest_lowers_to_slice() {
    check_downlevel("[a, ...r] = arr;", "a = arr[0], r = arr.slice(1);");
}
