use super::*;

fn pad_int(n: i32) -> String {
    format!("{n:10}")
}

// Go: internal/collections/ordered_map_test.go:TestOrderedMap
#[test]
fn ordered_map() {
    let mut m: OrderedMap<i32, String> = OrderedMap::default();
    assert!(!m.has(&1));

    const N: i32 = 1000;
    const START: i32 = 1;
    const END: i32 = START + N;

    for i in START..END {
        m.set(i, pad_int(i));
    }
    assert_eq!(m.size(), N as usize);

    // Overwrite existing keys in reverse order; size unchanged.
    for i in (START..END).rev() {
        m.set(i, pad_int(i));
    }
    assert_eq!(m.size(), N as usize);

    for i in START..END {
        let v = m.get(&i);
        assert!(v.is_some());
        assert_eq!(v.unwrap(), &pad_int(i));
    }

    for (k, v) in m.entries() {
        assert_eq!(v, &pad_int(*k));
    }

    let keys: Vec<i32> = m.keys().copied().collect();
    assert_eq!(keys.len(), N as usize);
    assert!(keys.is_sorted());

    let values: Vec<String> = m.values().cloned().collect();
    assert_eq!(values.len(), N as usize);
    assert!(values.is_sorted());

    assert_eq!(*m.keys().next().unwrap(), START);
    assert_eq!(m.values().next().unwrap(), &pad_int(START));

    for i in (START + 1)..END {
        let v = m.delete(&i);
        assert_eq!(v, Some(pad_int(i)));
        assert!(!m.has(&i));
        assert!(m.get(&i).is_none());
        assert!(m.delete(&i).is_none());
    }

    assert_eq!(m.size(), 1);
    assert!(m.has(&START));

    assert_eq!(m.delete(&START), Some(pad_int(START)));
    assert_eq!(m.size(), 0);
}

// Go: internal/collections/ordered_map_test.go:TestOrderedMapClone
#[test]
fn ordered_map_clone() {
    let mut m: OrderedMap<i32, String> = OrderedMap::default();
    m.set(1, "one".to_string());
    m.set(2, "two".to_string());

    let clone = m.clone();
    assert_eq!(clone.size(), 2);
    assert_eq!(clone.keys().copied().collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(
        clone.values().cloned().collect::<Vec<_>>(),
        vec!["one".to_string(), "two".to_string()]
    );
    assert_eq!(clone.get(&1), Some(&"one".to_string()));

    m.delete(&1);

    assert_eq!(m.size(), 1);
    assert_eq!(clone.size(), 2);
    assert_eq!(clone.keys().copied().collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(
        clone.values().cloned().collect::<Vec<_>>(),
        vec!["one".to_string(), "two".to_string()]
    );
}

// Go: internal/collections/ordered_map_test.go:TestOrderedMapClear
#[test]
fn ordered_map_clear() {
    let mut m: OrderedMap<i32, String> = OrderedMap::default();
    m.set(1, "one".to_string());
    m.set(2, "two".to_string());
    m.clear();
    assert_eq!(m.size(), 0);
}

// Go: internal/collections/ordered_map_test.go:TestOrderedMapWithSizeHint
#[test]
fn ordered_map_with_size_hint() {
    const N: usize = 1024;
    let mut m: OrderedMap<usize, usize> = OrderedMap::with_size_hint(N);
    let cap_before = m.0.capacity();
    for i in 0..N {
        m.set(i, i);
    }
    let cap_after = m.0.capacity();
    assert_eq!(m.size(), N);
    // With a size hint, filling exactly N entries must not reallocate.
    assert_eq!(cap_before, cap_after);
    assert!(cap_after >= N);
}

// Go: internal/collections/ordered_map_test.go:TestOrderedMapUnmarshalJSON/UnmarshalJSONV2
#[test]
fn ordered_map_unmarshal_json() {
    let m: OrderedMap<String, serde_json::Value> =
        serde_json::from_str(r#"{"a": 1, "b": "two", "c": { "d": 4 } }"#).unwrap();
    assert_eq!(m.size(), 3);
    assert_eq!(m.get_or_zero(&"a".to_string()).as_f64(), Some(1.0));

    // null is accepted (yields an empty map).
    let m_null: OrderedMap<String, serde_json::Value> = serde_json::from_str("null").unwrap();
    assert_eq!(m_null.size(), 0);

    // A non-object value is an error with the Go-aligned message.
    let r: Result<OrderedMap<String, serde_json::Value>, _> = serde_json::from_str(r#""foo""#);
    let err = r.unwrap_err();
    assert!(
        err.to_string()
            .contains("cannot unmarshal non-object JSON value into Map"),
        "unexpected error: {err}"
    );

    // A key type mismatch (int key from string) is an error.
    let r2: Result<OrderedMap<i32, serde_json::Value>, _> =
        serde_json::from_str(r#"{"a": 1, "b": "two"}"#);
    assert!(r2.is_err());
}
