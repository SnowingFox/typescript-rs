use super::*;

// Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.Kind
#[test]
fn kind_masks_low_three_bits() {
    let f = GeneratedIdentifierFlags::AUTO | GeneratedIdentifierFlags::RESERVED_IN_NESTED_SCOPES;
    assert_eq!(f.kind(), GeneratedIdentifierFlags::AUTO);
}

// Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.IsAuto
#[test]
fn is_auto() {
    assert!(GeneratedIdentifierFlags::AUTO.is_auto());
    assert!(!GeneratedIdentifierFlags::LOOP.is_auto());
}

// Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.IsLoop
#[test]
fn is_loop() {
    assert!(GeneratedIdentifierFlags::LOOP.is_loop());
    assert!(!GeneratedIdentifierFlags::AUTO.is_loop());
}

// Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.IsUnique
#[test]
fn is_unique() {
    assert!(GeneratedIdentifierFlags::UNIQUE.is_unique());
    assert!(!GeneratedIdentifierFlags::NODE.is_unique());
}

// Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.IsNode
#[test]
fn is_node() {
    assert!(GeneratedIdentifierFlags::NODE.is_node());
    assert!(!GeneratedIdentifierFlags::UNIQUE.is_node());
}

// Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.IsReservedInNestedScopes
#[test]
fn is_reserved_in_nested_scopes() {
    let f = GeneratedIdentifierFlags::UNIQUE | GeneratedIdentifierFlags::RESERVED_IN_NESTED_SCOPES;
    assert!(f.is_reserved_in_nested_scopes());
    assert!(!GeneratedIdentifierFlags::UNIQUE.is_reserved_in_nested_scopes());
}

// Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.IsOptimistic
#[test]
fn is_optimistic() {
    let f = GeneratedIdentifierFlags::UNIQUE | GeneratedIdentifierFlags::OPTIMISTIC;
    assert!(f.is_optimistic());
    assert!(!GeneratedIdentifierFlags::UNIQUE.is_optimistic());
}

// Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.IsFileLevel
#[test]
fn is_file_level() {
    let f = GeneratedIdentifierFlags::UNIQUE | GeneratedIdentifierFlags::FILE_LEVEL;
    assert!(f.is_file_level());
    assert!(!GeneratedIdentifierFlags::UNIQUE.is_file_level());
}

// Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.HasAllowNameSubstitution
#[test]
fn has_allow_name_substitution() {
    let f = GeneratedIdentifierFlags::NODE | GeneratedIdentifierFlags::ALLOW_NAME_SUBSTITUTION;
    assert!(f.has_allow_name_substitution());
    assert!(!GeneratedIdentifierFlags::NODE.has_allow_name_substitution());
}

// Go: internal/printer/generatedidentifierflags.go (constant bit values)
#[test]
fn constant_values_match_go() {
    assert_eq!(GeneratedIdentifierFlags::NONE.bits(), 0);
    assert_eq!(GeneratedIdentifierFlags::AUTO.bits(), 1);
    assert_eq!(GeneratedIdentifierFlags::LOOP.bits(), 2);
    assert_eq!(GeneratedIdentifierFlags::UNIQUE.bits(), 3);
    assert_eq!(GeneratedIdentifierFlags::NODE.bits(), 4);
    assert_eq!(GeneratedIdentifierFlags::KIND_MASK.bits(), 7);
    assert_eq!(
        GeneratedIdentifierFlags::RESERVED_IN_NESTED_SCOPES.bits(),
        1 << 3
    );
    assert_eq!(GeneratedIdentifierFlags::OPTIMISTIC.bits(), 1 << 4);
    assert_eq!(GeneratedIdentifierFlags::FILE_LEVEL.bits(), 1 << 5);
    assert_eq!(
        GeneratedIdentifierFlags::ALLOW_NAME_SUBSTITUTION.bits(),
        1 << 6
    );
}
