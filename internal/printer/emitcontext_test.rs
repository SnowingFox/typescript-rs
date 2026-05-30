use super::*;
use crate::generatedidentifierflags::GeneratedIdentifierFlags;
use tsgo_ast::{Kind, NodeArena, NodeData};

// Go: internal/printer/emitcontext.go:NewEmitContext
#[test]
fn new_starts_empty() {
    let ec = EmitContext::new();
    assert_eq!(ec.arena().node_count(), 0);
}

// Go: internal/printer/emitcontext.go:EmitContext.HasAutoGenerateInfo
#[test]
fn has_auto_generate_info_tracks_factory_names() {
    let mut ec = EmitContext::new();
    let name = ec.factory().new_temp_variable();
    assert!(ec.has_auto_generate_info(name));

    // A bare arena node has no auto-generate entry.
    let plain = ec.arena_mut().new_identifier("x");
    assert!(!ec.has_auto_generate_info(plain));
}

// Go: internal/printer/emitcontext.go:EmitContext.GetAutoGenerateInfo
#[test]
fn get_auto_generate_info_records_kind_and_id() {
    let mut ec = EmitContext::new();
    let a = ec.factory().new_temp_variable();
    let b = ec.factory().new_loop_variable();

    let info_a = ec.get_auto_generate_info(a).expect("info a");
    assert_eq!(info_a.flags.kind(), GeneratedIdentifierFlags::AUTO);
    let info_b = ec.get_auto_generate_info(b).expect("info b");
    assert_eq!(info_b.flags.kind(), GeneratedIdentifierFlags::LOOP);
    // Distinct names get distinct ids.
    assert_ne!(info_a.id, info_b.id);
}

// Go: internal/printer/emitcontext.go:EmitContext.EndVariableEnvironment / AddVariableDeclaration
#[test]
fn variable_environment_hoists_declarations_into_a_var_statement() {
    let mut ec = EmitContext::new();
    ec.start_variable_environment();
    let t1 = ec.factory().new_temp_variable();
    ec.add_variable_declaration(t1);
    let t2 = ec.factory().new_temp_variable();
    ec.add_variable_declaration(t2);
    let statements = ec.end_variable_environment();

    assert_eq!(statements.len(), 1, "one hoisted `var` statement");
    let var_statement = statements[0];
    assert_eq!(ec.arena().kind(var_statement), Kind::VariableStatement);
    // The statement declares both temps.
    let declaration_list = match ec.arena().data(var_statement) {
        NodeData::VariableStatement(d) => d.declaration_list,
        other => panic!("expected VariableStatement, got {other:?}"),
    };
    let declarations = match ec.arena().data(declaration_list) {
        NodeData::VariableDeclarationList(d) => d.declarations.nodes.clone(),
        other => panic!("expected VariableDeclarationList, got {other:?}"),
    };
    assert_eq!(declarations.len(), 2);
}

// Go: internal/printer/emitcontext.go:EmitContext.EndVariableEnvironment (empty scope)
#[test]
fn empty_variable_environment_hoists_nothing() {
    let mut ec = EmitContext::new();
    ec.start_variable_environment();
    assert!(ec.end_variable_environment().is_empty());
}

// Go: internal/printer/emitcontext.go:EmitContext.RequestEmitHelper / ReadEmitHelpers
// Requesting the same helper twice records it once; reading returns it and
// clears the requested set.
#[test]
fn requested_helpers_round_trip_and_dedup() {
    use crate::emithelpers::SET_FUNCTION_NAME_HELPER;
    let mut ec = EmitContext::new();
    ec.request_emit_helper(&SET_FUNCTION_NAME_HELPER);
    ec.request_emit_helper(&SET_FUNCTION_NAME_HELPER);
    let helpers = ec.read_emit_helpers();
    assert_eq!(helpers.len(), 1);
    assert!(helpers[0].is(&SET_FUNCTION_NAME_HELPER));
    assert!(ec.read_emit_helpers().is_empty());
}

// Go: internal/printer/emitcontext.go:EmitContext.RequestEmitHelper (dependencies)
// Requesting a helper with dependencies records the dependencies first.
#[test]
fn requested_helper_pulls_in_dependencies_first() {
    use crate::emithelpers::{
        CREATE_BINDING_HELPER, IMPORT_STAR_HELPER, SET_MODULE_DEFAULT_HELPER,
    };
    let mut ec = EmitContext::new();
    ec.request_emit_helper(&IMPORT_STAR_HELPER);
    let helpers = ec.read_emit_helpers();
    // Both dependencies are present and precede the dependent helper.
    let import_star = helpers.iter().position(|h| h.is(&IMPORT_STAR_HELPER));
    let create_binding = helpers.iter().position(|h| h.is(&CREATE_BINDING_HELPER));
    let set_module_default = helpers
        .iter()
        .position(|h| h.is(&SET_MODULE_DEFAULT_HELPER));
    assert!(create_binding < import_star);
    assert!(set_module_default < import_star);
    assert_eq!(helpers.len(), 3);
}

// Go: internal/printer/emitcontext.go:EmitContext.with_arena
#[test]
fn with_arena_preserves_existing_nodes() {
    let mut arena = NodeArena::new();
    let id = arena.new_identifier("preexisting");
    let mut ec = EmitContext::with_arena(arena);
    // The pre-existing node is still readable...
    assert_eq!(ec.arena().kind(id), Kind::Identifier);
    // ...and the factory appends new synthetic nodes after it.
    let t = ec.factory().new_temp_variable();
    assert!(t.index() > id.index());
}
