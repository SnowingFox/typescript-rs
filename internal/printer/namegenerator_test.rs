use super::*;
use crate::emitcontext::{AutoGenerateOptions, EmitContext};
use crate::generatedidentifierflags::GeneratedIdentifierFlags;

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
