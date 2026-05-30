use super::*;

// Go: internal/packagejson/expected_test.go:TestExpected
//
// Parses one document with a valid string, a type-mismatched string, an
// explicit null, and an absent field; each subcase asserts one field.
const DOC: &str = r#"{
    "name": "test",
    "version": 2,
    "exports": null
}"#;

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct PackageJson {
    name: Expected<String>,
    version: Expected<String>,
    exports: Expected<serde_json::Value>,
    main: Expected<String>,
}

fn parsed() -> PackageJson {
    tsgo_json::unmarshal(DOC.as_bytes()).expect("valid json")
}

// Go: internal/packagejson/expected_test.go:TestExpected/name
#[test]
fn expected_name_valid_string() {
    let p = parsed();
    let (value, valid) = p.name.get_value();
    assert!(valid);
    assert_eq!(value, "test");
}

// Go: internal/packagejson/expected_test.go:TestExpected/version
#[test]
fn expected_version_type_mismatch() {
    let p = parsed();
    let (value, valid) = p.version.get_value();
    assert!(!valid);
    assert_eq!(value, "");
}

// Go: internal/packagejson/expected_test.go:TestExpected/exports
#[test]
fn expected_exports_null() {
    let p = parsed();
    assert!(p.exports.is_null());
    assert!(!p.exports.is_valid());
}

// Go: internal/packagejson/expected_test.go:TestExpected/main
#[test]
fn expected_main_absent() {
    let p = parsed();
    assert!(!p.main.is_valid());
    assert!(!p.main.is_null());
    assert_eq!(p.main.get_value().0, "");
}
