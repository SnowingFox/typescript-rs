// Tests for snapshot module.
// Go: internal/project/snapshot_test.go

use super::*;

// Go: internal/project/snapshot_test.go:TestSnapshot/GetFile_returns_nil_for_non-existent_files
#[test]
fn snapshot_new_has_correct_id() {
    let pc = crate::projectcollection::ProjectCollection::new(
        Box::new(|s: &str| tsgo_tspath::Path(s.to_lowercase())),
        crate::configfileregistry::ConfigFileRegistry::new(),
    );
    let snap = Snapshot::new(42, pc);
    assert_eq!(snap.id(), 42);
}

#[test]
fn snapshot_project_collection_accessible() {
    let pc = crate::projectcollection::ProjectCollection::new(
        Box::new(|s: &str| tsgo_tspath::Path(s.to_lowercase())),
        crate::configfileregistry::ConfigFileRegistry::new(),
    );
    let snap = Snapshot::new(1, pc);
    assert!(snap.project_collection().projects().is_empty());
}

#[test]
fn snapshot_get_default_project_returns_none_when_empty() {
    let pc = crate::projectcollection::ProjectCollection::new(
        Box::new(|s: &str| tsgo_tspath::Path(s.to_lowercase())),
        crate::configfileregistry::ConfigFileRegistry::new(),
    );
    let snap = Snapshot::new(1, pc);
    let path = tsgo_tspath::Path("/a.ts".to_string());
    assert!(snap.get_default_project(&path).is_none());
}

#[test]
fn snapshot_get_default_project_delegates_to_collection() {
    let mut pc = crate::projectcollection::ProjectCollection::new(
        Box::new(|s: &str| tsgo_tspath::Path(s.to_lowercase())),
        crate::configfileregistry::ConfigFileRegistry::new(),
    );
    let project = crate::project::Project::new_configured_skeleton(
        "/app/tsconfig.json",
        tsgo_tspath::Path("/app/tsconfig.json".to_string()),
    );
    let config_path = tsgo_tspath::Path("/app/tsconfig.json".to_string());
    let file_path = tsgo_tspath::Path("/app/index.ts".to_string());
    pc.set_configured_project(config_path.clone(), project);
    pc.set_file_default_project(file_path.clone(), config_path);

    let snap = Snapshot::new(1, pc);
    let found = snap.get_default_project(&file_path);
    assert!(found.is_some());
    assert_eq!(found.unwrap().name(), "/app/tsconfig.json");
}
