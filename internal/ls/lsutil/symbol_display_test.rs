use super::*;

// Go: internal/ls/lsutil/symbol_display.go:ScriptElementKind
#[test]
fn script_element_kind_discriminants_match_iota() {
    assert_eq!(ScriptElementKind::Unknown as i32, 0);
    assert_eq!(ScriptElementKind::Warning as i32, 1);
    assert_eq!(ScriptElementKind::Keyword as i32, 2);
    assert_eq!(ScriptElementKind::ClassElement as i32, 5);
    assert_eq!(ScriptElementKind::FunctionElement as i32, 15);
    assert_eq!(
        ScriptElementKind::ConstructorImplementationElement as i32,
        22
    );
    assert_eq!(ScriptElementKind::LinkText as i32, 38);
}

// Go: internal/ls/lsutil/symbol_display.go:ScriptElementKind (zero value)
#[test]
fn script_element_kind_default_is_unknown() {
    assert_eq!(ScriptElementKind::default(), ScriptElementKind::Unknown);
}

// Go: internal/ls/lsutil/symbol_display.go:ScriptElementKindModifier (iota bit layout)
#[test]
fn modifier_bit_values_start_at_bit_one() {
    // Go's iota starts `Public` at `1 << iota` with iota == 1, so bit 0 is unused.
    assert_eq!(ScriptElementKindModifier::PUBLIC.bits(), 1 << 1);
    assert_eq!(ScriptElementKindModifier::PRIVATE.bits(), 1 << 2);
    assert_eq!(ScriptElementKindModifier::PROTECTED.bits(), 1 << 3);
    assert_eq!(ScriptElementKindModifier::EXPORTED.bits(), 1 << 4);
    assert_eq!(ScriptElementKindModifier::AMBIENT.bits(), 1 << 5);
    assert_eq!(ScriptElementKindModifier::DEPRECATED.bits(), 1 << 9);
    assert_eq!(ScriptElementKindModifier::DTS.bits(), 1 << 10);
    assert_eq!(ScriptElementKindModifier::CJS.bits(), 1 << 21);
    assert_eq!(ScriptElementKindModifier::empty().bits(), 0);
}

// Go: internal/ls/lsutil/symbol_display.go:ScriptElementKindModifier.Strings
#[test]
fn strings_returns_names_in_table_order() {
    let m = ScriptElementKindModifier::STATIC | ScriptElementKindModifier::PUBLIC;
    // Table order is public before static, regardless of insertion order.
    assert_eq!(m.strings(), vec!["public", "static"]);
}

// Go: internal/ls/lsutil/symbol_display.go:ScriptElementKindModifier.Strings
#[test]
fn strings_uses_dotted_names_for_file_extensions() {
    let m = ScriptElementKindModifier::DTS | ScriptElementKindModifier::TSX;
    assert_eq!(m.strings(), vec![".d.ts", ".tsx"]);
}

// Go: internal/ls/lsutil/symbol_display.go:ScriptElementKindModifier.Strings
#[test]
fn strings_empty_for_none() {
    assert!(ScriptElementKindModifier::empty().strings().is_empty());
}

// Go: internal/ls/lsutil/symbol_display.go:ScriptElementKindModifier.Strings
#[test]
fn strings_maps_exported_and_ambient_to_keywords() {
    let m = ScriptElementKindModifier::EXPORTED | ScriptElementKindModifier::AMBIENT;
    assert_eq!(m.strings(), vec!["export", "declare"]);
}

// Go: internal/ls/lsutil/symbol_display.go:FileExtensionKindModifiers
#[test]
fn file_extension_modifiers_contains_all_extension_flags() {
    for flag in [
        ScriptElementKindModifier::DTS,
        ScriptElementKindModifier::TS,
        ScriptElementKindModifier::TSX,
        ScriptElementKindModifier::JS,
        ScriptElementKindModifier::JSX,
        ScriptElementKindModifier::JSON,
        ScriptElementKindModifier::DMTS,
        ScriptElementKindModifier::MTS,
        ScriptElementKindModifier::MJS,
        ScriptElementKindModifier::DCTS,
        ScriptElementKindModifier::CTS,
        ScriptElementKindModifier::CJS,
    ] {
        assert!(FILE_EXTENSION_KIND_MODIFIERS.contains(flag), "{flag:?}");
    }
    // Non-extension flags are excluded.
    for flag in [
        ScriptElementKindModifier::PUBLIC,
        ScriptElementKindModifier::STATIC,
        ScriptElementKindModifier::DEPRECATED,
    ] {
        assert!(!FILE_EXTENSION_KIND_MODIFIERS.contains(flag), "{flag:?}");
    }
}
