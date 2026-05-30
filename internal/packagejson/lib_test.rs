use super::*;

// Go: internal/packagejson/packagejson_test.go:TestParse/duplicate names
//
// Go uses `json.AllowDuplicateNames(true)` so a repeated key keeps the last
// value. `assert.DeepEqual` ignores unexported fields (`actualJSONType`), so we
// compare only the value + validity (via `get_value`).
#[test]
fn parse_duplicate_names() {
    let content = br#"{
        "name": "test-package",
        "name": "test-package",
        "version": "1.0.0"
    }"#;

    let got = parse(content).expect("valid json");
    let want_name = expected_of("test-package".to_string());
    let want_version = expected_of("1.0.0".to_string());

    assert_eq!(got.header.name.get_value(), want_name.get_value());
    assert_eq!(got.header.version.get_value(), want_version.get_value());
    assert!(!got.header.type_.is_present());
}

// Go: internal/packagejson/packagejson.go:HasDependency
//
// No direct Go unit test; behavior level: a dependency under any field
// (here dev-only) is found.
#[test]
fn has_dependency_across_fields() {
    let f = parse(br#"{"devDependencies":{"x":"1.0.0"}}"#).expect("valid json");
    assert!(f.deps.has_dependency("x"));
    assert!(!f.deps.has_dependency("y"));
}

// Go: internal/packagejson/packagejson.go:GetRuntimeDependencyNames
//
// No direct Go unit test; behavior level: dev dependencies are excluded.
#[test]
fn runtime_deps_excludes_dev() {
    let f =
        parse(br#"{"dependencies":{"a":"1"},"devDependencies":{"b":"2"}}"#).expect("valid json");
    let names = f.deps.get_runtime_dependency_names();
    assert!(names.has(&"a".to_string()));
    assert!(!names.has(&"b".to_string()));
    assert_eq!(names.len(), 1);
}

// Go: internal/packagejson/packagejson.go:RangeDependencies
//
// No direct Go unit test; behavior level: every dependency field is visited
// with its label.
#[test]
fn range_dependencies_visits_all_fields() {
    let f =
        parse(br#"{"dependencies":{"a":"1"},"peerDependencies":{"p":"2"}}"#).expect("valid json");
    let mut seen: Vec<(String, String, String)> = Vec::new();
    f.deps.range_dependencies(|name, version, field| {
        seen.push((name.to_string(), version.to_string(), field.to_string()));
        true
    });
    assert_eq!(seen.len(), 2);
    assert!(seen.contains(&("a".to_string(), "1".to_string(), "dependencies".to_string())));
    assert!(seen.contains(&(
        "p".to_string(),
        "2".to_string(),
        "peerDependencies".to_string()
    )));
}
