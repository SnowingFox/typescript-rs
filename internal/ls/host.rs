//! Port of Go `internal/ls/host.go`: the [`LanguageServiceHost`] abstraction the
//! [`LanguageService`](crate::LanguageService) reads its outside-world state
//! through.
//!
//! # Divergence from Go (reachable subset)
//!
//! Go's `Host` interface bundles the snapshot the service needs from the
//! project layer: case sensitivity, file reads, the position [`Converters`], the
//! resolved [`UserPreferences`], source-map line info, the auto-import registry,
//! and the directory-listing trio used for module-specifier completions. The
//! reachable LS-root round (diagnostics + quick-info/hover) only needs the case
//! sensitivity and file-read facets, so this trait ports those two; the rest are
//! deferred with their blocking crates noted below.
//!
//! The position [`Converters`](tsgo_ls_lsconv::Converters) are *not* a host
//! method here: the service builds them from the program's own file snapshots at
//! construction (see [`LanguageService::new`](crate::LanguageService::new)),
//! which is behaviorally equivalent to Go (where the project builds the host's
//! converters from the same file set) and avoids the host carrying a second copy
//! of every file's text.
//!
//! DEFER(phase-7-ls): `Converters()` (built by the service instead — see above),
//! `GetPreferences`/`UserPreferences`, `GetECMALineInfo`, `AutoImportRegistry`,
//! and the `ReadDirectory`/`GetDirectories`/`DirectoryExists`/`FileExists`
//! module-specifier-completion facets.
//! blocked-by: `tsgo_ls_autoimport` (auto-import registry/view), `tsgo_sourcemap`
//! document-position mapping wiring, and the completions feature round.

/// The outside-world snapshot the [`LanguageService`](crate::LanguageService)
/// reads through.
///
/// Mirrors the reachable subset of Go's `ls.Host`: a snapshot-in-time view that
/// answers case-sensitivity and file-read questions. Implementors are typically
/// the project layer (P8); the language-service tests supply a small in-memory
/// stand-in.
///
/// Side effects: implementations may read a file system.
// Go: internal/ls/host.go:Host
pub trait LanguageServiceHost {
    /// Whether file names are compared case-sensitively (drives path
    /// canonicalization).
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/host.go:Host.UseCaseSensitiveFileNames
    fn use_case_sensitive_file_names(&self) -> bool;

    /// Reads the current contents of `file_name`, or `None` if it cannot be
    /// read.
    ///
    /// Mirrors Go's `Host.ReadFile(path) (contents string, ok bool)`, modeled as
    /// an `Option<String>` (`None` == `ok == false`).
    ///
    /// Side effects: may read the file system.
    // Go: internal/ls/host.go:Host.ReadFile
    fn read_file(&self, file_name: &str) -> Option<String>;
}
