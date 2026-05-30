//! `tsgo_sourcemap` — 1:1 Rust port of Go `internal/sourcemap`.
//!
//! Generates and parses Source Map v3 data: [`Generator`] incrementally
//! accumulates mappings and emits a [`RawSourceMap`] / JSON / base64 data URL.

mod decoder;
mod generator;
mod lineinfo;
mod source;
mod source_mapper;
mod util;

pub use decoder::*;
pub use generator::*;
pub use lineinfo::*;
pub use source::*;
pub use source_mapper::*;
pub use util::*;

/// Error returned by the fallible `Generator` mapping methods.
///
/// Wraps a static message identical to the Go `errors.New(..)` text, so callers
/// can assert on it byte-for-byte.
///
/// # Examples
/// ```
/// use tsgo_sourcemap::SourceMapError;
/// assert_eq!(
///     SourceMapError("sourceIndex is out of range").to_string(),
///     "sourceIndex is out of range",
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceMapError(pub &'static str);

impl std::fmt::Display for SourceMapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for SourceMapError {}
