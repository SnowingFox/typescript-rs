//! The [`Source`] trait describing a source file for source-map generation.
//!
//! 1:1 port of Go `internal/sourcemap/source.go`.

use tsgo_core::text::TextPos;

/// A source file the generator can map positions against.
// Go: internal/sourcemap/source.go:Source
pub trait Source {
    /// Returns the full source text.
    fn text(&self) -> &str;
    /// Returns the source file name.
    fn file_name(&self) -> &str;
    /// Returns the byte offsets at which each ECMA line begins.
    fn ecma_line_map(&self) -> &[TextPos];
}

#[cfg(test)]
#[path = "source_test.rs"]
mod tests;
