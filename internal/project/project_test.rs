// Go: internal/project/project.go (behavior tests for skeleton)
use crate::kind::{Kind, ProgramUpdateKind};
use crate::project::{Project, INFERRED_PROJECT_NAME};
use tsgo_tspath::Path;

// Go: internal/project/project.go:Kind constants
#[test]
fn test_project_name() {
    let p = Project::new_skeleton("/app/tsconfig.json", Kind::Configured, "/app");
    assert_eq!(p.name(), "/app/tsconfig.json");
}

#[test]
fn test_project_id() {
    let p = Project::new_skeleton("/app/tsconfig.json", Kind::Configured, "/app");
    assert_eq!(p.id(), &Path("/app/tsconfig.json".to_string()));
}

#[test]
fn test_project_kind() {
    let configured = Project::new_skeleton("tsconfig.json", Kind::Configured, "/");
    assert_eq!(configured.kind(), Kind::Configured);
    let inferred = Project::new_inferred_skeleton("/app");
    assert_eq!(inferred.kind(), Kind::Inferred);
}

#[test]
fn test_inferred_project_name() {
    let p = Project::new_inferred_skeleton("/app");
    assert_eq!(p.name(), INFERRED_PROJECT_NAME);
}

#[test]
fn test_display_name_configured() {
    let p = Project::new_skeleton("/app/tsconfig.json", Kind::Configured, "/app");
    let display = p.display_name("/app");
    assert_eq!(display, "tsconfig.json");
}

#[test]
fn test_display_name_inferred() {
    let p = Project::new_inferred_skeleton("/app/subdir");
    let display = p.display_name("/app");
    assert_eq!(display, "subdir");
}

#[test]
#[should_panic(expected = "ConfigFileName called on non-configured project")]
fn test_config_file_name_panics_on_inferred() {
    let p = Project::new_inferred_skeleton("/app");
    let _ = p.config_file_name();
}

#[test]
#[should_panic(expected = "ConfigFilePath called on non-configured project")]
fn test_config_file_path_panics_on_inferred() {
    let p = Project::new_inferred_skeleton("/app");
    let _ = p.config_file_path();
}

#[test]
fn test_config_file_name_configured() {
    let p = Project::new_skeleton("/app/tsconfig.json", Kind::Configured, "/app");
    assert_eq!(p.config_file_name(), "/app/tsconfig.json");
}

#[test]
fn test_project_dirty_by_default() {
    let p = Project::new_skeleton("tsconfig.json", Kind::Configured, "/");
    assert!(p.is_dirty());
}

#[test]
fn test_project_initial_update_kind() {
    let p = Project::new_skeleton("tsconfig.json", Kind::Configured, "/");
    assert_eq!(p.program_update_kind, ProgramUpdateKind::None);
    assert_eq!(p.program_last_update, 0);
}

#[test]
fn test_configured_project_skeleton() {
    let p = Project::new_configured_skeleton(
        "/workspace/tsconfig.json",
        Path("/workspace/tsconfig.json".to_string()),
    );
    assert_eq!(p.kind(), Kind::Configured);
    assert_eq!(p.current_directory(), "/workspace");
}

#[test]
fn test_project_print() {
    let p = Project::new_skeleton("/app/tsconfig.json", Kind::Configured, "/app");
    let mut buf = String::new();
    p.print(false, &mut buf);
    assert!(buf.contains("Project '/app/tsconfig.json'"));
    assert!(buf.contains("NoProgram"));
}
