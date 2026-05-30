use super::*;
use crate::generatedidentifierflags::GeneratedIdentifierFlags;
use tsgo_ast::{Kind, NodeArena};

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
