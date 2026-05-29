use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Sample {
    a: i32,
    b: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct One {
    a: i32,
}

#[derive(Deserialize, PartialEq, Debug)]
struct X {
    x: i32,
}

// Go: internal/json/json.go:Marshal (behavior-level supplement)
#[test]
fn marshal_compact_object() {
    let out = marshal(&Sample {
        a: 1,
        b: "x".into(),
    })
    .unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), r#"{"a":1,"b":"x"}"#);
}

// Go: internal/json/json.go:Marshal/Unmarshal (behavior-level supplement)
#[test]
fn marshal_unmarshal_round_trip() {
    let v = Sample {
        a: 42,
        b: "hello".into(),
    };
    let bytes = marshal(&v).unwrap();
    let back: Sample = unmarshal(&bytes).unwrap();
    assert_eq!(v, back);
}

// Go: internal/json/json.go:MarshalIndent (explicit branch, behavior-level supplement)
#[test]
fn marshal_indent_empty_is_compact() {
    let v = One { a: 1 };
    assert_eq!(marshal_indent(&v, "", "").unwrap(), marshal(&v).unwrap());
}

// Go: internal/json/json.go:MarshalIndent (behavior-level supplement)
#[test]
fn marshal_indent_two_spaces() {
    let out = marshal_indent(&One { a: 1 }, "", "  ").unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), "{\n  \"a\": 1\n}");
}

// Go: internal/json/json.go:MarshalIndent (with prefix, behavior-level supplement)
#[test]
fn marshal_indent_prefix() {
    let out = marshal_indent(&One { a: 1 }, "\t", "  ").unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), "{\n\t  \"a\": 1\n\t}");
}

// Go: internal/json/json.go:Deterministic (behavior-level supplement)
#[test]
fn deterministic_map_key_order() {
    let mut m: HashMap<String, i32> = HashMap::new();
    m.insert("b".into(), 2);
    m.insert("a".into(), 1);
    m.insert("c".into(), 3);
    let out1 = marshal_deterministic(&m).unwrap();
    let out2 = marshal_deterministic(&m).unwrap();
    assert_eq!(out1, out2);
    assert_eq!(String::from_utf8(out1).unwrap(), r#"{"a":1,"b":2,"c":3}"#);
}

// Go: internal/json/json.go:Unmarshal (behavior-level supplement)
#[test]
fn unmarshal_into_struct() {
    let s: X = unmarshal(br#"{"x":3}"#).unwrap();
    assert_eq!(s, X { x: 3 });
}

// Go: internal/json/json.go:MarshalWrite (behavior-level supplement)
#[test]
fn marshal_write_to_writer() {
    let v = Sample {
        a: 5,
        b: "z".into(),
    };
    let mut buf = Vec::new();
    marshal_write(&mut buf, &v).unwrap();
    assert_eq!(buf, marshal(&v).unwrap());
}
