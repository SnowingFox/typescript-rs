use super::*;

// Go: internal/core/arena.go:New
#[test]
fn arena_new_stable() {
    let mut a: Arena<i32> = Arena::new();
    let id1 = a.alloc(1);
    let id2 = a.alloc(2);
    let id3 = a.alloc(3);
    // All handles resolve to their own value.
    assert_eq!(a[id1], 1);
    assert_eq!(a[id2], 2);
    assert_eq!(a[id3], 3);
    assert_eq!(a.len(), 3);
    // Mutating one does not affect the others (independent slots).
    a[id2] = 20;
    assert_eq!(a[id1], 1);
    assert_eq!(a[id2], 20);
    assert_eq!(a[id3], 3);
}
