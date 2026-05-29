use super::*;

// Go: internal/collections/set.go:Add/Has/Delete/Len (behavior-level supplement)
#[test]
fn set_add_has_delete_len() {
    let mut s: Set<&str> = Set::default();
    s.add("a");
    s.add("b");
    assert!(s.has(&"a"));
    assert_eq!(s.len(), 2);
    s.delete(&"a");
    assert!(!s.has(&"a"));
}

// Go: internal/collections/set.go:AddIfAbsent (behavior-level supplement)
#[test]
fn set_add_if_absent() {
    let mut s: Set<i32> = Set::default();
    assert!(s.add_if_absent(1));
    assert!(!s.add_if_absent(1));
}

// Go: internal/collections/set.go:Union/UnionedWith (behavior-level supplement)
#[test]
fn set_union_and_unioned_with() {
    let mut a = Set::from_items(["a"]);
    let b = Set::from_items(["b"]);
    let u = a.unioned_with(&b);
    assert_eq!(u.len(), 2);
    assert_eq!(a.len(), 1); // unioned_with does not modify the receiver
    a.union(&b);
    assert_eq!(a.len(), 2);
}

// Go: internal/collections/set.go:Equals/IsSubsetOf/Intersects (behavior-level supplement)
#[test]
fn set_equals_subset_intersects() {
    let ab = Set::from_items(["a", "b"]);
    let ab2 = Set::from_items(["a", "b"]);
    assert!(ab.equals(&ab2));

    let a = Set::from_items(["a"]);
    assert!(a.is_subset_of(&ab));
    assert!(a.intersects(&a));

    let b = Set::from_items(["b"]);
    assert!(!a.intersects(&b));
}

// Go: internal/collections/set.go:NewSetFromItems (behavior-level supplement)
#[test]
fn set_from_items() {
    let s = Set::from_items(["a", "a", "b"]);
    assert_eq!(s.len(), 2);
}
