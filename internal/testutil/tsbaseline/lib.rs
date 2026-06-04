//! `tsgo_testutil_tsbaseline` — baseline utility functions for test infrastructure.
//!
//! 1:1 port of Go `internal/testutil/tsbaseline/util.go`.
//!
//! Provides path-prefix stripping, file-type detection, and test-path
//! sanitisation used throughout the compiler's test-baseline framework.

use regex::Regex;
use std::sync::LazyLock;

static LINE_DELIMITER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\r?\n").expect("valid regex"));

static NON_WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\S").expect("valid regex"));

static TS_EXTENSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.tsx?$").expect("valid regex"));

static TEST_PATH_CHARACTERS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[\^<>:"|?*%]"#).expect("valid regex"));

static TEST_PATH_DOTDOT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.\./").expect("valid regex"));

const LIB_FOLDER: &str = "built/local/";
const BUILT_FOLDER: &str = "/.ts";

/// Removes well-known test-path prefixes from `text`.
///
/// When `retain_trailing_directory_separator` is `true`, prefixes that contain
/// a trailing `/` keep the trailing slash (mirroring Go's
/// `testPathTrailingReplacerTrailingSeparator`).
// Go: internal/testutil/tsbaseline/util.go:removeTestPathPrefixes
pub fn remove_test_path_prefixes(text: &str, retain_trailing_directory_separator: bool) -> String {
    if retain_trailing_directory_separator {
        text.replace("/.ts/", "/")
            .replace("/.lib/", "/")
            .replace("/.src/", "/")
            .replace("bundled:///libs/", "/")
            .replace("file:///./ts/", "file:///")
            .replace("file:///./lib/", "file:///")
            .replace("file:///./src/", "file:///")
    } else {
        text.replace("/.ts/", "")
            .replace("/.lib/", "")
            .replace("/.src/", "")
            .replace("bundled:///libs/", "")
            .replace("file:///./ts/", "file:///")
            .replace("file:///./lib/", "file:///")
            .replace("file:///./src/", "file:///")
    }
}

/// Returns `true` if `file_path` names a default library file (e.g. `lib.d.ts`,
/// `lib.es2015.d.ts`).
// Go: internal/testutil/tsbaseline/util.go:isDefaultLibraryFile
pub fn is_default_library_file(file_path: &str) -> bool {
    let file_name = tsgo_tspath::get_base_file_name(file_path);
    file_name.starts_with("lib.") && file_name.ends_with(tsgo_tspath::EXTENSION_DTS)
}

/// Returns `true` if `file_path` is inside the built/local or `/.ts/` tree.
// Go: internal/testutil/tsbaseline/util.go:isBuiltFile
pub fn is_built_file(file_path: &str) -> bool {
    file_path.starts_with(LIB_FOLDER)
        || file_path.starts_with(&tsgo_tspath::ensure_trailing_directory_separator(
            BUILT_FOLDER,
        ))
}

/// Returns `true` if `path` looks like a `tsconfig*.json` file.
// Go: internal/testutil/tsbaseline/util.go:isTsConfigFile
pub fn is_tsconfig_file(path: &str) -> bool {
    path.contains("tsconfig") && path.contains("json")
}

/// Sanitises a test file path: replaces forbidden characters with `_`,
/// normalises slashes, replaces `../` with `__dotdot/`, lower-cases, and
/// strips the leading `/`.
// Go: internal/testutil/tsbaseline/util.go:sanitizeTestFilePath
pub fn sanitize_test_file_path(name: &str) -> String {
    let path = TEST_PATH_CHARACTERS.replace_all(name, "_");
    let path = tsgo_tspath::normalize_slashes(&path);
    let path = TEST_PATH_DOTDOT.replace_all(&path, "__dotdot/");
    let path = tsgo_tspath::to_path(&path, "", false);
    path.as_str().trim_start_matches('/').to_string()
}

/// Pre-compiled regex that splits on `\r?\n`.
pub fn line_delimiter() -> &'static Regex {
    &LINE_DELIMITER
}

/// Pre-compiled regex matching non-whitespace characters.
pub fn non_whitespace() -> &'static Regex {
    &NON_WHITESPACE
}

/// Pre-compiled regex matching `.ts` or `.tsx` extensions at end of string.
pub fn ts_extension() -> &'static Regex {
    &TS_EXTENSION
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
