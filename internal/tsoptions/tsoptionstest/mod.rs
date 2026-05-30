//! Test helpers for `tsoptions` (a [`ParseConfigHost`] backed by an in-memory
//! file system).
//!
//! 1:1 port of Go `internal/tsoptions/tsoptionstest`. Exposed as `pub mod` (not
//! `#[cfg(test)]`) because Go's external `_test` package consumes it; the
//! `GetParsedCommandLine` helper that needs the full tsconfig pipeline is
//! deferred with that pipeline.

use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use crate::commandlineparser::ParseConfigHost;

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
