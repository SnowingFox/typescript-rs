use super::*;
use crate::test_support::{build_reference_resolver, emit, parse_shared};
use std::rc::Rc;
use tsgo_core::tristate::Tristate;

// Runs the legacy-decorators transformer over `input` under
// `--experimentalDecorators` (no metadata, no resolver) and asserts the emitted
// JS.
fn check_legacy(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.experimental_decorators = Tristate::True;
    let mut tx = new_legacy_decorators_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "legacy({input:?})");
}

// Runs the legacy-decorators transformer over `input` under
// `--experimentalDecorators --emitDecoratorMetadata` with a scope-correct
// reference resolver (built from the same source, node ids aligned) and asserts
// the emitted JS. Drives `new_legacy_decorators_transformer_with_resolver`.
fn check_legacy_metadata(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let resolver = build_reference_resolver(input);
    let mut opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    opts.compiler_options.experimental_decorators = Tristate::True;
    opts.compiler_options.emit_decorator_metadata = Tristate::True;
    let mut tx = new_legacy_decorators_transformer_with_resolver(&opts, resolver);
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "legacy_meta({input:?})");
}

// Go: internal/transformers/tstransforms/legacydecorators.go:generateClassElementDecorationExpression
// Verified against Go / `tsc --experimentalDecorators`:
//   class C { @dec x: number; }
//   =>
//   class C { x; }
//   __decorate([dec], C.prototype, "x", void 0);
// (the decorator array is single-line here because the Rust printer always
// emits array literals inline; Go/tsc emit it multi-line.)
//
// Tracer bullet: an instance property decorator lowers to a trailing
// `__decorate([dec], C.prototype, "x", void 0);` statement, the property's
// decorators (and type annotation) are stripped, and the `__decorate` helper is
// emitted once in the module prologue. No `--emitDecoratorMetadata`, so no
// `__metadata` decorator is appended.
#[test]
fn instance_property_decorator_lowers_to_decorate_call() {
    check_legacy(
        "class C { @dec x: number; }",
        &format!(
            "{}\nclass C {{\n    x;\n}}\n__decorate([dec], C.prototype, \"x\", void 0);",
            DECORATE_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:generateClassElementDecorationExpression
// (the `descriptor` is `NewKeywordExpression(KindNullKeyword)` for a method member,
// vs `void 0` for a property — verified against the `else` branch).
// Verified against Go / `tsc --experimentalDecorators`:
//   class C { @dec m() {} }
//   =>
//   class C { m() { } }
//   __decorate([dec], C.prototype, "m", null);
//
// Tracer bullet (6ao): an instance *method* decorator lowers to a trailing
// `__decorate([dec], C.prototype, "m", null);` statement — the 4th argument is
// `null` (not the property's `void 0`), the method's decorator is stripped, and
// the `__decorate` helper is emitted once in the prologue. No metadata.
#[test]
fn instance_method_decorator_lowers_to_decorate_call() {
    check_legacy(
        "class C { @dec m() {} }",
        &format!(
            "{}\nclass C {{\n    m() {{ }}\n}}\n__decorate([dec], C.prototype, \"m\", null);",
            DECORATE_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:getClassMemberPrefix
// (`ast.IsStatic(member)` -> `GetDeclarationName(node)` i.e. `C`, applied to a
// *method* member just as for a property).
// Verified against Go / `tsc --experimentalDecorators`:
//   class C { @dec static m() {} }
//   =>
//   class C { static m() { } }
//   __decorate([dec], C, "m", null);
//
// A static method decorates the class constructor directly: the `__decorate`
// target is the bare class name `C` (not `C.prototype`), and the `static`
// modifier is kept on the stripped method.
#[test]
fn static_method_decorator_uses_class_name_prefix() {
    check_legacy(
        "class C { @dec static m() {} }",
        &format!(
            "{}\nclass C {{\n    static m() {{ }}\n}}\n__decorate([dec], C, \"m\", null);",
            DECORATE_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/metadata.go:injectClassElementTypeMetadata
// + typeserializer.go:serializeTypeNode (KindNumberKeyword -> "Number"), consumed
// by legacydecorators.go:transformAllDecoratorsOfDeclaration (metadata last).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   class C { @dec x: number; }
//   =>
//   class C { x; }
//   __decorate([dec, __metadata("design:type", Number)], C.prototype, "x", void 0);
//
// Headline (checker integration): with `--emitDecoratorMetadata`, a `design:type`
// metadata decorator is appended *after* the real decorators. `Number` is
// produced by the checker's `serialize_type_node_for_metadata` over the `:
// number` annotation (4at), threaded through the `EmitReferenceResolver`
// passthrough. Both the `__decorate` and `__metadata` helpers are emitted in the
// module prologue (`__decorate` priority 2 before `__metadata` priority 3).
#[test]
fn property_decorator_emits_design_type_metadata() {
    check_legacy_metadata(
        "class C { @dec x: number; }",
        &format!(
            "{}\n{}\nclass C {{\n    x;\n}}\n__decorate([dec, __metadata(\"design:type\", Number)], C.prototype, \"x\", void 0);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:getClassMemberPrefix
// (`ast.IsStatic(member)` -> `GetDeclarationName(node)` i.e. `C`, not the
// `C.prototype` used for instance members).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   class C { @dec static x: number; }
//   =>
//   class C { static x; }
//   __decorate([dec, __metadata("design:type", Number)], C, "x", void 0);
//
// A static member decorates the class constructor directly: the `__decorate`
// target is the bare class name `C`, and the `static` modifier is kept on the
// stripped property.
#[test]
fn static_property_decorator_uses_class_name_prefix() {
    check_legacy_metadata(
        "class C { @dec static x: number; }",
        &format!(
            "{}\n{}\nclass C {{\n    static x;\n}}\n__decorate([dec, __metadata(\"design:type\", Number)], C, \"x\", void 0);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: typeserializer.go:serializeTypeNode (KindStringKeyword -> "String").
// Coverage (generalization of the `Number` headline): the `design:type`
// constructor is whatever the checker's `serialize_type_node_for_metadata`
// returns, not a hard-coded `Number` — a `: string` annotation serializes to
// `String`. Green-on-arrival once the metadata path (slice 2) maps the full
// `SerializedTypeNode` enum, so this guards against a `Number`-only regression
// rather than driving new code.
#[test]
fn string_typed_property_serializes_to_string_constructor() {
    check_legacy_metadata(
        "class C { @dec x: string; }",
        &format!(
            "{}\n{}\nclass C {{\n    x;\n}}\n__decorate([dec, __metadata(\"design:type\", String)], C.prototype, \"x\", void 0);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: typeserializer.go:serializeTypeNode (`case KindArrayType, KindTupleType ->
// NewIdentifier("Array")`).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   class C { @dec x: number[]; }
//   =>
//   class C { x; }
//   __decorate([dec, __metadata("design:type", Array)], C.prototype, "x", void 0);
//
// An array-typed property's `design:type` is the global `Array` constructor:
// the checker (round 4av) maps `ArrayType` -> `SerializedTypeNode::Array`,
// threaded through `serialized_type_to_expression`.
#[test]
fn array_typed_property_serializes_to_array_constructor() {
    check_legacy_metadata(
        "class C { @dec x: number[]; }",
        &format!(
            "{}\n{}\nclass C {{\n    x;\n}}\n__decorate([dec, __metadata(\"design:type\", Array)], C.prototype, \"x\", void 0);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: typeserializer.go:serializeTypeNode (`case KindArrayType, KindTupleType ->
// NewIdentifier("Array")` — the tuple type is grouped with the array type).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   class C { @dec x: [number, string]; }
//   =>
//   __decorate([dec, __metadata("design:type", Array)], C.prototype, "x", void 0);
//
// A tuple-typed property's `design:type` is also the global `Array` constructor
// (checker 4av maps `TupleType` -> `SerializedTypeNode::Array`).
#[test]
fn tuple_typed_property_serializes_to_array_constructor() {
    check_legacy_metadata(
        "class C { @dec x: [number, string]; }",
        &format!(
            "{}\n{}\nclass C {{\n    x;\n}}\n__decorate([dec, __metadata(\"design:type\", Array)], C.prototype, \"x\", void 0);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: typeserializer.go:serializeTypeNode (`case KindFunctionType,
// KindConstructorType -> NewIdentifier("Function")`).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   class C { @dec x: () => void; }
//   =>
//   class C { x; }
//   __decorate([dec, __metadata("design:type", Function)], C.prototype, "x", void 0);
//
// A function-typed property's `design:type` is the global `Function`
// constructor (checker 4av maps `FunctionType` -> `SerializedTypeNode::Function`,
// threaded through `serialized_type_to_expression`).
#[test]
fn function_typed_property_serializes_to_function_constructor() {
    check_legacy_metadata(
        "class C { @dec x: () => void; }",
        &format!(
            "{}\n{}\nclass C {{\n    x;\n}}\n__decorate([dec, __metadata(\"design:type\", Function)], C.prototype, \"x\", void 0);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/metadata.go:getOldTypeMetadata
// (`shouldAddTypeMetadata` true for KindMethodDeclaration -> typeserializer.go
// `serializeTypeOfNode` KindMethodDeclaration -> `NewIdentifier("Function")`;
// `shouldAddReturnTypeMetadata` true for *every* method -> `serializeReturnTypeOfNode`
// with no annotation -> `NewVoidZeroExpression()`).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   class C { @dec m() {} }
//   =>
//   class C { m() { } }
//   __decorate([dec, __metadata("design:type", Function), __metadata("design:paramtypes", []), __metadata("design:returntype", void 0)], C.prototype, "m", null);
//
// Headline (6ao + 6ap): with `--emitDecoratorMetadata`, a decorated method
// appends a `design:type` = `Function` (Go hardcodes `Function` for methods, no
// checker), a `design:paramtypes` (6ap; the empty array `[]` for a 0-arg
// method), *and* a `design:returntype` (always emitted for methods; `void 0`
// with no return annotation), in that order, after the real decorator. Both
// `__decorate` (priority 2) and `__metadata` (priority 3) helpers land in the
// prologue. The `[]` here is the slice-2 (empty-params) coverage: it is
// green-on-arrival once slice 1 wires `serialize_parameter_types` (which yields
// `[]` for zero parameters).
#[test]
fn method_decorator_emits_design_type_function_and_void_returntype() {
    check_legacy_metadata(
        "class C { @dec m() {} }",
        &format!(
            "{}\n{}\nclass C {{\n    m() {{ }}\n}}\n__decorate([dec, __metadata(\"design:type\", Function), __metadata(\"design:paramtypes\", []), __metadata(\"design:returntype\", void 0)], C.prototype, \"m\", null);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/typeserializer.go:serializeReturnTypeOfNode
// (`IsFunctionLike(node) && node.Type() != nil` -> `serializeTypeNode(node.Type())`),
// the return-type annotation routed through the same checker serialization as a
// property's `design:type` (KindNumberKeyword -> "Number").
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   class C { @dec m(): number { return 1; } }
//   =>
//   class C { m() { return 1; } }
//   __decorate([dec, __metadata("design:type", Function), __metadata("design:paramtypes", []), __metadata("design:returntype", Number)], C.prototype, "m", null);
//
// Headline (6ao): a method's `design:returntype` serializes its *return-type
// annotation* via the checker (`serialize_type_node_for_metadata`), exactly as a
// property's `design:type` does — `: number` -> `Number` (not the `void 0`
// fallback used when the annotation is absent). `design:type` stays the
// hard-coded `Function`; the `design:paramtypes` is `[]` (no parameters, 6ap).
#[test]
fn method_decorator_serializes_return_type_annotation() {
    check_legacy_metadata(
        "class C { @dec m(): number { return 1; } }",
        &format!(
            "{}\n{}\nclass C {{\n    m() {{ return 1; }}\n}}\n__decorate([dec, __metadata(\"design:type\", Function), __metadata(\"design:paramtypes\", []), __metadata(\"design:returntype\", Number)], C.prototype, \"m\", null);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/metadata.go:getOldTypeMetadata
// (`shouldAddParamTypesMetadata` true for KindMethodDeclaration ->
// typeserializer.go:serializeParameterTypesOfNode: one serialized type per
// parameter, in order, via `serializeTypeOfNode(parameter)` ->
// `serializeTypeNode(parameter.Type())`). The `design:paramtypes` entry is
// appended *between* `design:type` and `design:returntype` (the
// `getOldTypeMetadata` order: type -> paramtypes -> returntype).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   class C { @dec m(a: number, b: string) {} }
//   =>
//   class C { m(a, b) { } }
//   __decorate([dec, __metadata("design:type", Function), __metadata("design:paramtypes", [Number, String]), __metadata("design:returntype", void 0)], C.prototype, "m", null);
//
// Headline (6ap): with `--emitDecoratorMetadata`, a decorated method emits a
// `design:paramtypes` array between `design:type` and `design:returntype`. Each
// parameter's type annotation is serialized through the same checker path as a
// property's `design:type` (`: number` -> `Number`, `: string` -> `String`),
// and the parameter type annotations are stripped from the lowered method body
// (`m(a: number, b: string)` -> `m(a, b)`).
#[test]
fn method_decorator_emits_design_paramtypes_for_typed_params() {
    check_legacy_metadata(
        "class C { @dec m(a: number, b: string) {} }",
        &format!(
            "{}\n{}\nclass C {{\n    m(a, b) {{ }}\n}}\n__decorate([dec, __metadata(\"design:type\", Function), __metadata(\"design:paramtypes\", [Number, String]), __metadata(\"design:returntype\", void 0)], C.prototype, \"m\", null);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:transformDecoratorsOfParameters
// (`NewParamHelper(decorator.Expression(), i, ...)` -> `__param(i, dec)`),
// gathered via `getDecoratorsOfParameters` and placed after the real decorators
// in `transformAllDecoratorsOfDeclaration`. A method with a decorated parameter
// is itself a "decorated class element" (`NodeOrChildIsDecorated`), even with no
// decorator on the method.
// Verified against `tsc --experimentalDecorators`:
//   class C { m(@pdec a) {} }
//   =>
//   class C { m(a) { } }
//   __decorate([__param(0, pdec)], C.prototype, "m", null);
//
// Headline (6ap): a parameter decorator lowers to a `__param(index, decorator)`
// entry in the member's `__decorate` array — here `__param(0, pdec)` for the
// 0th parameter. No `--emitDecoratorMetadata`, so no `design:*` entries. The
// `__param` helper (priority 4) is emitted in the prologue after `__decorate`
// (priority 2), and the parameter's decorator is stripped from the lowered
// method body (`m(@pdec a)` -> `m(a)`).
#[test]
fn parameter_decorator_lowers_to_param_helper() {
    check_legacy(
        "class C { m(@pdec a) {} }",
        &format!(
            "{}\n{}\nclass C {{\n    m(a) {{ }}\n}}\n__decorate([__param(0, pdec)], C.prototype, \"m\", null);",
            DECORATE_HELPER.text, PARAM_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:transformAllDecoratorsOfDeclaration
// (`decoratorExpressions = transformDecorators(decorators) ++
// transformDecoratorsOfParameters(parameters) ++ transformDecorators(metadata)`
// — the member's own decorators precede the `__param` entries, which precede
// metadata).
// Verified against `tsc --experimentalDecorators`:
//   class C { @dec m(@pdec a) {} }
//   =>
//   class C { m(a) { } }
//   __decorate([dec, __param(0, pdec)], C.prototype, "m", null);
//
// Coverage (6ap): a method with *both* a method decorator and a parameter
// decorator places the method decorator first (`dec`), then the `__param(0,
// pdec)` entry. Green-on-arrival once slices 1/3 wire the decorator + `__param`
// ordering; this guards the exact Go ordering (member decorators before
// `__param`).
#[test]
fn method_and_parameter_decorator_order_preserved() {
    check_legacy(
        "class C { @dec m(@pdec a) {} }",
        &format!(
            "{}\n{}\nclass C {{\n    m(a) {{ }}\n}}\n__decorate([dec, __param(0, pdec)], C.prototype, \"m\", null);",
            DECORATE_HELPER.text, PARAM_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:transformAllDecoratorsOfDeclaration
// + metadata.go:getOldTypeMetadata. With `--emitDecoratorMetadata`, a method
// with a parameter decorator emits the `__param` entry first, then the `design:*`
// metadata (`getOldTypeMetadata` order: type, paramtypes, returntype). An
// untyped parameter serializes to `Object` in the `design:paramtypes` array
// (Go's `serializeTypeNode(nil)`).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   class C { m(@pdec a) {} }
//   =>
//   class C { m(a) { } }
//   __decorate([__param(0, pdec), __metadata("design:type", Function), __metadata("design:paramtypes", [Object]), __metadata("design:returntype", void 0)], C.prototype, "m", null);
//
// Coverage (6ap): combines `__param` with the `design:paramtypes` array; the
// untyped parameter `a` serializes to `Object`. Green-on-arrival after slices
// 1/3; guards the `__param`-before-metadata ordering and the no-annotation
// `Object` fallback for a parameter type. All three helpers land in the prologue
// in priority order (`__decorate` 2, `__metadata` 3, `__param` 4).
#[test]
fn parameter_decorator_with_metadata_emits_param_and_object_paramtype() {
    check_legacy_metadata(
        "class C { m(@pdec a) {} }",
        &format!(
            "{}\n{}\n{}\nclass C {{\n    m(a) {{ }}\n}}\n__decorate([__param(0, pdec), __metadata(\"design:type\", Function), __metadata(\"design:paramtypes\", [Object]), __metadata(\"design:returntype\", void 0)], C.prototype, \"m\", null);",
            DECORATE_HELPER.text, METADATA_HELPER.text, PARAM_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:visitGetAccessorDeclaration
// + generateClassElementDecorationExpression (accessor descriptor is `null`,
// like a method — Go's `else` branch, since an accessor is not a
// `PropertyDeclaration`).
// Verified against Go / `tsc --experimentalDecorators`:
//   class C { @dec get x() { return 1; } }
//   =>
//   class C { get x() { return 1; } }
//   __decorate([dec], C.prototype, "x", null);
//
// Tracer bullet (6aq): a decorated instance *get accessor* lowers to a trailing
// `__decorate([dec], C.prototype, "x", null);` statement — the 4th argument is
// `null` (an accessor, like a method, not the property's `void 0`), the
// accessor's decorator is stripped, the getter body is kept, and the
// `__decorate` helper is emitted once in the prologue. No `--emitDecoratorMetadata`.
#[test]
fn instance_get_accessor_decorator_lowers_to_decorate_call() {
    check_legacy(
        "class C { @dec get x() { return 1; } }",
        &format!(
            "{}\nclass C {{\n    get x() {{ return 1; }}\n}}\n__decorate([dec], C.prototype, \"x\", null);",
            DECORATE_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:visitSetAccessorDeclaration
// (the symmetric set-accessor form; descriptor `null`, the set accessor's value
// parameter is rebuilt without its type annotation).
// Verified against Go / `tsc --experimentalDecorators`:
//   class C { @dec set x(v) {} }
//   =>
//   class C { set x(v) { } }
//   __decorate([dec], C.prototype, "x", null);
//
// Coverage (6aq): a decorated instance *set accessor* (no getter partner)
// lowers the same way — `__decorate([dec], C.prototype, "x", null);`, decorator
// stripped, value parameter kept. Green-on-arrival once slice 1 wires the
// accessor member-detection helpers + `rebuild_accessor_without_decorators`;
// guards the set-accessor arm symmetric to the getter tracer bullet.
#[test]
fn instance_set_accessor_decorator_lowers_to_decorate_call() {
    check_legacy(
        "class C { @dec set x(v) {} }",
        &format!(
            "{}\nclass C {{\n    set x(v) {{ }}\n}}\n__decorate([dec], C.prototype, \"x\", null);",
            DECORATE_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/metadata.go:getOldTypeMetadata
// (`shouldAddTypeMetadata` true for KindGetAccessor -> typeserializer.go
// `serializeTypeOfNode` KindGetAccessor -> `serializeTypeNode(getAccessorTypeNode(...))`,
// which for a getter with no setter is the getter's return type;
// `shouldAddParamTypesMetadata` is ALSO true for accessors -> a `design:paramtypes`
// array; `shouldAddReturnTypeMetadata` is method-only, so NO `design:returntype`).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`
// (baseline submodule/conformance/decoratorOnClassAccessor8 class E: a
// getter-only accessor emits design:type + design:paramtypes []):
//   class C { @dec get x(): number { return 1; } }
//   =>
//   class C { get x() { return 1; } }
//   __decorate([dec, __metadata("design:type", Number), __metadata("design:paramtypes", [])], C.prototype, "x", null);
//
// Headline (6aq): a decorated *get accessor* with a return annotation emits a
// `design:type` serialized from the getter's return type (`getAccessorTypeNode`
// = the getter's return type when there is no setter) — `: number` -> `Number`
// — *and* a `design:paramtypes` array (here `[]`, since the getter has no
// parameters and there is no setter to borrow them from), but NO
// `design:returntype` (that is method-only per `shouldAddReturnTypeMetadata`).
// The task's "ONLY design:type" hint is contradicted by Go/tsc, which emits
// both design:type and design:paramtypes for accessors.
#[test]
fn get_accessor_decorator_emits_design_type_from_return_and_empty_paramtypes() {
    check_legacy_metadata(
        "class C { @dec get x(): number { return 1; } }",
        &format!(
            "{}\n{}\nclass C {{\n    get x() {{ return 1; }}\n}}\n__decorate([dec, __metadata(\"design:type\", Number), __metadata(\"design:paramtypes\", [])], C.prototype, \"x\", null);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/metadata.go:getAccessorTypeNode
// (`accessors.SetAccessor != nil` -> `getSetAccessorTypeAnnotationNode` = the
// set accessor's value-parameter type) + serializeParameterTypesOfNode (the
// setter's own parameter -> `[Number]`).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`
// (baseline submodule/conformance/decoratorOnClassAccessor8 class F):
//   class C { @dec set x(v: number) {} }
//   =>
//   class C { set x(v) { } }
//   __decorate([dec, __metadata("design:type", Number), __metadata("design:paramtypes", [Number])], C.prototype, "x", null);
//
// Headline (6aq): a decorated *set accessor* (no getter partner) takes its
// `design:type` from the value-parameter type (`getAccessorTypeNode` ->
// `getSetAccessorTypeAnnotationNode`) — `v: number` -> `Number` — and its
// `design:paramtypes` from the setter's own parameter list (`[Number]`). The
// parameter type annotation is stripped from the lowered body (`set x(v)`).
// Green-on-arrival once slice 2 wires the accessor metadata (both the getter and
// setter arms are implemented together); guards the setter `design:type` source
// (value parameter) and the `[Number]` paramtypes.
#[test]
fn set_accessor_decorator_emits_design_type_and_paramtypes_from_value_param() {
    check_legacy_metadata(
        "class C { @dec set x(v: number) {} }",
        &format!(
            "{}\n{}\nclass C {{\n    set x(v) {{ }}\n}}\n__decorate([dec, __metadata(\"design:type\", Number), __metadata(\"design:paramtypes\", [Number])], C.prototype, \"x\", null);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:getAllDecoratorsOfAccessors
// (a get/set pair emits a *single* `__decorate`, owned by the first accessor
// with decorators; the partner accessor returns `nil` so it emits nothing).
// Verified against Go / `tsc --experimentalDecorators`:
//   class C { @dec get x() { return 1; } set x(v) {} }
//   =>
//   class C { get x() { return 1; } set x(v) { } }
//   __decorate([dec], C.prototype, "x", null);
//
// Headline (6aq): a decorated getter with an (undecorated) setter partner emits
// exactly ONE `__decorate([dec], C.prototype, "x", null);` — the getter owns it,
// and the setter does not emit its own. Both accessors are kept in the class
// body. No `--emitDecoratorMetadata`. Green-on-arrival once slices 1/2 wire the
// accessor lowering + `getAllAccessorDeclarations` pairing/ownership; guards the
// merge invariant (one combined `__decorate`, not two).
#[test]
fn get_set_pair_emits_single_decorate_owned_by_getter() {
    check_legacy(
        "class C { @dec get x() { return 1; } set x(v) {} }",
        &format!(
            "{}\nclass C {{\n    get x() {{ return 1; }}\n    set x(v) {{ }}\n}}\n__decorate([dec], C.prototype, \"x\", null);",
            DECORATE_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/metadata.go:getAccessorTypeNode +
// getParametersOfDecoratedDeclaration — a decorated *getter* with a setter
// partner pulls BOTH its `design:type` and its `design:paramtypes` from the
// *setter's* value parameter (the setter wins for `design:type`, and the getter
// borrows the setter's parameters).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`
// (baseline submodule/conformance/decoratorOnClassAccessor8 class A):
//   class C { @dec get x() { return 0; } set x(value: number) {} }
//   =>
//   class C { get x() { return 0; } set x(value) { } }
//   __decorate([dec, __metadata("design:type", Number), __metadata("design:paramtypes", [Number])], C.prototype, "x", null);
//
// Headline (6aq): the get/set pair merges into one `__decorate`, and the
// metadata crosses accessors — even though the *getter* is the decorated owner
// (and has no return annotation, which alone would serialize to `Object`), the
// `design:type` is `Number` from the setter's `value: number` parameter
// (`getAccessorTypeNode` prefers the setter), and `design:paramtypes` is
// `[Number]` borrowed from the setter (`getParametersOfDecoratedDeclaration`).
// The setter's parameter type is stripped in the lowered body (`set x(value)`).
#[test]
fn get_set_pair_metadata_crosses_to_setter_value_param() {
    check_legacy_metadata(
        "class C { @dec get x() { return 0; } set x(value: number) {} }",
        &format!(
            "{}\n{}\nclass C {{\n    get x() {{ return 0; }}\n    set x(value) {{ }}\n}}\n__decorate([dec, __metadata(\"design:type\", Number), __metadata(\"design:paramtypes\", [Number])], C.prototype, \"x\", null);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:getAllDecoratorsOfAccessors
// (`firstAccessorWithDecorators` may be the *second* accessor in declaration
// order; the undecorated first accessor emits nothing).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`
// (baseline submodule/conformance/decoratorOnClassAccessor8 class B):
//   class C { get x() { return 0; } @dec set x(value: number) {} }
//   =>
//   class C { get x() { return 0; } set x(value) { } }
//   __decorate([dec, __metadata("design:type", Number), __metadata("design:paramtypes", [Number])], C.prototype, "x", null);
//
// Coverage (6aq): the setter is decorated and declared *second*; it owns the
// single `__decorate`, and the undecorated getter (first) emits nothing. The
// `design:type` (`Number`) and `design:paramtypes` (`[Number]`) come from the
// setter's value parameter. Guards the ownership-by-first-decorated-accessor
// when the decorated accessor is not the first member of the pair.
#[test]
fn get_set_pair_setter_decorated_second_owns_single_decorate() {
    check_legacy_metadata(
        "class C { get x() { return 0; } @dec set x(value: number) {} }",
        &format!(
            "{}\n{}\nclass C {{\n    get x() {{ return 0; }}\n    set x(value) {{ }}\n}}\n__decorate([dec, __metadata(\"design:type\", Number), __metadata(\"design:paramtypes\", [Number])], C.prototype, \"x\", null);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:getClassMemberPrefix
// (`ast.IsStatic(member)` -> `GetDeclarationName(node)` i.e. `C`), applied to a
// static *get accessor* just as for a static method/property.
// Verified against Go / `tsc --experimentalDecorators`:
//   class C { @dec static get x() { return 1; } }
//   =>
//   class C { static get x() { return 1; } }
//   __decorate([dec], C, "x", null);
//
// Headline (6aq): a static get accessor decorates the class constructor
// directly — the `__decorate` target is the bare class name `C` (not
// `C.prototype`), the `static` modifier is kept on the stripped accessor, and
// the 4th argument is `null`. Green-on-arrival once slice 1 wires the accessor
// arm of `is_static_member`; guards the static prefix for accessors.
#[test]
fn static_get_accessor_decorator_uses_class_name_prefix() {
    check_legacy(
        "class C { @dec static get x() { return 1; } }",
        &format!(
            "{}\nclass C {{\n    static get x() {{ return 1; }}\n}}\n__decorate([dec], C, \"x\", null);",
            DECORATE_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:getClassMemberPrefix
// + metadata.go:getOldTypeMetadata for an accessor (static path still emits
// design:type + design:paramtypes; the prefix is `C`, not `C.prototype`).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   class C { @dec static get x(): number { return 1; } }
//   =>
//   class C { static get x() { return 1; } }
//   __decorate([dec, __metadata("design:type", Number), __metadata("design:paramtypes", [])], C, "x", null);
//
// Coverage (6aq): a static get accessor with metadata combines the static
// prefix `C` with the accessor metadata (design:type `Number` from the return
// type, design:paramtypes `[]`). Guards that the static-vs-instance prefix is
// orthogonal to the accessor metadata path.
#[test]
fn static_get_accessor_decorator_with_metadata_uses_class_name_prefix() {
    check_legacy_metadata(
        "class C { @dec static get x(): number { return 1; } }",
        &format!(
            "{}\n{}\nclass C {{\n    static get x() {{ return 1; }}\n}}\n__decorate([dec, __metadata(\"design:type\", Number), __metadata(\"design:paramtypes\", [])], C, \"x\", null);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Gate: without `--experimentalDecorators` the transform is inert — a decorated
// class passes through unchanged (decorators and type annotation intact, no
// `__decorate`). The legacy lowering is strictly gated on the option.
#[test]
fn without_experimental_decorators_class_is_unchanged() {
    let input = "class C { @dec x: number; }";
    let (ec, source_file) = parse_shared(input);
    let opts = TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    };
    // `experimental_decorators` left at its default (`Unknown`, i.e. off).
    let mut tx = new_legacy_decorators_transformer(&opts);
    let result = tx.transform_source_file(source_file);
    assert_eq!(
        emit(&ec, result, input),
        "class C {\n    @dec\n    x: number;\n}",
        "gate-off({input:?})"
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:transformClassDeclarationWithClassDecorators
// (Example 1: the simplest decorated-class case — `let C = class C {}` +
// trailing `C = __decorate([dec], C);`, via `getConstructorDecorationStatement`
// -> `generateConstructorDecorationExpression` -> `NewDecorateHelper(localName)`
// + `NewAssignmentExpression(localName, decorate)`).
// Verified against Go / `tsc --experimentalDecorators` (Go source comment
// Example 1):
//   @dec class C {}
//   =>
//   let C = class C {
//   };
//   C = __decorate([dec], C);
//
// Headline (6ar tracer bullet): a *class* decorator lowers the class declaration
// into a `let`-bound class expression (keeping the `class C` name) followed by a
// trailing `C = __decorate([dec], C);` assignment statement. The class
// decorator is stripped from the class expression, and the `__decorate` helper
// is emitted once in the module prologue. No self-reference, so no class alias.
#[test]
fn class_decorator_lowers_to_let_wrap_and_decorate() {
    check_legacy(
        "@dec class C {}",
        &format!(
            "{}\nlet C = class C {{\n}};\nC = __decorate([dec], C);",
            DECORATE_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:getAllDecoratorsOfClass
// + transformAllDecoratorsOfDeclaration -> transformDecorators (each decorator
// maps to `decorator.Expression()`, in source order). `node.Decorators()`
// returns the class decorators in declaration order.
// Verified against Go / `tsc --experimentalDecorators`:
//   @a @b class C {}
//   =>
//   let C = class C {
//   };
//   C = __decorate([a, b], C);
//
// Coverage (6ar): multiple class decorators are collected into the class
// `__decorate` array in source order (`a` before `b`). Green-on-arrival once
// behavior 1 wires `class_decorator_expressions` (which preserves the modifier
// list order); guards against a reversed / single-decorator-only regression.
#[test]
fn multiple_class_decorators_preserve_source_order() {
    check_legacy(
        "@a @b class C {}",
        &format!(
            "{}\nlet C = class C {{\n}};\nC = __decorate([a, b], C);",
            DECORATE_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:visitClassDeclaration
// (`ClassOrConstructorParameterIsDecorated` is true when a constructor parameter
// is decorated, even with no class decorator -> the class still takes the
// `transformClassDeclarationWithClassDecorators` path) + `getAllDecoratorsOfClass`
// (`getDecoratorsOfParameters(GetFirstConstructorWithBody(node))`) +
// `transformDecoratorsOfParameters` (`__param(i, dec)` into the CLASS decorator
// array, not a member one).
// Verified against Go / `tsc --experimentalDecorators`:
//   class C { constructor(@pdec a: number) {} }
//   =>
//   let C = class C {
//       constructor(a) { }
//   };
//   C = __decorate([__param(0, pdec)], C);
//
// Headline (6ar slice 3): a decorated *constructor parameter* makes the class
// itself "decorated", so it takes the `let C = class C {…}` wrapping even with
// no class decorator. The parameter decorator becomes a `__param(0, pdec)` entry
// in the CLASS `__decorate` array (targeting the constructor `C`), and the
// constructor's parameter decorator + type annotation are stripped from the
// lowered body (`constructor(a) { }`). The `__param` helper (priority 4) follows
// `__decorate` (priority 2) in the prologue.
#[test]
fn constructor_parameter_decorator_decorates_class_constructor() {
    check_legacy(
        "class C { constructor(@pdec a: number) {} }",
        &format!(
            "{}\n{}\nlet C = class C {{\n    constructor(a) {{ }}\n}};\nC = __decorate([__param(0, pdec)], C);",
            DECORATE_HELPER.text, PARAM_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:transformClassDeclarationWithClassDecorators
// (`statements = [varStatement] ++ decorationStatements ++ getConstructorDecorationStatement` —
// the member `__decorate`s precede the class-level one).
// Verified against Go / `tsc --experimentalDecorators`:
//   @dec class C { @mdec m() {} }
//   =>
//   let C = class C {
//       m() { }
//   };
//   __decorate([mdec], C.prototype, "m", null);
//   C = __decorate([dec], C);
//
// Coverage (6ar): a class decorator combined with a decorated *member* emits the
// member `__decorate` (instance member -> `C.prototype`) BEFORE the class-level
// `C = __decorate([dec], C)`. Green-on-arrival once behavior 1 wires the
// member-decoration loop inside the class-decorator path; guards the
// statement order (members first, class last) and that both decorations are
// emitted.
#[test]
fn class_decorator_with_decorated_method_emits_member_then_class_decorate() {
    check_legacy(
        "@dec class C { @mdec m() {} }",
        &format!(
            "{}\nlet C = class C {{\n    m() {{ }}\n}};\n__decorate([mdec], C.prototype, \"m\", null);\nC = __decorate([dec], C);",
            DECORATE_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/metadata.go:shouldAddParamTypesMetadata
// (`KindClassDeclaration` -> `GetFirstConstructorWithBody(node) != nil`) +
// getOldTypeMetadata (class emits ONLY `design:paramtypes`, no `design:type` /
// `design:returntype`) + serializeParameterTypesOfNode (the constructor's
// parameter types). The injected `@__metadata` lands last in the class
// `__decorate` array (`transformAllDecoratorsOfDeclaration`: decorators, then
// `__param`, then metadata).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   @dec class C { constructor(a: number) {} }
//   =>
//   let C = class C {
//       constructor(a) { }
//   };
//   C = __decorate([dec, __metadata("design:paramtypes", [Number])], C);
//
// Headline (6ar slice 4): with `--emitDecoratorMetadata`, a decorated class with
// a constructor body appends a `design:paramtypes` metadata to the CLASS
// `__decorate` array — the constructor's parameter types serialized through the
// checker (`: number` -> `Number`). A class emits NO `design:type` /
// `design:returntype` (those are member-only). The constructor parameter type is
// stripped from the lowered body (`constructor(a) { }`).
#[test]
fn class_constructor_paramtypes_metadata_under_emit_decorator_metadata() {
    check_legacy_metadata(
        "@dec class C { constructor(a: number) {} }",
        &format!(
            "{}\n{}\nlet C = class C {{\n    constructor(a) {{ }}\n}};\nC = __decorate([dec, __metadata(\"design:paramtypes\", [Number])], C);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: metadata.go:shouldAddParamTypesMetadata (no constructor with body ->
// false, so no `design:paramtypes`).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   @dec class C {}
//   =>
//   let C = class C {
//   };
//   C = __decorate([dec], C);
//
// Coverage (6ar): a decorated class with NO constructor emits no
// `design:paramtypes` metadata even under `--emitDecoratorMetadata` (the
// `GetFirstConstructorWithBody == nil` gate). Guards that the metadata is
// constructor-conditioned, not unconditional.
#[test]
fn class_decorator_without_constructor_emits_no_paramtypes_metadata() {
    check_legacy_metadata(
        "@dec class C {}",
        &format!(
            "{}\nlet C = class C {{\n}};\nC = __decorate([dec], C);",
            DECORATE_HELPER.text
        ),
    );
}

// Go: internal/transformers/tstransforms/legacydecorators.go:transformAllDecoratorsOfDeclaration
// (class decorators, then `__param`, then metadata) + metadata.go (class
// `design:paramtypes` from the constructor parameters).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   class C { constructor(@pdec a: number) {} }
//   =>
//   let C = class C {
//       constructor(a) { }
//   };
//   C = __decorate([__param(0, pdec), __metadata("design:paramtypes", [Number])], C);
//
// Coverage (6ar): a decorated constructor *parameter* under
// `--emitDecoratorMetadata` places the `__param(0, pdec)` entry BEFORE the
// `design:paramtypes` metadata in the class `__decorate` array, and the
// constructor's parameter type feeds the `design:paramtypes` (`[Number]`).
// Green-on-arrival once slices 3/4 wire `__param` + class metadata; guards the
// `__param`-before-metadata ordering for the class target. All three helpers
// land in the prologue in priority order (`__decorate` 2, `__metadata` 3,
// `__param` 4).
#[test]
fn constructor_parameter_decorator_with_metadata_orders_param_before_paramtypes() {
    check_legacy_metadata(
        "class C { constructor(@pdec a: number) {} }",
        &format!(
            "{}\n{}\n{}\nlet C = class C {{\n    constructor(a) {{ }}\n}};\nC = __decorate([__param(0, pdec), __metadata(\"design:paramtypes\", [Number])], C);",
            DECORATE_HELPER.text, METADATA_HELPER.text, PARAM_HELPER.text
        ),
    );
}

// Go: typeserializer.go:serializeTypeReferenceNode
// (`case TypeReferenceSerializationKindTypeWithConstructSignatureAndValue:
// return s.serializeEntityNameAsExpression(node.TypeName)`).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   class C {}
//   class D { @dec x: C; }
//   =>
//   class C {}
//   class D { x; }
//   __decorate([dec, __metadata("design:type", C)], D.prototype, "x", void 0);
//
// Headline (round 4ax/6an, consumes checker 4aw): a class-typed property's
// `design:type` is the referenced class's *identifier* itself (`C`), not the
// `Object` fallback. The checker's `get_type_reference_serialization_kind`
// classifies `: C` as `TypeWithConstructSignatureAndValue` (a runtime
// constructor), and the transformer emits the entity name as an expression.
#[test]
fn class_typed_property_serializes_to_entity_identifier() {
    check_legacy_metadata(
        "class C {}\nclass D { @dec x: C; }",
        &format!(
            "{}\n{}\nclass C {{\n}}\nclass D {{\n    x;\n}}\n__decorate([dec, __metadata(\"design:type\", C)], D.prototype, \"x\", void 0);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: typeserializer.go:serializeTypeReferenceNode
// (`case TypeReferenceSerializationKindObjectType: return
// s.f.NewIdentifier("Object")`).
// Verified against `tsc --experimentalDecorators --emitDecoratorMetadata`:
//   interface I {}
//   class D { @dec x: I; }
//   =>
//   class D { x; }
//   __decorate([dec, __metadata("design:type", Object)], D.prototype, "x", void 0);
//
// A type-only reference (an interface has type meaning but no runtime value)
// classifies as `ObjectType`, so its `design:type` is the `Object` fallback —
// *not* the entity identifier. Green-on-arrival once behavior 1 wires the
// `TypeReference` dispatch (the `ObjectType` arm maps to `Object`); guards that
// a type-only reference is not mistakenly emitted as its own identifier.
// (The `interface I {}` passes through verbatim because this transformer runs
// in isolation, without the type-eraser stage that would drop it in the full
// pipeline; the assertion of record is the `design:type` value.)
#[test]
fn interface_typed_property_serializes_to_object() {
    check_legacy_metadata(
        "interface I {}\nclass D { @dec x: I; }",
        &format!(
            "{}\n{}\ninterface I {{\n}}\nclass D {{\n    x;\n}}\n__decorate([dec, __metadata(\"design:type\", Object)], D.prototype, \"x\", void 0);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}

// Go: typeserializer.go:serializeTypeReferenceNode (`case
// TypeReferenceSerializationKindUnknown:`). Go's full form (outside a
// conditional-type branch) emits a `typeof (_a = Missing) === "function" ? _a :
// Object` guard; the reachable single-file port emits the `Object` tail (Go's
// `serializingConditionalTypeBranch` result). The conditional guard is DEFER'd
// (blocked-by `NewTempVariable`/`AddVariableDeclaration`).
// Verified `design:type` against `tsc --experimentalDecorators
// --emitDecoratorMetadata` for a resolvable class; the unresolved-name fallback
// is `Object`:
//   class D { @dec x: Missing; }
//   =>
//   class D { x; }
//   __decorate([dec, __metadata("design:type", Object)], D.prototype, "x", void 0);
//
// An unresolved entity name (`Missing` has no declaration and there are no lib
// globals) classifies as `Unknown` → `Object`. Green-on-arrival after behavior 1
// (the `Unknown` arm maps to `Object`).
#[test]
fn unresolved_type_reference_property_serializes_to_object() {
    check_legacy_metadata(
        "class D { @dec x: Missing; }",
        &format!(
            "{}\n{}\nclass D {{\n    x;\n}}\n__decorate([dec, __metadata(\"design:type\", Object)], D.prototype, \"x\", void 0);",
            DECORATE_HELPER.text, METADATA_HELPER.text
        ),
    );
}
