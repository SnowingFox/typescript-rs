//! Parse cache — caches parsed source files keyed by parse options + content hash.
//!
//! 1:1 port of Go `internal/project/parsecache.go`.
//!
//! The parse cache is a specialization of [`RefCountCache`] that stores parsed
//! AST source files, keyed by their parse options, content hash, and script
//! kind. This allows the project system to share parsed files across snapshots
//! that reference the same source content.
//!
//! # DEFER notes
//! - `SourceFileParseOptions` is a placeholder; the real type lives in
//!   `tsgo_ast` but is not yet ported. Replace when ast P2 covers it.
//! - `ParseCache` type alias and `new_parse_cache()` constructor are deferred
//!   until the `ast::SourceFile`, `parser::parse_source_file`, and `FileHandle`
//!   types are available.

use tsgo_core::scriptkind::ScriptKind;
use tsgo_tspath::Path as TsPath;

/// Placeholder for `ast.SourceFileParseOptions` (not yet ported to Rust ast crate).
///
/// # Examples
/// ```
/// use tsgo_project::parsecache::SourceFileParseOptions;
/// let opts = SourceFileParseOptions {
///     file_name: "foo.ts".to_string(),
///     path: Default::default(),
///     jsx: false,
///     force_external_module_indicator: false,
/// };
/// assert_eq!(opts.file_name, "foo.ts");
/// ```
// Go: internal/ast/parseoptions.go:SourceFileParseOptions
// DEFER(phase-3): Replace with tsgo_ast::SourceFileParseOptions when ported.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceFileParseOptions {
    /// The file name for this source file.
    pub file_name: String,
    /// The canonical path for this source file.
    pub path: TsPath,
    /// Whether JSX external module indicator is enabled.
    pub jsx: bool,
    /// Whether to force external module indicator.
    pub force_external_module_indicator: bool,
}

/// A 128-bit content hash (mirrors Go `xxh3.Uint128`).
///
/// # Examples
/// ```
/// use tsgo_project::parsecache::ContentHash;
/// let h = ContentHash { hi: 1, lo: 2 };
/// assert_eq!(h, ContentHash { hi: 1, lo: 2 });
/// ```
// Go: uses xxh3.Uint128
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ContentHash {
    /// High 64 bits.
    pub hi: u64,
    /// Low 64 bits.
    pub lo: u64,
}

/// The composite key for the parse cache.
///
/// Combines parse options, content hash, and script kind so that the same file
/// content parsed with different options or as a different script kind gets
/// separate cache entries.
///
/// # Examples
/// ```
/// use tsgo_project::parsecache::{ParseCacheKey, SourceFileParseOptions, ContentHash};
/// use tsgo_core::scriptkind::ScriptKind;
/// let key = ParseCacheKey::new(
///     SourceFileParseOptions {
///         file_name: "a.ts".to_string(),
///         path: Default::default(),
///         jsx: false,
///         force_external_module_indicator: false,
///     },
///     ContentHash { hi: 0xAB, lo: 0xCD },
///     ScriptKind::Ts,
/// );
/// assert_eq!(key.options.file_name, "a.ts");
/// assert_eq!(key.script_kind, ScriptKind::Ts);
/// ```
// Go: internal/project/parsecache.go:ParseCacheKey
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParseCacheKey {
    /// The source file parse options.
    pub options: SourceFileParseOptions,
    /// The script kind.
    pub script_kind: ScriptKind,
    /// The content hash.
    pub hash: ContentHash,
}

impl ParseCacheKey {
    /// Creates a new parse cache key.
    // Go: internal/project/parsecache.go:NewParseCacheKey
    pub fn new(
        options: SourceFileParseOptions,
        hash: ContentHash,
        script_kind: ScriptKind,
    ) -> Self {
        ParseCacheKey {
            options,
            script_kind,
            hash,
        }
    }
}

// DEFER(phase-8-session): ParseCache type alias + NewParseCache constructor
// blocked-by: ast::SourceFile, parser::parse_source_file, FileHandle trait

#[cfg(test)]
#[path = "parsecache_test.rs"]
mod tests;
