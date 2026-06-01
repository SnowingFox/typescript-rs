use super::*;
use crate::test_support::{build_reference_resolver, emit, parse_shared};
use crate::TransformOptions;
use std::rc::Rc;

// Runs the declaration transform over `input` (no resolver) and asserts the
// emitted `.d.ts` text equals `expected`. Ground truth is captured from
// `tsgo --declaration --emitDeclarationOnly`.
fn check(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let mut tx = new_declarations_transformer(&opts, None);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "declaration({input:?})");
}

// Like [`check`] but wires a scope-correct [`EmitReferenceResolver`] built from
// the same source (for the resolver-driven overload elision).
fn check_with_resolver(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let resolver = build_reference_resolver(input);
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    let mut tx = new_declarations_transformer(&opts, Some(resolver));
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "declaration({input:?})");
}

// ── Slice 1: function declaration → ambient signature ──────────────────────
// Go: transform.go:DeclarationTransformer.transformFunctionDeclaration
// tsgo --declaration: `function f(x: number): void {}` -> `declare function f(x: number): void;`
#[test]
fn function_declaration_becomes_declare_signature() {
    check(
        "function f(x: number): void { console.log(x); }",
        "declare function f(x: number): void;",
    );
}

// ── Slice 2: exported function → `export declare` ──────────────────────────
// Go: transform.go:ensureModifierFlags (export preserved, ambient added)
// tsgo --declaration: `export function f(x: number): void {}` -> `export declare function f(x: number): void;`
#[test]
fn exported_function_becomes_export_declare_signature() {
    check(
        "export function f(x: number): void {}",
        "export declare function f(x: number): void;",
    );
}

// Optional / rest / default parameters keep their annotated types; a default
// initializer becomes an optional `?` (Go's `ensureParameter` /
// `isOptionalParameter`).
// tsgo --declaration ground truth captured per case.
#[test]
fn function_parameter_forms_are_preserved() {
    check(
        "export function f(x?: number): void {}",
        "export declare function f(x?: number): void;",
    );
    check(
        "export function f(...args: number[]): void {}",
        "export declare function f(...args: number[]): void;",
    );
    check(
        "export function f(x: number = 5): void {}",
        "export declare function f(x?: number): void;",
    );
}

// Generic type parameters are kept on the emitted signature.
// tsgo --declaration: `export function f<T>(x: T): T { return x; }`
#[test]
fn function_type_parameters_are_kept() {
    check(
        "export function f<T>(x: T): T { return x; }",
        "export declare function f<T>(x: T): T;",
    );
}

// `async` is dropped from declaration emit; the `Promise<void>` return
// annotation is kept.
// tsgo --declaration: `export async function f(): Promise<void> {}`
#[test]
fn async_modifier_is_dropped() {
    check(
        "export async function f(): Promise<void> {}",
        "export declare function f(): Promise<void>;",
    );
}

// ── Slice 3: variable statement → declared variable ────────────────────────
// Go: transform.go:transformVariableStatement / transformVariableDeclaration
// tsgo --declaration: `export const x: number = 1;` -> `export declare const x: number;`
#[test]
fn exported_const_drops_initializer_keeps_annotation() {
    check(
        "export const x: number = 1;",
        "export declare const x: number;",
    );
}

// A non-exported top-level const still becomes `declare const` (the `const`
// keyword is preserved from the declaration-list flags).
// tsgo --declaration: `const x: number = 1;` -> `declare const x: number;`
#[test]
fn nonexported_const_becomes_declare_const() {
    check("const x: number = 1;", "declare const x: number;");
}

// Multiple declarators in one statement keep each annotation, drop each
// initializer.
// tsgo --declaration: `export const a: number = 1, b: string = "x";`
#[test]
fn multiple_declarators_each_keep_annotation() {
    check(
        "export const a: number = 1, b: string = \"x\";",
        "export declare const a: number, b: string;",
    );
}

// ── Slice 4: class declaration → ambient class ─────────────────────────────
// Go: transform.go:transformClassDeclaration + member handlers
// tsgo --declaration: member initializers/bodies stripped, annotations kept.
#[test]
fn class_declaration_becomes_ambient_class() {
    check(
        "export class C { x: number = 1; m(): number { return this.x; } }",
        "export declare class C {\n    x: number;\n    m(): number;\n}",
    );
}

// A non-exported class gains `declare`.
// tsgo --declaration: `class C { x: number = 1; }`
#[test]
fn nonexported_class_gains_declare() {
    check(
        "class C { x: number = 1; }",
        "declare class C {\n    x: number;\n}",
    );
}

// `private` members emit name-only (no type, no initializer); `public` is
// dropped; `static`/`readonly` are kept.
// tsgo --declaration ground truth captured per case.
#[test]
fn class_member_modifiers_and_visibility() {
    check(
        "export class C { private p: number = 1; public q: string = \"a\"; m(): void {} }",
        "export declare class C {\n    private p;\n    q: string;\n    m(): void;\n}",
    );
    check(
        "export class C { static m(x: number): void {} }",
        "export declare class C {\n    static m(x: number): void;\n}",
    );
    check(
        "export class C { readonly x: number = 1; }",
        "export declare class C {\n    readonly x: number;\n}",
    );
}

// A constructor keeps its parameter signature (body removed); parameter
// properties are hoisted to fields before the constructor.
// tsgo --declaration ground truth captured per case.
#[test]
fn class_constructor_and_parameter_properties() {
    check(
        "export class C { constructor(x: number) {} y: number = 2; }",
        "export declare class C {\n    constructor(x: number);\n    y: number;\n}",
    );
    check(
        "export class C { constructor(public x: number, private y: string) {} }",
        "export declare class C {\n    x: number;\n    private y;\n    constructor(x: number, y: string);\n}",
    );
}

// Get/set accessors keep their signatures (bodies removed).
// tsgo --declaration: `get x(): number {...} set x(v: number) {}`
#[test]
fn class_accessors_keep_signatures() {
    check(
        "export class C { get x(): number { return 1; } set x(v: number) {} }",
        "export declare class C {\n    get x(): number;\n    set x(v: number);\n}",
    );
}

// ── Slice 5: interface passthrough ─────────────────────────────────────────
// Go: transform.go:transformInterfaceDeclaration (no `declare`, members kept)
// tsgo --declaration: `interface I { a: number; }`
#[test]
fn interface_passes_through() {
    check(
        "interface I { a: number; }",
        "interface I {\n    a: number;\n}",
    );
    check(
        "interface I { m(x: number): void; p: string; }",
        "interface I {\n    m(x: number): void;\n    p: string;\n}",
    );
    check(
        "interface I extends J { a: number; }",
        "interface I extends J {\n    a: number;\n}",
    );
}

// ── Slice 6: type-alias passthrough ────────────────────────────────────────
// Go: transform.go:transformTypeAliasDeclaration (no `declare`)
// tsgo --declaration: `type T = number;`
#[test]
fn type_alias_passes_through() {
    check("type T = number;", "type T = number;");
    check(
        "export type T = { a: number; b: string };",
        "export type T = {\n    a: number;\n    b: string;\n};",
    );
}

// ── Slice 7: modifier idempotence (`declare` added once) ───────────────────
// Go: ensureModifierFlags — an already-ambient declaration is not doubly
// `declare`'d; a top-level declaration gets exactly one.
// tsgo --declaration: `declare function f(x: number): void;` is unchanged.
#[test]
fn declare_is_added_once_not_doubled() {
    check(
        "declare function f(x: number): void;",
        "declare function f(x: number): void;",
    );
    // A bodyless top-level function (an overload signature with no impl) is a
    // signature kept as `declare function`.
    check(
        "function f(x: number): void {}",
        "declare function f(x: number): void;",
    );
}

// Two statements in one file are each transformed independently.
// tsgo --declaration: const + function.
#[test]
fn multiple_top_level_statements() {
    check(
        "export const x: number = 1;\nexport function f(y: string): void {}",
        "export declare const x: number;\nexport declare function f(y: string): void;",
    );
}

// ── Slice 8: overload-implementation elision via the resolver ──────────────
// Go: transformTopLevelDeclaration — `IsImplementationOfOverload` elides the
// body-bearing implementation; the bodyless overload signatures are kept.
// tsgo --declaration ground truth captured.
#[test]
fn overload_implementation_is_elided_via_resolver() {
    check_with_resolver(
        "export function f(x: number): number;\nexport function f(x: string): string;\nexport function f(x: any): any { return x; }",
        "export declare function f(x: number): number;\nexport declare function f(x: string): string;",
    );
}

// Without a resolver, a lone implementation function (single declaration) is
// still kept as a signature (it is not an overload implementation).
#[test]
fn lone_function_with_body_is_kept_as_signature() {
    check(
        "export function f(x: number): number { return x; }",
        "export declare function f(x: number): number;",
    );
}
