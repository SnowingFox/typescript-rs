use super::*;

// Go: internal/packagejson/jsonvalue_test.go:testJSONValue
//
// All subcases parse the same document (mirroring the single Go table block)
// and assert one observable fact each.
const DOC: &str = r#"{
    "private": true,
    "false": false,
    "name": "test",
    "version": 2,
    "exports": {
        ".": {
            "import": "./test.ts",
            "default": "./test.ts"
        },
        "./test": [
            "./test1.ts",
            "./test2.ts",
            null
        ],
        "./null": null
    },
    "imports": null
}"#;

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct PackageJson {
    private: JsonValue,
    #[serde(rename = "false")]
    false_field: JsonValue,
    name: JsonValue,
    version: JsonValue,
    exports: JsonValue,
    imports: JsonValue,
    #[serde(rename = "notPresent")]
    not_present: JsonValue,
}

fn parsed() -> PackageJson {
    tsgo_json::unmarshal(DOC.as_bytes()).expect("valid json")
}

fn child<'a>(value: &'a JsonValue, key: &str) -> &'a JsonValue {
    value
        .as_object()
        .get(&key.to_string())
        .unwrap_or_else(|| panic!("missing key {key}"))
}

// Go: internal/packagejson/jsonvalue_test.go:testJSONValue/private
#[test]
fn jv_bool_true() {
    let p = parsed();
    assert_eq!(p.private.value_type(), JsonValueType::Boolean);
    assert!(matches!(p.private, JsonValue::Bool(true)));
}

// Go: internal/packagejson/jsonvalue_test.go:testJSONValue/false
#[test]
fn jv_bool_false() {
    let p = parsed();
    assert_eq!(p.false_field.value_type(), JsonValueType::Boolean);
    assert!(matches!(p.false_field, JsonValue::Bool(false)));
}

// Go: internal/packagejson/jsonvalue_test.go:testJSONValue/name
#[test]
fn jv_string() {
    let p = parsed();
    assert_eq!(p.name.value_type(), JsonValueType::String);
    assert_eq!(p.name.as_str(), "test");
}

// Go: internal/packagejson/jsonvalue_test.go:testJSONValue/version
#[test]
fn jv_number_is_f64() {
    let p = parsed();
    assert_eq!(p.version.value_type(), JsonValueType::Number);
    match p.version {
        JsonValue::Num(n) => assert_eq!(n, 2.0),
        other => panic!("expected number, got {}", other.value_type()),
    }
}

// Go: internal/packagejson/jsonvalue_test.go:testJSONValue/exports-size
#[test]
fn jv_object_size() {
    let p = parsed();
    assert_eq!(p.exports.value_type(), JsonValueType::Object);
    assert_eq!(p.exports.as_object().size(), 3);
}

// Go: internal/packagejson/jsonvalue_test.go:testJSONValue/exports-nested-object
#[test]
fn jv_nested_object() {
    let p = parsed();
    let dot = child(&p.exports, ".");
    assert_eq!(dot.value_type(), JsonValueType::Object);
    assert_eq!(child(dot, "import").as_str(), "./test.ts");
}

// Go: internal/packagejson/jsonvalue_test.go:testJSONValue/exports-array
#[test]
fn jv_array_type_and_len() {
    let p = parsed();
    let arr = child(&p.exports, "./test");
    assert_eq!(arr.value_type(), JsonValueType::Array);
    assert_eq!(arr.as_array().len(), 3);
}

// Go: internal/packagejson/jsonvalue_test.go:testJSONValue/exports-array-elements
#[test]
fn jv_array_elements() {
    let p = parsed();
    let arr = child(&p.exports, "./test");
    let elements = arr.as_array();
    assert_eq!(elements[0].as_str(), "./test1.ts");
    assert_eq!(elements[1].as_str(), "./test2.ts");
    assert_eq!(elements[2].value_type(), JsonValueType::Null);
}

// Go: internal/packagejson/jsonvalue_test.go:testJSONValue/exports-null-value
#[test]
fn jv_object_null_value() {
    let p = parsed();
    assert_eq!(
        child(&p.exports, "./null").value_type(),
        JsonValueType::Null
    );
}

// Go: internal/packagejson/jsonvalue_test.go:testJSONValue/imports-null
#[test]
fn jv_top_level_null() {
    let p = parsed();
    assert_eq!(p.imports.value_type(), JsonValueType::Null);
    assert!(!p.imports.is_present() || matches!(p.imports, JsonValue::Null));
}

// Go: internal/packagejson/jsonvalue_test.go:testJSONValue/notPresent
#[test]
fn jv_not_present() {
    let p = parsed();
    assert_eq!(p.not_present.value_type(), JsonValueType::NotPresent);
    assert!(!p.not_present.is_present());
}
