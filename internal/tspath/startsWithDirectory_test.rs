use super::*;

// Go: internal/tspath/startsWithDirectory_test.go:TestStartsWithDirectory
#[test]
fn starts_with_directory_cases() {
    // exact match case sensitive
    assert!(starts_with_directory(
        "/project/src/file.ts",
        "/project/src",
        true
    ));
    // exact match case insensitive
    assert!(starts_with_directory(
        "/project/src/file.ts",
        "/PROJECT/SRC",
        false
    ));
    // case sensitive mismatch
    assert!(!starts_with_directory(
        "/project/src/file.ts",
        "/PROJECT/SRC",
        true
    ));
    // file not in directory
    assert!(!starts_with_directory(
        "/project/lib/file.ts",
        "/project/src",
        true
    ));
    // file in subdirectory
    assert!(starts_with_directory(
        "/project/src/components/Button.tsx",
        "/project/src",
        true
    ));
    // file in parent directory
    assert!(!starts_with_directory(
        "/project/file.ts",
        "/project/src",
        true
    ));
    // windows style separators
    assert!(starts_with_directory(
        "C:\\project\\src\\file.ts",
        "C:\\project\\src",
        true
    ));
    // mixed separators
    assert!(!starts_with_directory(
        "/project/src/file.ts",
        "\\project\\src",
        true
    ));
    // empty directory name
    assert!(!starts_with_directory("/project/src/file.ts", "", true));
    // empty file name
    assert!(!starts_with_directory("", "/project/src", true));
    // identical paths
    assert!(!starts_with_directory("/project/src", "/project/src", true));
    // directory with trailing separator
    assert!(starts_with_directory(
        "/project/src/file.ts",
        "/project/src/",
        true
    ));
    // unicode characters
    assert!(starts_with_directory(
        "/project/测试/file.ts",
        "/project/测试",
        true
    ));
    // unicode case insensitive
    assert!(starts_with_directory(
        "/project/测试/file.ts",
        "/PROJECT/测试",
        false
    ));
}

// Go: internal/tspath/startsWithDirectory_test.go:TestStartsWithDirectoryEdgeCases
#[test]
fn starts_with_directory_edge_cases() {
    // file name shorter than directory
    assert!(!starts_with_directory("/proj", "/project", true));
    // file name starts with directory but no separator
    assert!(!starts_with_directory(
        "/projectsrc/file.ts",
        "/project",
        true
    ));
    // relative paths
    assert!(starts_with_directory("src/file.ts", "src", true));
    // absolute vs relative
    assert!(!starts_with_directory(
        "/project/src/file.ts",
        "project/src",
        true
    ));
}
