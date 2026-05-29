use super::*;

// Go: internal/ast/symbol.go:Symbol.IsExternalModule
#[test]
fn symbol_is_external_module() {
    let mut s = Symbol {
        flags: SymbolFlags::VALUE_MODULE,
        name: "\"foo\"".to_string(),
        ..Default::default()
    };
    assert!(s.is_external_module());

    s.name = "foo".to_string();
    assert!(!s.is_external_module());

    s.flags = SymbolFlags::CLASS;
    s.name = "\"foo\"".to_string();
    assert!(!s.is_external_module());
}

// Go: internal/ast/symbol.go:Symbol.CombinedLocalAndExportSymbolFlags
#[test]
fn symbol_combined_flags() {
    let s = Symbol {
        flags: SymbolFlags::FUNCTION,
        ..Default::default()
    };
    assert_eq!(
        s.combined_local_and_export_symbol_flags(None),
        SymbolFlags::FUNCTION
    );
    assert_eq!(
        s.combined_local_and_export_symbol_flags(Some(SymbolFlags::EXPORT_VALUE)),
        SymbolFlags::FUNCTION | SymbolFlags::EXPORT_VALUE
    );
}

// Go: internal/ast/symbol.go:EscapeAllInternalSymbolNames + InternalSymbolName* constants
#[test]
fn escape_internal_symbol_names() {
    assert_eq!(
        escape_all_internal_symbol_names(INTERNAL_SYMBOL_NAME_CALL),
        "__call"
    );
    assert_eq!(
        escape_all_internal_symbol_names(INTERNAL_SYMBOL_NAME_CONSTRUCTOR),
        "__constructor"
    );
    // Names without the prefix are unchanged.
    assert_eq!(escape_all_internal_symbol_names("plain"), "plain");
    // ExportEquals/Default are not prefixed.
    assert_eq!(INTERNAL_SYMBOL_NAME_EXPORT_EQUALS, "export=");
    assert_eq!(INTERNAL_SYMBOL_NAME_DEFAULT, "default");
}
