use super::*;
use crate::test_support::{emit, parse_shared};
use std::rc::Rc;

// Runs the class-fields transformer over `input` and asserts the emitted JS.
fn check_downlevel(input: &str, expected: &str) {
    let (ec, source_file) = parse_shared(input);
    let mut tx = new_class_fields_transformer(&TransformOptions {
        context: Some(Rc::clone(&ec)),
    });
    let result = tx.transform_source_file(source_file);
    assert_eq!(emit(&ec, result, input), expected, "downlevel({input:?})");
}

// Go: internal/transformers/estransforms/classfields.go:transformClassMembers (instance fields)
// Tracer bullet: a plain instance field initializer is hoisted into a
// synthesized constructor as a `this.x = ...` assignment.
//
// The synthesized constructor body prints on a single line because Go's
// `Block.MultiLine` flag is not yet carried by the Rust AST (a documented
// printer `TODO(port)`); the lowering itself (field -> `this.x = 1`) is the
// behavior under test.
#[test]
fn instance_field_initializer_moves_to_constructor() {
    check_downlevel(
        "class C { x = 1 }",
        "class C {\n    constructor() { this.x = 1; }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:transformClassMembers (multiple fields)
// Several instance fields become several `this.<name> = ...` assignments, in
// source order, and the field declarations are dropped.
#[test]
fn multiple_instance_fields_move_to_constructor() {
    check_downlevel(
        "class C { x = 1; y = 2 }",
        "class C {\n    constructor() { this.x = 1; this.y = 2; }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:transformConstructorBody (existing ctor, no super)
// With an existing constructor and no heritage, field initializers are inserted
// at the top of the constructor body, before the original statements.
#[test]
fn field_inits_prepend_to_existing_constructor() {
    check_downlevel(
        "class C { x = 1; constructor() { this.y = 2; } }",
        "class C {\n    constructor() { this.x = 1; this.y = 2; }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:transformConstructorBody (needsSyntheticConstructor)
// A derived class with no constructor gets a synthesized constructor that first
// forwards to `super(...arguments)`, then runs the field initializers.
#[test]
fn derived_class_synthesizes_constructor_with_super() {
    check_downlevel(
        "class C extends B { x = 1 }",
        "class C extends B {\n    constructor() { super(...arguments); this.x = 1; }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:transformConstructorBodyWorker (insert after super)
// With an existing constructor that calls `super(...)`, field initializers are
// inserted immediately after the `super()` statement.
#[test]
fn field_inits_inserted_after_super_call() {
    check_downlevel(
        "class C extends B { x = 1; constructor() { super(); this.y = 2; } }",
        "class C extends B {\n    constructor() { super(); this.x = 1; this.y = 2; }\n}",
    );
}

// Go: internal/transformers/estransforms/classfields.go:addPropertyOrClassStaticBlockStatements
// A static field initializer becomes a `C.x = ...` assignment emitted after the
// class declaration (the transform returns a `SyntaxList`).
#[test]
fn static_field_becomes_assignment_after_class() {
    check_downlevel("class C { static x = 1 }", "class C {\n}\nC.x = 1;");
}
