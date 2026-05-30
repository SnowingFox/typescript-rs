use super::*;

use tsgo_core::compileroptions::RESOLUTION_MODE_NONE;

// Go: internal/symlinks/knownsymlinks_test.go:TestNewKnownSymlink
#[test]
fn new_known_symlink_fields() {
    let cache = new_known_symlink("/test/dir", true);
    assert_eq!(cache.cwd, "/test/dir");
    assert!(cache.use_case_sensitive_file_names);
}

// Go: internal/symlinks/knownsymlinks.go:HasDirectory (behavior-level supplement, PORTING §8.6)
// Go has no direct test; verifies the trailing-separator normalization of the key.
#[test]
fn has_directory_reports_presence() {
    let cache = new_known_symlink("/test/dir", true);
    let no_sep = to_path("/test/symlink", "/test/dir", true);
    assert!(
        !cache.has_directory(no_sep.clone()),
        "Expected absent directory to be false"
    );

    let symlink_path = no_sep.ensure_trailing_directory_separator();
    let link = KnownDirectoryLink {
        real: "/real/path/".to_string(),
        real_path: to_path("/real/path", "/test/dir", true).ensure_trailing_directory_separator(),
    };
    cache.set_directory("/test/symlink", symlink_path, Some(link));

    // Query with a path lacking a trailing separator; has_directory must add it.
    assert!(
        cache.has_directory(no_sep),
        "Expected stored directory to be found"
    );
}

// Go: internal/symlinks/knownsymlinks_test.go:TestKnownSymlinksThreadSafety
#[test]
fn thread_safety_concurrent_set_directory() {
    let cache = new_known_symlink("/test/dir", true);
    let cache = &cache;

    // Go uses goroutines + a done channel; the Rust port uses scoped threads
    // (PORTING.md §6). 10 threads concurrently store distinct directories.
    std::thread::scope(|scope| {
        for id in 0u32..10 {
            scope.spawn(move || {
                let suffix = char::from_u32(id).unwrap();
                let symlink = format!("/test/symlink{suffix}");
                let symlink_path =
                    to_path(&symlink, "/test/dir", true).ensure_trailing_directory_separator();
                let real_directory = KnownDirectoryLink {
                    real: format!("/real/path{suffix}/"),
                    real_path: to_path(&format!("/real/path{suffix}"), "/test/dir", true)
                        .ensure_trailing_directory_separator(),
                };

                cache.set_directory(&symlink, symlink_path.clone(), Some(real_directory.clone()));

                let (stored, ok) = cache.directories().load(&symlink_path);
                assert!(ok, "Thread {id}: Expected directory to be stored");
                let stored = stored.expect("Thread: expected non-nil directory link");
                assert_eq!(
                    stored.real, real_directory.real,
                    "Thread {id}: Real mismatch"
                );
            });
        }
    });

    assert_eq!(
        cache.directories().size(),
        10,
        "Expected 10 directories to be stored"
    );
}

// Go: internal/symlinks/knownsymlinks_test.go:TestSetSymlinksFromResolutions
#[test]
fn set_symlinks_from_resolutions() {
    let cache = new_known_symlink("/test/dir", true);

    // Mock resolution data: (original_path, resolved_path).
    let resolved_modules = [
        ("/test/original/file1.ts", "/test/resolved/file1.ts"),
        ("/test/original/file2.ts", "/test/resolved/file2.ts"),
    ];

    let for_each_resolved_module =
        |callback: &mut dyn FnMut(&ResolvedModule, &str, ResolutionMode, Path),
         _file: Option<&SourceFileData>| {
            for (orig, resolved) in resolved_modules {
                let resolution = ResolvedModule {
                    original_path: orig.to_string(),
                    resolved_file_name: resolved.to_string(),
                    ..Default::default()
                };
                callback(
                    &resolution,
                    "module",
                    RESOLUTION_MODE_NONE,
                    to_path("/test/source.ts", "/test/dir", true),
                );
            }
        };

    let for_each_resolved_type_reference_directive =
        |_callback: &mut dyn FnMut(&ResolvedTypeReferenceDirective, &str, ResolutionMode, Path),
         _file: Option<&SourceFileData>| {
            // No type reference directives for this test.
        };

    cache.set_symlinks_from_resolutions(
        for_each_resolved_module,
        for_each_resolved_type_reference_directive,
    );

    for (orig, resolved) in resolved_modules {
        let symlink_path = to_path(orig, "/test/dir", true);
        let (stored, ok) = cache.files().load(&symlink_path);
        assert!(ok, "Expected file '{orig}' to be stored");
        assert_eq!(stored, resolved);
    }
}

// Go: internal/symlinks/knownsymlinks_test.go:TestProcessResolution
#[test]
fn process_resolution_empty_noop() {
    let cache = new_known_symlink("/test/dir", true);
    cache.process_resolution("", "");
    cache.process_resolution("original", "");
    cache.process_resolution("", "resolved");
    assert!(
        cache.files().is_empty(),
        "Expected no file mapping for empty inputs"
    );
    assert!(
        cache.directories().is_empty(),
        "Expected no directory mapping for empty inputs"
    );
}

// Go: internal/symlinks/knownsymlinks_test.go:TestProcessResolution
#[test]
fn process_resolution_valid() {
    let cache = new_known_symlink("/test/dir", true);
    let original_path = "/test/original/file.ts";
    let resolved_path = "/test/resolved/file.ts";
    cache.process_resolution(original_path, resolved_path);

    let symlink_path = to_path(original_path, "/test/dir", true);
    let (stored, ok) = cache.files().load(&symlink_path);
    assert!(ok, "Expected file to be stored");
    assert_eq!(stored, resolved_path);
}

// Go: internal/symlinks/knownsymlinks_test.go:TestSetFile
#[test]
fn set_file_stores_realpath() {
    let cache = new_known_symlink("/test/dir", true);
    let symlink = "/test/symlink/file.ts";
    let symlink_path = to_path(symlink, "/test/dir", true);
    let realpath = "/real/path/file.ts";

    cache.set_file(symlink, symlink_path.clone(), realpath);

    let (stored, ok) = cache.files().load(&symlink_path);
    assert!(ok, "Expected file to be stored");
    assert_eq!(stored, realpath);
}

// Go: internal/symlinks/knownsymlinks.go:SetFile (behavior-level supplement, PORTING §8.6)
// Go's TestSetFile does not assert the reverse map; this covers files_by_realpath.
#[test]
fn set_file_realpath_mapping() {
    let cache = new_known_symlink("/test/dir", true);
    let symlink = "/test/symlink/file.ts";
    let symlink_path = to_path(symlink, "/test/dir", true);
    let realpath = "/real/path/file.ts";

    cache.set_file(symlink, symlink_path, realpath);

    let realpath_path = to_path(realpath, "/test/dir", true);
    let (set, ok) = cache.files_by_realpath().load(&realpath_path);
    assert!(
        ok && set.size() != 0,
        "Expected realpath reverse mapping to be created"
    );
    assert!(
        set.has(&symlink.to_string()),
        "Expected symlink to be in reverse set"
    );
}

// Go: internal/symlinks/knownsymlinks_test.go:TestSetDirectory
#[test]
fn set_directory_stores_link() {
    let cache = new_known_symlink("/test/dir", true);
    let symlink_path =
        to_path("/test/symlink", "/test/dir", true).ensure_trailing_directory_separator();
    let real_directory = KnownDirectoryLink {
        real: "/real/path/".to_string(),
        real_path: to_path("/real/path", "/test/dir", true).ensure_trailing_directory_separator(),
    };

    cache.set_directory(
        "/test/symlink",
        symlink_path.clone(),
        Some(real_directory.clone()),
    );

    let (stored, ok) = cache.directories().load(&symlink_path);
    assert!(ok, "Expected directory to be stored");
    let stored = stored.expect("Expected non-nil directory link");
    assert_eq!(stored.real, real_directory.real);
    assert_eq!(stored.real_path, real_directory.real_path);
}

// Go: internal/symlinks/knownsymlinks_test.go:TestSetDirectory
#[test]
fn set_directory_realpath_mapping() {
    let cache = new_known_symlink("/test/dir", true);
    let symlink_path =
        to_path("/test/symlink", "/test/dir", true).ensure_trailing_directory_separator();
    let real_directory = KnownDirectoryLink {
        real: "/real/path/".to_string(),
        real_path: to_path("/real/path", "/test/dir", true).ensure_trailing_directory_separator(),
    };

    cache.set_directory("/test/symlink", symlink_path, Some(real_directory.clone()));

    let (set, ok) = cache
        .directories_by_realpath()
        .load(&real_directory.real_path);
    assert!(
        ok && set.size() != 0,
        "Expected realpath mapping to be created"
    );
    assert!(
        set.has(&"/test/symlink".to_string()),
        "Expected symlink '/test/symlink' to be in set"
    );
}

// Go: internal/symlinks/knownsymlinks_test.go:TestGuessDirectorySymlink/identical paths
#[test]
fn guess_identical_paths() {
    let cache = new_known_symlink("/test/dir", true);
    let (common_resolved, common_original) =
        cache.guess_directory_symlink("/test/path/file.ts", "/test/path/file.ts", "/test/dir");
    assert_eq!(common_resolved, "/");
    assert_eq!(common_original, "/");
}

// Go: internal/symlinks/knownsymlinks_test.go:TestGuessDirectorySymlink/different files same directory
#[test]
fn guess_diff_files_same_dir() {
    let cache = new_known_symlink("/test/dir", true);
    let (common_resolved, common_original) =
        cache.guess_directory_symlink("/test/path/file1.ts", "/test/path/file2.ts", "/test/dir");
    assert_eq!(common_resolved, "");
    assert_eq!(common_original, "");
}

// Go: internal/symlinks/knownsymlinks_test.go:TestGuessDirectorySymlink/different directories
#[test]
fn guess_diff_dirs() {
    let cache = new_known_symlink("/test/dir", true);
    let (common_resolved, common_original) =
        cache.guess_directory_symlink("/test/path1/file.ts", "/test/path2/file.ts", "/test/dir");
    assert_eq!(common_resolved, "/test/path1");
    assert_eq!(common_original, "/test/path2");
}

// Go: internal/symlinks/knownsymlinks_test.go:TestGuessDirectorySymlink/node_modules paths
#[test]
fn guess_node_modules_paths() {
    let cache = new_known_symlink("/test/dir", true);
    let (common_resolved, common_original) = cache.guess_directory_symlink(
        "/test/node_modules/pkg/file.ts",
        "/test/node_modules/pkg/file.ts",
        "/test/dir",
    );
    assert_eq!(common_resolved, "/test/node_modules/pkg");
    assert_eq!(common_original, "/test/node_modules/pkg");
}

// Go: internal/symlinks/knownsymlinks_test.go:TestGuessDirectorySymlink/scoped package paths
#[test]
fn guess_scoped_package_paths() {
    let cache = new_known_symlink("/test/dir", true);
    let (common_resolved, common_original) = cache.guess_directory_symlink(
        "/test/node_modules/@scope/pkg/file.ts",
        "/test/node_modules/@scope/pkg/file.ts",
        "/test/dir",
    );
    assert_eq!(common_resolved, "/test/node_modules/@scope/pkg");
    assert_eq!(common_original, "/test/node_modules/@scope/pkg");
}

// Go: internal/symlinks/knownsymlinks_test.go:TestIsNodeModulesOrScopedPackageDirectory/node_modules
#[test]
fn nm_node_modules() {
    let cache = new_known_symlink("/test/dir", true);
    assert!(cache.is_node_modules_or_scoped_package_directory("node_modules"));
}

// Go: internal/symlinks/knownsymlinks_test.go:TestIsNodeModulesOrScopedPackageDirectory/scoped package
#[test]
fn nm_scoped_package() {
    let cache = new_known_symlink("/test/dir", true);
    assert!(cache.is_node_modules_or_scoped_package_directory("@scope"));
}

// Go: internal/symlinks/knownsymlinks_test.go:TestIsNodeModulesOrScopedPackageDirectory/regular directory
#[test]
fn nm_regular_dir() {
    let cache = new_known_symlink("/test/dir", true);
    assert!(!cache.is_node_modules_or_scoped_package_directory("src"));
}

// Go: internal/symlinks/knownsymlinks_test.go:TestIsNodeModulesOrScopedPackageDirectory/empty string
#[test]
fn nm_empty_string() {
    let cache = new_known_symlink("/test/dir", true);
    assert!(!cache.is_node_modules_or_scoped_package_directory(""));
}

// Go: internal/symlinks/knownsymlinks_test.go:TestIsNodeModulesOrScopedPackageDirectory/case insensitive node_modules
#[test]
fn nm_uppercase_node_modules() {
    // The function is case sensitive; with case-sensitive file names,
    // "NODE_MODULES" is not equal to "node_modules".
    let cache = new_known_symlink("/test/dir", true);
    assert!(!cache.is_node_modules_or_scoped_package_directory("NODE_MODULES"));
}

// Go: internal/symlinks/knownsymlinks_test.go:TestIsNodeModulesOrScopedPackageDirectory/case insensitive scoped
#[test]
fn nm_uppercase_scoped() {
    // Only the leading `@` is checked, so "@SCOPE" is still a scoped package.
    let cache = new_known_symlink("/test/dir", true);
    assert!(cache.is_node_modules_or_scoped_package_directory("@SCOPE"));
}
