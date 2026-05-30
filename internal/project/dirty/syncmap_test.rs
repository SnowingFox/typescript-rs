use super::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

// Port of Go `internal/project/dirty/syncmap_test.go`. The Go value type is a
// pointer `*testValue`; this port uses an owned `TestValue` (the proxy routing
// keeps concurrent handles consistent without shared mutable values).

#[derive(Clone, Default, PartialEq, Debug)]
struct TestValue {
    data: String,
}

impl Cloneable for TestValue {
    fn clone_cow(&self) -> Self {
        self.clone()
    }
}

fn tv(s: &str) -> TestValue {
    TestValue { data: s.into() }
}

fn sync_base() -> HashMap<String, TestValue> {
    HashMap::from([("a".to_string(), tv("a0")), ("b".to_string(), tv("b0"))])
}

// The following tests are behavior-level (Go has no direct unit test for these
// SyncMap methods); they exercise the copy-on-write core.

// Go: internal/project/dirty/syncmap.go:SyncMap.LoadOrStore
#[test]
fn load_or_store_reports_existing_vs_new() {
    let sync_map = SyncMap::new(sync_base());
    // Existing base key: loaded == true.
    let (existing, loaded) = sync_map.load_or_store("a".to_string(), tv("ignored"));
    assert!(loaded);
    assert_eq!(existing.unwrap().value().data, "a0");
    // Absent key: stores a fresh dirty entry, loaded == false.
    let (created, loaded) = sync_map.load_or_store("c".to_string(), tv("c0"));
    assert!(!loaded);
    let created = created.unwrap();
    assert!(created.dirty());
    assert_eq!(created.value().data, "c0");
}

// Go: internal/project/dirty/syncmap.go:SyncMap.Finalize
#[test]
fn finalize_merges_overlay() {
    let sync_map = SyncMap::new(sync_base());

    // No changes yet.
    let (_, changed) = sync_map.finalize();
    assert!(!changed);

    // Change "a", add "c", delete "b".
    sync_map
        .load(&"a".to_string())
        .unwrap()
        .change(|v| v.data = "a1".into());
    sync_map.load_or_store("c".to_string(), tv("c0"));
    sync_map.delete("b".to_string());

    let (result, changed) = sync_map.finalize();
    assert!(changed);
    assert_eq!(result.get("a"), Some(&tv("a1")));
    assert_eq!(result.get("c"), Some(&tv("c0")));
    assert!(!result.contains_key("b"));
}

// Go: internal/project/dirty/syncmap.go:SyncMap.Delete
#[test]
fn map_delete_removes_base_key() {
    let sync_map = SyncMap::new(sync_base());
    sync_map.delete("a".to_string());
    assert!(sync_map.load(&"a".to_string()).is_none());
    let (result, changed) = sync_map.finalize();
    assert!(changed);
    assert!(!result.contains_key("a"));
    assert_eq!(result.get("b"), Some(&tv("b0")));
}

// Go: internal/project/dirty/syncmap.go:SyncMap.Range
#[test]
fn range_yields_live_entries() {
    let sync_map = SyncMap::new(sync_base());
    sync_map.load_or_store("c".to_string(), tv("c0"));
    sync_map.delete("a".to_string());

    let mut seen: HashMap<String, String> = HashMap::new();
    sync_map.range(|entry| {
        seen.insert(entry.value().data.clone(), String::new());
        true
    });
    // "a" deleted; "b" from base; "c" added.
    assert!(seen.contains_key("b0"));
    assert!(seen.contains_key("c0"));
    assert!(!seen.contains_key("a0"));
}

// Go: internal/project/dirty/syncmap.go:SyncMap.FinalizeWith
#[test]
fn finalize_with_invokes_hooks() {
    use std::cell::RefCell;
    let sync_map = SyncMap::new(sync_base());
    sync_map
        .load(&"a".to_string())
        .unwrap()
        .change(|v| v.data = "a1".into());
    sync_map.load_or_store("c".to_string(), tv("c0"));
    sync_map.delete("b".to_string());

    let changes: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let adds: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let deletes: RefCell<Vec<String>> = RefCell::new(Vec::new());

    let hooks = FinalizationHooks {
        on_change: Some(std::boxed::Box::new(
            |k: &String, old: &TestValue, new: &TestValue| {
                changes
                    .borrow_mut()
                    .push(format!("{}:{}->{}", k, old.data, new.data));
            },
        )),
        on_add: Some(std::boxed::Box::new(|k: &String, v: &TestValue| {
            adds.borrow_mut().push(format!("{}:{}", k, v.data));
        })),
        on_delete: Some(std::boxed::Box::new(|k: &String, _v: &TestValue| {
            deletes.borrow_mut().push(k.clone());
        })),
    };

    let (_result, changed) = sync_map.finalize_with(hooks);
    assert!(changed);
    assert_eq!(changes.into_inner(), vec!["a:a0->a1".to_string()]);
    assert_eq!(adds.into_inner(), vec!["c:c0".to_string()]);
    assert_eq!(deletes.into_inner(), vec!["b".to_string()]);
}

// Go: internal/project/dirty/syncmap_test.go:TestSyncMapProxyFor/no proxy when no race
#[test]
fn no_proxy_when_no_race() {
    let base = HashMap::from([("key1".to_string(), tv("original"))]);
    let sync_map = SyncMap::new(base);

    // Load and modify a single entry - no race condition.
    let entry = sync_map
        .load(&"key1".to_string())
        .expect("entry should be loaded");

    entry.change(|v| v.data = "changed".into());

    // Should not have a proxy since there was no race.
    assert!(
        entry.proxy_for().is_none(),
        "entry should not have proxyFor when no race occurs"
    );
    assert!(entry.dirty());
    assert_eq!(entry.value().data, "changed");
}

// Go: internal/project/dirty/syncmap_test.go:TestSyncMapProxyFor/proxy for race condition
#[test]
fn proxy_for_race_condition() {
    let base = HashMap::from([("key1".to_string(), tv("original"))]);
    let sync_map = SyncMap::new(base);

    // Load the same entry from multiple threads to simulate a race condition.
    let (entry1, entry2) = thread::scope(|s| {
        let h1 = s.spawn(|| {
            sync_map
                .load(&"key1".to_string())
                .expect("entry1 should be loaded")
        });
        let h2 = s.spawn(|| {
            sync_map
                .load(&"key1".to_string())
                .expect("entry2 should be loaded")
        });
        (h1.join().unwrap(), h2.join().unwrap())
    });

    // Both entries should exist and have the same initial value.
    assert_eq!(entry1.value().data, "original");
    assert_eq!(entry2.value().data, "original");
    assert!(!entry1.dirty());
    assert!(!entry2.dirty());

    // Now change both entries concurrently to trigger the proxy mechanism.
    thread::scope(|s| {
        let e1 = &entry1;
        let e2 = &entry2;
        s.spawn(move || e1.change(|v| v.data = "changed_by_entry1".into()));
        s.spawn(move || e2.change(|v| v.data = "changed_by_entry2".into()));
    });

    // After the race both entries should reflect the same final value.
    let final_value1 = entry1.value().data;
    let final_value2 = entry2.value().data;
    assert_eq!(
        final_value1, final_value2,
        "both entries should have the same final value"
    );

    // Both entries should be marked as dirty.
    assert!(entry1.dirty());
    assert!(entry2.dirty());

    // At least one entry should have proxyFor set (the one that lost the race).
    let has_proxy = entry1.proxy_for().is_some() || entry2.proxy_for().is_some();
    assert!(has_proxy, "at least one entry should have proxyFor set");

    // If entry1 has a proxy, it should point to entry2, and vice versa.
    if let Some(p) = entry1.proxy_for() {
        assert!(Arc::ptr_eq(&p, &entry2), "entry1 should proxy to entry2");
    }
    if let Some(p) = entry2.proxy_for() {
        assert!(Arc::ptr_eq(&p, &entry1), "entry2 should proxy to entry1");
    }
}

// Go: internal/project/dirty/syncmap_test.go:TestSyncMapProxyFor/proxy operations delegation
#[test]
fn proxy_operations_delegation() {
    let base = HashMap::from([("key1".to_string(), tv("original"))]);
    let sync_map = SyncMap::new(base);

    let entry1 = sync_map.load(&"key1".to_string()).expect("ok1");
    let entry2 = sync_map.load(&"key1".to_string()).expect("ok2");

    // Force one to become a proxy by making them both dirty in sequence.
    entry1.change(|v| v.data = "changed_by_entry1".into());
    entry2.change(|v| v.data = "changed_by_entry2".into());

    // Determine which is the proxy and which is the target.
    let (proxy, target) = if entry1.proxy_for().is_some() {
        (&entry1, &entry2)
    } else {
        (&entry2, &entry1)
    };

    // Change through proxy should affect target.
    proxy.change(|v| v.data = "changed_through_proxy".into());
    assert_eq!(target.value().data, "changed_through_proxy");
    assert_eq!(proxy.value().data, "changed_through_proxy");

    // ChangeIf through proxy should work.
    let changed = proxy.change_if(
        |v| v.data == "changed_through_proxy",
        |v| v.data = "conditional_change".into(),
    );
    assert!(changed);
    assert_eq!(target.value().data, "conditional_change");
    assert_eq!(proxy.value().data, "conditional_change");

    // Dirty status should be consistent.
    assert_eq!(target.dirty(), proxy.dirty());

    // Locked operations should work through proxy.
    proxy.locked(|v: &dyn Value<TestValue>| {
        v.change(&mut |val| val.data = "locked_change".into());
    });
    assert_eq!(target.value().data, "locked_change");
    assert_eq!(proxy.value().data, "locked_change");
}

// Go: internal/project/dirty/syncmap_test.go:TestSyncMapProxyFor/proxy delete operations
#[test]
fn proxy_delete_operations() {
    let base = HashMap::from([("key1".to_string(), tv("original"))]);
    let sync_map = SyncMap::new(base);

    let entry1 = sync_map.load(&"key1".to_string()).expect("ok");
    let entry2 = sync_map.load(&"key1".to_string()).expect("ok");

    entry1.change(|v| v.data = "modified".into());
    entry2.change(|v| v.data = "modified2".into());

    let proxy = if entry1.proxy_for().is_some() {
        &entry1
    } else {
        &entry2
    };

    // Delete through proxy should affect target.
    proxy.delete();

    // Both should reflect the deletion.
    assert!(
        sync_map.load(&"key1".to_string()).is_none(),
        "key should be deleted from sync map"
    );

    // DeleteIf through proxy should work.
    let base2 = HashMap::from([("key2".to_string(), tv("test"))]);
    let sync_map2 = SyncMap::new(base2);

    let entry3 = sync_map2.load(&"key2".to_string()).expect("ok");
    let entry4 = sync_map2.load(&"key2".to_string()).expect("ok");

    entry3.change(|v| v.data = "modified".into());
    entry4.change(|v| v.data = "modified2".into());

    let proxy2 = if entry3.proxy_for().is_some() {
        &entry3
    } else {
        &entry4
    };

    proxy2.delete_if(|v| v.data == "modified2" || v.data == "modified");

    assert!(
        sync_map2.load(&"key2".to_string()).is_none(),
        "key2 should be deleted conditionally"
    );
}
