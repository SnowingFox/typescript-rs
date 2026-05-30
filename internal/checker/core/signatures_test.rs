use super::*;
use tsgo_ast::SymbolId;

// Go: internal/checker/types.go:SignatureFlags (bit positions + masks)
#[test]
fn signature_flags_match_go() {
    assert_eq!(SignatureFlags::HAS_REST_PARAMETER.bits(), 1 << 0);
    assert_eq!(SignatureFlags::CONSTRUCT.bits(), 1 << 2);
    assert_eq!(SignatureFlags::ABSTRACT.bits(), 1 << 3);
    assert_eq!(
        SignatureFlags::CALL_CHAIN_FLAGS,
        SignatureFlags::IS_INNER_CALL_CHAIN | SignatureFlags::IS_OUTER_CALL_CHAIN
    );
    assert!(SignatureFlags::PROPAGATING_FLAGS.contains(SignatureFlags::CONSTRUCT));
    assert!(!SignatureFlags::PROPAGATING_FLAGS.contains(SignatureFlags::IS_INNER_CALL_CHAIN));
}

// Go: internal/checker/types.go:Signature (zero value)
#[test]
fn signature_new_defaults() {
    let s = Signature::new(SignatureFlags::HAS_REST_PARAMETER);
    assert!(s.flags.contains(SignatureFlags::HAS_REST_PARAMETER));
    assert_eq!(s.min_argument_count, 0);
    assert!(s.parameters.is_empty());
    assert!(s.resolved_return_type.is_none());
}

// Go: internal/checker/types.go:IndexInfo
#[test]
fn index_info_new() {
    let info = IndexInfo::new(TypeId(7), TypeId(5), true);
    assert_eq!(info.key_type, TypeId(7));
    assert_eq!(info.value_type, TypeId(5));
    assert!(info.is_readonly);
    assert!(info.declaration.is_none());
    assert!(info.index_symbol.is_none());
}

// Go: internal/checker/checker.go:Checker.signatureArena (sequential ids from 1)
#[test]
fn signature_arena_alloc_and_get() {
    let mut arena = SignatureArena::new();
    assert!(arena.is_empty());
    let mut sig = Signature::new(SignatureFlags::NONE);
    sig.parameters.push(SymbolId(3));
    let a = arena.alloc(sig);
    let b = arena.alloc(Signature::new(SignatureFlags::CONSTRUCT));
    assert_eq!(a, SignatureId(1));
    assert_eq!(b, SignatureId(2));
    assert_eq!(arena.len(), 2);
    assert_eq!(arena.get(a).parameters, vec![SymbolId(3)]);
    arena.get_mut(b).min_argument_count = 4;
    assert_eq!(arena.get(b).min_argument_count, 4);
}

// Go: internal/checker/types.go:TypeId-style handle (index is id - 1)
#[test]
fn signature_and_index_ids_arena_index() {
    assert_eq!(SignatureId(1).arena_index(), 0);
    assert_eq!(IndexInfoId(3).arena_index(), 2);
}

// Go: internal/checker/checker.go:Checker.indexInfoArena
#[test]
fn index_info_arena_alloc_and_get() {
    let mut arena = IndexInfoArena::new();
    let id = arena.alloc(IndexInfo::new(TypeId(1), TypeId(2), false));
    assert_eq!(id, IndexInfoId(1));
    assert_eq!(arena.len(), 1);
    assert_eq!(arena.get(id).key_type, TypeId(1));
}
