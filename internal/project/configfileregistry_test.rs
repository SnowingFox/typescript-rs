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
