use super::*;

// Go has no test for util.go; these are behavior-level tests for the
// three branches of CloneMapIfNil.

// Go: internal/project/dirty/util.go:CloneMapIfNil
#[test]
fn returns_dirty_map_when_present() {
    let dirty: HashMap<&str, i32> = HashMap::from([("a", 1), ("b", 2)]);
    let original: HashMap<&str, i32> = HashMap::from([("z", 9)]);
    let result = clone_map_if_nil(Some(dirty), Some(&original));
    assert_eq!(result.len(), 2);
    assert_eq!(result.get("a"), Some(&1));
}

// Go: internal/project/dirty/util.go:CloneMapIfNil
#[test]
fn clones_original_when_dirty_absent() {
    let mut original: HashMap<&str, i32> = HashMap::from([("a", 1)]);
    let result = clone_map_if_nil(None, Some(&original));
    assert_eq!(result.get("a"), Some(&1));

    // The clone is independent of the original.
    original.insert("b", 2);
    assert_eq!(result.len(), 1);
}

// Go: internal/project/dirty/util.go:CloneMapIfNil
#[test]
fn returns_empty_when_dirty_and_original_absent() {
    let result = clone_map_if_nil::<&str, i32>(None, None);
    assert!(result.is_empty());
}
