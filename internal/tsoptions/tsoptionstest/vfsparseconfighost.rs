//! A [`ParseConfigHost`] backed by an in-memory [`MapFs`].
//!
//! 1:1 port of Go `internal/tsoptions/tsoptionstest/vfsparseconfighost.go`.

use tsgo_tspath::get_root_length;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use crate::commandlineparser::ParseConfigHost;

/// Strips the rooted prefix of `path`, returning the path relative to its root.
///
/// Returns `path` unchanged when it has no root, `"."` when `path` is exactly
/// its own root, and the suffix after the root otherwise.
///
/// # Examples
/// ```
/// use tsgo_tsoptions::tsoptionstest::fix_root;
/// assert_eq!(fix_root("a/b"), "a/b");
/// assert_eq!(fix_root("/"), ".");
/// assert_eq!(fix_root("/a/b"), "a/b");
/// ```
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsoptionstest/vfsparseconfighost.go:fixRoot
pub fn fix_root(path: &str) -> &str {
    let root_length = get_root_length(path);
    if root_length == 0 {
        return path;
    }
    if path.len() == root_length {
        return ".";
    }
    &path[root_length..]
}

/// A [`ParseConfigHost`] backed by an in-memory [`MapFs`].
///
/// Side effects: none (holds an in-memory FS).
// Go: internal/tsoptions/tsoptionstest/vfsparseconfighost.go:VfsParseConfigHost
pub struct VfsParseConfigHost {
    /// The in-memory file system.
    pub vfs: MapFs,
    /// The current working directory.
    pub current_directory: String,
}

impl VfsParseConfigHost {
    /// Builds a host from `(path, contents)` pairs.
    ///
    /// # Examples
    /// ```
    /// use tsgo_tsoptions::tsoptionstest::VfsParseConfigHost;
    /// use tsgo_tsoptions::ParseConfigHost;
    /// let host = VfsParseConfigHost::new(&[("/p/a.ts", "let x = 1;")], "/p", true);
    /// assert_eq!(host.get_current_directory(), "/p");
    /// ```
    ///
    /// Side effects: allocates the in-memory tree.
    // Go: internal/tsoptions/tsoptionstest/vfsparseconfighost.go:NewVFSParseConfigHost
    pub fn new(
        files: &[(&str, &str)],
        current_directory: &str,
        use_case_sensitive_file_names: bool,
    ) -> Self {
        VfsParseConfigHost {
            vfs: MapFs::from_map(files.iter().copied(), use_case_sensitive_file_names),
            current_directory: current_directory.to_string(),
        }
    }
}

impl ParseConfigHost for VfsParseConfigHost {
    fn fs(&self) -> &dyn Fs {
        &self.vfs
    }

    fn get_current_directory(&self) -> &str {
        &self.current_directory
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Go: vfsparseconfighost.go:NewVFSParseConfigHost + FS/GetCurrentDirectory
    // (no Go unit test; behavior-level coverage of the vfs-backed host).
    #[test]
    fn host_fs_reports_file_existence() {
        let host = VfsParseConfigHost::new(&[("/p/a.ts", "let x = 1;")], "/p", true);
        assert!(host.fs().file_exists("/p/a.ts"));
        assert!(!host.fs().file_exists("/p/missing.ts"));
    }

    // Go: vfsparseconfighost.go:FS
    #[test]
    fn host_fs_reads_file_contents() {
        let host = VfsParseConfigHost::new(&[("/p/a.ts", "let x = 1;")], "/p", true);
        assert_eq!(
            host.fs().read_file("/p/a.ts").as_deref(),
            Some("let x = 1;")
        );
        assert_eq!(host.fs().read_file("/p/missing.ts"), None);
    }

    // Go: vfsparseconfighost.go:GetCurrentDirectory
    #[test]
    fn host_reports_current_directory() {
        let host = VfsParseConfigHost::new(&[], "/some/dir", true);
        assert_eq!(host.get_current_directory(), "/some/dir");
    }

    // Go: vfsparseconfighost.go:NewVFSParseConfigHost (useCaseSensitiveFileNames)
    #[test]
    fn host_honors_case_sensitivity_flag() {
        // Case-sensitive: a differently-cased lookup misses.
        let sensitive = VfsParseConfigHost::new(&[("/p/A.ts", "x")], "/p", true);
        assert!(sensitive.fs().file_exists("/p/A.ts"));
        assert!(!sensitive.fs().file_exists("/p/a.ts"));

        // Case-insensitive: the same lookup hits.
        let insensitive = VfsParseConfigHost::new(&[("/p/A.ts", "x")], "/p", false);
        assert!(insensitive.fs().file_exists("/p/a.ts"));
        assert!(!insensitive.fs().use_case_sensitive_file_names());
    }

    // Go: vfsparseconfighost.go:fixRoot (no Go unit test; behavior derived from
    // the implementation's branches).
    #[test]
    fn fix_root_returns_relative_path_unchanged() {
        // A path with no root (root length 0) is returned verbatim.
        assert_eq!(fix_root("a/b/c.ts"), "a/b/c.ts");
        assert_eq!(fix_root("./a"), "./a");
    }

    // Go: vfsparseconfighost.go:fixRoot
    #[test]
    fn fix_root_of_root_only_path_is_dot() {
        // When the whole path is just its root, Go returns ".".
        assert_eq!(fix_root("/"), ".");
        assert_eq!(fix_root("c:/"), ".");
    }

    // Go: vfsparseconfighost.go:fixRoot
    #[test]
    fn fix_root_strips_root_prefix() {
        // The portion after the root length is returned.
        assert_eq!(fix_root("/a/b.ts"), "a/b.ts");
        assert_eq!(fix_root("c:/a/b.ts"), "a/b.ts");
    }
}
