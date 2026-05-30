use super::*;
use std::collections::HashMap;

// Go has no test for mapbuilder.go; these are behavior-level tests for the
// dual-form (base vs builder) overlay. Here `VBase = i32` and `VBuilder =
// String`, with `build` mapping a builder string to its length.

fn make() -> MapBuilder<String, i32, String> {
    let base: HashMap<String, i32> = HashMap::from([("a".to_string(), 1)]);
    MapBuilder::new(base, |n: &i32| n.to_string(), |s: &String| s.len() as i32)
}

// Go: internal/project/dirty/mapbuilder.go:MapBuilder.Build
#[test]
fn build_passes_through_base_when_no_changes() {
    let mb = make();
    let result = mb.build();
    assert_eq!(result.len(), 1);
    assert_eq!(result.get("a"), Some(&1));
}

// Go: internal/project/dirty/mapbuilder.go:MapBuilder.Set
#[test]
fn set_adds_builder_value_built_into_base_form() {
    let mut mb = make();
    mb.set("b".to_string(), "xy".to_string());
    assert!(mb.has(&"b".to_string()));
    let result = mb.build();
    assert_eq!(result.len(), 2);
    assert_eq!(result.get("a"), Some(&1));
    // "xy" has length 2.
    assert_eq!(result.get("b"), Some(&2));
}

// Go: internal/project/dirty/mapbuilder.go:MapBuilder.Delete
#[test]
fn delete_removes_key_and_clears_dirty() {
    let mut mb = make();
    mb.set("b".to_string(), "xyz".to_string());
    mb.delete("b".to_string());
    assert!(!mb.has(&"b".to_string()));

    mb.delete("a".to_string());
    assert!(!mb.has(&"a".to_string()));

    let result = mb.build();
    assert!(result.is_empty());
}

// Go: internal/project/dirty/mapbuilder.go:MapBuilder.Set
#[test]
fn set_clears_pending_delete() {
    let mut mb = make();
    mb.delete("a".to_string());
    assert!(!mb.has(&"a".to_string()));
    mb.set("a".to_string(), "zzzz".to_string());
    assert!(mb.has(&"a".to_string()));
    let result = mb.build();
    // "zzzz" has length 4.
    assert_eq!(result.get("a"), Some(&4));
}

// Go: internal/project/dirty/mapbuilder.go:MapBuilder.Clear
#[test]
fn clear_marks_all_base_keys_deleted() {
    let mut mb = make();
    mb.set("b".to_string(), "xy".to_string());
    mb.clear();
    assert!(!mb.has(&"a".to_string()));
    assert!(!mb.has(&"b".to_string()));
    let result = mb.build();
    assert!(result.is_empty());
}

// Go: internal/project/dirty/mapbuilder.go:MapBuilder.Has
#[test]
fn has_reflects_base_overlay_and_deletes() {
    let mb = make();
    assert!(mb.has(&"a".to_string()));
    assert!(!mb.has(&"missing".to_string()));
}
