use super::*;

use std::rc::Rc;

use crate::core::test_support::StubProgram;
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::tristate::Tristate;

/// Binds `text` as `/a.ts` with `allowUnreachableCode = false` and returns the
/// reported `TS7027` `(start, length)` spans (in source order).
fn ts7027_spans(text: &str) -> Vec<(i32, i32)> {
    let options = CompilerOptions {
        allow_unreachable_code: Tristate::False,
        ..Default::default()
    };
    let p = Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts", text, options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.get_diagnostics(root)
        .iter()
        .filter(|d| d.code == 7027)
        .map(|d| (d.start, d.length))
        .collect()
}

/// Binds `text` with the given `allow_unreachable_code` tristate and returns
/// the count of reported `TS7027` diagnostics.
fn ts7027_count(text: &str, allow: Tristate) -> usize {
    let options = CompilerOptions {
        allow_unreachable_code: allow,
        ..Default::default()
    };
    let p = Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts", text, options,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    c.get_diagnostics(root)
        .iter()
        .filter(|d| d.code == 7027)
        .count()
}

// A statement after `throw` is unreachable -> one TS7027 spanning it.
// Go: internal/checker/checker.go:Checker.checkSourceElementUnreachable
#[test]
fn unreachable_after_throw_reports_ts7027() {
    let text = "function f() { throw new Error(); const x = 2; }";
    let spans = ts7027_spans(text);
    let start = text.find("const").unwrap() as i32;
    assert_eq!(spans, vec![(start, "const x = 2;".len() as i32)]);
}

// A statement after `break` (inside a loop) is unreachable -> one TS7027.
// Go: internal/checker/checker.go:Checker.checkSourceElementUnreachable
#[test]
fn unreachable_after_break_reports_ts7027() {
    let text = "while (true) { break; const x = 2; }";
    let spans = ts7027_spans(text);
    let start = text.find("const").unwrap() as i32;
    assert_eq!(spans, vec![(start, "const x = 2;".len() as i32)]);
}

// A statement after `continue` (inside a loop) is unreachable -> one TS7027.
// Go: internal/checker/checker.go:Checker.checkSourceElementUnreachable
#[test]
fn unreachable_after_continue_reports_ts7027() {
    let text = "while (true) { continue; const x = 2; }";
    let spans = ts7027_spans(text);
    let start = text.find("const").unwrap() as i32;
    assert_eq!(spans, vec![(start, "const x = 2;".len() as i32)]);
}

// GUARD: reachable code reports no TS7027.
#[test]
fn reachable_code_reports_no_ts7027() {
    assert!(ts7027_spans("function f() { const x = 1; return x; }").is_empty());
}

// GUARD: `allowUnreachableCode: true` suppresses the diagnostic entirely
// (Go gates the whole check on `allowUnreachableCode != true`), while
// `false` (error) and `unset` differ only in collection: `false` emits the
// error baseline diagnostic; `unset` produces a SUGGESTION that is not part of
// the error baseline (and is not modeled here), so neither `true` nor `unset`
// yields an error-category TS7027.
#[test]
fn allow_unreachable_code_true_suppresses_ts7027() {
    let text = "function f() { return 1; const x = 2; }";
    assert_eq!(ts7027_count(text, Tristate::True), 0);
    // Unset: a suggestion (separate collection, not modeled) — no error here.
    assert_eq!(ts7027_count(text, Tristate::Unknown), 0);
    // Explicit false: an error.
    assert_eq!(ts7027_count(text, Tristate::False), 1);
}

// GUARD: a maximal RUN of consecutive unreachable statements collapses into a
// SINGLE TS7027 spanning the first statement's start to the last statement's
// end (Go reports once per maximal run, not once per statement). Mirrors the
// `reachabilityChecks10` corpus shape (`throw; a; b;`).
// Go: internal/checker/checker.go:Checker.checkSourceElementUnreachable (forward scan)
#[test]
fn maximal_unreachable_run_reports_once() {
    let text = "throw new Error(\"\");\nconsole.log(\"1\");\nconsole.log(\"2\");";
    let spans = ts7027_spans(text);
    let start = text.find("console").unwrap() as i32;
    // The run ends at the end of the LAST statement (`console.log("2");`), i.e.
    // the end of the source text here.
    let end = text.len() as i32;
    assert_eq!(
        spans,
        vec![(start, end - start)],
        "the two unreachable statements collapse into one diagnostic"
    );
}

// GUARD: a non-potentially-executable statement (a `type` alias) following an
// unreachable point is NOT reported (Go's `IsPotentiallyExecutableNode`
// excludes type-only declarations), matching `reachabilityChecks11`'s
// `namespace A3` (the `type T` is silent).
#[test]
fn type_alias_in_unreachable_position_is_not_reported() {
    let text = "throw new Error(); type T = string;";
    assert!(
        ts7027_spans(text).is_empty(),
        "a type alias is not potentially executable, so no TS7027"
    );
}

// GUARD: an unreachable, UNINSTANTIATED namespace (containing only an interface)
// is NOT reported (Go's `IsInstantiatedModule` is false), while an unreachable
// INSTANTIATED one (containing a value) IS — the `isSourceElementUnreachable`
// ModuleDeclaration arm. Mirrors `reachabilityChecks11` `namespace A1` (silent)
// vs `namespace A2` (reported), exercised here on a namespace that is itself an
// unreachable top-level statement (module-body descent is deferred).
#[test]
fn uninstantiated_namespace_is_not_reported_instantiated_is() {
    let silent = "throw new Error(); namespace N { interface F {} }";
    assert!(
        ts7027_spans(silent).is_empty(),
        "an uninstantiated (type-only) namespace is not reported"
    );
    let reported = "throw new Error(); namespace N { var x = 1; }";
    assert_eq!(
        ts7027_spans(reported).len(),
        1,
        "an instantiated namespace IS reported"
    );
}

// GUARD: an unreachable non-const `enum` IS reported, but an unreachable
// `const enum` is reported ONLY when `preserveConstEnums` is set (Go's
// `isSourceElementUnreachable` EnumDeclaration arm). Mirrors
// `reachabilityChecks11` `f3` (enum, reported) / `f4` (const enum, reported
// under `@preserveConstEnums: true`).
#[test]
fn enum_unreachable_arm_respects_const_and_preserve_const_enums() {
    // Non-const enum: reported regardless of preserveConstEnums.
    assert_eq!(
        ts7027_spans("throw new Error(); enum E { X }").len(),
        1,
        "a non-const enum after an unreachable point is reported"
    );

    // const enum without preserveConstEnums: NOT reported.
    let opts = CompilerOptions {
        allow_unreachable_code: Tristate::False,
        ..Default::default()
    };
    let text = "throw new Error(); const enum E { X }";
    let p = Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts", text, opts,
    ));
    let root = p.root();
    let mut c = Checker::new_checker(p);
    assert_eq!(
        c.get_diagnostics(root)
            .iter()
            .filter(|d| d.code == 7027)
            .count(),
        0,
        "a const enum is const-enum-only -> not reported without preserveConstEnums"
    );

    // const enum WITH preserveConstEnums: reported.
    let opts2 = CompilerOptions {
        allow_unreachable_code: Tristate::False,
        preserve_const_enums: Tristate::True,
        ..Default::default()
    };
    let p2 = Rc::new(StubProgram::parse_and_bind_with_options(
        "/a.ts", text, opts2,
    ));
    let root2 = p2.root();
    let mut c2 = Checker::new_checker(p2);
    assert_eq!(
        c2.get_diagnostics(root2)
            .iter()
            .filter(|d| d.code == 7027)
            .count(),
        1,
        "a const enum IS reported under preserveConstEnums"
    );
}
