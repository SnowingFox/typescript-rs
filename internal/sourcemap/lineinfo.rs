//! Line-start indexed view over generated/source text ([`EcmaLineInfo`]).
//!
//! 1:1 port of Go `internal/sourcemap/lineinfo.go`.

use tsgo_core::text::TextPos;
use tsgo_core::EcmaLineStarts;

/// Source text together with the byte offsets at which each line begins.
///
/// # Examples
/// ```
/// use tsgo_sourcemap::create_ecma_line_info;
/// use tsgo_core::compute_ecma_line_starts;
/// let text = "ab\ncd";
/// let info = create_ecma_line_info(text, compute_ecma_line_starts(text));
/// assert_eq!(info.line_count(), 2);
/// ```
// Go: internal/sourcemap/lineinfo.go:ECMALineInfo
pub struct EcmaLineInfo {
    text: String,
    line_starts: EcmaLineStarts,
}

/// Creates an [`EcmaLineInfo`] from `text` and its precomputed `line_starts`.
///
/// # Examples
/// ```
/// use tsgo_sourcemap::create_ecma_line_info;
/// use tsgo_core::compute_ecma_line_starts;
/// let info = create_ecma_line_info("x", compute_ecma_line_starts("x"));
/// assert_eq!(info.line_count(), 1);
/// ```
///
/// Side effects: none (pure constructor).
// Go: internal/sourcemap/lineinfo.go:CreateECMALineInfo
pub fn create_ecma_line_info(text: &str, line_starts: EcmaLineStarts) -> EcmaLineInfo {
    EcmaLineInfo {
        text: text.to_string(),
        line_starts,
    }
}

impl EcmaLineInfo {
    /// Returns the number of lines.
    ///
    /// Side effects: none (pure).
    // Go: internal/sourcemap/lineinfo.go:LineCount
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Returns the text of the 0-based `line`, including its trailing line
    /// terminator (if any).
    ///
    /// # Examples
    /// ```
    /// use tsgo_sourcemap::create_ecma_line_info;
    /// use tsgo_core::compute_ecma_line_starts;
    /// let text = "ab\ncd";
    /// let info = create_ecma_line_info(text, compute_ecma_line_starts(text));
    /// assert_eq!(info.line_text(0), "ab\n");
    /// assert_eq!(info.line_text(1), "cd");
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/sourcemap/lineinfo.go:LineText
    pub fn line_text(&self, line: usize) -> &str {
        let pos = self.line_starts[line].0 as usize;
        let end = if line + 1 < self.line_starts.len() {
            self.line_starts[line + 1].0 as usize
        } else {
            self.text.len()
        };
        &self.text[pos..end]
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn line_starts(&self) -> &[TextPos] {
        &self.line_starts
    }
}

#[cfg(test)]
#[path = "lineinfo_test.rs"]
mod tests;
