// Go: internal/project/configfileregistry.go (no direct test file — behavior tests)
use crate::configfileregistry::{
    ConfigFileEntry, ConfigFileNames, ConfigFileRegistry, PendingReload,
};
use std::collections::HashMap;
use tsgo_tspath::Path;

fn path(s: &str) -> Path {
    Path(s.to_string())
}

#[test]
fn test_empty_registry() {
    let reg = ConfigFileRegistry::new();
    assert_eq!(reg.get_config_file_name(&path("/src/app.ts")), "");
    assert!(reg.get_entry(&path("/tsconfig.json")).is_none());
    assert!(reg.get_test_config_entry(&path("/tsconfig.json")).is_none());
}

#[test]
fn test_get_config_file_name() {
    let mut reg = ConfigFileRegistry::new();
    reg.set_config_file_names(
        path("/src/app.ts"),
        ConfigFileNames {
            nearest_config_file_name: "/tsconfig.json".to_string(),
            ancestors: HashMap::new(),
        },
    );
    assert_eq!(
        reg.get_config_file_name(&path("/src/app.ts")),
        "/tsconfig.json"
    );
    assert_eq!(reg.get_config_file_name(&path("/other.ts")), "");
}

#[test]
fn test_get_ancestor_config_file_name() {
    let mut reg = ConfigFileRegistry::new();
    let mut ancestors = HashMap::new();
    ancestors.insert(
        "/a/b/tsconfig.json".to_string(),
        "/a/tsconfig.json".to_string(),
    );
    reg.set_config_file_names(
        path("/a/b/src/app.ts"),
        ConfigFileNames {
            nearest_config_file_name: "/a/b/tsconfig.json".to_string(),
            ancestors,
        },
    );
    assert_eq!(
        reg.get_ancestor_config_file_name(&path("/a/b/src/app.ts"), "/a/b/tsconfig.json"),
        "/a/tsconfig.json"
    );
    assert_eq!(
        reg.get_ancestor_config_file_name(&path("/a/b/src/app.ts"), "/nonexistent"),
        ""
    );
}

#[test]
fn test_shallow_clone_independence() {
    let mut reg = ConfigFileRegistry::new();
    reg.set_config_file_names(
        path("/a.ts"),
        ConfigFileNames {
            nearest_config_file_name: "/tsconfig.json".to_string(),
            ancestors: HashMap::new(),
        },
    );
    let clone = reg.shallow_clone();
    reg.remove_config_file_names(&path("/a.ts"));
    assert_eq!(reg.get_config_file_name(&path("/a.ts")), "");
    assert_eq!(clone.get_config_file_name(&path("/a.ts")), "/tsconfig.json");
}

#[test]
fn test_config_entry_retaining_sets() {
    let mut reg = ConfigFileRegistry::new();
    let mut retaining_projects = HashMap::new();
    retaining_projects.insert(path("/proj1"), ());
    retaining_projects.insert(path("/proj2"), ());
    reg.set_config(
        path("/tsconfig.json"),
        ConfigFileEntry {
            file_name: "/tsconfig.json".to_string(),
            pending_reload: PendingReload::Full,
            retaining_projects,
            retaining_open_files: HashMap::new(),
            retaining_configs: HashMap::new(),
        },
    );
    let entry = reg.get_test_config_entry(&path("/tsconfig.json")).unwrap();
    assert_eq!(entry.file_name, "/tsconfig.json");
    assert_eq!(entry.retaining_projects.len(), 2);
}

#[test]
fn test_for_each_config_entry() {
    let mut reg = ConfigFileRegistry::new();
    reg.set_config(
        path("/a/tsconfig.json"),
        ConfigFileEntry {
            file_name: "/a/tsconfig.json".to_string(),
            pending_reload: PendingReload::None,
            retaining_projects: HashMap::new(),
            retaining_open_files: HashMap::new(),
            retaining_configs: HashMap::new(),
        },
    );
    reg.set_config(
        path("/b/tsconfig.json"),
        ConfigFileEntry {
            file_name: "/b/tsconfig.json".to_string(),
            pending_reload: PendingReload::FileNames,
            retaining_projects: HashMap::new(),
            retaining_open_files: HashMap::new(),
            retaining_configs: HashMap::new(),
        },
    );
    let mut count = 0;
    reg.for_each_config_entry(|_path, _entry| count += 1);
    assert_eq!(count, 2);
}

#[test]
fn test_config_file_names_deep_clone() {
    let mut ancestors = HashMap::new();
    ancestors.insert("a".to_string(), "b".to_string());
    let names = ConfigFileNames {
        nearest_config_file_name: "tsconfig.json".to_string(),
        ancestors,
    };
    let clone = names.deep_clone();
    assert_eq!(
        clone.nearest_config_file_name,
        names.nearest_config_file_name
    );
    assert_eq!(clone.ancestors.get("a").unwrap(), "b");
}

#[test]
fn test_config_entry_deep_clone() {
    let mut retaining = HashMap::new();
    retaining.insert(path("/proj"), ());
    let entry = ConfigFileEntry {
        file_name: "/tsconfig.json".to_string(),
        pending_reload: PendingReload::Full,
        retaining_projects: retaining,
        retaining_open_files: HashMap::new(),
        retaining_configs: HashMap::new(),
    };
    let clone = entry.deep_clone();
    assert_eq!(clone.file_name, entry.file_name);
    assert_eq!(clone.pending_reload, PendingReload::Full);
    assert!(clone.retaining_projects.contains_key(&path("/proj")));
}

#[test]
fn test_remove_config() {
    let mut reg = ConfigFileRegistry::new();
    reg.set_config(
        path("/tsconfig.json"),
        ConfigFileEntry {
            file_name: "/tsconfig.json".to_string(),
            pending_reload: PendingReload::None,
            retaining_projects: HashMap::new(),
            retaining_open_files: HashMap::new(),
            retaining_configs: HashMap::new(),
        },
    );
    assert!(reg.get_entry(&path("/tsconfig.json")).is_some());
    reg.remove_config(&path("/tsconfig.json"));
    assert!(reg.get_entry(&path("/tsconfig.json")).is_none());
}

#[test]
fn test_default_is_empty() {
    let reg = ConfigFileRegistry::default();
    assert_eq!(reg.custom_config_file_name, "");
    let mut count = 0;
    reg.for_each_config_entry(|_, _| count += 1);
    assert_eq!(count, 0);
}

// --- register/unregister/update tests ---

#[test]
fn test_register_config_adds_entry() {
    let mut reg = ConfigFileRegistry::new();
    let config_path = path("/project/tsconfig.json");
    let project_path = path("/project");

    reg.register_config(&config_path, "/project/tsconfig.json", &project_path);

    let entry = reg
        .get_entry(&config_path)
        .expect("entry should exist after register");
    assert_eq!(entry.file_name, "/project/tsconfig.json");
    assert!(entry.retaining_projects.contains_key(&project_path));
    assert_eq!(entry.pending_reload, PendingReload::Full);
}

#[test]
fn test_double_register_increments_refcount() {
    let mut reg = ConfigFileRegistry::new();
    let config_path = path("/tsconfig.json");
    let proj_a = path("/proj_a");
    let proj_b = path("/proj_b");

    reg.register_config(&config_path, "/tsconfig.json", &proj_a);
    reg.register_config(&config_path, "/tsconfig.json", &proj_b);

    let entry = reg.get_entry(&config_path).unwrap();
    assert_eq!(entry.retaining_projects.len(), 2);
    assert!(entry.retaining_projects.contains_key(&proj_a));
    assert!(entry.retaining_projects.contains_key(&proj_b));
}

#[test]
fn test_unregister_config_decrements_and_removes() {
    let mut reg = ConfigFileRegistry::new();
    let config_path = path("/tsconfig.json");
    let proj_a = path("/proj_a");
    let proj_b = path("/proj_b");

    reg.register_config(&config_path, "/tsconfig.json", &proj_a);
    reg.register_config(&config_path, "/tsconfig.json", &proj_b);

    // Unregister one project — entry should still exist
    reg.unregister_config(&config_path, &proj_a);
    let entry = reg.get_entry(&config_path).unwrap();
    assert_eq!(entry.retaining_projects.len(), 1);
    assert!(!entry.retaining_projects.contains_key(&proj_a));
    assert!(entry.retaining_projects.contains_key(&proj_b));

    // Unregister last project — entry should be removed
    reg.unregister_config(&config_path, &proj_b);
    assert!(
        reg.get_entry(&config_path).is_none(),
        "entry should be removed when no retainers remain"
    );
}

#[test]
fn test_update_config_marks_full_reload() {
    let mut reg = ConfigFileRegistry::new();
    let config_path = path("/tsconfig.json");
    let project_path = path("/project");

    reg.register_config(&config_path, "/tsconfig.json", &project_path);

    // Clear the pending reload to simulate a loaded state
    reg.configs.get_mut(&config_path).unwrap().pending_reload = PendingReload::None;
    assert_eq!(
        reg.get_entry(&config_path).unwrap().pending_reload,
        PendingReload::None
    );

    // Update should mark it for full reload
    let updated = reg.update_config(&config_path);
    assert!(
        updated,
        "update_config should return true for existing entry"
    );
    assert_eq!(
        reg.get_entry(&config_path).unwrap().pending_reload,
        PendingReload::Full
    );
}

#[test]
fn test_update_config_nonexistent_returns_false() {
    let mut reg = ConfigFileRegistry::new();
    let result = reg.update_config(&path("/nonexistent/tsconfig.json"));
    assert!(
        !result,
        "update_config should return false for missing entry"
    );
}
