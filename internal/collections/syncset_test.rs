use super::*;

// Go: internal/collections/syncset.go:AddIfAbsent (behavior-level supplement)
#[test]
fn sync_set_add_if_absent() {
    let s: SyncSet<&str> = SyncSet::default();
    assert!(s.add_if_absent("k"));
    assert!(!s.add_if_absent("k"));
    assert!(s.has(&"k"));
}

// Go: internal/collections/syncset.go:Size/IsEmpty/ToSlice (behavior-level supplement)
#[test]
fn sync_set_size_is_empty_to_slice() {
    let s: SyncSet<i32> = SyncSet::default();
    assert!(s.is_empty());
    s.add(1);
    s.add(2);
    assert_eq!(s.size(), 2);
    let mut sl = s.to_slice();
    sl.sort();
    assert_eq!(sl, vec![1, 2]);
}
