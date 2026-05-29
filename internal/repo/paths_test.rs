use super::*;

// Go: internal/repo/paths.go:RootPath/rootPath
#[test]
fn root_path_contains_workspace_manifest() {
    let root = std::path::Path::new(root_path());
    assert!(
        root.join("Cargo.toml").exists(),
        "root_path() should contain the workspace Cargo.toml: {root:?}"
    );
}

// Go: internal/repo/paths.go:rootPath (IsAbs assertion)
#[test]
fn root_path_is_absolute() {
    assert!(std::path::Path::new(root_path()).is_absolute());
}

// Go: internal/repo/paths.go:TypeScriptSubmodulePath
#[test]
fn submodule_path_suffix() {
    let p = typescript_submodule_path();
    let expected = std::path::Path::new("_submodules").join("TypeScript");
    assert!(
        p.ends_with(expected.to_str().unwrap()),
        "{p} should end with _submodules/TypeScript"
    );
}

// Go: internal/repo/paths.go:TestDataPath
#[test]
fn test_data_path_suffix() {
    assert!(test_data_path().ends_with("testdata"));
}

// Go: internal/repo/paths.go:TypeScriptSubmoduleExists
#[test]
fn submodule_exists_matches_disk() {
    let on_disk = std::path::Path::new(typescript_submodule_path())
        .join("package.json")
        .exists();
    assert_eq!(typescript_submodule_exists(), on_disk);
}

// Go: internal/repo/paths.go:SkipIfNoTypeScriptSubmodule
#[test]
fn skip_helper_no_panic() {
    // The helper returns whether to skip; it must mirror the negation of
    // submodule presence and never panic.
    assert_eq!(
        skip_if_no_typescript_submodule(),
        !typescript_submodule_exists()
    );
}
