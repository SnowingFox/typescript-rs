use super::*;

// Go: internal/tspath/ignoredpaths_test.go:TestContainsIgnoredPath
#[test]
fn contains_ignored_path_cases() {
    // node_modules dot path
    assert!(contains_ignored_path("/project/node_modules/.pnpm/file.ts"));
    // git directory
    assert!(contains_ignored_path("/project/.git/hooks/pre-commit"));
    // emacs lock file
    assert!(contains_ignored_path("/project/src/file.ts.#"));
    // regular file path
    assert!(!contains_ignored_path("/project/src/file.ts"));
    // node_modules without dot
    assert!(!contains_ignored_path(
        "/project/node_modules/lodash/index.js"
    ));
    // empty path
    assert!(!contains_ignored_path(""));
    // path with multiple ignored patterns
    assert!(contains_ignored_path(
        "/project/node_modules/.pnpm/.git/.#file.ts"
    ));
    // case sensitive test
    assert!(!contains_ignored_path(
        "/project/NODE_MODULES/.PNPM/file.ts"
    ));
    // path with ignored pattern in middle
    assert!(contains_ignored_path(
        "/project/src/node_modules/.pnpm/dist/file.js"
    ));
    // path with ignored pattern at end
    assert!(contains_ignored_path("/project/src/file.ts.#"));
}

// Go: internal/tspath/ignoredpaths_test.go:TestIgnoredPathsPatterns
#[test]
fn ignored_paths_patterns() {
    let expected_patterns = ["/node_modules/.", "/.git", ".#"];
    for pattern in expected_patterns {
        let test_path = format!("/test{pattern}/file.ts");
        assert!(
            contains_ignored_path(&test_path),
            "expected pattern {pattern:?} to be detected in {test_path:?}"
        );
    }
}

// Go: internal/tspath/ignoredpaths_test.go:TestIgnoredPathsEdgeCases
#[test]
fn ignored_paths_edge_cases() {
    // pattern at start (pattern is "/node_modules/." not "/node_modules.")
    assert!(!contains_ignored_path("/node_modules./file.ts"));
    // pattern at end
    assert!(contains_ignored_path("/project/file.ts.#"));
    // multiple occurrences
    assert!(contains_ignored_path(
        "/project/.git/node_modules./.git/file.ts"
    ));
    // no slashes
    assert!(!contains_ignored_path("node_modules.file.ts"));
    // single slash
    assert!(!contains_ignored_path("/file.ts"));
}
