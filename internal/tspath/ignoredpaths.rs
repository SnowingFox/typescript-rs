//! Detection of paths that should be ignored (node_modules dotfiles, `.git`,
//! emacs lock files).
//!
//! 1:1 port of Go `internal/tspath/ignoredpaths.go`.

const IGNORED_PATHS: &[&str] = &["/node_modules/.", "/.git", ".#"];

/// Reports whether `path` contains any ignored substring pattern.
///
/// # Examples
/// ```
/// use tsgo_tspath::contains_ignored_path;
/// assert!(contains_ignored_path("/project/.git/hooks/pre-commit"));
/// assert!(!contains_ignored_path("/project/src/file.ts"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/ignoredpaths.go:ContainsIgnoredPath
pub fn contains_ignored_path(path: &str) -> bool {
    IGNORED_PATHS.iter().any(|pattern| path.contains(pattern))
}

#[cfg(test)]
#[path = "ignoredpaths_test.rs"]
mod tests;
