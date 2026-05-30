use super::*;

use std::cell::RefCell;

// Go: internal/outputpaths/commonsourcedirectory.go:GetCommonSourceDirectory (rootDir branch)
#[test]
fn common_source_directory_root_dir() {
    let options = CompilerOptions {
        root_dir: "/r".into(),
        ..Default::default()
    };
    assert_eq!(
        get_common_source_directory(&options, Vec::new, "/", true, None),
        "/r/"
    );
}

// Go: internal/outputpaths/commonsourcedirectory.go:GetCommonSourceDirectory (configFilePath branch)
#[test]
fn common_source_directory_config_file() {
    let options = CompilerOptions {
        config_file_path: "/p/tsconfig.json".into(),
        ..Default::default()
    };
    assert_eq!(
        get_common_source_directory(&options, Vec::new, "/", true, None),
        "/p/"
    );
}

// Go: internal/outputpaths/commonsourcedirectory.go:GetCommonSourceDirectory (compute branch)
#[test]
fn common_source_directory_computed() {
    let options = CompilerOptions::default();
    assert_eq!(
        get_common_source_directory(&options, || vec!["/src/a.ts".to_string()], "/", true, None),
        "/src/"
    );
}

// Go: internal/outputpaths/commonsourcedirectory.go:GetCommonSourceDirectory
// (checkSourceFilesBelongToPath is invoked with the chosen path)
#[test]
fn common_source_directory_check_callback_invoked() {
    let seen = RefCell::new(String::new());
    let check = |_files: &[String], path: &str| -> bool {
        *seen.borrow_mut() = path.to_string();
        true
    };
    let options = CompilerOptions {
        root_dir: "/r".into(),
        ..Default::default()
    };
    let result = get_common_source_directory(&options, Vec::new, "/", true, Some(&check));
    assert_eq!(result, "/r/");
    assert_eq!(*seen.borrow(), "/r");
}

// Go: internal/outputpaths/commonsourcedirectory.go:GetComputedCommonSourceDirectory
#[test]
fn common_source_dir_single() {
    let files = vec!["/src/a.ts".to_string()];
    assert_eq!(
        get_computed_common_source_directory(&files, "/", true),
        "/src/"
    );
}

// Go: internal/outputpaths/commonsourcedirectory.go:computeCommonSourceDirectoryOfFilenames
#[test]
fn common_source_dir_multi() {
    let files = vec![
        "/src/a/x.ts".to_string(),
        "/src/a/y.ts".to_string(),
        "/src/b/z.ts".to_string(),
    ];
    assert_eq!(
        get_computed_common_source_directory(&files, "/", true),
        "/src/"
    );
}

// Go: internal/outputpaths/commonsourcedirectory.go:computeCommonSourceDirectoryOfFilenames
// POSIX inputs always share the root component, so the common directory narrows
// to the root rather than to "" (the empty result needs distinct roots; see
// `common_source_dir_distinct_roots`).
#[test]
fn common_source_dir_narrows_to_root() {
    let files = vec!["/a/x.ts".to_string(), "/b/y.ts".to_string()];
    assert_eq!(get_computed_common_source_directory(&files, "/", true), "/");
}

// Go: internal/outputpaths/commonsourcedirectory.go:computeCommonSourceDirectoryOfFilenames
// (the `i == 0` early-return branch: first component differs -> "")
#[test]
fn common_source_dir_distinct_roots() {
    let files = vec!["c:/a/x.ts".to_string(), "d:/b/y.ts".to_string()];
    assert_eq!(
        get_computed_common_source_directory(&files, "c:/", true),
        ""
    );
}

// Go: internal/outputpaths/commonsourcedirectory.go:computeCommonSourceDirectoryOfFilenames
// ("Can happen when all input files are .d.ts files": the emitted list is empty
// and the function falls back to the current directory, verbatim, no separator)
#[test]
fn common_source_dir_all_dts_falls_back_to_cwd() {
    let files: Vec<String> = Vec::new();
    assert_eq!(
        compute_common_source_directory_of_filenames(&files, "/cwd", true),
        "/cwd"
    );
}
