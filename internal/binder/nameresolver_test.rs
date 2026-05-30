//! Unit tests for the name-resolution helpers.

use super::*;
use rustc_hash::FxHashMap;
use tsgo_ast::{NodeArena, Symbol, SymbolId};

// Go: internal/binder/nameresolver.go:NameResolver
#[test]
fn name_resolver_new_is_empty() {
    let r = NameResolver::new();
    assert!(r.globals.is_empty());
}

// Go: internal/binder/nameresolver.go:GetLocalSymbolForExportDefault
#[test]
fn local_symbol_for_export_default_none_when_no_declarations() {
    let arena = NodeArena::new();
    let symbols: Vec<Symbol> = vec![Symbol::default()];
    let locals: FxHashMap<_, _> = FxHashMap::default();
    assert_eq!(
        get_local_symbol_for_export_default(&symbols, &locals, &arena, SymbolId(0)),
        None
    );
}
