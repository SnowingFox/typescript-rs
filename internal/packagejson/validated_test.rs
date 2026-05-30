use super::*;

// Go: internal/packagejson/validated.go:TypeValidatedField
//
// Go has no direct unit test; these exercise the interface through
// `Expected<T>` with Go-derived expectations (string field, type mismatch,
// absent field).
#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct Doc {
    name: Expected<String>,
    version: Expected<String>,
    main: Expected<String>,
}

fn parsed() -> Doc {
    tsgo_json::unmarshal(br#"{"name":"test","version":2}"#).unwrap()
}

// Go: internal/packagejson/validated.go:TypeValidatedField (valid string field)
#[test]
fn validated_present_valid_string() {
    let d = parsed();
    let field: &dyn TypeValidatedField = &d.name;
    assert!(field.is_present());
    assert!(field.is_valid());
    assert_eq!(field.expected_json_type(), "string");
    assert_eq!(field.actual_json_type(), "string");
}

// Go: internal/packagejson/validated.go:TypeValidatedField (type mismatch)
#[test]
fn validated_type_mismatch() {
    let d = parsed();
    let field: &dyn TypeValidatedField = &d.version;
    assert!(field.is_present());
    assert!(!field.is_valid());
    assert_eq!(field.expected_json_type(), "string");
    assert_eq!(field.actual_json_type(), "number");
}

// Go: internal/packagejson/validated.go:TypeValidatedField (absent field)
#[test]
fn validated_absent() {
    let d = parsed();
    let field: &dyn TypeValidatedField = &d.main;
    assert!(!field.is_present());
}
