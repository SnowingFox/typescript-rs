use super::*;

// Go: internal/tspath/untitled_test.go:TestUntitledPathHandling
#[test]
fn untitled_path_handling() {
    let untitled_path = "^/untitled/ts-nul-authority/Untitled-2";

    // GetEncodedRootLength should return 2 for "^/".
    assert_eq!(get_encoded_root_length(untitled_path), 2);

    // IsRootedDiskPath should return true.
    assert!(is_rooted_disk_path(untitled_path));

    // ToPath should not resolve untitled paths against the current directory.
    let current_dir = "/home/user/project";
    let path = to_path(untitled_path, current_dir, true);
    assert_eq!(path.as_str(), "^/untitled/ts-nul-authority/Untitled-2");

    // GetNormalizedAbsolutePath should not resolve untitled paths.
    let normalized = get_normalized_absolute_path(untitled_path, current_dir);
    assert_eq!(normalized, "^/untitled/ts-nul-authority/Untitled-2");
}

// Go: internal/tspath/untitled_test.go:TestUntitledPathEdgeCases
#[test]
fn untitled_path_edge_cases() {
    let cases: &[(&str, i32, bool)] = &[
        ("^/", 2, true),
        ("^/untitled/ts-nul-authority/test", 2, true),
        ("^", 0, false),
        ("^x", 0, false),
        ("^^/", 0, false),
        ("x^/", 0, false),
        (
            "^/untitled/ts-nul-authority/path/with/deeper/structure",
            2,
            true,
        ),
    ];
    for &(path, expected, is_rooted) in cases {
        assert_eq!(get_encoded_root_length(path), expected, "path {path}");
        assert_eq!(is_rooted_disk_path(path), is_rooted, "path {path}");
    }
}
