use super::*;
use std::cmp::Ordering;

// Go: internal/tspath/path_test.go:TestNormalizeSlashes
#[test]
fn normalize_slashes() {
    assert_eq!(super::normalize_slashes("a"), "a");
    assert_eq!(super::normalize_slashes("a/b"), "a/b");
    assert_eq!(super::normalize_slashes("a\\b"), "a/b");
    assert_eq!(
        super::normalize_slashes("\\\\server\\path"),
        "//server/path"
    );
}

// Go: internal/tspath/path_test.go:TestGetRootLength
#[test]
fn get_root_length_cases() {
    assert_eq!(get_root_length("a"), 0);
    assert_eq!(get_root_length("/"), 1);
    assert_eq!(get_root_length("/path"), 1);
    assert_eq!(get_root_length("c:"), 2);
    assert_eq!(get_root_length("c:d"), 0);
    assert_eq!(get_root_length("c:/"), 3);
    assert_eq!(get_root_length("c:\\"), 3);
    assert_eq!(get_root_length("//server"), 8);
    assert_eq!(get_root_length("//server/share"), 9);
    assert_eq!(get_root_length("\\\\server"), 8);
    assert_eq!(get_root_length("\\\\server\\share"), 9);
    assert_eq!(get_root_length("file:///"), 8);
    assert_eq!(get_root_length("file:///path"), 8);
    assert_eq!(get_root_length("file:///c:"), 10);
    assert_eq!(get_root_length("file:///c:d"), 8);
    assert_eq!(get_root_length("file:///c:/path"), 11);
    assert_eq!(get_root_length("file:///c%3a"), 12);
    assert_eq!(get_root_length("file:///c%3ad"), 8);
    assert_eq!(get_root_length("file:///c%3a/path"), 13);
    assert_eq!(get_root_length("file:///c%3A"), 12);
    assert_eq!(get_root_length("file:///c%3Ad"), 8);
    assert_eq!(get_root_length("file:///c%3A/path"), 13);
    assert_eq!(get_root_length("file://localhost"), 16);
    assert_eq!(get_root_length("file://localhost/"), 17);
    assert_eq!(get_root_length("file://localhost/path"), 17);
    assert_eq!(get_root_length("file://localhost/c:"), 19);
    assert_eq!(get_root_length("file://localhost/c:d"), 17);
    assert_eq!(get_root_length("file://localhost/c:/path"), 20);
    assert_eq!(get_root_length("file://localhost/c%3a"), 21);
    assert_eq!(get_root_length("file://localhost/c%3ad"), 17);
    assert_eq!(get_root_length("file://localhost/c%3a/path"), 22);
    assert_eq!(get_root_length("file://localhost/c%3A"), 21);
    assert_eq!(get_root_length("file://localhost/c%3Ad"), 17);
    assert_eq!(get_root_length("file://localhost/c%3A/path"), 22);
    assert_eq!(get_root_length("file://server"), 13);
    assert_eq!(get_root_length("file://server/"), 14);
    assert_eq!(get_root_length("file://server/path"), 14);
    assert_eq!(get_root_length("file://server/c:"), 14);
    assert_eq!(get_root_length("file://server/c:d"), 14);
    assert_eq!(get_root_length("file://server/c:/d"), 14);
    assert_eq!(get_root_length("file://server/c%3a"), 14);
    assert_eq!(get_root_length("file://server/c%3ad"), 14);
    assert_eq!(get_root_length("file://server/c%3a/d"), 14);
    assert_eq!(get_root_length("file://server/c%3A"), 14);
    assert_eq!(get_root_length("file://server/c%3Ad"), 14);
    assert_eq!(get_root_length("file://server/c%3A/d"), 14);
    assert_eq!(get_root_length("http://server"), 13);
    assert_eq!(get_root_length("http://server/path"), 14);
}

// Go: internal/tspath/path_test.go:TestPathIsAbsolute
#[test]
fn path_is_absolute_cases() {
    assert!(path_is_absolute("/path/to/file.ext"));
    assert!(path_is_absolute("c:/path/to/file.ext"));
    assert!(path_is_absolute("file:///path/to/file.ext"));
    assert!(!path_is_absolute("path/to/file.ext"));
    assert!(!path_is_absolute("./path/to/file.ext"));
}

// Go: internal/tspath/path_test.go:TestIsUrl
#[test]
fn is_url_cases() {
    assert!(!is_url("a"));
    assert!(!is_url("/"));
    assert!(!is_url("c:"));
    assert!(!is_url("c:d"));
    assert!(!is_url("c:/"));
    assert!(!is_url("c:\\"));
    assert!(!is_url("//server"));
    assert!(!is_url("//server/share"));
    assert!(!is_url("\\\\server"));
    assert!(!is_url("\\\\server\\share"));
    assert!(is_url("file:///path"));
    assert!(is_url("file:///c:"));
    assert!(is_url("file:///c:d"));
    assert!(is_url("file:///c:/path"));
    assert!(is_url("file://server"));
    assert!(is_url("file://server/path"));
    assert!(is_url("http://server"));
    assert!(is_url("http://server/path"));
}

// Go: internal/tspath/path_test.go:TestIsRootedDiskPath
#[test]
fn is_rooted_disk_path_cases() {
    assert!(!is_rooted_disk_path("a"));
    assert!(is_rooted_disk_path("/"));
    assert!(is_rooted_disk_path("c:"));
    assert!(!is_rooted_disk_path("c:d"));
    assert!(is_rooted_disk_path("c:/"));
    assert!(is_rooted_disk_path("c:\\"));
    assert!(is_rooted_disk_path("//server"));
    assert!(is_rooted_disk_path("//server/share"));
    assert!(is_rooted_disk_path("\\\\server"));
    assert!(is_rooted_disk_path("\\\\server\\share"));
    assert!(!is_rooted_disk_path("file:///path"));
    assert!(!is_rooted_disk_path("file:///c:"));
    assert!(!is_rooted_disk_path("file:///c:d"));
    assert!(!is_rooted_disk_path("file:///c:/path"));
    assert!(!is_rooted_disk_path("file://server"));
    assert!(!is_rooted_disk_path("file://server/path"));
    assert!(!is_rooted_disk_path("http://server"));
    assert!(!is_rooted_disk_path("http://server/path"));
}

// Go: internal/tspath/path_test.go:TestGetDirectoryPath
#[test]
fn get_directory_path_cases() {
    assert_eq!(get_directory_path(""), "");
    assert_eq!(get_directory_path("a"), "");
    assert_eq!(get_directory_path("a/b"), "a");
    assert_eq!(get_directory_path("/"), "/");
    assert_eq!(get_directory_path("/a"), "/");
    assert_eq!(get_directory_path("/a/"), "/");
    assert_eq!(get_directory_path("/a/b"), "/a");
    assert_eq!(get_directory_path("/a/b/"), "/a");
    assert_eq!(get_directory_path("c:"), "c:");
    assert_eq!(get_directory_path("c:d"), "");
    assert_eq!(get_directory_path("c:/"), "c:/");
    assert_eq!(get_directory_path("c:/path"), "c:/");
    assert_eq!(get_directory_path("c:/path/"), "c:/");
    assert_eq!(get_directory_path("//server"), "//server");
    assert_eq!(get_directory_path("//server/"), "//server/");
    assert_eq!(get_directory_path("//server/share"), "//server/");
    assert_eq!(get_directory_path("//server/share/"), "//server/");
    assert_eq!(get_directory_path("\\\\server"), "//server");
    assert_eq!(get_directory_path("\\\\server\\"), "//server/");
    assert_eq!(get_directory_path("\\\\server\\share"), "//server/");
    assert_eq!(get_directory_path("\\\\server\\share\\"), "//server/");
    assert_eq!(get_directory_path("file:///"), "file:///");
    assert_eq!(get_directory_path("file:///path"), "file:///");
    assert_eq!(get_directory_path("file:///path/"), "file:///");
    assert_eq!(get_directory_path("file:///c:"), "file:///c:");
    assert_eq!(get_directory_path("file:///c:d"), "file:///");
    assert_eq!(get_directory_path("file:///c:/"), "file:///c:/");
    assert_eq!(get_directory_path("file:///c:/path"), "file:///c:/");
    assert_eq!(get_directory_path("file:///c:/path/"), "file:///c:/");
    assert_eq!(get_directory_path("file://server"), "file://server");
    assert_eq!(get_directory_path("file://server/"), "file://server/");
    assert_eq!(get_directory_path("file://server/path"), "file://server/");
    assert_eq!(get_directory_path("file://server/path/"), "file://server/");
    assert_eq!(get_directory_path("http://server"), "http://server");
    assert_eq!(get_directory_path("http://server/"), "http://server/");
    assert_eq!(get_directory_path("http://server/path"), "http://server/");
    assert_eq!(get_directory_path("http://server/path/"), "http://server/");
}

// Go: internal/tspath/path_test.go:TestGetPathComponents
#[test]
fn get_path_components_cases() {
    assert_eq!(get_path_components("", ""), vec![""]);
    assert_eq!(get_path_components("a", ""), vec!["", "a"]);
    assert_eq!(get_path_components("./a", ""), vec!["", ".", "a"]);
    assert_eq!(get_path_components("/", ""), vec!["/"]);
    assert_eq!(get_path_components("/a", ""), vec!["/", "a"]);
    assert_eq!(get_path_components("/a/", ""), vec!["/", "a"]);
    assert_eq!(get_path_components("c:", ""), vec!["c:"]);
    assert_eq!(get_path_components("c:d", ""), vec!["", "c:d"]);
    assert_eq!(get_path_components("c:/", ""), vec!["c:/"]);
    assert_eq!(get_path_components("c:/path", ""), vec!["c:/", "path"]);
    assert_eq!(get_path_components("//server", ""), vec!["//server"]);
    assert_eq!(get_path_components("//server/", ""), vec!["//server/"]);
    assert_eq!(
        get_path_components("//server/share", ""),
        vec!["//server/", "share"]
    );
    assert_eq!(get_path_components("file:///", ""), vec!["file:///"]);
    assert_eq!(
        get_path_components("file:///path", ""),
        vec!["file:///", "path"]
    );
    assert_eq!(get_path_components("file:///c:", ""), vec!["file:///c:"]);
    assert_eq!(
        get_path_components("file:///c:d", ""),
        vec!["file:///", "c:d"]
    );
    assert_eq!(get_path_components("file:///c:/", ""), vec!["file:///c:/"]);
    assert_eq!(
        get_path_components("file:///c:/path", ""),
        vec!["file:///c:/", "path"]
    );
    assert_eq!(
        get_path_components("file://server", ""),
        vec!["file://server"]
    );
    assert_eq!(
        get_path_components("file://server/", ""),
        vec!["file://server/"]
    );
    assert_eq!(
        get_path_components("file://server/path", ""),
        vec!["file://server/", "path"]
    );
    assert_eq!(
        get_path_components("http://server", ""),
        vec!["http://server"]
    );
    assert_eq!(
        get_path_components("http://server/", ""),
        vec!["http://server/"]
    );
    assert_eq!(
        get_path_components("http://server/path", ""),
        vec!["http://server/", "path"]
    );
}

fn rpc(items: &[&str]) -> Vec<String> {
    super::reduce_path_components(&items.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

// Go: internal/tspath/path_test.go:TestReducePathComponents
#[test]
fn reduce_path_components_cases() {
    assert_eq!(rpc(&[""]), vec![""]);
    assert_eq!(rpc(&["", "."]), vec![""]);
    assert_eq!(rpc(&["", ".", "a"]), vec!["", "a"]);
    assert_eq!(rpc(&["", "a", "."]), vec!["", "a"]);
    assert_eq!(rpc(&["", ".."]), vec!["", ".."]);
    assert_eq!(rpc(&["", "..", ".."]), vec!["", "..", ".."]);
    assert_eq!(rpc(&["", "..", ".", ".."]), vec!["", "..", ".."]);
    assert_eq!(rpc(&["", "a", ".."]), vec![""]);
    assert_eq!(rpc(&["", "..", "a"]), vec!["", "..", "a"]);
    assert_eq!(rpc(&["/"]), vec!["/"]);
    assert_eq!(rpc(&["/", "."]), vec!["/"]);
    assert_eq!(rpc(&["/", ".."]), vec!["/"]);
    assert_eq!(rpc(&["/", "a", ".."]), vec!["/"]);
}

// Go: internal/tspath/path_test.go:TestCombinePaths
#[test]
fn combine_paths_cases() {
    assert_eq!(
        combine_paths("path", &["to", "file.ext"]),
        "path/to/file.ext"
    );
    assert_eq!(
        combine_paths("path", &["dir", "..", "to", "file.ext"]),
        "path/dir/../to/file.ext"
    );
    assert_eq!(
        combine_paths("/path", &["to", "file.ext"]),
        "/path/to/file.ext"
    );
    assert_eq!(combine_paths("/path", &["/to", "file.ext"]), "/to/file.ext");
    assert_eq!(
        combine_paths("c:/path", &["to", "file.ext"]),
        "c:/path/to/file.ext"
    );
    assert_eq!(
        combine_paths("c:/path", &["c:/to", "file.ext"]),
        "c:/to/file.ext"
    );
    assert_eq!(
        combine_paths("file:///path", &["to", "file.ext"]),
        "file:///path/to/file.ext"
    );
    assert_eq!(
        combine_paths("file:///path", &["file:///to", "file.ext"]),
        "file:///to/file.ext"
    );
    assert_eq!(
        combine_paths("/", &["/node_modules/@types"]),
        "/node_modules/@types"
    );
    assert_eq!(combine_paths("/a/..", &[""]), "/a/..");
    assert_eq!(combine_paths("/a/..", &["b"]), "/a/../b");
    assert_eq!(combine_paths("/a/..", &["b/"]), "/a/../b/");
    assert_eq!(combine_paths("/a/..", &["/"]), "/");
    assert_eq!(combine_paths("/a/..", &["/b"]), "/b");
}

// Go: internal/tspath/path_test.go:TestResolvePath
#[test]
fn resolve_path_cases() {
    assert_eq!(resolve_path("", &[]), "");
    assert_eq!(resolve_path(".", &[]), "");
    assert_eq!(resolve_path("./", &[]), "");
    assert_eq!(resolve_path("..", &[]), "..");
    assert_eq!(resolve_path("../", &[]), "../");
    assert_eq!(resolve_path("/", &[]), "/");
    assert_eq!(resolve_path("/.", &[]), "/");
    assert_eq!(resolve_path("/./", &[]), "/");
    assert_eq!(resolve_path("/../", &[]), "/");
    assert_eq!(resolve_path("/a", &[]), "/a");
    assert_eq!(resolve_path("/a/", &[]), "/a/");
    assert_eq!(resolve_path("/a/.", &[]), "/a");
    assert_eq!(resolve_path("/a/./", &[]), "/a/");
    assert_eq!(resolve_path("/a/./b", &[]), "/a/b");
    assert_eq!(resolve_path("/a/./b/", &[]), "/a/b/");
    assert_eq!(resolve_path("/a/..", &[]), "/");
    assert_eq!(resolve_path("/a/../", &[]), "/");
    assert_eq!(resolve_path("/a/../b", &[]), "/b");
    assert_eq!(resolve_path("/a/../b/", &[]), "/b/");
    assert_eq!(resolve_path("/a/..", &["b"]), "/b");
    assert_eq!(resolve_path("/a/..", &["/"]), "/");
    assert_eq!(resolve_path("/a/..", &["b/"]), "/b/");
    assert_eq!(resolve_path("/a/..", &["/b"]), "/b");
    assert_eq!(resolve_path("/a/.", &["b"]), "/a/b");
    assert_eq!(resolve_path("/a/.", &["."]), "/a");
    assert_eq!(resolve_path("a", &["b", "c"]), "a/b/c");
    assert_eq!(resolve_path("a", &["b", "/c"]), "/c");
    assert_eq!(resolve_path("a", &["b", "../c"]), "a/c");
}

// Go: internal/tspath/path_test.go:TestGetNormalizedAbsolutePath
#[test]
fn get_normalized_absolute_path_cases() {
    let gnap = get_normalized_absolute_path;
    assert_eq!(gnap("/", ""), "/");
    assert_eq!(gnap("/.", ""), "/");
    assert_eq!(gnap("/./", ""), "/");
    assert_eq!(gnap("/../", ""), "/");
    assert_eq!(gnap("/a", ""), "/a");
    assert_eq!(gnap("/a/", ""), "/a");
    assert_eq!(gnap("/a/.", ""), "/a");
    assert_eq!(gnap("/a/foo.", ""), "/a/foo.");
    assert_eq!(gnap("/a/./", ""), "/a");
    assert_eq!(gnap("/a/./b", ""), "/a/b");
    assert_eq!(gnap("/a/./b/", ""), "/a/b");
    assert_eq!(gnap("/a/..", ""), "/");
    assert_eq!(gnap("/a/../", ""), "/");
    assert_eq!(gnap("/a/../b", ""), "/b");
    assert_eq!(gnap("/a/../b/", ""), "/b");
    assert_eq!(gnap("/a/..", "/"), "/");
    assert_eq!(gnap("/a/..", "b/"), "/");
    assert_eq!(gnap("/a/..", "/b"), "/");
    assert_eq!(gnap("/a/.", "b"), "/a");
    assert_eq!(gnap("/a/.", "."), "/a");

    // Backslash variants.
    assert_eq!(gnap("\\", ""), "/");
    assert_eq!(gnap("\\.", ""), "/");
    assert_eq!(gnap("\\.\\", ""), "/");
    assert_eq!(gnap("\\..\\", ""), "/");
    assert_eq!(gnap("\\a\\.\\", ""), "/a");
    assert_eq!(gnap("\\a\\.\\b", ""), "/a/b");
    assert_eq!(gnap("\\a\\.\\b\\", ""), "/a/b");
    assert_eq!(gnap("\\a\\..", ""), "/");
    assert_eq!(gnap("\\a\\..\\", ""), "/");
    assert_eq!(gnap("\\a\\..\\b", ""), "/b");
    assert_eq!(gnap("\\a\\..\\b\\", ""), "/b");
    assert_eq!(gnap("\\a\\..", "\\"), "/");
    assert_eq!(gnap("\\a\\..", "b\\"), "/");
    assert_eq!(gnap("\\a\\..", "\\b"), "/");
    assert_eq!(gnap("\\a\\.", "b"), "/a");
    assert_eq!(gnap("\\a\\.", "."), "/a");

    // Relative paths with empty currentDirectory.
    assert_eq!(gnap("", ""), "");
    assert_eq!(gnap(".", ""), "");
    assert_eq!(gnap("./", ""), "");
    assert_eq!(gnap("..", ""), "..");
    assert_eq!(gnap("../", ""), "..");

    // Interaction with currentDirectory.
    assert_eq!(gnap("", "/home"), "/home");
    assert_eq!(gnap(".", "/home"), "/home");
    assert_eq!(gnap("./", "/home"), "/home");
    assert_eq!(gnap("..", "/home"), "/");
    assert_eq!(gnap("../", "/home"), "/");
    assert_eq!(gnap("a", "b"), "b/a");
    assert_eq!(gnap("a", "b/c"), "b/c/a");

    // Base names with dots.
    assert_eq!(gnap(".a", ""), ".a");
    assert_eq!(gnap("..a", ""), "..a");
    assert_eq!(gnap("a.", ""), "a.");
    assert_eq!(gnap("a..", ""), "a..");

    assert_eq!(gnap("/base/./.a", ""), "/base/.a");
    assert_eq!(gnap("/base/../.a", ""), "/.a");
    assert_eq!(gnap("/base/./..a", ""), "/base/..a");
    assert_eq!(gnap("/base/../..a", ""), "/..a");
    assert_eq!(gnap("/base/./..a/b", ""), "/base/..a/b");
    assert_eq!(gnap("/base/../..a/b", ""), "/..a/b");

    assert_eq!(gnap("/base/./a.", ""), "/base/a.");
    assert_eq!(gnap("/base/../a.", ""), "/a.");
    assert_eq!(gnap("/base/./a..", ""), "/base/a..");
    assert_eq!(gnap("/base/../a..", ""), "/a..");
    assert_eq!(gnap("/base/./a../b", ""), "/base/a../b");
    assert_eq!(gnap("/base/../a../b", ""), "/a../b");

    assert_eq!(gnap("a/..", ""), "");
    assert_eq!(gnap("/a//", ""), "/a");
    assert_eq!(gnap("//a", "a"), "//a/");
    assert_eq!(gnap("/\\", ""), "//");
    assert_eq!(gnap("a///", "a"), "a/a");
    assert_eq!(gnap("/.//", ""), "/");
    assert_eq!(gnap("//\\\\", ""), "///");
    assert_eq!(gnap(".//a", "."), "a");
    assert_eq!(gnap("a/../..", ""), "..");
    assert_eq!(gnap("../..", "\\a"), "/");
    assert_eq!(gnap("a:", "b"), "a:/");
    assert_eq!(gnap("a/../..", ".."), "../..");
    assert_eq!(gnap("a/../..", "b"), "");
    assert_eq!(gnap("a//../..", ".."), "../..");

    // Consecutive intermediate slashes.
    assert_eq!(gnap("a//b", ""), "a/b");
    assert_eq!(gnap("a///b", ""), "a/b");
    assert_eq!(gnap("a/b//c", ""), "a/b/c");
    assert_eq!(gnap("/a/b//c", ""), "/a/b/c");
    assert_eq!(gnap("//a/b//c", ""), "//a/b/c");

    // Backslashes converted to slashes then collapsed.
    assert_eq!(gnap("a\\\\b", ""), "a/b");
    assert_eq!(gnap("a\\\\\\b", ""), "a/b");
    assert_eq!(gnap("a\\b\\\\c", ""), "a/b/c");
    assert_eq!(gnap("\\a\\b\\\\c", ""), "/a/b/c");
    assert_eq!(gnap("\\\\a\\b\\\\c", ""), "//a/b/c");

    // Mixed slashes.
    assert_eq!(gnap("a/\\b", ""), "a/b");
    assert_eq!(gnap("a\\/b", ""), "a/b");
    assert_eq!(gnap("a\\/\\b", ""), "a/b");
    assert_eq!(gnap("a\\b//c", ""), "a/b/c");
}

// Go: internal/tspath/path_test.go:TestGetNormalizedAbsolutePathWithoutRoot
#[test]
fn get_normalized_absolute_path_without_root_cases() {
    assert_eq!(
        get_normalized_absolute_path_without_root("/a/b/c.txt", "/a/b"),
        "a/b/c.txt"
    );
    assert_eq!(
        get_normalized_absolute_path_without_root("c:/work/hello.txt", "c:/work"),
        "work/hello.txt"
    );
    assert_eq!(
        get_normalized_absolute_path_without_root("c:/work/hello.txt", "d:/worspaces"),
        "work/hello.txt"
    );
}

// Go: internal/tspath/path_test.go:TestGetRelativePathToDirectoryOrUrl
#[test]
fn get_relative_path_to_directory_or_url_cases() {
    let o = ComparePathsOptions::default();
    let f = |dir: &str, p: &str| get_relative_path_to_directory_or_url(dir, p, false, &o);
    assert_eq!(f("/", "/"), "");
    assert_eq!(f("/a", "/a"), "");
    assert_eq!(f("/a/", "/a"), "");
    assert_eq!(f("/a", "/"), "..");
    assert_eq!(f("/a", "/b"), "../b");
    assert_eq!(f("/a/b", "/b"), "../../b");
    assert_eq!(f("/a/b/c", "/b"), "../../../b");
    assert_eq!(f("/a/b/c", "/b/c"), "../../../b/c");
    assert_eq!(f("/a/b/c", "/a/b"), "..");
    assert_eq!(f("c:", "d:"), "d:/");
    assert_eq!(f("file:///", "file:///"), "");
    assert_eq!(f("file:///a", "file:///a"), "");
    assert_eq!(f("file:///a/", "file:///a"), "");
    assert_eq!(f("file:///a", "file:///"), "..");
    assert_eq!(f("file:///a", "file:///b"), "../b");
    assert_eq!(f("file:///a/b", "file:///b"), "../../b");
    assert_eq!(f("file:///a/b/c", "file:///b"), "../../../b");
    assert_eq!(f("file:///a/b/c", "file:///b/c"), "../../../b/c");
    assert_eq!(f("file:///a/b/c", "file:///a/b"), "..");
    assert_eq!(f("file:///c:", "file:///d:"), "file:///d:/");
}

// Go: internal/tspath/path_test.go:TestToFileNameLowerCase
#[test]
fn to_file_name_lower_case_cases() {
    assert_eq!(
        to_file_name_lower_case("/user/UserName/projects/Project/file.ts"),
        "/user/username/projects/project/file.ts"
    );
    assert_eq!(
        to_file_name_lower_case("/user/UserName/projects/projectß/file.ts"),
        "/user/username/projects/projectß/file.ts"
    );
    assert_eq!(
        to_file_name_lower_case("/user/UserName/projects/İproject/file.ts"),
        "/user/username/projects/İproject/file.ts"
    );
    assert_eq!(
        to_file_name_lower_case("/user/UserName/projects/ı/file.ts"),
        "/user/username/projects/ı/file.ts"
    );
}

// Go: internal/tspath/path_test.go:TestToPath
#[test]
fn to_path_cases() {
    assert_eq!(
        to_path("file.ext", "path/to", false).as_str(),
        "path/to/file.ext"
    );
    assert_eq!(
        to_path("file.ext", "/path/to", true).as_str(),
        "/path/to/file.ext"
    );
    assert_eq!(
        to_path("/path/to/../file.ext", "path/to", true).as_str(),
        "/path/file.ext"
    );
}

// Go: internal/tspath/path_test.go:TestPathIsRelative
#[test]
fn path_is_relative_cases() {
    // Forward-slash forms (relative).
    assert!(path_is_relative("."));
    assert!(path_is_relative(".."));
    assert!(path_is_relative("./"));
    assert!(path_is_relative("../"));
    assert!(path_is_relative("./foo/bar"));
    assert!(path_is_relative("../foo/bar"));
    assert!(path_is_relative(&format!("../{}", "foo/".repeat(100))));
    // Non-relative.
    assert!(!path_is_relative(""));
    assert!(!path_is_relative("foo"));
    assert!(!path_is_relative("foo/bar"));
    assert!(!path_is_relative("/foo/bar"));
    assert!(!path_is_relative("c:/foo/bar"));

    // Backslash forms (init() in Go duplicates the set with `/` replaced by `\`).
    assert!(path_is_relative("."));
    assert!(path_is_relative(".."));
    assert!(path_is_relative(".\\"));
    assert!(path_is_relative("..\\"));
    assert!(path_is_relative(".\\foo\\bar"));
    assert!(path_is_relative("..\\foo\\bar"));
    assert!(path_is_relative(&format!("..\\{}", "foo\\".repeat(100))));
    assert!(!path_is_relative(""));
    assert!(!path_is_relative("foo"));
    assert!(!path_is_relative("foo\\bar"));
    assert!(!path_is_relative("\\foo\\bar"));
    assert!(!path_is_relative("c:\\foo\\bar"));
}

fn common_parents(paths: &[&str], min: usize) -> (Vec<String>, std::collections::HashSet<String>) {
    get_common_parents(
        paths,
        min,
        get_path_components,
        &ComparePathsOptions::default(),
    )
}

// Go: internal/tspath/path_test.go:TestGetCommonParents/empty input
#[test]
fn get_common_parents_empty_input() {
    let (got, ignored) = common_parents(&[], 1);
    assert_eq!(ignored.len(), 0);
    assert_eq!(got.len(), 0);
}

// Go: internal/tspath/path_test.go:TestGetCommonParents/single path returns itself
#[test]
fn get_common_parents_single() {
    let (got, ignored) = common_parents(&["/a/b/c/d"], 1);
    assert_eq!(ignored.len(), 0);
    assert_eq!(got, vec!["/a/b/c/d"]);
}

// Go: internal/tspath/path_test.go:TestGetCommonParents/paths shorter than minComponents are ignored
#[test]
fn get_common_parents_short_ignored() {
    let (got, ignored) = common_parents(&["/a/b/c/d", "/a/b/c/e", "/a/b/f/g", "/x/y"], 4);
    assert_eq!(ignored.len(), 1);
    assert!(ignored.contains("/x/y"));
    assert_eq!(got, vec!["/a/b/c", "/a/b/f/g"]);
}

// Go: internal/tspath/path_test.go:TestGetCommonParents/three paths share /a/b
#[test]
fn get_common_parents_three_share_ab() {
    let (got, ignored) = common_parents(&["/a/b/c/d", "/a/b/c/e", "/a/b/f/g"], 1);
    assert_eq!(ignored.len(), 0);
    assert_eq!(got, vec!["/a/b"]);
}

// Go: internal/tspath/path_test.go:TestGetCommonParents/mixed with short path collapses to root when minComponents=1
#[test]
fn get_common_parents_mixed_collapse_root() {
    let (got, ignored) = common_parents(&["/a/b/c/d", "/a/b/c/e", "/a/b/f/g", "/x/y/z"], 1);
    assert_eq!(ignored.len(), 0);
    assert_eq!(got, vec!["/"]);
}

// Go: internal/tspath/path_test.go:TestGetCommonParents/mixed with short path preserves both when minComponents=3
#[test]
fn get_common_parents_mixed_preserve_min3() {
    let (got, ignored) = common_parents(&["/a/b/c/d", "/a/b/c/e", "/a/b/f/g", "/x/y/z"], 3);
    assert_eq!(ignored.len(), 0);
    assert_eq!(got, vec!["/a/b", "/x/y/z"]);
}

// Go: internal/tspath/path_test.go:TestGetCommonParents/different volumes are returned individually
#[test]
fn get_common_parents_diff_volumes() {
    let (got, ignored) = common_parents(&["c:/a/b/c/d", "d:/a/b/c/d"], 1);
    assert_eq!(ignored.len(), 0);
    assert_eq!(got, vec!["c:/a/b/c/d", "d:/a/b/c/d"]);
}

// Go: internal/tspath/path_test.go:TestGetCommonParents/duplicate paths deduplicate result
#[test]
fn get_common_parents_duplicate_dedup() {
    let (got, ignored) = common_parents(&["/a/b/c/d", "/a/b/c/d"], 1);
    assert_eq!(ignored.len(), 0);
    assert_eq!(got, vec!["/a/b/c/d"]);
}

// Go: internal/tspath/path_test.go:TestGetCommonParents/paths with few components are returned as-is when minComponents met
#[test]
fn get_common_parents_few_components_asis() {
    let (got, ignored) = common_parents(&["/a/b/c/d", "/x/y"], 2);
    assert_eq!(ignored.len(), 0);
    assert_eq!(got, vec!["/a/b/c/d", "/x/y"]);
}

// Go: internal/tspath/path_test.go:TestGetCommonParents/minComponents=2
#[test]
fn get_common_parents_min2() {
    let (got, ignored) = common_parents(&["/a/b/c/d", "/a/z/c/e", "/a/aaa/f/g", "/x/y/z"], 2);
    assert_eq!(ignored.len(), 0);
    assert_eq!(got, vec!["/a", "/x/y/z"]);
}

// Go: internal/tspath/path_test.go:TestGetCommonParents/trailing separators are handled
#[test]
fn get_common_parents_trailing_seps() {
    let (got, ignored) = common_parents(&["/a/b/", "/a/b/c"], 1);
    assert_eq!(ignored.len(), 0);
    assert_eq!(got, vec!["/a/b"]);
}

// Go: internal/tspath/path_test.go:TestCompareNumberOfDirectorySeparators (behavior-level supplement)
#[test]
fn compare_number_of_directory_separators_basic() {
    assert_eq!(
        compare_number_of_directory_separators("/a/b", "/a"),
        Ordering::Greater
    );
    assert_eq!(
        compare_number_of_directory_separators("/a", "/a/b"),
        Ordering::Less
    );
    assert_eq!(
        compare_number_of_directory_separators("/a/b", "/c/d"),
        Ordering::Equal
    );
}
