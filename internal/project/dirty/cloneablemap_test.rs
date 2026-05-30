use super::*;

// Go has no test for cloneablemap.go; this is a behavior-level test for the
// `maps.Clone`-equivalent shallow clone exposed via `Cloneable`.

// Go: internal/project/dirty/cloneablemap.go:CloneableMap.Clone
#[test]
fn clone_cow_produces_independent_map() {
    let mut original: CloneableMap<&str, i32> = CloneableMap::new();
    original.insert("a", 1);
    original.insert("b", 2);

    let mut clone = original.clone_cow();
    clone.insert("c", 3);
    *clone.get_mut("a").unwrap() = 99;

    // Mutating the clone does not affect the original map's entries.
    assert_eq!(original.len(), 2);
    assert_eq!(original.get("a"), Some(&1));
    assert!(original.get("c").is_none());

    assert_eq!(clone.len(), 3);
    assert_eq!(clone.get("a"), Some(&99));
}
