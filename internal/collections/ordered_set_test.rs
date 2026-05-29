use super::*;

// Go: internal/collections/ordered_set_test.go:TestOrderedSet
#[test]
fn ordered_set() {
    let mut s: OrderedSet<i32> = OrderedSet::default();
    s.add(1);
    s.add(2);
    s.add(3);

    assert!(s.has(&1));
    assert!(s.has(&2));
    assert!(s.has(&3));

    assert!(s.delete(&2));

    let values: Vec<i32> = s.values().copied().collect();
    assert_eq!(values.len(), 2);
    assert!(values.is_sorted());

    s.clear();
    assert_eq!(s.size(), 0);
    assert!(!s.has(&1));
    assert!(!s.has(&2));
    assert!(!s.has(&3));

    let s2 = s.clone();
    assert_eq!(s2.size(), 0);
}

// Go: internal/collections/ordered_set_test.go:TestOrderedSetWithSizeHint
#[test]
fn ordered_set_with_size_hint() {
    const N: usize = 1024;
    let mut s: OrderedSet<usize> = OrderedSet::with_size_hint(N);
    let cap_before = s.0.capacity();
    for i in 0..N {
        s.add(i);
    }
    let cap_after = s.0.capacity();
    assert_eq!(s.size(), N);
    assert_eq!(cap_before, cap_after);
}
