//! Program-state snapshot: per-file version/signature info and the hashing used
//! to detect text/shape changes between incremental builds.
//!
//! 1:1 port of the reachable subset of Go `internal/execute/incremental/snapshot.go`.

use rustc_hash::FxHashMap;
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_tspath::Path;

use crate::reference_map::ReferenceMap;

/// `impliedNodeFormat` value for CommonJS, mirroring Go's
/// `core.ResolutionModeCommonJS` (`ModuleKind::CommonJs == 1`). It is the
/// default implied format encoded by the compact bare-string file-info form.
// Go: internal/core/compileroptions.go:ResolutionModeCommonJS
pub const RESOLUTION_MODE_COMMON_JS: i32 = 1;

/// Computes a file's `version`: the stable content hash of its text.
///
/// This is the value Go stores as `FileInfo.version` (`computeHash(file.Text())`),
/// used to detect whether a file's text changed between incremental builds.
///
/// # Examples
/// ```
/// use tsgo_incremental::{compute_file_version, compute_hash};
/// let v = compute_file_version("const x = 1;");
/// assert_eq!(v, compute_hash("const x = 1;", false));
/// assert_ne!(v, compute_file_version("const x = 2;"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/execute/incremental/programtosnapshot.go:computeProgramFileChanges
pub fn compute_file_version(text: &str) -> String {
    compute_hash(text, false)
}

/// Computes a file's `.d.ts` shape `signature`.
///
/// Go hashes the emitted declaration text (plus its emit diagnostics) via
/// `computeSignatureWithDiagnostics`. The declaration emitter is not yet wired
/// up here, so this is the reachable text-based approximation: the hash of the
/// provided declaration text. For a fresh build with no prior signature, a
/// file's signature defaults to its [`compute_file_version`] (Go sets
/// `signature = version`), which [`FileInfo::for_fresh_text`] reflects.
///
/// # Examples
/// ```
/// use tsgo_incremental::{compute_signature, compute_hash};
/// assert_eq!(compute_signature("export declare const x: number;\n"),
///            compute_hash("export declare const x: number;\n", false));
/// ```
///
/// Side effects: none (pure).
// Go: internal/execute/incremental/snapshot.go:computeSignatureWithDiagnostics
// DEFER(P6-9b): real declaration-emit signature. blocked-by: declarations
// transformer + checker EmitResolver (`.d.ts` emit). Approximated with a
// text-based hash per the porting brief.
pub fn compute_signature(declaration_text: &str) -> String {
    compute_hash(declaration_text, false)
}

/// Per-file incremental state: the content `version` hash, the `.d.ts`
/// `signature` hash, whether the file augments the global scope, and its
/// `impliedNodeFormat`.
///
/// 1:1 port of Go `incremental.FileInfo`. Fields are public for the reachable
/// subset (Go uses private fields + getters).
// Go: internal/execute/incremental/snapshot.go:FileInfo
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileInfo {
    /// Hash of the file's text (see [`compute_hash`]).
    pub version: String,
    /// Hash of the file's emitted `.d.ts` (its "shape"); defaults to `version`.
    pub signature: String,
    /// Whether changes to this file invalidate every other file.
    pub affects_global_scope: bool,
    /// The file's implied module format (`core.ResolutionMode` as `i32`).
    pub implied_node_format: i32,
}

impl FileInfo {
    /// Builds the [`FileInfo`] for a file on a fresh (non-incremental) build:
    /// its `version` is the text hash and its `signature` defaults to that same
    /// version, mirroring Go's `oldProgram == nil` path
    /// (`signature = version`).
    ///
    /// `affects_global_scope` and `implied_node_format` are caller-provided
    /// (the latter as a `core.ResolutionMode` `i32`, e.g.
    /// [`RESOLUTION_MODE_COMMON_JS`]).
    ///
    /// Side effects: none (pure).
    // Go: internal/execute/incremental/programtosnapshot.go:computeProgramFileChanges
    // (oldProgram == nil branch: signature = version)
    pub fn for_fresh_text(
        text: &str,
        affects_global_scope: bool,
        implied_node_format: i32,
    ) -> Self {
        let version = compute_file_version(text);
        FileInfo {
            signature: version.clone(),
            version,
            affects_global_scope,
            implied_node_format,
        }
    }
}

/// Computes the stable content hash Go uses for a file's `version` (and, by
/// default, its `signature`): the canonical 128-bit XXH3 of `text`, hex-encoded.
///
/// When `hash_with_text` is set (used only by tests for readable diffs), the
/// original text is appended after a `-`, mirroring Go's `ComputeHash`.
///
/// # Examples
/// ```
/// use tsgo_incremental::compute_hash;
/// // Byte-for-byte parity with `cmd/tsgo --incremental` for `b.ts`.
/// assert_eq!(
///     compute_hash("export const b = 1;\n", false),
///     "90312e1cbc42534115cfa9601aa41950"
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/execute/incremental/snapshot.go:ComputeHash
pub fn compute_hash(text: &str, hash_with_text: bool) -> String {
    // zeebo/xxh3 `HashString128(text).Bytes()` is the canonical XXH3-128 big-endian
    // byte layout (high64 first), which is exactly the big-endian hex of the u128.
    let hash = format!("{:032x}", xxh3::hash128_with_seed(text.as_bytes(), 0));
    if hash_with_text {
        format!("{hash}-{text}")
    } else {
        hash
    }
}

/// The serialized program state: per-file [`FileInfo`], the compiler options
/// used, and the `referencedMap` import graph.
///
/// Reachable subset of Go `incremental.snapshot` (the diagnostics caches,
/// change set, pending-emit set, and emit signatures are DEFER).
// Go: internal/execute/incremental/snapshot.go:snapshot
#[derive(Debug, Default)]
pub struct Snapshot {
    /// Per-file version/signature/flags, keyed by canonical path.
    pub file_infos: FxHashMap<Path, FileInfo>,
    /// The import graph: file -> files it references.
    pub referenced_map: ReferenceMap,
    /// The compiler options this snapshot was built with.
    pub options: CompilerOptions,
}

#[cfg(test)]
#[path = "snapshot_test.rs"]
mod tests;
