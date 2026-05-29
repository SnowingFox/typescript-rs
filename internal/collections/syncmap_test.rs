use super::*;

// Go: internal/collections/syncmap_test.go:TestSyncMapWithNil
// Go's SyncMap[string, any] tolerates nil values; modeled here as Option<i32>
// where None represents Go's nil.
#[test]
fn sync_map_with_nil() {
    let m: SyncMap<String, Option<i32>> = SyncMap::default();

    // Missing key: load miss.
    let (got1, ok) = m.load(&"foo".to_string());
    assert!(!ok);
    assert_eq!(got1, None);

    // Store nil then load: present with nil value.
    m.store("foo".to_string(), None);
    let (got2, ok) = m.load(&"foo".to_string());
    assert!(ok);
    assert_eq!(got2, None);

    // LoadOrStore nil: newly inserted (not loaded).
    let (too, loaded) = m.load_or_store("too".to_string(), None);
    assert!(!loaded);
    assert_eq!(too, None);

    // Range does not panic.
    m.range(|_k, _v| true);
}
