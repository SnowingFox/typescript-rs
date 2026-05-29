//! Behavior tests for `ProjectReference` (Go has no `_test.go`; behavior-level).

use super::*;

// Go: internal/core/projectreference.go:ResolveConfigFileNameOfProjectReference (behavior-level)
#[test]
fn json_path_is_returned_as_is() {
    assert_eq!(
        resolve_config_file_name_of_project_reference("/proj/tsconfig.json"),
        "/proj/tsconfig.json"
    );
}

// Go: internal/core/projectreference.go:ResolveConfigFileNameOfProjectReference (behavior-level)
#[test]
fn directory_path_gets_tsconfig_json_appended() {
    assert_eq!(
        resolve_config_file_name_of_project_reference("/proj"),
        "/proj/tsconfig.json"
    );
}

// Go: internal/core/projectreference.go:ResolveProjectReferencePath (behavior-level)
#[test]
fn resolve_project_reference_path_uses_the_path_field() {
    let reference = ProjectReference {
        path: "/proj".to_string(),
        ..Default::default()
    };
    assert_eq!(
        resolve_project_reference_path(&reference),
        "/proj/tsconfig.json"
    );
}
