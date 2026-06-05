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

// Runs the declaration transform with a resolver and the given compiler options,
// returning the produced diagnostics as `(code, message)` pairs. Ground truth is
// captured from `tsgo --declaration --emitDeclarationOnly [--isolatedDeclarations]`.
fn check_diagnostics_opts(input: &str, isolated: bool) -> Vec<(i32, String)> {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let (ec, source_file) = parse_shared(input);
    let resolver = build_reference_resolver(input);
    let isolated_declarations = if isolated {
        Tristate::True
    } else {
        Tristate::default()
    };
    let compiler_options = CompilerOptions {
        isolated_declarations,
        ..Default::default()
    };
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        compiler_options,
    };
    let (mut tx, diags) = new_declarations_transformer_with_diagnostics(&opts, Some(resolver));
    let _ = tx.transform_source_file(source_file);
    let out = diags
        .borrow()
        .iter()
        .map(|d| (d.code, d.message.clone()))
        .collect();
    out
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

// ── D-F2 slice 1: inferred variable type → synthesized keyword ─────────────
// Go: transform.go:ensureType -> CreateTypeOfDeclaration. A non-const `let n = 1`
// widens its inferred literal to the `number` keyword type node.
// tsgo --declaration: `let n = 1;` -> `declare let n: number;`
// (RED before D-F2: `ensure_type` returned `None`, so emit was `declare let n;`).
#[test]
fn inferred_let_gets_number_annotation() {
    check_with_resolver("let n = 1;", "declare let n: number;");
}

// An exported inferred `let` keeps `export` and gains the synthesized type.
// tsgo --declaration: `export let n = 1;` -> `export declare let n: number;`.
#[test]
fn inferred_exported_let_gets_number_annotation() {
    check_with_resolver("export let n = 1;", "export declare let n: number;");
}

// ── D-F2 slice 2: literal const keeps its initializer (not a type) ─────────
// Go: transform.go:shouldPrintWithInitializer (IsLiteralConstDeclaration) ->
// ensureNoInitializer keeps the literal value; ensureType returns nil.
// tsgo --declaration: `const x = 1;` -> `declare const x = 1;`.
#[test]
fn literal_const_keeps_initializer() {
    check_with_resolver("const x = 1;", "declare const x = 1;");
    check_with_resolver("export const x = 1;", "export declare const x = 1;");
}

// A literal const string / boolean likewise keeps its initializer verbatim.
// tsgo --declaration: `const s = "a";` -> `declare const s = "a";`;
//                     `const b = true;` -> `declare const b = true;`.
#[test]
fn literal_const_string_and_boolean_keep_initializer() {
    check_with_resolver("const s = \"a\";", "declare const s = \"a\";");
    check_with_resolver("const b = true;", "declare const b = true;");
}

// ── D-F2 slice 3: inferred function return type ────────────────────────────
// Go: transform.go:ensureType -> CreateReturnTypeOfSignatureDeclaration. An
// un-annotated `function f() { return 1; }` infers the `number` return type.
// tsgo --declaration: `function f() { return 1; }` -> `declare function f(): number;`.
#[test]
fn inferred_function_return_type() {
    check_with_resolver(
        "function f() { return 1; }",
        "declare function f(): number;",
    );
    check_with_resolver(
        "export function f() { return 1; }",
        "export declare function f(): number;",
    );
}

// ── D-F2 slice 4: inferred class property type ─────────────────────────────
// Go: transform.go:transformPropertyDeclaration -> ensureType. An un-annotated
// `x = 1` field widens to `x: number`.
// tsgo --declaration: `class C { x = 1; }` -> `declare class C {\n    x: number;\n}`.
#[test]
fn inferred_class_property_type() {
    check_with_resolver("class C { x = 1; }", "declare class C {\n    x: number;\n}");
    check_with_resolver(
        "export class C { s = \"h\"; }",
        "export declare class C {\n    s: string;\n}",
    );
}

// ── D-F2 slice 5: inferred array type → `number[]` ─────────────────────────
// Go: typeToTypeNode array reference arm. `const xs = [1, 2]` (with a global
// `Array` in scope) synthesizes `number[]` (an array, not a literal const).
// tsgo --declaration: `interface Array<T> {}\nconst xs = [1, 2];`
//   -> `interface Array<T> {\n}\ndeclare const xs: number[];`.
#[test]
fn inferred_array_type_is_number_array() {
    check_with_resolver(
        "interface Array<T> {}\nconst xs = [1, 2];",
        "interface Array<T> {\n}\ndeclare const xs: number[];",
    );
}

// ── D-F2 slice 6: inferred object-literal type → type literal ──────────────
// Go: createAnonymousTypeNode. `const o = { a: 1 }` synthesizes a multiline
// `{ a: number; }` type literal (the property value widened to `number`).
// tsgo --declaration: `const o = { a: 1 };` -> `declare const o: {\n    a: number;\n};`.
#[test]
fn inferred_object_literal_type() {
    check_with_resolver(
        "const o = { a: 1 };",
        "declare const o: {\n    a: number;\n};",
    );
    check_with_resolver(
        "const o = { a: 1, b: \"x\" };",
        "declare const o: {\n    a: number;\n    b: string;\n};",
    );
}

// ── D-F2 slice 7: no regression — annotated declarations still copy through ─
// An annotated `const x: number = 1` is NOT a literal const (its type is the
// annotation `number`, not a fresh literal), so the resolver path keeps the
// annotation and strips the initializer (unchanged from D-F1).
// tsgo --declaration: `export const x: number = 1;` -> `export declare const x: number;`.
#[test]
fn annotated_const_with_resolver_keeps_annotation() {
    check_with_resolver(
        "export const x: number = 1;",
        "export declare const x: number;",
    );
    // An annotated function return is copied through (no body inference).
    check_with_resolver(
        "export function f(x: number): void {}",
        "export declare function f(x: number): void;",
    );
}

// ── D-F3 slice 1: non-exported elision in a module ─────────────────────────
// Go: transform.go:transformVariableStatement (getBindingNameVisible) +
// isDeclarationAndNotVisible. In a MODULE (the file is one because of the
// top-level `export`), a non-exported top-level `const b = 2;` is not visible
// to declaration emit and is elided; only the exported `a` survives.
// tsgo --declaration: `export const a = 1;\nconst b = 2;` -> `export declare const a = 1;`.
// (RED before D-F3: the transform emitted both `a` and `b`.)
#[test]
fn nonexported_const_is_elided_in_a_module() {
    check_with_resolver(
        "export const a = 1;\nconst b = 2;",
        "export declare const a = 1;",
    );
}

// A non-exported top-level function / class / interface in a module is likewise
// elided (Go: isDeclarationAndNotVisible -> !IsDeclarationVisible).
// tsgo --declaration: `export const a = 1;\nfunction g() {}` -> `export declare const a = 1;`.
#[test]
fn nonexported_function_is_elided_in_a_module() {
    check_with_resolver(
        "export const a = 1;\nfunction g(): void {}",
        "export declare const a = 1;",
    );
}

// ── D-F3 slice 2: a global script keeps every top-level declaration ────────
// Go: isDeclarationAndNotVisible -> IsDeclarationVisible -> IsGlobalSourceFile.
// A file with NO `import`/`export` is a global script, so a non-exported
// `const b = 2;` stays (it is a global).
// tsgo --declaration: `const b = 2;` -> `declare const b = 2;`.
#[test]
fn script_keeps_nonexported_const() {
    check_with_resolver("const b = 2;", "declare const b = 2;");
}

// A script keeps several non-exported declarations (all are globals).
// tsgo --declaration: `const b = 2;\nfunction g(): void {}` ->
//   `declare const b = 2;\ndeclare function g(): void;`.
#[test]
fn script_keeps_multiple_nonexported_declarations() {
    check_with_resolver(
        "const b = 2;\nfunction g(): void {}",
        "declare const b = 2;\ndeclare function g(): void;",
    );
}

// ── D-F3 slice 3: import kept iff referenced in an emitted type position ────
// Go: transformImportDeclaration filters named bindings by visibility; the
// reachable port uses the resolver's `is_referenced` (an import referenced in
// an emitted type annotation is kept). The unreferenced import is elided.
// tsgo --declaration:
//   `import { T } from "./m"; export const x: T = null as any;`
//     -> `import { T } from "./m";\nexport declare const x: T;`
//   `import { T } from "./m"; export const x = 1;`
//     -> `export declare const x = 1;`
#[test]
fn referenced_import_is_kept() {
    check_with_resolver(
        "import { T } from \"./m\";\nexport const x: T = null as any;",
        "import { T } from \"./m\";\nexport declare const x: T;",
    );
}

#[test]
fn unreferenced_import_is_elided() {
    check_with_resolver(
        "import { T } from \"./m\";\nexport const x = 1;",
        "export declare const x = 1;",
    );
}

// Only the referenced specifier of a multi-name import is kept.
// tsgo --declaration: `import { T, U } from "./m"; export const x: T = null as any;`
//   -> `import { T } from "./m";\nexport declare const x: T;`.
#[test]
fn only_referenced_named_import_specifier_is_kept() {
    check_with_resolver(
        "import { T, U } from \"./m\";\nexport const x: T = null as any;",
        "import { T } from \"./m\";\nexport declare const x: T;",
    );
}

// An `export { T };` re-export is kept as-is, and the import it references is
// kept (the export use marks the import referenced).
// tsgo --declaration: `import { T } from "./m"; export { T };`
//   -> `import { T } from "./m";\nexport { T };`.
#[test]
fn export_named_reexport_keeps_referenced_import() {
    check_with_resolver(
        "import { T } from \"./m\";\nexport { T };",
        "import { T } from \"./m\";\nexport { T };",
    );
}

// ── D-F3 slice 4: private-name accessibility diagnostic (4025) ─────────────
// Go: transform.go:visitDeclarationSubtree (KindTypeQuery) -> checkEntityNameVisibility
// -> tracker.handleSymbolAccessibilityError -> getVariableDeclarationTypeVisibilityDiagnosticMessage.
// `export let b: typeof a;` where `a` is a block-scoped (not visible) `var`
// references a private name in the emitted type, reported as 4025.
// tsgo --declaration: `{ var a = ""; }\nexport let b: typeof a;` ->
//   a.ts(4,22): error TS4025: Exported variable 'b' has or is using private name 'a'.
// (RED before D-F3: no diagnostics were produced.)
#[test]
fn typeof_private_name_reports_4025() {
    let diags = check_diagnostics_opts("{\n    var a = \"\";\n}\nexport let b: typeof a;", false);
    assert_eq!(
        diags,
        vec![(
            4025,
            "Exported variable 'b' has or is using private name 'a'.".to_string()
        )]
    );
}

// A visible (exported) referenced name produces no private-name diagnostic.
#[test]
fn typeof_visible_name_reports_nothing() {
    let diags = check_diagnostics_opts("export const a = \"\";\nexport let b: typeof a;", false);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

// ── D-F3 slice 5: --isolatedDeclarations explicit-return-type (9007/9008) ──
// Go: transform.go:ensureType -> CreateReturnTypeOfSignatureDeclaration ->
// tracker.ReportInferenceFallback -> getIsolatedDeclarationError ->
// createReturnTypeError. A function/method with no return-type annotation whose
// body yields no syntactically-derivable type (here: a void body, no value
// return) needs an explicit annotation.
// tsgo --declaration --isolatedDeclarations:
//   `export function noReturn() {}` -> a.ts(1,17): error TS9007: Function must
//     have an explicit return type annotation with --isolatedDeclarations.
// (RED before D-F3: no diagnostics were produced.)
#[test]
fn isolated_declarations_void_function_reports_9007() {
    let diags = check_diagnostics_opts("export function noReturn() {}", true);
    assert_eq!(
        diags,
        vec![(
            9007,
            "Function must have an explicit return type annotation with --isolatedDeclarations."
                .to_string()
        )]
    );
}

// A method with a void body (no value return, no annotation) reports 9008.
// tsgo --declaration --isolatedDeclarations: `export class C { m() {} }`
//   -> a.ts(1,18): error TS9008: Method must have an explicit return type
//      annotation with --isolatedDeclarations.
#[test]
fn isolated_declarations_void_method_reports_9008() {
    let diags = check_diagnostics_opts("export class C { m() {} }", true);
    assert_eq!(
        diags,
        vec![(
            9008,
            "Method must have an explicit return type annotation with --isolatedDeclarations."
                .to_string()
        )]
    );
}

// A function returning a primitive literal has a syntactically-derivable type,
// so no annotation is required (tsgo emits `declare function f(): number;` with
// no diagnostic under --isolatedDeclarations).
#[test]
fn isolated_declarations_literal_return_is_ok() {
    let diags = check_diagnostics_opts("export function f() { return 1; }", true);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

// An explicit return annotation suppresses the diagnostic.
#[test]
fn isolated_declarations_annotated_void_is_ok() {
    let diags = check_diagnostics_opts("export function f(): void {}", true);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

// Without --isolatedDeclarations, a void function is fine (the type is inferred).
#[test]
fn void_function_without_isolated_declarations_reports_nothing() {
    let diags = check_diagnostics_opts("export function noReturn() {}", false);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

// ── Enum declaration → ambient enum ─────────────────────────────────────────
// Go: transform.go:transformEnumDeclaration. An exported enum is kept with
// `declare` added; member initializers are kept (they are compile-time constants).
// tsgo --declaration: `export enum Color { Red, Green, Blue }`
//   → `export declare enum Color {\n    Red,\n    Green,\n    Blue\n}`.
#[test]
fn enum_declaration_becomes_declare_enum() {
    check(
        "export enum Color { Red, Green, Blue }",
        "export declare enum Color {\n    Red,\n    Green,\n    Blue\n}",
    );
}

// A non-exported enum gains `declare`.
#[test]
fn nonexported_enum_gains_declare() {
    check(
        "enum Dir { Up, Down }",
        "declare enum Dir {\n    Up,\n    Down\n}",
    );
}

// Enum members with explicit initializers keep their values.
#[test]
fn enum_members_with_initializers_keep_values() {
    check(
        "export enum Status { Active = 1, Inactive = 0 }",
        "export declare enum Status {\n    Active = 1,\n    Inactive = 0\n}",
    );
}

// ── Namespace declaration → ambient namespace ──────────────────────────────
// Go: transform.go:transformModuleDeclaration. A namespace is kept with
// `declare` added and body statements recursively transformed.
#[test]
fn namespace_declaration_becomes_declare_namespace() {
    check(
        "export namespace Utils { export function id(x: number): number { return x; } }",
        "export declare namespace Utils {\n    export function id(x: number): number;\n}",
    );
}

// A non-exported namespace gains `declare`.
#[test]
fn nonexported_namespace_gains_declare() {
    check(
        "namespace Internal { export const x: number = 1; }",
        "declare namespace Internal {\n    export const x: number;\n}",
    );
}

// A namespace with multiple members keeps each transformed member.
#[test]
fn namespace_multiple_members() {
    check(
        "export namespace NS { export const x: number = 1; export function f(y: string): void {} export interface I { a: number; } }",
        "export declare namespace NS {\n    export const x: number;\n    export function f(y: string): void;\n    export interface I {\n        a: number;\n    }\n}",
    );
}

// ── isolatedDeclarations: explicit types → clean ────────────────────────────
// Under --isolatedDeclarations, a function with ALL explicit type annotations
// (parameters + return type) produces zero diagnostics.
#[test]
fn isolated_declarations_explicit_types_clean_dts() {
    let diags = check_diagnostics_opts(
        "export function add(a: number, b: number): number { return a + b; }",
        true,
    );
    assert!(
        diags.is_empty(),
        "explicit types should produce no isolated-declarations diagnostics: {diags:?}"
    );
}

// Under --isolatedDeclarations, a class with explicit property types and method
// return types produces zero diagnostics.
#[test]
fn isolated_declarations_class_with_explicit_types_clean() {
    let diags = check_diagnostics_opts(
        "export class Calc { value: number = 0; add(x: number): number { return this.value + x; } }",
        true,
    );
    assert!(
        diags.is_empty(),
        "class with explicit types should produce no diagnostics: {diags:?}"
    );
}

// Under --isolatedDeclarations, a multi-statement void body reports 9007.
#[test]
fn isolated_declarations_void_multi_statement_reports_9007() {
    let diags = check_diagnostics_opts(
        "export function setup() { console.log('init'); console.log('done'); }",
        true,
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].0, 9007);
}

// Under --isolatedDeclarations, a method with a void body reports 9008.
#[test]
fn isolated_declarations_method_no_return_reports_9008() {
    let diags = check_diagnostics_opts(
        "export class Logger { log() { console.log('msg'); } }",
        true,
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].0, 9008);
}

// ── T4-終 slice 1: generic type alias → type params preserved ───────────────
// Go: transform.go:transformTypeAliasDeclaration. A generic type alias preserves
// its type parameter list and aliased type.
// tsgo --declaration: `export type Result<T, E> = { ok: T } | { err: E };`
//   → `export type Result<T, E> = {\n    ok: T;\n} | {\n    err: E;\n};`
#[test]
fn generic_type_alias_preserves_type_params() {
    check(
        "export type Result<T, E> = { ok: T } | { err: E };",
        "export type Result<T, E> = {\n    ok: T;\n} | {\n    err: E;\n};",
    );
}

// A constrained generic type alias keeps the `extends` constraint.
// tsgo --declaration: `export type Boxed<T extends object> = { value: T };`
#[test]
fn generic_type_alias_with_constraint() {
    check(
        "export type Boxed<T extends object> = { value: T };",
        "export type Boxed<T extends object> = {\n    value: T;\n};",
    );
}

// ── T4-終 slice 2: variable declaration — `let`/`var` with annotation ───────
// Go: transform.go:transformVariableStatement. A `let` gains `declare` and the
// annotation is preserved (initializer stripped).
// tsgo --declaration: `export let x: number = 1;` → `export declare let x: number;`
#[test]
fn let_declaration_with_annotation() {
    check("export let x: number = 1;", "export declare let x: number;");
}

// A `var` also gains `declare` with annotation preserved.
// tsgo --declaration: `var y: string = "hello";` → `declare var y: string;`
#[test]
fn var_declaration_with_annotation() {
    check("var y: string = \"hello\";", "declare var y: string;");
}

// ── T4-終 slice 3: import re-export from module preserved ──────────────────
// Go: transform.go:transformExportDeclaration (ExportDeclaration arm).
// `export { Foo } from "./bar";` is preserved as-is in the .d.ts.
// tsgo --declaration: kept verbatim (the module specifier is not rewritten in
// this reachable subset).
#[test]
fn import_reexport_from_module_is_preserved() {
    check(
        "export { Foo } from \"./bar\";",
        "export { Foo } from \"./bar\";",
    );
}

// A type-only re-export is also preserved.
// tsgo --declaration: `export type { Bar } from "./baz";` → preserved.
#[test]
fn type_only_reexport_from_module_is_preserved() {
    check(
        "export type { Bar } from \"./baz\";",
        "export type { Bar } from \"./baz\";",
    );
}

// A star re-export is preserved.
// tsgo --declaration: `export * from "./mod";` → preserved.
#[test]
fn star_reexport_from_module_is_preserved() {
    check("export * from \"./mod\";", "export * from \"./mod\";");
}

// ── T4-終 slice 4: default export function / class ─────────────────────────
// Go: transform.go:ensureModifierFlags / maskModifierFlags. A `default` export
// keeps `export default` (no `declare` added alongside `default`).
// tsgo --declaration: `export default function foo(): void {}` →
//   `export default function foo(): void;`
#[test]
fn export_default_function_becomes_signature() {
    check(
        "export default function foo(): void {}",
        "export default function foo(): void;",
    );
}

// An anonymous default-exported function also works. The printer inserts a
// space between `function` and `()` even when no name is present.
// tsgo --declaration: `export default function(): void {}` →
//   `export default function (): void;`
#[test]
fn export_default_anonymous_function() {
    check(
        "export default function(): void {}",
        "export default function (): void;",
    );
}

// A default-exported class is kept without `declare`.
// tsgo --declaration: `export default class C { x: number = 1; }` →
//   `export default class C {\n    x: number;\n}`
#[test]
fn export_default_class() {
    check(
        "export default class C { x: number = 1; }",
        "export default class C {\n    x: number;\n}",
    );
}

// ── T4-終 slice 5: overloaded functions — signatures preserved (bare) ──────
// Without a resolver, all function signatures (overloads + implementation)
// are kept — the resolver is needed to distinguish the implementation from
// the declarations. The bare path keeps everything as ambient signatures.
// tsgo --declaration (no overload elision without resolver): each signature
// is kept.
#[test]
fn overloaded_functions_all_signatures_kept_bare() {
    check(
        "function f(x: number): number;\nfunction f(x: string): string;\nfunction f(x: any): any { return x; }",
        "declare function f(x: number): number;\ndeclare function f(x: string): string;\ndeclare function f(x: any): any;",
    );
}

// ── T4-終 slice 6: generic interface with constraint ───────────────────────
// Go: transform.go:transformInterfaceDeclaration. An interface with constrained
// type params preserves them.
// tsgo --declaration: `export interface Container<T extends object> { value: T; }`
#[test]
fn generic_interface_with_constraint() {
    check(
        "export interface Container<T extends object> { value: T; }",
        "export interface Container<T extends object> {\n    value: T;\n}",
    );
}

// A generic class with a constraint preserves type parameters.
// tsgo --declaration: `export class Box<T extends string> { value: T; constructor(v: T) { this.value = v; } }`
#[test]
fn generic_class_with_constraint() {
    check(
        "export class Box<T extends string> { value: T; constructor(v: T) { this.value = v; } }",
        "export declare class Box<T extends string> {\n    value: T;\n    constructor(v: T);\n}",
    );
}

// A function with multiple constrained type params.
// tsgo --declaration: `export function merge<T extends object, U extends object>(a: T, b: U): T & U { ... }`
#[test]
fn generic_function_with_multiple_constraints() {
    check(
        "export function merge<T extends object, U extends object>(a: T, b: U): T & U { return Object.assign(a, b); }",
        "export declare function merge<T extends object, U extends object>(a: T, b: U): T & U;",
    );
}

// The 9007 diagnostic carries a 9031 "add a return type" related suggestion.
#[test]
fn isolated_declarations_9007_has_related_suggestion() {
    use tsgo_core::compileroptions::CompilerOptions;
    use tsgo_core::tristate::Tristate;
    let input = "export function noReturn() {}";
    let (ec, source_file) = parse_shared(input);
    let resolver = build_reference_resolver(input);
    let compiler_options = CompilerOptions {
        isolated_declarations: Tristate::True,
        ..Default::default()
    };
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        compiler_options,
    };
    let (mut tx, diags) = new_declarations_transformer_with_diagnostics(&opts, Some(resolver));
    let _ = tx.transform_source_file(source_file);
    let diags = diags.borrow();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 9007);
    assert_eq!(diags[0].related_information.len(), 1);
    assert_eq!(diags[0].related_information[0].code, 9031);
    assert_eq!(
        diags[0].related_information[0].message,
        "Add a return type to the function declaration."
    );
}

// ── T4-終 slice 8: combined / edge cases ───────────────────────────────────

// A generic default-exported function: `export default` + type parameters.
// tsgo --declaration: `export default function identity<T>(x: T): T { return x; }`
//   → `export default function identity<T>(x: T): T;`
#[test]
fn generic_default_export_function() {
    check(
        "export default function identity<T>(x: T): T { return x; }",
        "export default function identity<T>(x: T): T;",
    );
}

// A file with a mix of re-exports, type aliases, and variable declarations.
// tsgo --declaration: combined output.
#[test]
fn mixed_file_with_reexports_types_and_vars() {
    check(
        "export type ID = string;\nexport const VERSION: number = 1;\nexport { Foo } from \"./foo\";",
        "export type ID = string;\nexport declare const VERSION: number;\nexport { Foo } from \"./foo\";",
    );
}

// An `export * as ns from "./mod"` namespace re-export is preserved.
// tsgo --declaration: kept verbatim.
#[test]
fn namespace_reexport_star_as() {
    check(
        "export * as utils from \"./utils\";",
        "export * as utils from \"./utils\";",
    );
}

// A `const enum` (enum with `const` modifier) keeps its `const` modifier.
// tsgo --declaration: `export const enum Direction { Up, Down }`
#[test]
fn const_enum_preserves_const_modifier() {
    check(
        "export const enum Direction { Up, Down }",
        "export declare const enum Direction {\n    Up,\n    Down\n}",
    );
}

// An abstract class preserves the `abstract` modifier.
// tsgo --declaration: `export abstract class Base { abstract m(): void; }`
#[test]
fn abstract_class_preserves_modifier() {
    check(
        "export abstract class Base { abstract m(): void; }",
        "export declare abstract class Base {\n    abstract m(): void;\n}",
    );
}
