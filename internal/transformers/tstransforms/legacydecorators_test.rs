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

// Go: internal/transformers/tstransforms/legacydecorators.go:visitClassDeclaration
// (the `decorated` -> `transformClassDeclarationWithClassDecorators` branch).
// DEFER guard: a class *decorator* (`@dec class C {}`) is not yet lowered to the
// `let C = class C {}; C = __decorate([dec], C);` wrapping; the class passes
// through unchanged. blocked-by: the `let`-binding / class-alias wrapping +
// emit-name forms.
#[test]
fn class_decorator_is_left_unchanged() {
    check_legacy("@dec class C {}", "@dec\nclass C {\n}");
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
