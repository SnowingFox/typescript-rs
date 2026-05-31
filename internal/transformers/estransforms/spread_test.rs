use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// Runs the spread transformer over `input` and asserts the emitted JS.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_spread_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "downlevel({input:?})");
}

// The `__spreadArray` helper definition emitted in the module prologue once an
// array-literal spread requests it. Verbatim from `SPREAD_ARRAY_HELPER` (the
// `tsc --target es5` literal) — used to build the expected emit text.
const SPREAD_ARRAY_PROLOGUE: &str = r#"var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {
    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
        if (ar || !(i in from)) {
            if (!ar) ar = Array.prototype.slice.call(from, 0, i);
            ar[i] = from[i];
        }
    }
    return to.concat(ar || Array.prototype.slice.call(from));
};"#;

// Go: (no Go port yet) microsoft/TypeScript src/compiler/transformers/es2015.ts:transformAndSpreadElements
// Verified against `tsc --target es5`:
//   [...a, b];  ->  __spreadArray(__spreadArray([], a, true), [b], false);
// Tracer bullet: a leading spread then a trailing element. The accumulator
// starts at `[]` (the literal opens with a spread), the spread segment `a`
// folds in with pack=true, and the trailing `[b]` literal segment with
// pack=false. The `__spreadArray` helper definition is emitted once in the
// module prologue.
#[test]
fn array_spread_then_element_lowers_to_spread_array_segments() {
    check_downlevel(
        "[...a, b];",
        &format!("{SPREAD_ARRAY_PROLOGUE}\n__spreadArray(__spreadArray([], a, true), [b], false);"),
    );
}

// Go: (no Go port yet) microsoft/TypeScript src/compiler/transformers/es2015.ts:transformAndSpreadElements
// Verified against `tsc --target es5`:
//   const c = [...a];  ->  var c = __spreadArray([], a, true);
// (only the spread is lowered here; the `const`->`var` is a separate ES2015
// stage we do not run, so the declaration kind is preserved.)
// Coverage (generalization of the slice-1 fold): a single spread segment still
// folds into `__spreadArray([], a, true)` — there is no array-literal
// single-segment shortcut (that shortcut is argument-list only).
#[test]
fn single_array_spread_folds_into_spread_array() {
    check_downlevel(
        "const c = [...a];",
        &format!("{SPREAD_ARRAY_PROLOGUE}\nconst c = __spreadArray([], a, true);"),
    );
}

// Go: (no Go port yet) microsoft/TypeScript src/compiler/transformers/es2015.ts:transformAndSpreadElements
// Verified against `tsc --target es5`:
//   [1, ...a, 2];  ->  __spreadArray(__spreadArray([1], a, true), [2], false);
// Coverage (generalization): a leading non-spread run makes the accumulator
// start at the first literal segment `[1]` (not `[]`), exercising the
// `starts_with_spread = false` path.
#[test]
fn leading_literal_array_spread_starts_accumulator_at_first_segment() {
    check_downlevel(
        "[1, ...a, 2];",
        &format!(
            "{SPREAD_ARRAY_PROLOGUE}\n__spreadArray(__spreadArray([1], a, true), [2], false);"
        ),
    );
}

// ---- Call-argument spread (round 6aa) -----------------------------------

// Go: (no Go port yet) microsoft/TypeScript src/compiler/transformers/es2015.ts:visitCallExpression
// Verified against `tsc --target es5`:
//   f(...args);  ->  f.apply(void 0, args);
// Tracer bullet: a plain identifier callee with a single spread argument. In an
// argument list, a lone non-packed spread is passed directly (no `__spreadArray`
// wrapper, so no helper prologue), and the receiver `this` is `void 0`.
#[test]
fn call_with_single_spread_argument_lowers_to_apply() {
    check_downlevel("f(...args);", "f.apply(void 0, args);");
}

// Go: (no Go port yet) microsoft/TypeScript src/compiler/transformers/es2015.ts:visitCallExpression
// Verified against `tsc --target es5`:
//   f(a, ...args);  ->  f.apply(void 0, __spreadArray([a], args, false));
// Coverage (generalization): a leading non-spread argument means the segment
// list has more than one entry, so the shortcut does not apply and the args
// fold via `__spreadArray`. In an *argument list* the spread `pack` flag is
// `false` (unlike array literals, where it is `true`).
#[test]
fn call_with_leading_argument_and_spread_folds_into_spread_array() {
    check_downlevel(
        "f(a, ...args);",
        &format!("{SPREAD_ARRAY_PROLOGUE}\nf.apply(void 0, __spreadArray([a], args, false));"),
    );
}

// Go: (no Go port yet) microsoft/TypeScript src/compiler/transformers/es2015.ts:visitCallExpression
// Verified against `tsc --target es5`:
//   o.m(...args);  ->  o.m.apply(o, args);
// A simple member callee captures the receiver `o` as the `apply` `this`
// argument (reusing the identifier directly — no temp needed because a plain
// identifier is side-effect-free).
#[test]
fn member_call_with_spread_captures_receiver_as_this() {
    check_downlevel("o.m(...args);", "o.m.apply(o, args);");
}

// ---- DEFER guards (round 6aa) -------------------------------------------

// Go: (no Go port yet) microsoft/TypeScript src/compiler/transformers/es2015.ts:visitNewExpression
// DEFER guard: `new C(...args)` needs the construct + `C.bind.apply(...)` form
// (`new (C.bind.apply(C, __spreadArray([void 0], args, false)))()`), which is
// out of this round's scope. The `new` expression is left structurally
// unchanged rather than mis-lowered. blocked-by: the `new`-target bind form.
#[test]
fn new_expression_spread_is_left_unchanged() {
    check_downlevel("new C(...args);", "new C(...args);");
}

// Go: (no Go port yet) microsoft/TypeScript src/compiler/transformers/es2015.ts:visitCallExpression
// DEFER guard: a non-simple member receiver (`a.b.m(...args)`) would need a
// hoisted temp to capture `this` once (`tsc`: `(_a = a.b).m.apply(_a, args)`),
// which needs the variable-environment capture machinery. The reachable subset
// captures only a plain identifier receiver, so this is left unchanged.
// blocked-by: the `createCallBinding` temp-capture.
#[test]
fn non_simple_member_receiver_call_spread_is_left_unchanged() {
    check_downlevel("a.b.m(...args);", "a.b.m(...args);");
}
