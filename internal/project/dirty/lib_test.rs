use super::*;
use std::collections::HashMap;

// Go has no test for map.go; these are behavior-level tests for the
// copy-on-write Map/MapEntry surface. Expected values are derived from the Go
// implementation in map.go.

#[derive(Clone, Default, PartialEq, Debug)]
struct V {
    data: String,
}

impl Cloneable for V {
    fn clone_cow(&self) -> Self {
        self.clone()
    }
}

fn v(s: &str) -> V {
    V { data: s.into() }
}

fn base() -> HashMap<String, V> {
    HashMap::from([("a".to_string(), v("a0")), ("b".to_string(), v("b0"))])
}

// Go: internal/project/dirty/map.go:Map.Get
#[test]
fn get_reads_base_value_clean_and_finalize_unchanged() {
    let m = Map::new(base());
    let entry = m.get(&"a".to_string()).expect("entry exists");
    assert_eq!(entry.value(), v("a0"));
    assert_eq!(entry.original(), v("a0"));
    assert!(!entry.dirty());
    assert!(m.get(&"missing".to_string()).is_none());

    let (result, changed) = m.finalize();
    assert!(!changed);
    assert_eq!(result.len(), 2);
    assert_eq!(result.get("a"), Some(&v("a0")));
}

// Go: internal/project/dirty/map.go:Map.Change
#[test]
fn change_clones_on_write_and_tracks_in_overlay() {
    let m = Map::new(base());
    m.change(&"a".to_string(), |val| val.data = "a1".into());

    // A re-get returns the same dirty entry, reflecting the change.
    let entry = m.get(&"a".to_string()).expect("entry exists");
    assert!(entry.dirty());
    assert_eq!(entry.value(), v("a1"));
    assert_eq!(entry.original(), v("a0"));

    // Base map value remains untouched (copy-on-write).
    let (result, changed) = m.finalize();
    assert!(changed);
    assert_eq!(result.get("a"), Some(&v("a1")));
    assert_eq!(result.get("b"), Some(&v("b0")));
}

// Go: internal/project/dirty/map.go:Map.Add
#[test]
fn add_inserts_fresh_dirty_entry() {
    let m = Map::new(base());
    m.add("c".to_string(), v("c0"));
    let entry = m.get(&"c".to_string()).expect("added entry");
    assert!(entry.dirty());
    assert_eq!(entry.value(), v("c0"));
    // Added entries have no original (zero value).
    assert_eq!(entry.original(), V::default());

    let (result, changed) = m.finalize();
    assert!(changed);
    assert_eq!(result.len(), 3);
    assert_eq!(result.get("c"), Some(&v("c0")));
}

// Go: internal/project/dirty/map.go:Map.Delete
#[test]
fn delete_removes_key_on_finalize() {
    let m = Map::new(base());
    assert!(m.try_delete(&"a".to_string()));
    assert!(!m.try_delete(&"missing".to_string()));
    assert!(m.get(&"a".to_string()).is_none());

    let (result, changed) = m.finalize();
    assert!(changed);
    assert_eq!(result.len(), 1);
    assert!(!result.contains_key("a"));
    assert_eq!(result.get("b"), Some(&v("b0")));
}

// Go: internal/project/dirty/map.go:Map.Change
#[test]
#[should_panic(expected = "tried to change a non-existent entry")]
fn change_panics_on_missing_key() {
    let m = Map::new(base());
    m.change(&"missing".to_string(), |val| val.data = "x".into());
}

// Go: internal/project/dirty/map.go:Map.Delete
#[test]
#[should_panic(expected = "tried to delete a non-existent entry")]
fn delete_panics_on_missing_key() {
    let m = Map::new(base());
    m.delete(&"missing".to_string());
}

// Go: internal/project/dirty/map.go:Map.Range
#[test]
fn range_yields_live_entries_and_skips_deleted() {
    let m = Map::new(base());
    m.add("c".to_string(), v("c0"));
    m.try_delete(&"a".to_string());

    let mut seen: HashMap<String, V> = HashMap::new();
    m.range(|entry| {
        seen.insert(entry.key(), entry.value());
        true
    });

    // "a" was deleted; "b" comes from base; "c" was added.
    assert_eq!(seen.len(), 2);
    assert_eq!(seen.get("b"), Some(&v("b0")));
    assert_eq!(seen.get("c"), Some(&v("c0")));
    assert!(!seen.contains_key("a"));
}

// Go: internal/project/dirty/map.go:Map.Clear
#[test]
fn clear_empties_overlay_and_base() {
    let mut m = Map::new(base());
    m.add("c".to_string(), v("c0"));
    m.clear();
    assert!(m.get(&"a".to_string()).is_none());
    assert!(m.get(&"c".to_string()).is_none());
    let (result, changed) = m.finalize();
    assert!(!changed);
    assert!(result.is_empty());
}

// Go: internal/project/dirty/map.go:MapEntry.Replace
#[test]
fn replace_sets_value_and_marks_dirty() {
    let m = Map::new(base());
    let entry = m.get(&"a".to_string()).expect("entry");
    entry.replace(v("replaced"));
    assert!(entry.dirty());
    assert_eq!(entry.value(), v("replaced"));
    assert_eq!(entry.original(), v("a0"));
    let (result, _) = m.finalize();
    assert_eq!(result.get("a"), Some(&v("replaced")));
}

// Go: internal/project/dirty/map.go:MapEntry.ChangeIf
#[test]
fn change_if_applies_only_when_condition_holds() {
    let m = Map::new(base());
    let entry = m.get(&"a".to_string()).expect("entry");
    assert!(!entry.change_if(|val| val.data == "nope", |val| val.data = "x".into()));
    assert!(!entry.dirty());
    assert!(entry.change_if(|val| val.data == "a0", |val| val.data = "a1".into()));
    assert!(entry.dirty());
    assert_eq!(entry.value(), v("a1"));
}

// Go: internal/project/dirty/map.go:MapEntry.Locked
#[test]
fn locked_passes_entry_as_value_view() {
    let m = Map::new(base());
    let entry = m.get(&"a".to_string()).expect("entry");
    entry.locked(|view: &dyn Value<V>| {
        assert_eq!(view.value(), v("a0"));
        view.change(&mut |val| val.data = "via_locked".into());
    });
    assert_eq!(entry.value(), v("via_locked"));
    assert!(entry.dirty());
}

// Go: internal/project/dirty/map.go:MapEntry.Change
#[test]
#[should_panic(expected = "tried to change a deleted entry")]
fn change_panics_on_deleted_entry() {
    let m = Map::new(base());
    let entry = m.get(&"a".to_string()).expect("entry");
    entry.delete();
    entry.change(|val| val.data = "x".into());
}
