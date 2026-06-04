use super::*;

#[test]
fn test_remove_test_path_prefixes_ts() {
    assert_eq!(
        remove_test_path_prefixes("/.ts/foo/bar.ts", false),
        "foo/bar.ts"
    );
}

#[test]
fn test_remove_test_path_prefixes_lib() {
    assert_eq!(
        remove_test_path_prefixes("/.lib/foo/bar.d.ts", false),
        "foo/bar.d.ts"
    );
}

#[test]
fn test_remove_test_path_prefixes_src() {
    assert_eq!(
        remove_test_path_prefixes("/.src/index.ts", false),
        "index.ts"
    );
}

#[test]
fn test_remove_test_path_prefixes_bundled() {
    assert_eq!(
        remove_test_path_prefixes("bundled:///libs/lib.d.ts", false),
        "lib.d.ts"
    );
}

#[test]
fn test_remove_test_path_prefixes_trailing_separator() {
    assert_eq!(remove_test_path_prefixes("/.ts/foo/", true), "/foo/");
    assert_eq!(remove_test_path_prefixes("/.ts/foo/", false), "foo/");
}

#[test]
fn test_remove_test_path_prefixes_file_uri() {
    assert_eq!(
        remove_test_path_prefixes("file:///./ts/foo.ts", false),
        "file:///foo.ts"
    );
    assert_eq!(
        remove_test_path_prefixes("file:///./lib/lib.d.ts", false),
        "file:///lib.d.ts"
    );
    assert_eq!(
        remove_test_path_prefixes("file:///./src/index.ts", false),
        "file:///index.ts"
    );
}

#[test]
fn test_is_default_library_file() {
    assert!(is_default_library_file("lib.d.ts"));
    assert!(is_default_library_file("lib.es2015.d.ts"));
    assert!(is_default_library_file("/path/to/lib.dom.d.ts"));
    assert!(!is_default_library_file("mylib.d.ts"));
    assert!(!is_default_library_file("lib.ts"));
    assert!(!is_default_library_file("foo.d.ts"));
}

#[test]
fn test_is_built_file() {
    assert!(is_built_file("built/local/lib.d.ts"));
    assert!(is_built_file("/.ts/foo.ts"));
    assert!(!is_built_file("/src/foo.ts"));
    assert!(!is_built_file("other/local/lib.d.ts"));
}

#[test]
fn test_is_tsconfig_file() {
    assert!(is_tsconfig_file("tsconfig.json"));
    assert!(is_tsconfig_file("/project/tsconfig.base.json"));
    assert!(!is_tsconfig_file("package.json"));
    assert!(!is_tsconfig_file("config.ts"));
}

#[test]
fn test_sanitize_test_file_path_basic() {
    assert_eq!(sanitize_test_file_path("test/foo.ts"), "test/foo.ts");
}

#[test]
fn test_sanitize_test_file_path_special_chars() {
    let sanitized = sanitize_test_file_path("test<file>:name.ts");
    assert!(!sanitized.contains('<'));
    assert!(!sanitized.contains('>'));
    assert!(!sanitized.contains(':'));
}

#[test]
fn test_sanitize_test_file_path_dotdot() {
    let sanitized = sanitize_test_file_path("../parent/file.ts");
    assert!(sanitized.contains("__dotdot"));
    assert!(!sanitized.contains("../"));
}

#[test]
fn test_sanitize_test_file_path_strips_leading_slash() {
    let path = sanitize_test_file_path("/absolute/path.ts");
    assert!(!path.starts_with('/'));
}
