use super::*;

// Go has no test for box.go; these are behavior-level tests exercising the
// public copy-on-write surface. Expected values are derived from the Go
// implementation in box.go.

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

// Go: internal/project/dirty/box.go:NewBox
#[test]
fn new_box_starts_clean_with_value_equal_to_original() {
    let b = Box::new(v("orig"));
    assert_eq!(b.value(), v("orig"));
    assert_eq!(b.original(), v("orig"));
    assert!(!b.dirty());
    assert_eq!(b.finalize(), (v("orig"), false));
}

// Go: internal/project/dirty/box.go:Box.Change
#[test]
fn change_mutates_value_marks_dirty_and_preserves_original() {
    let b = Box::new(v("orig"));
    b.change(|val| val.data = "changed".into());
    assert_eq!(b.value(), v("changed"));
    assert_eq!(b.original(), v("orig"));
    assert!(b.dirty());
    assert_eq!(b.finalize(), (v("changed"), true));
}

// Go: internal/project/dirty/box.go:Box.Set
#[test]
fn set_replaces_value_marks_dirty_and_clears_delete() {
    let b = Box::new(v("orig"));
    b.delete();
    b.set(v("new"));
    assert_eq!(b.value(), v("new"));
    assert!(b.dirty());
    assert_eq!(b.finalize(), (v("new"), true));
}

// Go: internal/project/dirty/box.go:Box.Delete
#[test]
fn delete_marks_deleted_and_value_is_zero() {
    let b = Box::new(v("orig"));
    b.delete();
    assert_eq!(b.value(), V::default());
    assert_eq!(b.original(), v("orig"));
    assert_eq!(b.finalize(), (V::default(), true));
}

// Go: internal/project/dirty/box.go:Box.ChangeIf
#[test]
fn change_if_applies_only_when_condition_holds() {
    let b = Box::new(v("orig"));
    let applied = b.change_if(|val| val.data == "nope", |val| val.data = "x".into());
    assert!(!applied);
    assert!(!b.dirty());
    assert_eq!(b.value(), v("orig"));

    let applied = b.change_if(|val| val.data == "orig", |val| val.data = "yes".into());
    assert!(applied);
    assert!(b.dirty());
    assert_eq!(b.value(), v("yes"));
}

// Go: internal/project/dirty/box.go:Box.Locked
#[test]
fn locked_passes_box_as_value_view() {
    let b = Box::new(v("orig"));
    b.locked(|view: &dyn Value<V>| {
        assert_eq!(view.value(), v("orig"));
        view.change(&mut |val| val.data = "via_locked".into());
    });
    assert_eq!(b.value(), v("via_locked"));
    assert!(b.dirty());
}
