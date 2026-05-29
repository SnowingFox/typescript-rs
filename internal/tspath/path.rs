//! TypeScript path model and path utilities (normalization, root length,
//! combining, relative paths, case canonicalization, comparison).
//!
//! 1:1 port of Go `internal/tspath/path.go`.
//!
//! Internally, paths are represented as strings with `/` as the directory
//! separator. [`Path`] is a newtype over `String` for an already
//! rooted/reduced/case-canonicalized path used as a cache key. This is *not*
//! [`std::path::Path`] (which uses platform separators).

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use tsgo_stringutil::{
    compare_strings_case_insensitive, equate_string_case_insensitive, get_string_comparer,
    get_string_equality_comparer,
};

/// The directory separator used internally for all paths.
// Go: internal/tspath/path.go:DirectorySeparator
pub const DIRECTORY_SEPARATOR: char = '/';

const URL_SCHEME_SEPARATOR: &str = "://";

/// A rooted, reduced, and case-canonicalized path used as a stable map key.
///
/// Mirrors Go's `type Path string`. Construct via [`to_path`].
///
/// # Examples
/// ```
/// use tsgo_tspath::to_path;
/// let p = to_path("file.ext", "/path/to", true);
/// assert_eq!(p.as_str(), "/path/to/file.ext");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path(
    /// The canonicalized path string.
    pub String,
);

impl Path {
    /// Returns the path as a string slice.
    ///
    /// Side effects: none (pure).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the directory portion of this path.
    ///
    /// Side effects: none (pure).
    // Go: internal/tspath/path.go:GetDirectoryPath
    pub fn get_directory_path(&self) -> Path {
        Path(get_directory_path(&self.0))
    }

    /// Returns this path without a trailing directory separator.
    ///
    /// Side effects: none (pure).
    // Go: internal/tspath/path.go:RemoveTrailingDirectorySeparator
    pub fn remove_trailing_directory_separator(&self) -> Path {
        Path(remove_trailing_directory_separator(&self.0).to_string())
    }

    /// Returns this path with a trailing directory separator ensured.
    ///
    /// Side effects: none (pure).
    // Go: internal/tspath/path.go:EnsureTrailingDirectorySeparator
    pub fn ensure_trailing_directory_separator(&self) -> Path {
        Path(ensure_trailing_directory_separator(&self.0))
    }

    /// Reports whether `child` is contained within or equal to this path.
    ///
    /// Since [`Path`] values are already rooted, reduced, and case-canonical,
    /// this is a simple string prefix check.
    ///
    /// Side effects: none (pure).
    // Go: internal/tspath/path.go:ContainsPath
    pub fn contains_path(&self, child: &Path) -> bool {
        let p = self.0.as_bytes();
        let c = child.0.as_bytes();
        if p.is_empty() {
            return false;
        }
        self.0 == child.0
            || (c.len() > p.len()
                && child.0.starts_with(self.0.as_str())
                && (p[p.len() - 1] == b'/' || c[p.len()] == b'/'))
    }
}

/// Reports whether a byte corresponds to `/` or `\`.
fn is_any_directory_separator(ch: u8) -> bool {
    ch == b'/' || ch == b'\\'
}

/// Reports whether a path starts with a URL scheme (e.g. `http://`, `file://`).
///
/// # Examples
/// ```
/// use tsgo_tspath::is_url;
/// assert!(is_url("file:///path"));
/// assert!(!is_url("/path"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:IsUrl
pub fn is_url(path: &str) -> bool {
    get_encoded_root_length(path) < 0
}

/// Reports whether a path is an absolute disk path (e.g. `/`, `c:`, `c:/`).
///
/// # Examples
/// ```
/// use tsgo_tspath::is_rooted_disk_path;
/// assert!(is_rooted_disk_path("/"));
/// assert!(!is_rooted_disk_path("a"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:IsRootedDiskPath
pub fn is_rooted_disk_path(path: &str) -> bool {
    get_encoded_root_length(path) > 0
}

/// Reports whether a path consists only of a path root.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:IsDiskPathRoot
pub fn is_disk_path_root(path: &str) -> bool {
    let root_length = get_encoded_root_length(path);
    root_length > 0 && root_length as usize == path.len()
}

/// Reports whether a file name represents a dynamic/virtual file (untitled,
/// e.g. `^/untitled/...`).
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:IsDynamicFileName
pub fn is_dynamic_file_name(file_name: &str) -> bool {
    file_name.starts_with("^/")
}

/// Reports whether a path starts with an absolute component (`/`, `c:/`,
/// `file://`, etc.).
///
/// # Examples
/// ```
/// use tsgo_tspath::path_is_absolute;
/// assert!(path_is_absolute("/path/to/file.ext"));
/// assert!(!path_is_absolute("./path/to/file.ext"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:PathIsAbsolute
pub fn path_is_absolute(path: &str) -> bool {
    get_encoded_root_length(path) != 0
}

/// Reports whether a path ends with a directory separator (`/` or `\`).
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:HasTrailingDirectorySeparator
pub fn has_trailing_directory_separator(path: &str) -> bool {
    let bytes = path.as_bytes();
    !bytes.is_empty() && is_any_directory_separator(bytes[bytes.len() - 1])
}

/// Combines paths. An absolute path replaces any previous path; relative paths
/// are not simplified.
///
/// # Examples
/// ```
/// use tsgo_tspath::combine_paths;
/// assert_eq!(combine_paths("path", &["to", "file.ext"]), "path/to/file.ext");
/// assert_eq!(combine_paths("/path", &["/to", "file.ext"]), "/to/file.ext");
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:CombinePaths
pub fn combine_paths(first_path: &str, paths: &[&str]) -> String {
    let first_path = normalize_slashes(first_path);

    let mut size = first_path.len() + paths.len();
    for p in paths {
        size += p.len();
    }
    let mut b = String::with_capacity(size);
    b.push_str(&first_path);

    // Track the start offset so an absolute trailing path can "reset" the result.
    let mut start = 0usize;

    for &trailing_path in paths {
        if trailing_path.is_empty() {
            continue;
        }
        let trailing_path = normalize_slashes(trailing_path);
        let current_empty = b.len() == start;
        if current_empty || get_root_length(&trailing_path) != 0 {
            start = b.len();
            b.push_str(&trailing_path);
        } else {
            if !has_trailing_directory_separator(&b[start..]) {
                b.push(DIRECTORY_SEPARATOR);
            }
            b.push_str(&trailing_path);
        }
    }
    b[start..].to_string()
}

/// Splits a path into components, combining with `current_directory` first.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetPathComponents
pub fn get_path_components(path: &str, current_directory: &str) -> Vec<String> {
    let path = combine_paths(current_directory, &[path]);
    let root_length = get_root_length(&path);
    path_components(&path, root_length)
}

fn path_components(path: &str, root_length: usize) -> Vec<String> {
    let root = &path[..root_length];
    let mut rest: Vec<&str> = path[root_length..].split('/').collect();
    if let Some(last) = rest.last() {
        if last.is_empty() {
            rest.pop();
        }
    }
    let mut result = Vec::with_capacity(1 + rest.len());
    result.push(root.to_string());
    for r in rest {
        result.push(r.to_string());
    }
    result
}

/// Reports whether a byte is an ASCII volume character (`a-z` / `A-Z`).
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:IsVolumeCharacter
pub fn is_volume_character(ch: u8) -> bool {
    ch.is_ascii_alphabetic()
}

fn get_file_url_volume_separator_end(url: &str, start: usize) -> i32 {
    let bytes = url.as_bytes();
    if bytes.len() <= start {
        return -1;
    }
    let ch0 = bytes[start];
    if ch0 == b':' {
        return (start + 1) as i32;
    }
    if ch0 == b'%' && bytes.len() > start + 2 && bytes[start + 1] == b'3' {
        let ch2 = bytes[start + 2];
        if ch2 == b'a' || ch2 == b'A' {
            return (start + 3) as i32;
        }
    }
    -1
}

/// Computes the encoded root length: positive for disk roots, negative
/// (bitwise complement) for URL roots, 0 for relative paths.
///
/// Handles POSIX (`/`), UNC (`//server/`), DOS (`c:/`), untitled (`^/`), and
/// URL (`file://`/`http://`) roots. URL roots are encoded with `!x` to mark
/// "this is a URL". Use [`get_root_length`] to decode.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetEncodedRootLength
pub fn get_encoded_root_length(path: &str) -> i32 {
    let bytes = path.as_bytes();
    let ln = bytes.len();
    if ln == 0 {
        return 0;
    }
    let ch0 = bytes[0];

    // POSIX or UNC
    if ch0 == b'/' || ch0 == b'\\' {
        if ln == 1 || bytes[1] != ch0 {
            return 1; // POSIX: "/" (or non-normalized "\")
        }
        let offset = 2;
        match bytes[offset..].iter().position(|&b| b == ch0) {
            None => return ln as i32, // UNC: "//server" or "\\server"
            Some(p1) => return (p1 + offset + 1) as i32, // UNC: "//server/" or "\\server\"
        }
    }

    // DOS
    if is_volume_character(ch0) && ln > 1 && bytes[1] == b':' {
        if ln == 2 {
            return 2; // DOS: "c:" (but not "c:d")
        }
        let ch2 = bytes[2];
        if ch2 == b'/' || ch2 == b'\\' {
            return 3; // DOS: "c:/" or "c:\"
        }
    }

    // Untitled paths (e.g. "^/untitled/ts-nul-authority/Untitled-1")
    if ch0 == b'^' && ln > 1 && bytes[1] == b'/' {
        return 2; // Untitled: "^/"
    }

    // URL
    if let Some(scheme_end) = path.find(URL_SCHEME_SEPARATOR) {
        let authority_start = scheme_end + URL_SCHEME_SEPARATOR.len();
        if let Some(authority_length) = path[authority_start..].find('/') {
            let authority_end = authority_start + authority_length;

            // For local "file" URLs, include the leading DOS volume (if present).
            // A host of "" or "localhost" is special-cased per RFC 1738.
            let scheme = &path[..scheme_end];
            let authority = &path[authority_start..authority_end];
            if scheme == "file"
                && (authority.is_empty() || authority == "localhost")
                && ln > authority_end + 2
                && is_volume_character(bytes[authority_end + 1])
            {
                let volume_separator_end =
                    get_file_url_volume_separator_end(path, authority_end + 2);
                if volume_separator_end != -1 {
                    let vse = volume_separator_end as usize;
                    if vse == ln {
                        return !volume_separator_end;
                    }
                    if bytes[vse] == b'/' {
                        return !(volume_separator_end + 1);
                    }
                }
            }
            return !((authority_end + 1) as i32); // URL: "file://server/", "http://server/"
        }
        return !(ln as i32); // URL: "file://server", "http://server"
    }

    // relative
    0
}

/// Decodes the root length: the absolute byte length of a path's root.
///
/// # Examples
/// ```
/// use tsgo_tspath::get_root_length;
/// assert_eq!(get_root_length("/path"), 1);
/// assert_eq!(get_root_length("c:/"), 3);
/// assert_eq!(get_root_length("file:///c:"), 10);
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetRootLength
pub fn get_root_length(path: &str) -> usize {
    let root_length = get_encoded_root_length(path);
    if root_length < 0 {
        (!root_length) as usize
    } else {
        root_length as usize
    }
}

/// Returns the directory portion of `path` (up to the last non-terminal
/// separator), excluding any trailing separator.
///
/// # Examples
/// ```
/// use tsgo_tspath::get_directory_path;
/// assert_eq!(get_directory_path("/a/b"), "/a");
/// assert_eq!(get_directory_path("/"), "/");
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetDirectoryPath
pub fn get_directory_path(path: &str) -> String {
    let normalized = normalize_slashes(path);
    let root_length = get_root_length(&normalized);
    if root_length == normalized.len() {
        return normalized;
    }
    let trimmed = remove_trailing_directory_separator(&normalized);
    let end = match trimmed.rfind('/') {
        Some(i) => i.max(root_length),
        None => root_length,
    };
    trimmed[..end].to_string()
}

/// Joins path components back into a path string.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetPathFromPathComponents
pub fn get_path_from_path_components(path_components: &[String]) -> String {
    if path_components.is_empty() {
        return String::new();
    }
    let mut root = path_components[0].clone();
    if !root.is_empty() {
        root = ensure_trailing_directory_separator(&root);
    }
    root + &path_components[1..].join("/")
}

/// Replaces backslashes with forward slashes.
///
/// # Examples
/// ```
/// use tsgo_tspath::normalize_slashes;
/// assert_eq!(normalize_slashes("a\\b"), "a/b");
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:NormalizeSlashes
pub fn normalize_slashes(path: &str) -> String {
    path.replace('\\', "/")
}

fn reduce_path_components(components: &[String]) -> Vec<String> {
    if components.is_empty() {
        return Vec::new();
    }
    let mut reduced: Vec<String> = vec![components[0].clone()];
    for component in &components[1..] {
        if component.is_empty() {
            continue;
        }
        if component == "." {
            continue;
        }
        if component == ".." {
            if reduced.len() > 1 {
                if reduced[reduced.len() - 1] != ".." {
                    reduced.pop();
                    continue;
                }
            } else if reduced[0] != *"" {
                continue;
            }
        }
        reduced.push(component.clone());
    }
    reduced
}

/// Combines and resolves paths. Absolute paths replace prior ones; `.`/`..` are
/// resolved; trailing separators preserved.
///
/// # Examples
/// ```
/// use tsgo_tspath::resolve_path;
/// assert_eq!(resolve_path("/a/./b", &[]), "/a/b");
/// assert_eq!(resolve_path("/a/..", &["b"]), "/b");
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:ResolvePath
pub fn resolve_path(path: &str, paths: &[&str]) -> String {
    let combined_path = if !paths.is_empty() {
        combine_paths(path, paths)
    } else {
        normalize_slashes(path)
    };
    normalize_path(&combined_path)
}

/// Resolves a triple-slash reference against a containing file.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:ResolveTripleslashReference
pub fn resolve_tripleslash_reference(module_name: &str, containing_file: &str) -> String {
    let base_path = get_directory_path(containing_file);
    if is_rooted_disk_path(module_name) {
        return normalize_path(module_name);
    }
    normalize_path(&combine_paths(&base_path, &[module_name]))
}

/// Returns the normalized path components of `path` combined with
/// `current_directory`.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetNormalizedPathComponents
pub fn get_normalized_path_components(path: &str, current_directory: &str) -> Vec<String> {
    let combined = combine_paths(current_directory, &[path]);
    get_normalized_path_components_from_combined(&combined)
}

fn get_normalized_path_components_from_combined(path: &str) -> Vec<String> {
    let bytes = path.as_bytes();
    let root_length = get_root_length(path);
    let mut components: Vec<String> = Vec::with_capacity(8);
    components.push(path[..root_length].to_string());

    let len = bytes.len();
    let mut i = root_length;
    while i < len {
        while i < len && bytes[i] == b'/' {
            i += 1;
        }
        if i >= len {
            break;
        }
        let start = i;
        while i < len && bytes[i] != b'/' {
            i += 1;
        }
        let component = &path[start..i];

        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            if components.len() > 1 {
                if components[components.len() - 1] != ".." {
                    components.pop();
                    continue;
                }
            } else if components[0] != *"" {
                continue;
            }
        }
        components.push(component.to_string());
    }
    components
}

/// Returns the normalized absolute path without its root prefix.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetNormalizedAbsolutePathWithoutRoot
pub fn get_normalized_absolute_path_without_root(
    file_name: &str,
    current_directory: &str,
) -> String {
    let absolute_path = get_normalized_absolute_path(file_name, current_directory);
    let root_length = get_root_length(&absolute_path);
    absolute_path[root_length..].to_string()
}

/// Computes the normalized absolute path of `file_name` against
/// `current_directory` (hand-written high-performance normalizer).
///
/// # Examples
/// ```
/// use tsgo_tspath::get_normalized_absolute_path;
/// assert_eq!(get_normalized_absolute_path("/a/../b", ""), "/b");
/// assert_eq!(get_normalized_absolute_path("a", "b"), "b/a");
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetNormalizedAbsolutePath
pub fn get_normalized_absolute_path(file_name: &str, current_directory: &str) -> String {
    let mut root_length = get_root_length(file_name);
    let file_name: String = if root_length == 0 && !current_directory.is_empty() {
        combine_paths(current_directory, &[file_name])
    } else {
        // CombinePaths normalizes slashes, so it is not necessary in the other branch.
        normalize_slashes(file_name)
    };
    root_length = get_root_length(&file_name);

    if let Some(simple_normalized) = simple_normalize_path(&file_name) {
        let length = simple_normalized.len();
        if length > root_length {
            return remove_trailing_directory_separator(&simple_normalized).to_string();
        }
        if length == root_length && root_length != 0 {
            return ensure_trailing_directory_separator(&simple_normalized);
        }
        return simple_normalized;
    }

    let bytes = file_name.as_bytes();
    let length = bytes.len();
    let root = &file_name[..root_length];
    // `normalized` is only initialized once `file_name` is found to be non-normalized.
    let mut changed = false;
    let mut normalized = String::new();
    let mut index = root_length;
    let mut normalized_up_to = index;
    let mut seen_non_dot_dot_segment = root_length != 0;
    while index < length {
        // At beginning of segment.
        let mut segment_start = index;
        let mut ch = bytes[index];
        while ch == b'/' {
            index += 1;
            if index < length {
                ch = bytes[index];
            } else {
                break;
            }
        }
        if index > segment_start {
            // Seen superfluous separator.
            if !changed {
                // Matches Go: fileName[:max(rootLength, segmentStart-1)].
                normalized =
                    file_name[..root_length.max(segment_start.saturating_sub(1))].to_string();
                changed = true;
            }
            if index == length {
                break;
            }
            segment_start = index;
        }
        // Past any superfluous separators.
        let segment_end = match bytes[index + 1..].iter().position(|&b| b == b'/') {
            None => length,
            Some(p) => p + index + 1,
        };
        let segment_length = segment_end - segment_start;
        if segment_length == 1 && bytes[index] == b'.' {
            // "." segment (skip)
            if !changed {
                normalized = file_name[..normalized_up_to].to_string();
                changed = true;
            }
        } else if segment_length == 2 && bytes[index] == b'.' && bytes[index + 1] == b'.' {
            // ".." segment
            if !seen_non_dot_dot_segment {
                if changed {
                    if normalized.len() == root_length {
                        normalized.push_str("..");
                    } else {
                        normalized.push_str("/..");
                    }
                } else {
                    normalized_up_to = index + 2;
                }
            } else if !changed {
                if normalized_up_to >= 1 {
                    let search = &file_name[..normalized_up_to - 1];
                    let cut = match search.rfind('/') {
                        Some(i) => i.max(root_length),
                        None => root_length,
                    };
                    normalized = file_name[..cut].to_string();
                } else {
                    normalized = file_name[..normalized_up_to].to_string();
                }
                changed = true;
                seen_non_dot_dot_segment = (normalized.len() != root_length || root_length != 0)
                    && normalized != ".."
                    && !normalized.ends_with("/..");
            } else {
                match normalized.rfind('/') {
                    Some(i) => {
                        normalized = normalized[..i.max(root_length)].to_string();
                    }
                    None => {
                        normalized = root.to_string();
                    }
                }
                seen_non_dot_dot_segment = (normalized.len() != root_length || root_length != 0)
                    && normalized != ".."
                    && !normalized.ends_with("/..");
            }
        } else if changed {
            if normalized.len() != root_length {
                normalized.push('/');
            }
            seen_non_dot_dot_segment = true;
            normalized.push_str(&file_name[segment_start..segment_end]);
        } else {
            seen_non_dot_dot_segment = true;
            normalized_up_to = segment_end;
        }
        index = segment_end + 1;
    }
    if changed {
        return normalized;
    }
    if length > root_length {
        return remove_trailing_directory_separators(&file_name);
    }
    if length == root_length {
        return ensure_trailing_directory_separator(&file_name);
    }
    file_name
}

fn simple_normalize_path(path: &str) -> Option<String> {
    // Most paths don't require normalization.
    if !has_relative_path_segment(path) {
        return Some(path.to_string());
    }
    // Some paths only require cleanup of `/./` or leading `./`.
    let simplified = path.replace("/./", "/");
    let trimmed: &str = simplified.strip_prefix("./").unwrap_or(simplified.as_str());
    let trimmed_differs = trimmed.len() != simplified.len();
    if trimmed != path
        && !has_relative_path_segment(trimmed)
        && !(trimmed_differs && trimmed.starts_with('/'))
    {
        // If we trimmed a leading "./" and the path now starts with "/", we changed the meaning.
        return Some(trimmed.to_string());
    }
    None
}

/// Reports whether `p` contains `.`, `..`, `./`, `../`, `/.`, `/..`, `//`,
/// `/./`, or `/../` (hand-written replacement for the regex).
// Go: internal/tspath/path.go:hasRelativePathSegment
fn has_relative_path_segment(p: &str) -> bool {
    let bytes = p.as_bytes();
    let n = bytes.len();
    if n == 0 {
        return false;
    }
    if p == "." || p == ".." {
        return true;
    }
    // Leading "./" OR "../".
    if bytes[0] == b'.' {
        if n >= 2 && bytes[1] == b'/' {
            return true;
        }
        if n >= 3 && bytes[1] == b'.' && bytes[2] == b'/' {
            return true;
        }
    }
    // Trailing "/." OR "/..".
    if bytes[n - 1] == b'.' {
        if n >= 2 && bytes[n - 2] == b'/' {
            return true;
        }
        if n >= 3 && bytes[n - 2] == b'.' && bytes[n - 3] == b'/' {
            return true;
        }
    }

    // Look for any `//`, `/./`, or `/../`.
    let mut prev_slash = false;
    let mut seg_len: i32 = 0;
    let mut dot_count: i32 = 0;
    for &c in bytes {
        if c == b'/' {
            if prev_slash {
                return true;
            }
            if (seg_len == 1 && dot_count == 1) || (seg_len == 2 && dot_count == 2) {
                return true;
            }
            prev_slash = true;
            seg_len = 0;
            dot_count = 0;
            continue;
        }
        if c == b'.' {
            if dot_count >= 0 {
                dot_count += 1;
            }
        } else {
            dot_count = -1;
        }
        seg_len += 1;
        prev_slash = false;
    }
    (seg_len == 1 && dot_count == 1) || (seg_len == 2 && dot_count == 2)
}

/// Fully normalizes a path (slashes, `.`/`..`), preserving trailing separators.
///
/// # Examples
/// ```
/// use tsgo_tspath::normalize_path;
/// assert_eq!(normalize_path("/a/../b/"), "/b/");
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:NormalizePath
pub fn normalize_path(path: &str) -> String {
    let path = normalize_slashes(path);
    if let Some(normalized) = simple_normalize_path(&path) {
        return normalized;
    }
    let mut normalized = get_normalized_absolute_path(&path, "");
    if !normalized.is_empty() && has_trailing_directory_separator(&path) {
        normalized = ensure_trailing_directory_separator(&normalized);
    }
    normalized
}

/// Returns a case-canonical form of `file_name` (lower-cased unless case
/// sensitive).
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetCanonicalFileName
pub fn get_canonical_file_name(file_name: &str, use_case_sensitive_file_names: bool) -> String {
    if use_case_sensitive_file_names {
        return file_name.to_string();
    }
    to_file_name_lower_case(file_name)
}

/// Lower-cases a file name for case-insensitive keys, keeping `\u0130` (capital
/// I with dot above) case-sensitive so it does not collide with its lowercase.
///
/// # Examples
/// ```
/// use tsgo_tspath::to_file_name_lower_case;
/// assert_eq!(to_file_name_lower_case("/A/B.TS"), "/a/b.ts");
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:ToFileNameLowerCase
pub fn to_file_name_lower_case(file_name: &str) -> String {
    const I_WITH_DOT: char = '\u{0130}';
    let bytes = file_name.as_bytes();
    let mut ascii = true;
    let mut needs_lower = false;
    for &c in bytes {
        if c >= 0x80 {
            ascii = false;
            break;
        }
        if c.is_ascii_uppercase() {
            needs_lower = true;
        }
    }
    if ascii {
        if !needs_lower {
            return file_name.to_string();
        }
        let mut b = Vec::with_capacity(bytes.len());
        for &c in bytes {
            if c.is_ascii_uppercase() {
                b.push(c + (b'a' - b'A'));
            } else {
                b.push(c);
            }
        }
        // PERF(port): Go uses unsafe.String for zero-copy; here we build a safe String.
        return String::from_utf8(b).expect("ascii bytes are valid utf-8");
    }

    file_name
        .chars()
        .map(|r| {
            if r == I_WITH_DOT {
                r
            } else {
                // PERF(port): Go uses unicode.ToLower (1:1 simple fold); we take
                // the first char of full lowercasing, which matches common cases.
                r.to_lowercase().next().unwrap_or(r)
            }
        })
        .collect()
}

/// Converts a file name into a canonicalized [`Path`].
///
/// # Examples
/// ```
/// use tsgo_tspath::to_path;
/// assert_eq!(to_path("/path/to/../file.ext", "path/to", true).as_str(), "/path/file.ext");
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:ToPath
pub fn to_path(file_name: &str, base_path: &str, use_case_sensitive_file_names: bool) -> Path {
    let non_canonicalized_path = if is_rooted_disk_path(file_name) {
        normalize_path(file_name)
    } else {
        get_normalized_absolute_path(file_name, base_path)
    };
    Path(get_canonical_file_name(
        &non_canonicalized_path,
        use_case_sensitive_file_names,
    ))
}

/// Removes a single trailing directory separator if present.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:RemoveTrailingDirectorySeparator
pub fn remove_trailing_directory_separator(path: &str) -> &str {
    if has_trailing_directory_separator(path) {
        &path[..path.len() - 1]
    } else {
        path
    }
}

/// Removes all trailing directory separators.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:RemoveTrailingDirectorySeparators
pub fn remove_trailing_directory_separators(path: &str) -> String {
    let mut p = path;
    while has_trailing_directory_separator(p) {
        p = remove_trailing_directory_separator(p);
    }
    p.to_string()
}

/// Ensures a path ends with a trailing directory separator.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:EnsureTrailingDirectorySeparator
pub fn ensure_trailing_directory_separator(path: &str) -> String {
    if !has_trailing_directory_separator(path) {
        format!("{path}/")
    } else {
        path.to_string()
    }
}

/// Returns the path components of `to` relative to `from`.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetPathComponentsRelativeTo
pub fn get_path_components_relative_to(
    from: &str,
    to: &str,
    options: &ComparePathsOptions,
) -> Vec<String> {
    let from_components =
        reduce_path_components(&get_path_components(from, &options.current_directory));
    let to_components =
        reduce_path_components(&get_path_components(to, &options.current_directory));

    let mut start = 0usize;
    let max_common_components = from_components.len().min(to_components.len());
    let string_equaler = options.get_equality_comparer();
    while start < max_common_components {
        let from_component = &from_components[start];
        let to_component = &to_components[start];
        if start == 0 {
            if !equate_string_case_insensitive(from_component, to_component) {
                break;
            }
        } else if !string_equaler(from_component, to_component) {
            break;
        }
        start += 1;
    }

    if start == 0 {
        return to_components;
    }

    let num_dot_dot_slashes = from_components.len() - start;
    let mut result = Vec::with_capacity(1 + num_dot_dot_slashes + to_components.len() - start);
    result.push(String::new());
    for _ in 0..num_dot_dot_slashes {
        result.push("..".to_string());
    }
    for component in &to_components[start..] {
        result.push(component.clone());
    }
    result
}

/// Returns the relative path from a directory to `to`.
///
/// # Panics
/// Panics if one path is absolute and the other is relative.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetRelativePathFromDirectory
pub fn get_relative_path_from_directory(
    from_directory: &str,
    to: &str,
    options: &ComparePathsOptions,
) -> String {
    if (get_root_length(from_directory) > 0) != (get_root_length(to) > 0) {
        panic!("paths must either both be absolute or both be relative");
    }
    let path_components = get_path_components_relative_to(from_directory, to, options);
    get_path_from_path_components(&path_components)
}

/// Returns the relative path from a file to `to` (dot-prefixed if needed).
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetRelativePathFromFile
pub fn get_relative_path_from_file(from: &str, to: &str, options: &ComparePathsOptions) -> String {
    ensure_path_is_non_module_name(&get_relative_path_from_directory(
        &get_directory_path(from),
        to,
        options,
    ))
}

/// Converts an absolute path to a relative one against the current directory.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:ConvertToRelativePath
pub fn convert_to_relative_path(
    absolute_or_relative_path: &str,
    options: &ComparePathsOptions,
) -> String {
    if !is_rooted_disk_path(absolute_or_relative_path) {
        return absolute_or_relative_path.to_string();
    }
    get_relative_path_to_directory_or_url(
        &options.current_directory,
        absolute_or_relative_path,
        false,
        options,
    )
}

/// Returns the relative path to a directory or URL.
///
/// # Examples
/// ```
/// use tsgo_tspath::{get_relative_path_to_directory_or_url, ComparePathsOptions};
/// let opts = ComparePathsOptions::default();
/// assert_eq!(get_relative_path_to_directory_or_url("/a/b", "/b", false, &opts), "../../b");
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetRelativePathToDirectoryOrUrl
pub fn get_relative_path_to_directory_or_url(
    directory_path_or_url: &str,
    relative_or_absolute_path: &str,
    is_absolute_path_an_url: bool,
    options: &ComparePathsOptions,
) -> String {
    let mut path_components =
        get_path_components_relative_to(directory_path_or_url, relative_or_absolute_path, options);

    if is_absolute_path_an_url && is_rooted_disk_path(&path_components[0]) {
        let first_component = path_components[0].clone();
        let prefix = if first_component.as_bytes()[0] == b'/' {
            "file://"
        } else {
            "file:///"
        };
        path_components[0] = format!("{prefix}{first_component}");
    }

    get_path_from_path_components(&path_components)
}

/// Returns the base file name (the trailing path segment).
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetBaseFileName
pub fn get_base_file_name(path: &str) -> String {
    let path = normalize_slashes(path);
    let root_length = get_root_length(&path);
    if root_length == path.len() {
        return String::new();
    }
    let trimmed = remove_trailing_directory_separator(&path);
    let last = trimmed.rfind('/').map(|i| i + 1).unwrap_or(0);
    let start = get_root_length(trimmed).max(last);
    trimmed[start..].to_string()
}

/// Gets the file extension for a path, optionally restricted to `extensions`.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetAnyExtensionFromPath
pub fn get_any_extension_from_path(path: &str, extensions: &[&str], ignore_case: bool) -> String {
    if !extensions.is_empty() {
        return get_any_extension_from_path_worker(
            remove_trailing_directory_separator(path),
            extensions,
            get_string_equality_comparer(ignore_case),
        );
    }
    let base_file_name = get_base_file_name(path);
    if let Some(idx) = base_file_name.rfind('.') {
        return base_file_name[idx..].to_string();
    }
    String::new()
}

fn get_any_extension_from_path_worker(
    path: &str,
    extensions: &[&str],
    comparer: fn(&str, &str) -> bool,
) -> String {
    for &extension in extensions {
        let result = try_get_extension_from_path_with_comparer(path, extension, comparer);
        if !result.is_empty() {
            return result;
        }
    }
    String::new()
}

fn try_get_extension_from_path_with_comparer(
    path: &str,
    extension: &str,
    comparer: fn(&str, &str) -> bool,
) -> String {
    let extension_owned;
    let extension: &str = if !extension.starts_with('.') {
        extension_owned = format!(".{extension}");
        &extension_owned
    } else {
        extension
    };
    let pb = path.as_bytes();
    let eb = extension.as_bytes();
    if pb.len() >= eb.len() && pb[pb.len() - eb.len()] == b'.' {
        let path_extension = &path[path.len() - extension.len()..];
        if comparer(path_extension, extension) {
            return path_extension.to_string();
        }
    }
    String::new()
}

/// Reports whether `path` ends with `extension` (and is longer than it).
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:FileExtensionIs
pub fn file_extension_is(path: &str, extension: &str) -> bool {
    path.len() > extension.len() && path.ends_with(extension)
}

/// Reports whether a path is relative (`.`, `..`, or starting with `./`, `../`,
/// `.\`, `..\`).
///
/// # Examples
/// ```
/// use tsgo_tspath::path_is_relative;
/// assert!(path_is_relative("./foo/bar"));
/// assert!(!path_is_relative("/foo/bar"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:PathIsRelative
pub fn path_is_relative(path: &str) -> bool {
    if path == "." || path == ".." {
        return true;
    }
    let bytes = path.as_bytes();
    let n = bytes.len();
    if n >= 2 && bytes[0] == b'.' && (bytes[1] == b'/' || bytes[1] == b'\\') {
        return true;
    }
    if n >= 3 && bytes[0] == b'.' && bytes[1] == b'.' && (bytes[2] == b'/' || bytes[2] == b'\\') {
        return true;
    }
    false
}

/// Ensures a path is absolute or dot-relative so it is not confused with a bare
/// module name.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:EnsurePathIsNonModuleName
pub fn ensure_path_is_non_module_name(path: &str) -> String {
    if !path_is_absolute(path) && !path_is_relative(path) {
        format!("./{path}")
    } else {
        path.to_string()
    }
}

/// Reports whether a module name is "relative" (dot-relative or rooted disk).
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:IsExternalModuleNameRelative
pub fn is_external_module_name_relative(module_name: &str) -> bool {
    path_is_relative(module_name) || is_rooted_disk_path(module_name)
}

/// Options controlling path comparison (case sensitivity and base directory).
#[derive(Clone, Debug, Default)]
pub struct ComparePathsOptions {
    /// Whether file names are compared case-sensitively.
    pub use_case_sensitive_file_names: bool,
    /// The base directory used to root relative inputs.
    pub current_directory: String,
}

impl ComparePathsOptions {
    /// Returns the three-way comparator implied by these options.
    ///
    /// Side effects: none (pure).
    // Go: internal/tspath/path.go:GetComparer
    pub fn get_comparer(&self) -> fn(&str, &str) -> Ordering {
        get_string_comparer(!self.use_case_sensitive_file_names)
    }

    fn get_equality_comparer(&self) -> fn(&str, &str) -> bool {
        get_string_equality_comparer(!self.use_case_sensitive_file_names)
    }
}

/// Compares two paths under the given options.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:ComparePaths
pub fn compare_paths(a: &str, b: &str, options: &ComparePathsOptions) -> Ordering {
    let a = combine_paths(&options.current_directory, &[a]);
    let b = combine_paths(&options.current_directory, &[b]);

    if a == b {
        return Ordering::Equal;
    }
    if a.is_empty() {
        return Ordering::Less;
    }
    if b.is_empty() {
        return Ordering::Greater;
    }

    // Shortcut if the root segments differ (no need for path reduction).
    let a_root = &a[..get_root_length(&a)];
    let b_root = &b[..get_root_length(&b)];
    let result = compare_strings_case_insensitive(a_root, b_root);
    if result != Ordering::Equal {
        return result;
    }

    // Shortcut if there are no relative segments in the non-root portion.
    let a_rest = &a[a_root.len()..];
    let b_rest = &b[b_root.len()..];
    if !has_relative_path_segment(a_rest) && !has_relative_path_segment(b_rest) {
        return options.get_comparer()(a_rest, b_rest);
    }

    // Slower component-by-component comparison.
    let a_components = reduce_path_components(&get_path_components(&a, ""));
    let b_components = reduce_path_components(&get_path_components(&b, ""));
    let shared_length = a_components.len().min(b_components.len());
    let comparer = options.get_comparer();
    for i in 1..shared_length {
        let result = comparer(&a_components[i], &b_components[i]);
        if result != Ordering::Equal {
            return result;
        }
    }
    a_components.len().cmp(&b_components.len())
}

/// Compares two paths case-sensitively.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:ComparePathsCaseSensitive
pub fn compare_paths_case_sensitive(a: &str, b: &str, current_directory: &str) -> Ordering {
    compare_paths(
        a,
        b,
        &ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: current_directory.to_string(),
        },
    )
}

/// Compares two paths case-insensitively.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:ComparePathsCaseInsensitive
pub fn compare_paths_case_insensitive(a: &str, b: &str, current_directory: &str) -> Ordering {
    compare_paths(
        a,
        b,
        &ComparePathsOptions {
            use_case_sensitive_file_names: false,
            current_directory: current_directory.to_string(),
        },
    )
}

/// Reports whether `parent` contains `child` (component-wise).
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:ContainsPath
pub fn contains_path(parent: &str, child: &str, options: &ComparePathsOptions) -> bool {
    let parent = combine_paths(&options.current_directory, &[parent]);
    let child = combine_paths(&options.current_directory, &[child]);
    if parent.is_empty() || child.is_empty() {
        return false;
    }
    if parent == child {
        return true;
    }
    let parent_components = reduce_path_components(&get_path_components(&parent, ""));
    let child_components = reduce_path_components(&get_path_components(&child, ""));
    if child_components.len() < parent_components.len() {
        return false;
    }

    let component_comparer = options.get_equality_comparer();
    for (i, parent_component) in parent_components.iter().enumerate() {
        let comparer: fn(&str, &str) -> bool = if i == 0 {
            equate_string_case_insensitive
        } else {
            component_comparer
        };
        if !comparer(parent_component, &child_components[i]) {
            return false;
        }
    }
    true
}

/// Calls `callback` on `directory` and each ancestor, returning the first
/// `(result, true)`.
///
/// # Examples
/// ```
/// use tsgo_tspath::for_each_ancestor_directory;
/// let found = for_each_ancestor_directory("/a/b/c", |dir| {
///     if dir == "/a" { (dir.to_string(), true) } else { (String::new(), false) }
/// });
/// assert_eq!(found, ("/a".to_string(), true));
/// ```
///
/// Side effects: none (pure aside from `callback`'s own effects).
// Go: internal/tspath/path.go:ForEachAncestorDirectory
pub fn for_each_ancestor_directory<T: Default>(
    directory: &str,
    mut callback: impl FnMut(&str) -> (T, bool),
) -> (T, bool) {
    let mut directory = directory.to_string();
    loop {
        let (result, stop) = callback(&directory);
        if stop {
            return (result, true);
        }
        let parent_path = get_directory_path(&directory);
        if parent_path == directory {
            return (T::default(), false);
        }
        directory = parent_path;
    }
}

/// Like [`for_each_ancestor_directory`], but also stops at the global cache
/// location.
///
/// Side effects: none (pure aside from `callback`'s own effects).
// Go: internal/tspath/path.go:ForEachAncestorDirectoryStoppingAtGlobalCache
pub fn for_each_ancestor_directory_stopping_at_global_cache<T: Default>(
    global_cache_location: &str,
    directory: &str,
    mut callback: impl FnMut(&str) -> (T, bool),
) -> T {
    let (result, _) = for_each_ancestor_directory(directory, |ancestor_directory| {
        let (result, stop) = callback(ancestor_directory);
        if stop || ancestor_directory == global_cache_location {
            (result, true)
        } else {
            (result, false)
        }
    });
    result
}

/// [`Path`]-typed variant of [`for_each_ancestor_directory`].
///
/// Side effects: none (pure aside from `callback`'s own effects).
// Go: internal/tspath/path.go:ForEachAncestorDirectoryPath
pub fn for_each_ancestor_directory_path<T: Default>(
    directory: &Path,
    mut callback: impl FnMut(&Path) -> (T, bool),
) -> (T, bool) {
    for_each_ancestor_directory(&directory.0, |d| callback(&Path(d.to_string())))
}

/// Reports whether the base file name contains a `.`.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:HasExtension
pub fn has_extension(file_name: &str) -> bool {
    get_base_file_name(file_name).contains('.')
}

/// Splits a leading DOS volume (`c:`) off a path; returns `(volume, rest, ok)`.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:SplitVolumePath
pub fn split_volume_path(path: &str) -> (String, String, bool) {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && is_volume_character(bytes[0]) && bytes[1] == b':' {
        return (path[0..2].to_lowercase(), path[2..].to_string(), true);
    }
    (String::new(), path.to_string(), false)
}

struct GroupAccum {
    head: Vec<String>,
    tails: Vec<Vec<String>>,
}

/// Returns the smallest set of parent directories covering all `paths` with at
/// least `min_components` components; shorter paths are returned in the ignored
/// set.
///
/// # Panics
/// Panics if `min_components < 1`.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:GetCommonParents
pub fn get_common_parents(
    paths: &[&str],
    min_components: usize,
    get_path_components_fn: fn(&str, &str) -> Vec<String>,
    options: &ComparePathsOptions,
) -> (Vec<String>, HashSet<String>) {
    if min_components < 1 {
        panic!("minComponents must be at least 1");
    }
    if paths.is_empty() {
        return (Vec::new(), HashSet::new());
    }
    if paths.len() == 1 {
        if reduce_path_components(&get_path_components_fn(
            paths[0],
            &options.current_directory,
        ))
        .len()
            < min_components
        {
            let mut ignored = HashSet::new();
            ignored.insert(paths[0].to_string());
            return (Vec::new(), ignored);
        }
        return (vec![paths[0].to_string()], HashSet::new());
    }

    let mut ignored = HashSet::new();
    let mut path_components: Vec<Vec<String>> = Vec::with_capacity(paths.len());
    for &path in paths {
        let components =
            reduce_path_components(&get_path_components_fn(path, &options.current_directory));
        if components.len() < min_components {
            ignored.insert(path.to_string());
        } else {
            path_components.push(components);
        }
    }

    let results = get_common_parents_worker(&path_components, min_components as i32, options);
    let result_paths = results
        .iter()
        .map(|comps| get_path_from_path_components(comps))
        .collect();
    (result_paths, ignored)
}

fn get_common_parents_worker(
    component_groups: &[Vec<String>],
    min_components: i32,
    options: &ComparePathsOptions,
) -> Vec<Vec<String>> {
    if component_groups.is_empty() {
        return Vec::new();
    }
    let mut max_depth = component_groups[0].len();
    for comps in &component_groups[1..] {
        if comps.len() < max_depth {
            max_depth = comps.len();
        }
    }

    let equality = options.get_equality_comparer();
    for last_common_index in 0..max_depth {
        let candidate = &component_groups[0][last_common_index];
        for comps in &component_groups[1..] {
            if !equality(candidate, &comps[last_common_index]) {
                // divergence
                if (last_common_index as i32) < min_components {
                    // Not enough components, fan out.
                    let mut ordered_groups: Vec<Path> = Vec::new();
                    let mut new_groups: HashMap<Path, GroupAccum> = HashMap::new();
                    for g in component_groups {
                        let key = to_path(
                            &g[last_common_index],
                            &options.current_directory,
                            options.use_case_sensitive_file_names,
                        );
                        let entry = new_groups.entry(key.clone()).or_insert_with(|| {
                            ordered_groups.push(key.clone());
                            GroupAccum {
                                head: Vec::new(),
                                tails: Vec::new(),
                            }
                        });
                        entry.head = g[..last_common_index + 1].to_vec();
                        entry.tails.push(g[last_common_index + 1..].to_vec());
                    }
                    ordered_groups.sort();
                    let mut result: Vec<Vec<String>> = Vec::new();
                    for key in &ordered_groups {
                        let group = &new_groups[key];
                        let sub_results = get_common_parents_worker(
                            &group.tails,
                            min_components - (last_common_index as i32 + 1),
                            options,
                        );
                        for sr in &sub_results {
                            let mut combined = group.head.clone();
                            combined.extend(sr.iter().cloned());
                            result.push(combined);
                        }
                    }
                    return result;
                }
                return vec![component_groups[0][..last_common_index].to_vec()];
            }
        }
    }

    vec![component_groups[0][..max_depth].to_vec()]
}

/// Reports whether `file_name` is located within `directory_name`.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:StartsWithDirectory
pub fn starts_with_directory(
    file_name: &str,
    directory_name: &str,
    use_case_sensitive_file_names: bool,
) -> bool {
    if directory_name.is_empty() {
        return false;
    }

    let canonical_file_name = get_canonical_file_name(file_name, use_case_sensitive_file_names);
    let mut canonical_directory_name =
        get_canonical_file_name(directory_name, use_case_sensitive_file_names);
    if let Some(s) = canonical_directory_name.strip_suffix('/') {
        canonical_directory_name = s.to_string();
    }
    if let Some(s) = canonical_directory_name.strip_suffix('\\') {
        canonical_directory_name = s.to_string();
    }

    canonical_file_name.starts_with(&format!("{canonical_directory_name}/"))
        || canonical_file_name.starts_with(&format!("{canonical_directory_name}\\"))
}

/// Compares paths by the number of `/` separators.
///
/// Side effects: none (pure).
// Go: internal/tspath/path.go:CompareNumberOfDirectorySeparators
pub fn compare_number_of_directory_separators(path1: &str, path2: &str) -> Ordering {
    path1.matches('/').count().cmp(&path2.matches('/').count())
}

#[cfg(test)]
#[path = "path_test.rs"]
mod path_tests;

#[cfg(test)]
#[path = "startsWithDirectory_test.rs"]
mod starts_with_directory_tests;

#[cfg(test)]
#[path = "untitled_test.rs"]
mod untitled_tests;
