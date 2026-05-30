//! Path-component counting used to compare relative vs base-url specifiers.
//!
//! 1:1 port of Go `internal/modulespecifiers/compare.go`.

/// Counts the `/`-separated components in `path`, ignoring a leading `./`.
///
/// Used by `getLocalModuleSpecifier` to prefer the relative import with the
/// fewest path components over a `baseUrl`-derived one.
///
/// # Examples
/// ```
/// use tsgo_modulespecifiers::count_path_components;
/// // Counts `/` separators after stripping a leading `./`.
/// assert_eq!(count_path_components("./a/b/c"), 2);
/// assert_eq!(count_path_components("a/b"), 1);
/// ```
///
/// Side effects: none (pure).
// Go: internal/modulespecifiers/compare.go:CountPathComponents
pub fn count_path_components(path: &str) -> usize {
    let initial = if path.starts_with("./") { 2 } else { 0 };
    path[initial..].matches('/').count()
}

#[cfg(test)]
#[path = "compare_test.rs"]
mod tests;
