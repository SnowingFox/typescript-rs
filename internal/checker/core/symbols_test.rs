use super::*;

// Go: internal/checker/types.go:SymbolReferenceLinks (zero value)
#[test]
fn symbol_reference_links_default_is_empty() {
    let links = SymbolReferenceLinks::default();
    assert_eq!(links.reference_kinds, SymbolFlags::empty());
}

// Go: internal/core/linkstore.go:LinkStore.Get (lazy default on first access)
#[test]
fn symbol_links_lazily_create_default_on_get() {
    let mut links: SymbolLinks<SymbolReferenceLinks> = SymbolLinks::default();
    let id = SymbolId(7);
    assert!(!links.has(&id));
    assert_eq!(links.try_get(&id), None);

    // First `get` materializes a default record.
    assert_eq!(links.get(id).reference_kinds, SymbolFlags::empty());
    assert!(links.has(&id));
}

// Go: internal/core/linkstore.go:LinkStore.Get (mutation persists per symbol)
#[test]
fn symbol_links_persist_mutations_per_symbol() {
    let mut links: SymbolLinks<SymbolReferenceLinks> = SymbolLinks::default();
    let a = SymbolId(1);
    let b = SymbolId(2);

    links.get(a).reference_kinds = SymbolFlags::VALUE;
    links.get(b).reference_kinds = SymbolFlags::TYPE;

    assert_eq!(
        links.try_get(&a).unwrap().reference_kinds,
        SymbolFlags::VALUE
    );
    assert_eq!(
        links.try_get(&b).unwrap().reference_kinds,
        SymbolFlags::TYPE
    );
}

// Go: internal/checker/types.go:ValueSymbolLinks / AliasSymbolLinks / ModuleSymbolLinks (zero values)
#[test]
fn new_link_store_records_default_to_empty() {
    let v = ValueSymbolLinks::default();
    assert!(v.resolved_type.is_none());
    assert!(v.target.is_none());
    assert!(!v.function_or_constructor_checked);

    let a = AliasSymbolLinks::default();
    assert!(a.alias_target.is_none());
    assert!(!a.referenced);

    let m = ModuleSymbolLinks::default();
    assert!(m.resolved_exports.is_empty());
    assert!(!m.exports_checked);
}

// Go: internal/checker/types.go:DeclaredTypeLinks / TypeAliasLinks (zero values)
#[test]
fn declared_type_and_type_alias_links_default() {
    let d = DeclaredTypeLinks::default();
    assert!(d.declared_type.is_none());
    assert!(!d.interface_checked);

    let a = TypeAliasLinks::default();
    assert!(a.declared_type.is_none());
    assert!(a.type_parameters.is_empty());
}

// Go: internal/checker/checker.go:Checker.getMergedSymbol / recordMergedSymbol
#[test]
fn merged_symbols_canonicalize_to_target() {
    let mut merged = MergedSymbols::default();
    // Unrecorded symbols map to themselves.
    assert_eq!(merged.get(SymbolId(1)), SymbolId(1));
    merged.record(SymbolId(9), SymbolId(1));
    assert_eq!(merged.get(SymbolId(1)), SymbolId(9));
    // Other symbols are unaffected.
    assert_eq!(merged.get(SymbolId(2)), SymbolId(2));
}

// Go: internal/checker/checker.go:Checker.resolveSymbol / resolveAlias
#[test]
fn skip_alias_follows_only_aliases() {
    // Non-alias symbol returned unchanged.
    assert_eq!(
        skip_alias(SymbolId(1), SymbolFlags::VALUE, None),
        SymbolId(1)
    );
    // Alias with a known target is followed.
    let links = AliasSymbolLinks {
        alias_target: Some(SymbolId(7)),
        ..Default::default()
    };
    assert_eq!(
        skip_alias(SymbolId(1), SymbolFlags::ALIAS, Some(&links)),
        SymbolId(7)
    );
    // Alias whose target is not yet resolved is returned unchanged (deferred).
    assert_eq!(
        skip_alias(SymbolId(3), SymbolFlags::ALIAS, None),
        SymbolId(3)
    );
}
