use super::*;

// Go: internal/collections/multimap.go:Add/Get/Remove (behavior-level supplement)
#[test]
fn multimap_add_get_remove() {
    let mut m: MultiMap<&str, i32> = MultiMap::default();
    m.add("k", 1);
    m.add("k", 2);
    assert_eq!(m.get(&"k"), &[1, 2]);
    m.remove(&"k", &1);
    assert_eq!(m.get(&"k"), &[2]);
    m.remove(&"k", &2);
    assert!(!m.has(&"k"));
    assert_eq!(m.get(&"k"), &[] as &[i32]);
}

// Go: internal/collections/multimap.go:GroupBy (behavior-level supplement)
#[test]
fn multimap_group_by() {
    let m = group_by(vec![1, 2, 3, 4], |v| *v % 2);
    assert_eq!(m.get(&0), &[2, 4]);
    assert_eq!(m.get(&1), &[1, 3]);
    assert_eq!(m.len(), 2);
}
