// Go: internal/project/projectcollection.go (behavior tests)
use crate::configfileregistry::ConfigFileRegistry;
use crate::kind::Kind;
use crate::project::Project;
use crate::projectcollection::{
    find_default_configured_project_from_program_inclusion, ProjectCollection,
};
use tsgo_tspath::Path;

fn path(s: &str) -> Path {
    Path(s.to_string())
}

fn make_collection() -> ProjectCollection {
    ProjectCollection::new(
        Box::new(|s: &str| Path(s.to_string())),
        ConfigFileRegistry::new(),
    )
}

#[test]
fn test_empty_collection() {
    let pc = make_collection();
    assert!(pc.configured_projects().is_empty());
    assert!(pc.inferred_project().is_none());
    assert!(pc.projects().is_empty());
}

#[test]
fn test_add_configured_project() {
    let mut pc = make_collection();
    let p = Project::new_skeleton("/app/tsconfig.json", Kind::Configured, "/app");
    pc.set_configured_project(path("/app/tsconfig.json"), p);
    assert_eq!(pc.configured_projects().len(), 1);
    assert!(pc.configured_project(&path("/app/tsconfig.json")).is_some());
}

#[test]
fn test_configured_projects_sorted() {
    let mut pc = make_collection();
    pc.set_configured_project(
        path("/z/tsconfig.json"),
        Project::new_skeleton("/z/tsconfig.json", Kind::Configured, "/z"),
    );
    pc.set_configured_project(
        path("/a/tsconfig.json"),
        Project::new_skeleton("/a/tsconfig.json", Kind::Configured, "/a"),
    );
    let names: Vec<&str> = pc.configured_projects().iter().map(|p| p.name()).collect();
    assert_eq!(names, vec!["/a/tsconfig.json", "/z/tsconfig.json"]);
}

#[test]
fn test_inferred_project() {
    let mut pc = make_collection();
    let p = Project::new_inferred_skeleton("/app");
    pc.set_inferred_project(p);
    assert!(pc.inferred_project().is_some());
    assert_eq!(pc.projects().len(), 1);
}

#[test]
fn test_projects_includes_inferred() {
    let mut pc = make_collection();
    pc.set_configured_project(
        path("/a/tsconfig.json"),
        Project::new_skeleton("/a/tsconfig.json", Kind::Configured, "/a"),
    );
    pc.set_inferred_project(Project::new_inferred_skeleton("/app"));
    assert_eq!(pc.projects().len(), 2);
}

#[test]
fn test_get_project_by_path_configured() {
    let mut pc = make_collection();
    pc.set_configured_project(
        path("/tsconfig.json"),
        Project::new_skeleton("/tsconfig.json", Kind::Configured, "/"),
    );
    let p = pc.get_project_by_path(&path("/tsconfig.json"));
    assert!(p.is_some());
    assert_eq!(p.unwrap().name(), "/tsconfig.json");
}

#[test]
fn test_get_project_by_path_inferred() {
    let mut pc = make_collection();
    pc.set_inferred_project(Project::new_inferred_skeleton("/app"));
    let p = pc.get_project_by_path(&path("/dev/null/inferred"));
    assert!(p.is_some());
    assert_eq!(p.unwrap().kind(), Kind::Inferred);
}

#[test]
fn test_get_project_by_path_missing() {
    let pc = make_collection();
    assert!(pc.get_project_by_path(&path("/nonexistent")).is_none());
}

#[test]
fn test_remove_configured_project() {
    let mut pc = make_collection();
    pc.set_configured_project(
        path("/tsconfig.json"),
        Project::new_skeleton("/tsconfig.json", Kind::Configured, "/"),
    );
    let removed = pc.remove_configured_project(&path("/tsconfig.json"));
    assert!(removed.is_some());
    assert!(pc.configured_projects().is_empty());
}

#[test]
fn test_clear_inferred_project() {
    let mut pc = make_collection();
    pc.set_inferred_project(Project::new_inferred_skeleton("/app"));
    assert!(pc.inferred_project().is_some());
    pc.clear_inferred_project();
    assert!(pc.inferred_project().is_none());
}

#[test]
fn test_get_default_project_from_mapping() {
    let mut pc = make_collection();
    pc.set_configured_project(
        path("/tsconfig.json"),
        Project::new_skeleton("/tsconfig.json", Kind::Configured, "/"),
    );
    pc.set_file_default_project(path("/src/app.ts"), path("/tsconfig.json"));
    let p = pc.get_default_project(&path("/src/app.ts"));
    assert!(p.is_some());
    assert_eq!(p.unwrap().name(), "/tsconfig.json");
}

#[test]
fn test_get_default_project_inferred() {
    let mut pc = make_collection();
    pc.set_inferred_project(Project::new_inferred_skeleton("/app"));
    pc.set_file_default_project(path("/src/app.ts"), path("/dev/null/inferred"));
    let p = pc.get_default_project(&path("/src/app.ts"));
    assert!(p.is_some());
    assert_eq!(p.unwrap().kind(), Kind::Inferred);
}

#[test]
fn test_get_default_project_no_mapping() {
    let pc = make_collection();
    assert!(pc.get_default_project(&path("/src/app.ts")).is_none());
}

#[test]
fn test_shallow_clone_independence() {
    let mut pc = make_collection();
    pc.set_configured_project(
        path("/tsconfig.json"),
        Project::new_skeleton("/tsconfig.json", Kind::Configured, "/"),
    );
    let clone = pc.shallow_clone();
    pc.remove_configured_project(&path("/tsconfig.json"));
    assert!(pc.configured_projects().is_empty());
    assert_eq!(clone.configured_projects().len(), 1);
}

#[test]
fn test_find_default_from_program_inclusion_empty() {
    let (result, multiple) = find_default_configured_project_from_program_inclusion(
        "/a.ts",
        &path("/a.ts"),
        &[],
        &|_| None,
    );
    assert!(result.is_none());
    assert!(!multiple);
}

#[test]
fn test_find_default_from_program_inclusion_single() {
    let (result, _) = find_default_configured_project_from_program_inclusion(
        "/a.ts",
        &path("/a.ts"),
        &[path("/tsconfig.json")],
        &|_| None,
    );
    assert_eq!(result, Some(path("/tsconfig.json")));
}
