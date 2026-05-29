use super::*;

// Go: internal/ast/symbolflags.go (base bit positions)
#[test]
fn symbol_flags_base_bits() {
    assert_eq!(SymbolFlags::NONE.bits(), 0);
    assert_eq!(SymbolFlags::FUNCTION_SCOPED_VARIABLE.bits(), 1 << 0);
    assert_eq!(SymbolFlags::BLOCK_SCOPED_VARIABLE.bits(), 1 << 1);
    assert_eq!(SymbolFlags::PROPERTY.bits(), 1 << 2);
    assert_eq!(SymbolFlags::CLASS.bits(), 1 << 5);
    assert_eq!(SymbolFlags::CONST_ENUM.bits(), 1 << 7);
    assert_eq!(SymbolFlags::REGULAR_ENUM.bits(), 1 << 8);
    assert_eq!(SymbolFlags::ALIAS.bits(), 1 << 21);
    assert_eq!(SymbolFlags::GLOBAL_LOOKUP.bits(), 1 << 30);
}

// Go: internal/ast/symbolflags.go (unions — values from Go literals)
#[test]
fn symbol_flags_unions() {
    assert_eq!(SymbolFlags::ALL.bits(), (1 << 30) - 1);
    assert_eq!(
        SymbolFlags::ENUM,
        SymbolFlags::REGULAR_ENUM | SymbolFlags::CONST_ENUM
    );
    assert_eq!(
        SymbolFlags::VARIABLE,
        SymbolFlags::FUNCTION_SCOPED_VARIABLE | SymbolFlags::BLOCK_SCOPED_VARIABLE
    );
    assert_eq!(
        SymbolFlags::ACCESSOR,
        SymbolFlags::GET_ACCESSOR | SymbolFlags::SET_ACCESSOR
    );
    assert_eq!(
        SymbolFlags::MODULE,
        SymbolFlags::VALUE_MODULE | SymbolFlags::NAMESPACE_MODULE
    );
}

// Go: internal/ast/symbolflags.go (excludes — AND NOT semantics)
#[test]
fn symbol_flags_excludes() {
    assert_eq!(
        SymbolFlags::FUNCTION_SCOPED_VARIABLE_EXCLUDES,
        SymbolFlags::VALUE & !SymbolFlags::FUNCTION_SCOPED_VARIABLE
    );
    assert_eq!(
        SymbolFlags::BLOCK_SCOPED_VARIABLE_EXCLUDES,
        SymbolFlags::VALUE
    );
    assert_eq!(
        SymbolFlags::PROPERTY_EXCLUDES,
        SymbolFlags::VALUE & !(SymbolFlags::PROPERTY | SymbolFlags::ACCESSOR)
    );
}
