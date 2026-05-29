use super::*;

// Go: internal/ast/modifierflags.go (base bit positions)
#[test]
fn modifier_flags_base_bits() {
    assert_eq!(ModifierFlags::NONE.bits(), 0);
    assert_eq!(ModifierFlags::PUBLIC.bits(), 1 << 0);
    assert_eq!(ModifierFlags::PRIVATE.bits(), 1 << 1);
    assert_eq!(ModifierFlags::PROTECTED.bits(), 1 << 2);
    assert_eq!(ModifierFlags::READONLY.bits(), 1 << 3);
    assert_eq!(ModifierFlags::OVERRIDE.bits(), 1 << 4);
    assert_eq!(ModifierFlags::EXPORT.bits(), 1 << 5);
    assert_eq!(ModifierFlags::STATIC.bits(), 1 << 8);
    assert_eq!(ModifierFlags::DECORATOR.bits(), 1 << 15);
    assert_eq!(ModifierFlags::DEPRECATED.bits(), 1 << 16);
    // JSDoc cache-only segment starts at bit 23.
    assert_eq!(ModifierFlags::JSDOC_PUBLIC.bits(), 1 << 23);
    assert_eq!(ModifierFlags::HAS_COMPUTED_JSDOC_MODIFIERS.bits(), 1 << 28);
    assert_eq!(ModifierFlags::HAS_COMPUTED_FLAGS.bits(), 1 << 29);
}

// Go: internal/ast/modifierflags.go (unions — values from Go literals)
#[test]
fn modifier_flags_unions() {
    assert_eq!(
        ModifierFlags::ACCESSIBILITY_MODIFIER,
        ModifierFlags::PUBLIC | ModifierFlags::PRIVATE | ModifierFlags::PROTECTED
    );
    assert_eq!(
        ModifierFlags::PARAMETER_PROPERTY_MODIFIER,
        ModifierFlags::ACCESSIBILITY_MODIFIER | ModifierFlags::READONLY | ModifierFlags::OVERRIDE
    );
    assert_eq!(
        ModifierFlags::EXPORT_DEFAULT,
        ModifierFlags::EXPORT | ModifierFlags::DEFAULT
    );
    assert_eq!(
        ModifierFlags::MODIFIER,
        ModifierFlags::ALL & !ModifierFlags::DECORATOR
    );
}
