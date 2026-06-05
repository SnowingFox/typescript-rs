use super::*;

// === WatchRegistry tests ===

// Go: basic acquire/release ref counting
#[test]
fn watch_registry_acquire_returns_true_on_first_ref() {
    let reg = WatchRegistry::new();
    let watcher = FileSystemWatcher {
        glob_pattern: PatternOrRelativePattern {
            pattern: Some("**/*.ts".to_string()),
            relative_pattern: None,
        },
        kind: Some(WatchKind::CREATE | WatchKind::CHANGE),
    };
    let is_new = reg.acquire(&watcher, "w1".to_string());
    assert!(is_new, "first acquire should return true");
}

#[test]
fn watch_registry_acquire_returns_false_on_subsequent_refs() {
    let reg = WatchRegistry::new();
    let watcher = FileSystemWatcher {
        glob_pattern: PatternOrRelativePattern {
            pattern: Some("**/*.ts".to_string()),
            relative_pattern: None,
        },
        kind: Some(WatchKind::CREATE | WatchKind::CHANGE),
    };
    reg.acquire(&watcher, "w1".to_string());
    let is_new = reg.acquire(&watcher, "w1".to_string());
    assert!(!is_new, "second acquire should return false");
}

#[test]
fn watch_registry_release_removes_on_last_ref() {
    let reg = WatchRegistry::new();
    let watcher = FileSystemWatcher {
        glob_pattern: PatternOrRelativePattern {
            pattern: Some("src/**/*".to_string()),
            relative_pattern: None,
        },
        kind: Some(WatchKind::CREATE | WatchKind::CHANGE | WatchKind::DELETE),
    };
    reg.acquire(&watcher, "w2".to_string());
    let (id, removed) = reg.release(&watcher);
    assert!(removed);
    assert_eq!(id, "w2");
}

#[test]
fn watch_registry_release_decrements_without_removing() {
    let reg = WatchRegistry::new();
    let watcher = FileSystemWatcher {
        glob_pattern: PatternOrRelativePattern {
            pattern: Some("lib/**/*".to_string()),
            relative_pattern: None,
        },
        kind: Some(WatchKind::CHANGE),
    };
    reg.acquire(&watcher, "w3".to_string());
    reg.acquire(&watcher, "w3".to_string());
    let (id, removed) = reg.release(&watcher);
    assert!(!removed);
    assert_eq!(id, "");
}

#[test]
fn watch_registry_release_nonexistent_returns_false() {
    let reg = WatchRegistry::new();
    let watcher = FileSystemWatcher {
        glob_pattern: PatternOrRelativePattern {
            pattern: Some("nonexistent/**/*".to_string()),
            relative_pattern: None,
        },
        kind: Some(WatchKind::CREATE),
    };
    let (id, removed) = reg.release(&watcher);
    assert!(!removed);
    assert_eq!(id, "");
}

// === Pending watcher tests ===

#[test]
fn watch_registry_pending_lifecycle() {
    let reg = WatchRegistry::new();
    let id = "pending-1".to_string();
    assert!(!reg.is_pending(&id));
    reg.mark_pending(id.clone());
    assert!(reg.is_pending(&id));
    reg.clear_pending(&id);
    assert!(!reg.is_pending(&id));
}

// === Helper function tests ===

#[test]
fn get_recursive_glob_pattern_basic() {
    assert_eq!(get_recursive_glob_pattern("/workspace"), "/workspace/**/*");
}

#[test]
fn get_recursive_glob_pattern_removes_trailing_slash() {
    assert_eq!(get_recursive_glob_pattern("/workspace/"), "/workspace/**/*");
}

#[test]
fn recursive_directory_glob_pattern_plain() {
    let result = recursive_directory_glob_pattern("/src/lib", false);
    assert_eq!(result, "/src/lib/**/*");
}

#[test]
fn recursive_directory_glob_pattern_relative() {
    let result = recursive_directory_glob_pattern("/src/lib", true);
    assert!(result.starts_with("file:///"));
    assert!(result.ends_with("/**/*"));
}

// === perceivedOsRootLengthForWatching tests ===

#[test]
fn perceived_root_length_empty() {
    let components: Vec<&str> = vec![];
    assert_eq!(perceived_os_root_length_for_watching(&components), 0);
}

#[test]
fn perceived_root_length_single() {
    assert_eq!(perceived_os_root_length_for_watching(&["/"]), 1);
}

#[test]
fn perceived_root_length_unc_root() {
    assert_eq!(
        perceived_os_root_length_for_watching(&["//server", "share", "dir"]),
        2
    );
}

#[test]
fn perceived_root_length_windows_users() {
    assert_eq!(
        perceived_os_root_length_for_watching(&["C:/", "Users", "username", "project"]),
        3
    );
}

#[test]
fn perceived_root_length_windows_non_users() {
    assert_eq!(
        perceived_os_root_length_for_watching(&["C:/", "Program Files", "app"]),
        1
    );
}

#[test]
fn perceived_root_length_linux_home() {
    assert_eq!(
        perceived_os_root_length_for_watching(&["/", "home", "user", "project"]),
        3
    );
}

#[test]
fn perceived_root_length_linux_non_home() {
    assert_eq!(
        perceived_os_root_length_for_watching(&["/", "usr", "lib"]),
        1
    );
}

// === WatchedFiles basic tests ===

#[test]
fn watched_files_name_and_kind() {
    let wf = WatchedFiles::<Vec<String>>::new(
        "test-watcher",
        WatchKind::CREATE | WatchKind::CHANGE,
        false,
        |_| PatternsAndIgnored::default(),
    );
    assert_eq!(wf.name(), "test-watcher");
    assert_eq!(wf.watch_kind(), WatchKind::CREATE | WatchKind::CHANGE);
}

#[test]
fn watched_files_computes_globs_from_input() {
    let wf = WatchedFiles::new(
        "src-watcher",
        WatchKind::CREATE | WatchKind::CHANGE | WatchKind::DELETE,
        false,
        |paths: &Vec<String>| {
            let globs: Vec<String> = paths.iter().map(|p| format!("{}/**/*", p)).collect();
            PatternsAndIgnored {
                patterns_inside_workspace: globs,
                ..Default::default()
            }
        },
    );
    wf.set_input(vec!["/src".to_string(), "/lib".to_string()]);
    let w = wf.watchers();
    assert_eq!(w.workspace_watchers.len(), 2);
    assert_eq!(
        w.workspace_watchers[0].glob_pattern.pattern.as_deref(),
        Some("/lib/**/*")
    );
    assert_eq!(
        w.workspace_watchers[1].glob_pattern.pattern.as_deref(),
        Some("/src/**/*")
    );
}

// === newRecursiveDirectoryWatcher tests ===

#[test]
fn new_recursive_directory_watcher_plain_glob() {
    let w = new_recursive_directory_watcher("/project/src", WatchKind::CREATE, false);
    assert_eq!(w.glob_pattern.pattern.as_deref(), Some("/project/src/**/*"));
    assert!(w.glob_pattern.relative_pattern.is_none());
    assert_eq!(w.kind, Some(WatchKind::CREATE));
}

#[test]
fn new_recursive_directory_watcher_relative_pattern() {
    let w = new_recursive_directory_watcher("/project/src", WatchKind::CHANGE, true);
    assert!(w.glob_pattern.pattern.is_none());
    let rp = w.glob_pattern.relative_pattern.as_ref().unwrap();
    assert_eq!(rp.pattern, "**/*");
    assert!(rp.base_uri.uri.as_ref().unwrap().0.starts_with("file:///"));
}

// === file_system_watcher_glob_string tests ===

#[test]
fn glob_string_from_plain_pattern() {
    let w = FileSystemWatcher {
        glob_pattern: PatternOrRelativePattern {
            pattern: Some("/ws/**/*.ts".to_string()),
            relative_pattern: None,
        },
        kind: None,
    };
    assert_eq!(file_system_watcher_glob_string(&w), "/ws/**/*.ts");
}

#[test]
fn glob_string_from_relative_pattern() {
    let w = FileSystemWatcher {
        glob_pattern: PatternOrRelativePattern {
            pattern: None,
            relative_pattern: Some(RelativePattern {
                base_uri: WorkspaceFolderOrURI {
                    workspace_folder: None,
                    uri: Some(URI("file:///base".to_string())),
                },
                pattern: "**/*".to_string(),
            }),
        },
        kind: None,
    };
    assert_eq!(file_system_watcher_glob_string(&w), "file:///base/**/*");
}
