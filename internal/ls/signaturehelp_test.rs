use super::*;

use tsgo_core::scriptkind::ScriptKind;
use tsgo_lsproto::Position;
use tsgo_parser::{parse_source_file, SourceFileParseOptions};

use crate::test_support::build_service;

// Go: internal/ls/signaturehelp.go:GetSignatureHelpItems — inside the empty
// argument list of `f(|)`, signature help shows the one signature
// `f(a: number, b: string): void` with the active parameter at index 0.
#[test]
fn provide_signature_help_basic_empty_arg_list() {
    let src = "function f(a: number, b: string): void {}\nf()";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    // `f()` is on line 1; the cursor sits right after the `(` at character 2.
    let help = ls
        .provide_signature_help(
            "/m.ts",
            Position {
                line: 1,
                character: 2,
            },
        )
        .expect("signature help for `f(|)`");
    assert_eq!(help.signatures.len(), 1);
    assert_eq!(help.signatures[0].label, "f(a: number, b: string): void");
    assert_eq!(help.active_signature, Some(0));
    assert_eq!(help.active_parameter, Some(0));
}

// Go: internal/ls/signaturehelp.go:getArgumentIndexOrCount — after the first
// comma (`f(1, |)`), the active parameter advances to index 1.
#[test]
fn provide_signature_help_active_parameter_after_comma() {
    let src = "function f(a: number, b: string): void {}\nf(1, )";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    // `f(1, )` is on line 1; the cursor sits just before `)` at character 5.
    let help = ls
        .provide_signature_help(
            "/m.ts",
            Position {
                line: 1,
                character: 5,
            },
        )
        .expect("signature help for `f(1, |)`");
    assert_eq!(help.signatures[0].label, "f(a: number, b: string): void");
    assert_eq!(help.active_parameter, Some(1));
}

// Go: internal/ls/signaturehelp.go:getArgumentIndexOrCount — with the cursor
// inside the second argument (`f(1, "x"|)`), the active parameter is index 1.
#[test]
fn provide_signature_help_active_parameter_inside_second_arg() {
    let src = "function f(a: number, b: string): void {}\nf(1, \"x\")";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    // `f(1, "x")` is on line 1; the cursor sits just after `"x"` at character 8.
    let help = ls
        .provide_signature_help(
            "/m.ts",
            Position {
                line: 1,
                character: 8,
            },
        )
        .expect("signature help for `f(1, \"x\"|)`");
    assert_eq!(help.active_parameter, Some(1));
}

// Go: internal/ls/signaturehelp.go:itemInfoForParameters /
// createSignatureHelpParameterFromLabel — each parameter is one
// `ParameterInformation` whose label is the `name: type` substring of the
// signature label.
#[test]
fn provide_signature_help_parameter_labels() {
    let src = "function f(a: number, b: string): void {}\nf()";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    let help = ls
        .provide_signature_help(
            "/m.ts",
            Position {
                line: 1,
                character: 2,
            },
        )
        .expect("signature help for `f(|)`");
    let params = &help.signatures[0].parameters;
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].label, "a: number");
    assert_eq!(params[1].label, "b: string");
    // Each parameter label is a substring of the full signature label (the LSP
    // contract for string-form `ParameterInformation.label`).
    let signature_label = &help.signatures[0].label;
    assert!(signature_label.contains(&params[0].label));
    assert!(signature_label.contains(&params[1].label));
}

// A `new C(|)` on a class currently yields no help: a class value symbol's type
// is its instance type (no call signatures), and the only public checker API
// returns call — not construct — signatures, so construct signatures are
// DEFERRED. The structural call detection must still resolve to `None` (never a
// panic).
// Go: internal/ls/signaturehelp.go:getCandidateOrTypeInfo (construct signatures)
#[test]
fn provide_signature_help_new_expression_is_none_without_construct_signatures() {
    let src = "class C { constructor(a: number) {} }\nnew C()";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    // `new C()` is on line 1; the cursor sits right after the `(` at character 6.
    assert!(ls
        .provide_signature_help(
            "/m.ts",
            Position {
                line: 1,
                character: 6,
            },
        )
        .is_none());
}

// A cursor on the call target itself (`f|(1)`, before the `(`) is not inside the
// argument list, so there is no signature help (Go's `findContainingList`
// returning nil for the call target).
// Go: internal/ls/signaturehelp.go:getArgumentOrParameterListAndIndex (findContainingList nil)
#[test]
fn provide_signature_help_on_call_target_is_none() {
    let src = "function f(a: number): void {}\nf(1)";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    // The cursor sits right after `f`, before `(`, at line 1 character 1.
    assert!(ls
        .provide_signature_help(
            "/m.ts",
            Position {
                line: 1,
                character: 1,
            },
        )
        .is_none());
}

// A position with no enclosing call yields no signature help (and never panics).
// Go: internal/ls/signaturehelp.go:GetSignatureHelpItems (argumentInfo == nil)
#[test]
fn provide_signature_help_outside_any_call_is_none() {
    let src = "const x = 1;\nx";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    // The `x` use is on line 1 at character 1.
    assert!(ls
        .provide_signature_help(
            "/m.ts",
            Position {
                line: 1,
                character: 1,
            },
        )
        .is_none());
}

// An unknown file yields no signature help (no panic).
// Go: internal/ls/signaturehelp.go:getProgramAndFile (missing file)
#[test]
fn provide_signature_help_unknown_file_is_none() {
    let mut ls = build_service(
        &[("/m.ts", "function f(a: number): void {}\nf(1)")],
        "/",
        &["/m.ts"],
    );
    assert!(ls
        .provide_signature_help(
            "/missing.ts",
            Position {
                line: 0,
                character: 0
            }
        )
        .is_none());
}

// A no-parameter function renders `h(): void` with an empty parameter list and
// the active parameter at index 0.
// Go: internal/ls/signaturehelp.go:itemInfoForParameters (empty parameter list)
#[test]
fn provide_signature_help_no_parameters() {
    let src = "function h(): void {}\nh()";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    let help = ls
        .provide_signature_help(
            "/m.ts",
            Position {
                line: 1,
                character: 2,
            },
        )
        .expect("signature help for `h(|)`");
    assert_eq!(help.signatures.len(), 1);
    assert_eq!(help.signatures[0].label, "h(): void");
    assert!(help.signatures[0].parameters.is_empty());
    assert_eq!(help.active_parameter, Some(0));
}

// A method call (`o.m(|)`) resolves through a property-access callee: the
// call-target name is the method `m`, so the label is `m(a: number): void`.
// Go: internal/ls/signaturehelp.go:createSignatureHelpItems (getExpressionFromInvocation property access)
#[test]
fn provide_signature_help_property_access_callee() {
    let src = "interface I { m(a: number): void; }\ndeclare const o: I;\no.m()";
    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    // `o.m()` is on line 2; the cursor sits right after the `(` at character 4.
    let help = ls
        .provide_signature_help(
            "/m.ts",
            Position {
                line: 2,
                character: 4,
            },
        )
        .expect("signature help for `o.m(|)`");
    assert_eq!(help.signatures.len(), 1);
    assert_eq!(help.signatures[0].label, "m(a: number): void");
    assert_eq!(help.signatures[0].parameters.len(), 1);
    assert_eq!(help.signatures[0].parameters[0].label, "a: number");
    assert_eq!(help.active_parameter, Some(0));
}

// Unit: `active_parameter_index` counts the arguments fully completed before the
// position. With `f(1, 22, 333)`, the cursor advances one index per completed
// argument.
// Go: internal/ls/signaturehelp.go:getArgumentIndexOrCount
#[test]
fn active_parameter_index_counts_completed_arguments() {
    let src = "f(1, 22, 333)";
    let result = parse_source_file(SourceFileParseOptions::default(), src, ScriptKind::Ts);
    let nav =
        NavSourceFile::from_borrowed_arena(&result.arena, result.source_file, src.to_string());
    let call = find_enclosing_call(&nav, 2).expect("the enclosing call");
    let (_callee, args) = call_parts(nav.arena(), call).expect("the argument list");
    let nodes = &args.nodes;
    // Argument end offsets: `1` -> 3, `22` -> 7, `333` -> 12.
    assert_eq!(active_parameter_index(nav.arena(), nodes, 2), 0); // inside `1`
    assert_eq!(active_parameter_index(nav.arena(), nodes, 3), 0); // at end of `1`
    assert_eq!(active_parameter_index(nav.arena(), nodes, 5), 1); // inside `22`
    assert_eq!(active_parameter_index(nav.arena(), nodes, 9), 2); // inside `333`
    assert_eq!(active_parameter_index(nav.arena(), nodes, 12), 2); // at end of `333`

    // Past every argument (a trailing-comma empty slot) -> the argument count.
    assert_eq!(active_parameter_index(nav.arena(), nodes, 13), 3);
}
