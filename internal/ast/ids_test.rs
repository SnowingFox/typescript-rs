use super::*;

// Go: internal/ast/ids.go:NodeId
#[test]
fn node_id_is_copy_and_comparable() {
    let a = NodeId(0);
    let b = NodeId(1);
    let c = a;
    assert_ne!(a, b);
    assert_eq!(a, c);
    assert_eq!(a.index(), 0);
    assert_eq!(b.index(), 1);
}

// Go: internal/ast/ids.go:SymbolId
#[test]
fn symbol_id_distinct_type() {
    let a = SymbolId(2);
    let b = SymbolId(2);
    assert_eq!(a, b);
    assert_eq!(a.index(), 2);
}
