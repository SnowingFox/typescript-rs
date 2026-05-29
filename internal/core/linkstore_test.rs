//! Behavior tests for `LinkStore` (Go has no `_test.go`; behavior-level).

use super::*;

// Go: internal/core/linkstore.go:Has/TryGet (behavior-level; no Go _test.go)
#[test]
fn absent_key_reports_has_false_and_try_get_none() {
    let store: LinkStore<&str, i32> = LinkStore::default();
    assert!(!store.has(&"missing"));
    assert_eq!(store.try_get(&"missing"), None);
}

// Go: internal/core/linkstore.go:Get (behavior-level; no Go _test.go)
#[test]
fn get_lazily_creates_default_and_is_mutable() {
    let mut store: LinkStore<&str, i32> = LinkStore::default();
    // First access creates a default (0) and returns a mutable ref.
    assert_eq!(*store.get("a"), 0);
    *store.get("a") = 5;
    assert!(store.has(&"a"));
    assert_eq!(store.try_get(&"a"), Some(&5));
}
