//! `tsgo_bundled` — 1:1 Rust port of Go `internal/bundled`.
//!
//! Provides access to the TypeScript standard library declaration files
//! (`lib.*.d.ts`) that are compiled directly into the binary, exposed to the
//! compiler and language service through a `bundled:///` virtual file system.
//! This lets a single executable type-check programs with zero external
//! dependencies.
//!
//! DIVERGENCE(port): Go selects the embedded vs on-disk implementation with the
//! `noembed` build tag (`embed.go` / `noembed.go`). This port implements only
//! the default embedded variant, so [`EMBEDDED`] is always `true`.

mod embed;
mod embed_generated;
mod libs_generated;

pub use embed::{is_bundled, lib_path, wrap_fs, WrappedFs};
pub use libs_generated::LIB_NAMES;

use tsgo_tspath::normalize_slashes;

/// Whether the bundled files are served from an embedded file system.
///
/// Always `true` in this port (only the embedded variant is implemented).
///
/// # Examples
/// ```
/// assert!(tsgo_bundled::EMBEDDED);
/// ```
///
/// Side effects: none (compile-time constant).
// Go: internal/bundled/bundled.go:Embedded
pub const EMBEDDED: bool = true;

/// Returns the on-disk path to the source `libs` directory.
///
/// This resolves the crate's own `libs/` directory at compile time, so it is
/// only meaningful when the source tree is present (i.e. in tests).
///
/// DIVERGENCE(port): Go derives the directory from `runtime.Caller(0)` and
/// panics outside of tests. This port uses the compile-time
/// `CARGO_MANIFEST_DIR` and omits the test-only guard.
///
/// # Examples
/// ```
/// let p = tsgo_bundled::testing_lib_path();
/// assert!(std::path::Path::new(&p).join("lib.d.ts").exists());
/// ```
///
/// Side effects: none (pure; the returned path may later be read by the caller).
// Go: internal/bundled/bundled.go:TestingLibPath
pub fn testing_lib_path() -> String {
    normalize_slashes(concat!(env!("CARGO_MANIFEST_DIR"), "/libs"))
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
