use super::*;

// Go: internal/ast/checkflags.go (base bit positions)
#[test]
fn check_flags_base_bits() {
    assert_eq!(CheckFlags::NONE.bits(), 0);
    assert_eq!(CheckFlags::INSTANTIATED.bits(), 1 << 0);
    assert_eq!(CheckFlags::SYNTHETIC_PROPERTY.bits(), 1 << 1);
    assert_eq!(CheckFlags::SYNTHETIC_METHOD.bits(), 1 << 2);
    assert_eq!(CheckFlags::LATE.bits(), 1 << 12);
    assert_eq!(CheckFlags::INDEX_SYMBOL.bits(), 1 << 23);
}

// Go: internal/ast/checkflags.go (unions — values from Go literals)
#[test]
fn check_flags_unions() {
    assert_eq!(
        CheckFlags::SYNTHETIC,
        CheckFlags::SYNTHETIC_PROPERTY | CheckFlags::SYNTHETIC_METHOD
    );
    assert_eq!(
        CheckFlags::NON_UNIFORM_AND_LITERAL,
        CheckFlags::HAS_NON_UNIFORM_TYPE | CheckFlags::HAS_LITERAL_TYPE
    );
    assert_eq!(
        CheckFlags::PARTIAL,
        CheckFlags::READ_PARTIAL | CheckFlags::WRITE_PARTIAL
    );
}
