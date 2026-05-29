//! Repository-root and test-only path discovery.
//!
//! 1:1 port of Go `internal/repo/paths.go`.
//!
//! DIVERGENCE(port): Go derives the repository root at run time via
//! `runtime.Caller(0)` and then walks upward looking for `go.mod`. Rust instead
//! uses the compile-time `CARGO_MANIFEST_DIR` of this crate and walks upward to
//! the workspace `Cargo.toml` (the one declaring `[workspace]`). This is more
//! robust than reflective run-time discovery; the `-trimpath` panic branch has
//! no equivalent because the manifest dir is a compile-time constant.

use std::path::Path;
use std::sync::OnceLock;

// Walks upward from this crate's manifest directory to the workspace root,
// identified by a `Cargo.toml` that declares `[workspace]`.
fn compute_root_path() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let mut dir = Path::new(manifest_dir);
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.is_file() {
            if let Ok(contents) = std::fs::read_to_string(&cargo_toml) {
                if contents.contains("[workspace]") {
                    return dir.to_string_lossy().into_owned();
                }
            }
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => panic!("could not find workspace Cargo.toml above {manifest_dir}"),
        }
    }
}

/// Returns the absolute path to the repository (Cargo workspace) root.
///
/// The result is computed once and cached. The root is found by walking upward
/// from this crate's source directory to the `Cargo.toml` that declares the
/// `[workspace]` table.
///
/// # Examples
/// ```
/// use std::path::Path;
/// assert!(Path::new(tsgo_repo::root_path()).is_absolute());
/// ```
///
/// Side effects: reads `Cargo.toml` files while searching (first call only).
///
/// # Panics
/// Panics if no workspace `Cargo.toml` is found above this crate.
// Go: internal/repo/paths.go:RootPath/rootPath
pub fn root_path() -> &'static str {
    static ROOT: OnceLock<String> = OnceLock::new();
    ROOT.get_or_init(compute_root_path).as_str()
}

/// Returns the absolute path to the vendored TypeScript submodule
/// (`<root>/_submodules/TypeScript`).
///
/// The path is returned whether or not the submodule is checked out; use
/// [`typescript_submodule_exists`] to test for presence.
///
/// # Examples
/// ```
/// assert!(tsgo_repo::typescript_submodule_path().ends_with("TypeScript"));
/// ```
///
/// Side effects: none after the cached [`root_path`] is computed.
// Go: internal/repo/paths.go:TypeScriptSubmodulePath
pub fn typescript_submodule_path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        Path::new(root_path())
            .join("_submodules")
            .join("TypeScript")
            .to_string_lossy()
            .into_owned()
    })
    .as_str()
}

/// Returns the absolute path to the repository `testdata` directory
/// (`<root>/testdata`).
///
/// # Examples
/// ```
/// assert!(tsgo_repo::test_data_path().ends_with("testdata"));
/// ```
///
/// Side effects: none after the cached [`root_path`] is computed.
// Go: internal/repo/paths.go:TestDataPath
pub fn test_data_path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        Path::new(root_path())
            .join("testdata")
            .to_string_lossy()
            .into_owned()
    })
    .as_str()
}

/// Reports whether the TypeScript submodule is checked out, by testing for
/// `<submodule>/package.json`.
///
/// The result is computed once and cached.
///
/// # Examples
/// ```
/// // Returns a deterministic boolean reflecting the working tree.
/// let _present: bool = tsgo_repo::typescript_submodule_exists();
/// ```
///
/// Side effects: stats `<submodule>/package.json` on the first call.
// Go: internal/repo/paths.go:TypeScriptSubmoduleExists
pub fn typescript_submodule_exists() -> bool {
    static EXISTS: OnceLock<bool> = OnceLock::new();
    *EXISTS.get_or_init(|| {
        Path::new(typescript_submodule_path())
            .join("package.json")
            .exists()
    })
}

/// Test helper: reports whether a test should be skipped because the TypeScript
/// submodule is absent.
///
/// DIVERGENCE(port): Go's `SkipIfNoTypeScriptSubmodule(t)` calls `t.Skipf`
/// directly. Rust has no library-level test-skip primitive, so this returns
/// `true` when the caller should `return` early to skip; `false` otherwise.
///
/// # Examples
/// ```
/// if tsgo_repo::skip_if_no_typescript_submodule() {
///     // caller would `return;` to skip a submodule-dependent test
/// }
/// ```
///
/// Side effects: none beyond the cached [`typescript_submodule_exists`].
// Go: internal/repo/paths.go:SkipIfNoTypeScriptSubmodule
pub fn skip_if_no_typescript_submodule() -> bool {
    !typescript_submodule_exists()
}

#[cfg(test)]
#[path = "paths_test.rs"]
mod tests;
