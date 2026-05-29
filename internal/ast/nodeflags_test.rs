use super::*;

// Go: internal/ast/nodeflags.go (base bit positions)
#[test]
fn node_flags_base_bits() {
    assert_eq!(NodeFlags::NONE.bits(), 0);
    assert_eq!(NodeFlags::LET.bits(), 1 << 0);
    assert_eq!(NodeFlags::CONST.bits(), 1 << 1);
    assert_eq!(NodeFlags::USING.bits(), 1 << 2);
    assert_eq!(NodeFlags::REPARSED.bits(), 1 << 3);
    assert_eq!(NodeFlags::SYNTHESIZED.bits(), 1 << 4);
    assert_eq!(NodeFlags::CONTAINS_THIS.bits(), 1 << 7);
    assert_eq!(NodeFlags::JAVA_SCRIPT_FILE.bits(), 1 << 16);
    assert_eq!(NodeFlags::AMBIENT.bits(), 1 << 23);
    assert_eq!(NodeFlags::IN_WITH_STATEMENT.bits(), 1 << 24);
    assert_eq!(NodeFlags::UNREACHABLE.bits(), 1 << 27);
}

// Go: internal/ast/nodeflags.go (combination constants — values from Go literals)
#[test]
fn node_flags_combinations() {
    assert_eq!(
        NodeFlags::BLOCK_SCOPED,
        NodeFlags::LET | NodeFlags::CONST | NodeFlags::USING
    );
    assert_eq!(NodeFlags::CONSTANT, NodeFlags::CONST | NodeFlags::USING);
    assert_eq!(NodeFlags::AWAIT_USING, NodeFlags::CONST | NodeFlags::USING);
    assert_eq!(
        NodeFlags::REACHABILITY_CHECK_FLAGS,
        NodeFlags::HAS_IMPLICIT_RETURN | NodeFlags::HAS_EXPLICIT_RETURN
    );
    assert_eq!(
        NodeFlags::REACHABILITY_AND_EMIT_FLAGS,
        NodeFlags::REACHABILITY_CHECK_FLAGS | NodeFlags::HAS_ASYNC_FUNCTIONS
    );
    assert_eq!(NodeFlags::CONTEXT_FLAGS.bits(), 25263104);
    assert_eq!(NodeFlags::TYPE_EXCLUDES_FLAGS.bits(), (1 << 11) | (1 << 13));
    assert_eq!(
        NodeFlags::PERMANENTLY_SET_INCREMENTAL_FLAGS,
        NodeFlags::POSSIBLY_CONTAINS_DYNAMIC_IMPORT | NodeFlags::POSSIBLY_CONTAINS_IMPORT_META
    );
    // Identifier reuse of the ContainsThis bit.
    assert_eq!(
        NodeFlags::IDENTIFIER_HAS_EXTENDED_UNICODE_ESCAPE,
        NodeFlags::CONTAINS_THIS
    );
}
