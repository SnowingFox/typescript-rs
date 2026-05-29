use super::*;

// Go: internal/collections/cow.go:CopyOnWriteMap/Set/ensureOwned (behavior-level supplement)
#[test]
fn cow_map_read_shared_write_clones() {
    let mut c: CopyOnWriteMap<&str, i32> = CopyOnWriteMap::default();
    c.set("a", 1);

    let scope = c.enter_scope();
    // Inherited read sees the parent's entry.
    assert_eq!(c.get(&"a"), Some(&1));
    // First write clones the backing map.
    c.set("a", 2);
    assert_eq!(c.get(&"a"), Some(&2));

    c.restore(scope);
    // Parent's view is unaffected by the child's write.
    assert_eq!(c.get(&"a"), Some(&1));
}

// Go: internal/collections/cow.go:EnterScope (behavior-level supplement)
#[test]
fn cow_enter_scope_restores() {
    let mut c: CopyOnWriteMap<&str, i32> = CopyOnWriteMap::default();
    c.set("a", 1);

    let scope = c.enter_scope();
    c.set("b", 2);
    assert!(c.has(&"b"));

    c.restore(scope);
    assert!(!c.has(&"b"));
    assert!(c.has(&"a"));
}

// Go: internal/collections/cow.go:CopyOnWriteSet (behavior-level supplement)
#[test]
fn cow_set_basic() {
    let mut c: CopyOnWriteSet<&str> = CopyOnWriteSet::default();
    c.add("k");
    assert!(c.has(&"k"));

    let scope = c.enter_scope();
    c.add("j");
    assert!(c.has(&"j"));

    c.restore(scope);
    assert!(!c.has(&"j"));
    assert!(c.has(&"k"));
}
