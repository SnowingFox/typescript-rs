use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// Runs the class-fields transformer over `input` and asserts the emitted JS.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_class_fields_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
        ..Default::default()
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "downlevel({input:?})");
}

// Go: internal/transformers/estransforms/classfields.go:transformClassMembers (instance fields)
// Tracer bullet: a plain instance field initializer is hoisted into a
// synthesized constructor as a `this.x = ...` assignment.
#[test]
fn instance_field_initializer_moves_to_constructor() {
    check_downlevel(
        "class C { x = 1 }",
        "class C {\n    constructor() {\n        this.x = 1;\n    }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:transformClassMembers (multiple fields)
// Several instance fields become several `this.<name> = ...` assignments, in
// source order, and the field declarations are dropped.
#[test]
fn multiple_instance_fields_move_to_constructor() {
    check_downlevel(
        "class C { x = 1; y = 2 }",
        "class C {\n    constructor() {\n        this.x = 1;\n        this.y = 2;\n    }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:transformConstructorBody (existing ctor, no super)
// With an existing constructor and no heritage, field initializers are inserted
// at the top of the constructor body, before the original statements.
#[test]
fn field_inits_prepend_to_existing_constructor() {
    check_downlevel(
        "class C { x = 1; constructor() { this.y = 2; } }",
        "class C {\n    constructor() {\n        this.x = 1;\n        this.y = 2;\n    }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:transformConstructorBody (needsSyntheticConstructor)
// A derived class with no constructor gets a synthesized constructor that first
// forwards to `super(...arguments)`, then runs the field initializers.
#[test]
fn derived_class_synthesizes_constructor_with_super() {
    check_downlevel(
        "class C extends B { x = 1 }",
        "class C extends B {\n    constructor() {\n        super(...arguments);\n        this.x = 1;\n    }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:transformConstructorBodyWorker (insert after super)
// With an existing constructor that calls `super(...)`, field initializers are
// inserted immediately after the `super()` statement.
#[test]
fn field_inits_inserted_after_super_call() {
    check_downlevel(
        "class C extends B { x = 1; constructor() { super(); this.y = 2; } }",
        "class C extends B {\n    constructor() {\n        super();\n        this.x = 1;\n        this.y = 2;\n    }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:addPropertyOrClassStaticBlockStatements
// A static field initializer becomes a `C.x = ...` assignment emitted after the
// class declaration (the transform returns a `SyntaxList`).
#[test]
fn static_field_becomes_assignment_after_class() {
    check_downlevel("class C { static x = 1 }", "class C {\n}\nC.x = 1;");
}

// Go: internal/transformers/estransforms/classfields.go:transformPrivateFieldInitializer
//   + addPrivateIdentifierToEnvironment (WeakMap brand) — direct `.set`/`.get`
//   form (the named-helper `__classPrivateFieldSet` import form is DEFER'd).
// A private instance field is lowered to a module-scope `WeakMap` brand declared
// before the class, and its initializer becomes `_C_x.set(this, ...)` in the
// synthesized constructor (the brand + class are returned as a `SyntaxList`).
#[test]
fn private_field_initializer_uses_weakmap_set() {
    check_downlevel(
        "class C { #x = 1 }",
        "var _C_x = new WeakMap();\nclass C {\n    constructor() {\n        _C_x.set(this, 1);\n    }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:createPrivateIdentifierAccess (direct .get form)
// A private field *read* `this.#x` inside a method body is rewritten to a
// `_C_x.get(this)` WeakMap lookup using the class-scoped private environment.
#[test]
fn private_field_read_uses_weakmap_get() {
    check_downlevel(
        "class C { #x = 1; m() { return this.#x; } }",
        "var _C_x = new WeakMap();\nclass C {\n    constructor() {\n        _C_x.set(this, 1);\n    }\n    m() { return _C_x.get(this); }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:createPrivateIdentifierAssignment (direct .set form)
// A private field *write* `this.#x = v` inside a method body is rewritten to a
// `_C_x.set(this, v)` WeakMap store using the class-scoped private environment.
#[test]
fn private_field_write_uses_weakmap_set() {
    check_downlevel(
        "class C { #x = 1; m(v) { this.#x = v; } }",
        "var _C_x = new WeakMap();\nclass C {\n    constructor() {\n        _C_x.set(this, 1);\n    }\n    m(v) { _C_x.set(this, v); }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:getPropertyNameExpressionIfNeeded
// A computed instance-field name is cached in a temp declared before the class
// (so the key is evaluated once, at class-definition time), and the field
// initializer becomes `this[<temp>] = ...` in the constructor.
#[test]
fn computed_field_name_is_hoisted_to_temp() {
    check_downlevel(
        "class C { [k] = 1 }",
        "var _a = k;\nclass C {\n    constructor() {\n        this[_a] = 1;\n    }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:visitClassExpression
// (round 6o) Instance-field lowering also applies in *expression* position: a
// class expression's field initializer is hoisted into a synthesized
// constructor while the node stays a class *expression* (no statement hoisting
// is needed for the instance-field-only shape, so no IIFE/temp wrapper).
#[test]
fn class_expression_instance_field_moves_to_constructor() {
    check_downlevel(
        "const C = class { x = 1 };",
        "const C = class {\n    constructor() {\n        this.x = 1;\n    }\n};",
    );
}

// Go: internal/transformers/estransforms/classfields.go:transformAutoAccessor
//   + createAccessorPropertyBackingField / createAccessorPropertyGetRedirector /
//   createAccessorPropertySetRedirector.
// (round 6q) An instance auto-accessor (`accessor x = 1`) lowers to a private
// backing field plus a get/set redirector pair. The backing name is allocated
// with the round-6p emit-context node-based generated *private* name generator
// (`new_generated_private_name_for_node` with the `_accessor_storage` suffix),
// so the field, getter, and setter all reference the same `#x_accessor_storage`
// at emit time. This is the ES2022-native shape (private elements not lowered to
// WeakMap), so the backing field stays a class member rather than a WeakMap.
#[test]
fn instance_auto_accessor_lowers_to_backing_field_and_redirectors() {
    check_downlevel(
        "class C { accessor x = 1; }",
        "class C {\n    #x_accessor_storage = 1;\n    get x() { return this.#x_accessor_storage; }\n    set x(value) { this.#x_accessor_storage = value; }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:createAccessorPropertyBackingField
//   (nil initializer).
// (round 6q) An auto-accessor with no initializer produces a backing field with
// no initializer; the redirector pair is unchanged. (Same lowering path as the
// initialized form — the initializer is simply threaded through as optional.)
#[test]
fn auto_accessor_without_initializer() {
    check_downlevel(
        "class C { accessor x; }",
        "class C {\n    #x_accessor_storage;\n    get x() { return this.#x_accessor_storage; }\n    set x(value) { this.#x_accessor_storage = value; }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:transformAutoAccessor
//   (static receiver) + visitModifier (keeps `static`, strips `accessor`).
// (round 6q) A **static** auto-accessor keeps the `static` modifier on the
// backing field and both redirectors; the redirector bodies use `this` (the
// class object inside a static member) as the receiver.
#[test]
fn static_auto_accessor_keeps_static_modifier() {
    check_downlevel(
        "class C { static accessor x = 1; }",
        "class C {\n    static #x_accessor_storage = 1;\n    static get x() { return this.#x_accessor_storage; }\n    static set x(value) { this.#x_accessor_storage = value; }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:visitClassExpression
//   -> transformAutoAccessor.
// (round 6q) The auto-accessor lowering also runs in *expression* position: a
// class expression with an `accessor` member is rewritten in place (no statement
// hoisting is needed for the native backing-field shape, so no IIFE/temp
// wrapper).
#[test]
fn class_expression_auto_accessor_lowers_in_place() {
    check_downlevel(
        "const C = class { accessor x = 1 };",
        "const C = class {\n    #x_accessor_storage = 1;\n    get x() { return this.#x_accessor_storage; }\n    set x(value) { this.#x_accessor_storage = value; }\n};",
    );
}

// Go: internal/transformers/estransforms/classfields.go:visitClassExpressionInNewClassLexicalEnvironment
//   (the `hasTransformableStatics` branch: temp = class, static assignments, temp).
// (round 6r) Tracer: a class *expression* with a single static field is lowered
// by wrapping the class in a comma sequence with a hoisted temp:
// `(_a = class {}, _a.x = 1, _a)`. The temp (`_a`) is allocated via the
// emit-context name generator and declared with a `var _a;` hoisted to the
// enclosing (source-file) scope's variable environment.
#[test]
fn class_expression_static_field_hoists_to_comma_sequence_with_temp() {
    check_downlevel(
        "const C = class { static x = 1 };",
        "var _a;\nconst C = (_a = class {\n}, _a.x = 1, _a);",
    );
}

// Go: internal/transformers/estransforms/classfields.go:visitClassExpressionInNewClassLexicalEnvironment
//   (multiple static initializers appended to the comma sequence in order).
// (round 6r) Multiple static fields in a class expression become several
// `_a.<name> = ...` assignments in source order, all sharing the single wrapper
// temp, then the trailing `_a`. (Same comma-sequence path as the tracer; this
// locks the multi-field reachable face.)
#[test]
fn class_expression_multiple_static_fields_share_one_temp() {
    check_downlevel(
        "const C = class { static x = 1; static y = 2 };",
        "var _a;\nconst C = (_a = class {\n}, _a.x = 1, _a.y = 2, _a);",
    );
}

// Go: internal/transformers/estransforms/classfields.go:visitClassExpressionInNewClassLexicalEnvironment
//   (instance fields -> constructor inside the wrapped class; statics -> comma).
// (round 6r) A class expression mixing an **instance** field and a **static**
// field lowers both: the instance field moves into a synthesized constructor
// *inside* the wrapped class value, and the static field becomes an `_a.y = ...`
// assignment in the comma sequence.
#[test]
fn class_expression_instance_and_static_fields_lower_together() {
    check_downlevel(
        "const C = class { x = 1; static y = 2 };",
        "var _a;\nconst C = (_a = class {\n    constructor() {\n        this.x = 1;\n    }\n}, _a.y = 2, _a);",
    );
}

// Go: internal/transformers/estransforms/classfields.go:visitClassExpressionInNewClassLexicalEnvironment
//   (the `pendingExpressions` inlining branch — DEFER'd here).
// (round 6r) A class *expression* with a **computed instance field** would need
// the computed-name key temp inlined into the comma sequence as a pending
// expression (Go's `pendingExpressions`). This port's computed-name handling
// instead hoists a deterministic `var _a = k;` *statement* (a `SyntaxList`),
// which is illegal in expression position, so such a class expression is left
// unchanged. (Pre-statement hoisting other than static assignments defers.)
#[test]
fn class_expression_with_computed_field_is_left_unchanged() {
    check_downlevel(
        "const C = class { [k] = 1 };",
        "const C = class {\n    [k] = 1;\n};",
    );
}

// Go: internal/transformers/estransforms/classfields.go:visitClassExpressionInNewClassLexicalEnvironment
// (round 6r) The class-expression static-field lowering keeps a **named** class
// expression's name (`class D`) inside the comma sequence; the static
// assignment still targets the wrapper temp (`_a.x`), not the class name, so
// the lowering is identical to the anonymous form aside from the retained name.
// (Replaces the round-6o `class_expression_with_static_field_is_left_unchanged`
// guard, now that the temp-wrapper form is ported.)
#[test]
fn named_class_expression_static_field_keeps_name_in_comma_sequence() {
    check_downlevel(
        "const C = class D { static x = 1 };",
        "var _a;\nconst C = (_a = class D {\n}, _a.x = 1, _a);",
    );
}
