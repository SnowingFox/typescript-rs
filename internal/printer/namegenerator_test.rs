use super::*;
use crate::emitcontext::{AutoGenerateOptions, EmitContext};
use crate::generatedidentifierflags::GeneratedIdentifierFlags;
use tsgo_ast::{Kind, NodeList, TokenFlags};

// Go: internal/printer/namegenerator_test.go:TestTempVariable1
#[test]
fn temp_variable_1() {
    let mut ec = EmitContext::new();
    let name1 = ec.factory().new_temp_variable();
    let name2 = ec.factory().new_temp_variable();

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "_a");
    assert_eq!(g.generate_name(name2), "_b");
}

// Go: internal/printer/namegenerator_test.go:TestTempVariable2
#[test]
fn temp_variable_2() {
    let mut ec = EmitContext::new();
    let opts = AutoGenerateOptions {
        prefix: "A".to_string(),
        suffix: "B".to_string(),
        ..Default::default()
    };
    let name1 = ec.factory().new_temp_variable_ex(opts.clone());
    let name2 = ec.factory().new_temp_variable_ex(opts);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "A_aB");
    assert_eq!(g.generate_name(name2), "A_bB");
}

// Go: internal/printer/namegenerator_test.go:TestTempVariable3
#[test]
fn temp_variable_3() {
    let mut ec = EmitContext::new();
    let name1 = ec.factory().new_temp_variable();

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "_a");
    assert_eq!(g.generate_name(name1), "_a");
}

// Go: internal/printer/namegenerator_test.go:TestTempVariableScoped
#[test]
fn temp_variable_scoped() {
    let mut ec = EmitContext::new();
    let name1 = ec.factory().new_temp_variable();
    let name2 = ec.factory().new_temp_variable();

    let mut g = NameGenerator::new(&ec);
    let text1 = g.generate_name(name1);
    g.push_scope(false);
    let text2 = g.generate_name(name2);
    g.pop_scope(false);

    assert_eq!(text1, "_a");
    assert_eq!(text2, "_a");
}

// Go: internal/printer/namegenerator_test.go:TestTempVariableScopedReserved
#[test]
fn temp_variable_scoped_reserved() {
    let mut ec = EmitContext::new();
    let name1 = ec.factory().new_temp_variable_ex(AutoGenerateOptions {
        flags: GeneratedIdentifierFlags::RESERVED_IN_NESTED_SCOPES,
        ..Default::default()
    });
    let name2 = ec.factory().new_temp_variable();

    let mut g = NameGenerator::new(&ec);
    let text1 = g.generate_name(name1);
    g.push_scope(false);
    let text2 = g.generate_name(name2);
    g.pop_scope(false);

    assert_eq!(text1, "_a");
    assert_eq!(text2, "_b");
}

// Go: internal/printer/namegenerator_test.go:TestLoopVariable1
#[test]
fn loop_variable_1() {
    let mut ec = EmitContext::new();
    let name1 = ec.factory().new_loop_variable();
    let name2 = ec.factory().new_loop_variable();

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "_i");
    assert_eq!(g.generate_name(name2), "_a");
}

// Go: internal/printer/namegenerator_test.go:TestLoopVariable2
#[test]
fn loop_variable_2() {
    let mut ec = EmitContext::new();
    let opts = AutoGenerateOptions {
        prefix: "A".to_string(),
        suffix: "B".to_string(),
        ..Default::default()
    };
    let name1 = ec.factory().new_loop_variable_ex(opts.clone());
    let name2 = ec.factory().new_loop_variable_ex(opts);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "A_iB");
    assert_eq!(g.generate_name(name2), "A_aB");
}

// Go: internal/printer/namegenerator_test.go:TestLoopVariable3
#[test]
fn loop_variable_3() {
    let mut ec = EmitContext::new();
    let name1 = ec.factory().new_loop_variable();

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "_i");
    assert_eq!(g.generate_name(name1), "_i");
}

// Go: internal/printer/namegenerator_test.go:TestLoopVariableScoped
#[test]
fn loop_variable_scoped() {
    let mut ec = EmitContext::new();
    let name1 = ec.factory().new_loop_variable();
    let name2 = ec.factory().new_loop_variable();

    let mut g = NameGenerator::new(&ec);
    let text1 = g.generate_name(name1);
    g.push_scope(false);
    let text2 = g.generate_name(name2);
    g.pop_scope(false);

    assert_eq!(text1, "_i");
    assert_eq!(text2, "_i");
}

// Go: internal/printer/namegenerator_test.go:TestUniqueName1
#[test]
fn unique_name_1() {
    let mut ec = EmitContext::new();
    let name1 = ec.factory().new_unique_name("foo");
    let name2 = ec.factory().new_unique_name("foo");

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "foo_1");
    assert_eq!(g.generate_name(name2), "foo_2");
}

// Go: internal/printer/namegenerator_test.go:TestUniqueName2
#[test]
fn unique_name_2() {
    let mut ec = EmitContext::new();
    let name1 = ec.factory().new_unique_name("foo");

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "foo_1");
    // Expected to be same because generate_name goes off node identity.
    assert_eq!(g.generate_name(name1), "foo_1");
}

// Go: internal/printer/namegenerator_test.go:TestUniqueNameScoped
#[test]
fn unique_name_scoped() {
    let mut ec = EmitContext::new();
    let name1 = ec.factory().new_unique_name("foo");
    let name2 = ec.factory().new_unique_name("foo");

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "foo_1");

    g.push_scope(false);
    // Matches Strada, but is incorrect.
    assert_eq!(g.generate_name(name2), "foo_2");
    g.pop_scope(false);
}

// Go: internal/printer/namegenerator_test.go:TestUniquePrivateName1
#[test]
fn unique_private_name_1() {
    let mut ec = EmitContext::new();
    let name1 = ec.factory().new_unique_private_name("#foo");
    let name2 = ec.factory().new_unique_private_name("#foo");

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "#foo_1");
    assert_eq!(g.generate_name(name2), "#foo_2");
}

// Go: internal/printer/namegenerator_test.go:TestUniquePrivateName2
#[test]
fn unique_private_name_2() {
    let mut ec = EmitContext::new();
    let name1 = ec.factory().new_unique_private_name("#foo");

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "#foo_1");
    assert_eq!(g.generate_name(name1), "#foo_1");
}

// Node-based generated names (`GenerateNameForNode`).
//
// Go drives these by parsing+binding real source and calling `.Name()`; the
// binder-free port synthesizes the equivalent source node via the arena
// factory, then requests a node-based generated name and asserts the
// materialized text. Cases that require binder `Locals`
// (`isUniqueLocalName` for module/enum) or an external module specifier
// (import/export) are DEFERRED (see namegenerator.rs / tests.md).

// Go: internal/printer/namegenerator_test.go:TestGeneratedNameForIdentifier1
#[test]
fn generated_name_for_identifier_1() {
    let mut ec = EmitContext::new();
    let n = ec.factory().new_identifier("f");
    let name1 = ec.factory().new_generated_name_for_node(n);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "f_1");
}

// Go: internal/printer/namegenerator_test.go:TestGeneratedNameForIdentifier2
#[test]
fn generated_name_for_identifier_2() {
    let mut ec = EmitContext::new();
    let n = ec.factory().new_identifier("f");
    let name1 = ec.factory().new_generated_name_for_node_ex(
        n,
        AutoGenerateOptions {
            prefix: "a".to_string(),
            suffix: "b".to_string(),
            ..Default::default()
        },
    );

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "afb");
}

// Go: internal/printer/namegenerator_test.go:TestGeneratedNameForIdentifier3
#[test]
fn generated_name_for_identifier_3() {
    let mut ec = EmitContext::new();
    let n = ec.factory().new_identifier("f");
    let name1 = ec.factory().new_generated_name_for_node_ex(
        n,
        AutoGenerateOptions {
            prefix: "a".to_string(),
            suffix: "b".to_string(),
            ..Default::default()
        },
    );
    let name2 = ec.factory().new_generated_name_for_node(name1);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name2), "afb_1");
}

// Cached node-based names are stable: two generated names derived from the same
// source node yield identical text (the second is a node-id cache hit).
//
// Go's `TestGeneratedNameForNodeCached` proves this with a namespace node, which
// requires binder `Locals` (DEFERRED). The binder-free port exercises the same
// node-id cache via an identifier node.
// Go: internal/printer/namegenerator_test.go:TestGeneratedNameForNodeCached
#[test]
fn generated_name_for_node_cached() {
    let mut ec = EmitContext::new();
    let n = ec.factory().new_identifier("foo");
    let name1 = ec.factory().new_generated_name_for_node(n);
    let name2 = ec.factory().new_generated_name_for_node(n);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "foo_1");
    assert_eq!(g.generate_name(name2), "foo_1");
}

// Go: internal/printer/namegenerator_test.go:TestGeneratedNameForExportAssignment
#[test]
fn generated_name_for_export_assignment() {
    let mut ec = EmitContext::new();
    // `export default 0`
    let zero = ec.arena_mut().new_numeric_literal("0", TokenFlags::NONE);
    let n = ec
        .arena_mut()
        .new_export_assignment(None, false, None, zero);
    let name1 = ec.factory().new_generated_name_for_node(n);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "default_1");
}

// Go: internal/printer/namegenerator_test.go:TestGeneratedNameForClassExpression
#[test]
fn generated_name_for_class_expression() {
    let mut ec = EmitContext::new();
    // `(class {})`
    let n = ec.arena_mut().new_class_like(
        Kind::ClassExpression,
        None,
        None,
        None,
        None,
        NodeList::new(vec![]),
    );
    let name1 = ec.factory().new_generated_name_for_node(n);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "class_1");
}

// Go: internal/printer/namegenerator_test.go:TestGeneratedNameForFunctionDeclaration1
#[test]
fn generated_name_for_function_declaration_1() {
    let mut ec = EmitContext::new();
    // `function f() {}`
    let f = ec.arena_mut().new_identifier("f");
    let n = ec.arena_mut().new_function_declaration(
        None,
        None,
        Some(f),
        None,
        NodeList::new(vec![]),
        None,
        None,
        None,
    );
    let name1 = ec.factory().new_generated_name_for_node(n);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "f_1");
}

// Go: internal/printer/namegenerator_test.go:TestGeneratedNameForFunctionDeclaration2
#[test]
fn generated_name_for_function_declaration_2() {
    let mut ec = EmitContext::new();
    // `export default function () {}`
    let n = ec.arena_mut().new_function_declaration(
        None,
        None,
        None,
        None,
        NodeList::new(vec![]),
        None,
        None,
        None,
    );
    let name1 = ec.factory().new_generated_name_for_node(n);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "default_1");
}

// Go: internal/printer/namegenerator_test.go:TestGeneratedNameForClassDeclaration1
#[test]
fn generated_name_for_class_declaration_1() {
    let mut ec = EmitContext::new();
    // `export class C {}`
    let c = ec.arena_mut().new_identifier("C");
    let n = ec.arena_mut().new_class_like(
        Kind::ClassDeclaration,
        None,
        Some(c),
        None,
        None,
        NodeList::new(vec![]),
    );
    let name1 = ec.factory().new_generated_name_for_node(n);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "C_1");
}

// Go: internal/printer/namegenerator_test.go:TestGeneratedNameForClassDeclaration2
#[test]
fn generated_name_for_class_declaration_2() {
    let mut ec = EmitContext::new();
    // `export default class {}`
    let n = ec.arena_mut().new_class_like(
        Kind::ClassDeclaration,
        None,
        None,
        None,
        None,
        NodeList::new(vec![]),
    );
    let name1 = ec.factory().new_generated_name_for_node(n);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "default_1");
}

// Go: internal/printer/namegenerator_test.go:TestGeneratedNameForMethod1
#[test]
fn generated_name_for_method_1() {
    let mut ec = EmitContext::new();
    // `class C { m() {} }` -> the `m` member
    let m = ec.arena_mut().new_identifier("m");
    let method = ec.arena_mut().new_method_declaration(
        None,
        None,
        m,
        None,
        None,
        NodeList::new(vec![]),
        None,
        None,
        None,
    );
    let name1 = ec.factory().new_generated_name_for_node(method);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "m_1");
}

// Go: internal/printer/namegenerator_test.go:TestGeneratedNameForMethod2
#[test]
fn generated_name_for_method_2() {
    let mut ec = EmitContext::new();
    // `class C { 0() {} }` -> the `0` member (non-identifier name)
    let zero = ec.arena_mut().new_numeric_literal("0", TokenFlags::NONE);
    let method = ec.arena_mut().new_method_declaration(
        None,
        None,
        zero,
        None,
        None,
        NodeList::new(vec![]),
        None,
        None,
        None,
    );
    let name1 = ec.factory().new_generated_name_for_node(method);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "_a");
}

// Go: internal/printer/namegenerator_test.go:TestGeneratedPrivateNameForMethod
#[test]
fn generated_private_name_for_method() {
    let mut ec = EmitContext::new();
    // `class C { m() {} }` -> private generated name for the `m` member
    let m = ec.arena_mut().new_identifier("m");
    let method = ec.arena_mut().new_method_declaration(
        None,
        None,
        m,
        None,
        None,
        NodeList::new(vec![]),
        None,
        None,
        None,
    );
    let name1 = ec.factory().new_generated_private_name_for_node(method);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "#m_1");
}

// Go: internal/printer/namegenerator_test.go:TestGeneratedNameForComputedPropertyName
#[test]
fn generated_name_for_computed_property_name() {
    let mut ec = EmitContext::new();
    // `class C { [x] }` -> the `[x]` computed name
    let x = ec.arena_mut().new_identifier("x");
    let computed = ec.arena_mut().new_computed_property_name(x);
    let name1 = ec.factory().new_generated_name_for_node(computed);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "_a");
}

// Go: internal/printer/namegenerator_test.go:TestGeneratedNameForOther
#[test]
fn generated_name_for_other() {
    let mut ec = EmitContext::new();
    // An object literal expression: a node kind with no dedicated name strategy.
    let n = ec
        .arena_mut()
        .new_object_literal_expression(NodeList::new(vec![]));
    let name1 = ec.factory().new_generated_name_for_node(n);

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "_a");
}

// Go: internal/printer/namegenerator_test.go:TestUniquePrivateNameScoped
#[test]
fn unique_private_name_scoped() {
    let mut ec = EmitContext::new();
    let name1 = ec.factory().new_unique_private_name("#foo");
    let name2 = ec.factory().new_unique_private_name("#foo");

    let mut g = NameGenerator::new(&ec);
    assert_eq!(g.generate_name(name1), "#foo_1");

    // Private names are always reserved in nested scopes.
    g.push_scope(false);
    assert_eq!(g.generate_name(name2), "#foo_2");
    g.pop_scope(false);
}
