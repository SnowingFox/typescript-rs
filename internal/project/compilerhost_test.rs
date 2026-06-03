// Go: internal/project/compilerhost.go (behavior tests for skeleton)
use crate::compilerhost::CompilerHost;
use tsgo_tspath::Path;

#[test]
fn test_default_library_path() {
    let host = CompilerHost::new_skeleton(
        Path("/tsconfig.json".to_string()),
        "/app",
        "/usr/lib/typescript",
    );
    assert_eq!(host.default_library_path(), "/usr/lib/typescript");
}

#[test]
fn test_get_current_directory() {
    let host = CompilerHost::new_skeleton(Path("/tsconfig.json".to_string()), "/workspace", "/lib");
    assert_eq!(host.get_current_directory(), "/workspace");
}

#[test]
fn test_config_file_path() {
    let host = CompilerHost::new_skeleton(Path("/app/tsconfig.json".to_string()), "/app", "/lib");
    assert_eq!(
        host.config_file_path(),
        &Path("/app/tsconfig.json".to_string())
    );
}

#[test]
fn test_freeze_once() {
    let mut host = CompilerHost::new_skeleton(Path("/tsconfig.json".to_string()), "/app", "/lib");
    assert!(!host.is_frozen());
    host.freeze();
    assert!(host.is_frozen());
}

#[test]
#[should_panic(expected = "freeze can only be called once")]
fn test_freeze_twice_panics() {
    let mut host = CompilerHost::new_skeleton(Path("/tsconfig.json".to_string()), "/app", "/lib");
    host.freeze();
    host.freeze();
}
