//! Port of Go `internal/ls/lsconv/linemap.go`.
//!
//! Line-start tables for LSP position conversion. Unlike
//! `core::compute_line_starts`, this only treats `\n`, `\r`, and `\r\n` as line
//! breaks and additionally records whether the text is ASCII-only (so UTF-16
//! rescans can be skipped on the hot path).

use tsgo_core::text::TextPos;

use crate::converters::decode_rune;

/// The byte offsets at which each line starts.
// Go: internal/ls/lsconv/linemap.go:LSPLineStarts
pub type LSPLineStarts = Vec<TextPos>;

/// A line-start table together with an ASCII-only flag.
// Go: internal/ls/lsconv/linemap.go:LSPLineMap
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LSPLineMap {
    /// Byte offset of the start of each line (line 0 starts at 0).
    pub line_starts: LSPLineStarts,
    /// Whether the source text contains only ASCII bytes (`< 0x80`).
    pub ascii_only: bool,
}

/// Computes the [`LSPLineMap`] for `text`, recording the byte offset of every
/// line start and whether the text is ASCII-only.
///
/// Only `\n`, `\r`, and `\r\n` are treated as line breaks (unlike
/// `core::compute_line_starts`, which also recognizes Unicode line separators).
///
/// # Examples
/// ```
/// use tsgo_ls_lsconv::compute_lsp_line_starts;
/// let lm = compute_lsp_line_starts(b"a\nbc");
/// assert_eq!(lm.line_starts.len(), 2);
/// assert!(lm.ascii_only);
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/lsconv/linemap.go:ComputeLSPLineStarts
pub fn compute_lsp_line_starts(text: &[u8]) -> LSPLineMap {
    let text_len = text.len();
    // Capacity hint mirrors Go's `strings.Count(text, "\n") + 1`.
    let cap = text.iter().filter(|&&b| b == b'\n').count() + 1;
    let mut line_starts: LSPLineStarts = Vec::with_capacity(cap);
    let mut ascii_only = true;

    let mut pos = 0usize;
    let mut line_start = 0usize;
    while pos < text_len {
        let b = text[pos];
        if b < 0x80 {
            // utf8.RuneSelf
            pos += 1;
            match b {
                b'\r' => {
                    if pos < text_len && text[pos] == b'\n' {
                        pos += 1;
                    }
                    line_starts.push(TextPos(line_start as i32));
                    line_start = pos;
                }
                b'\n' => {
                    line_starts.push(TextPos(line_start as i32));
                    line_start = pos;
                }
                _ => {}
            }
        } else {
            let (_, size) = decode_rune(&text[pos..]);
            pos += size;
            ascii_only = false;
        }
    }
    line_starts.push(TextPos(line_start as i32));

    LSPLineMap {
        line_starts,
        ascii_only,
    }
}

impl LSPLineMap {
    /// Returns the index of the line containing `target_pos`.
    ///
    /// On an exact line-start hit it returns that line; otherwise it returns
    /// the previous line start (the line that contains `target_pos`), clamping
    /// to line 0 for positions before the first start.
    ///
    /// # Examples
    /// ```
    /// use tsgo_ls_lsconv::compute_lsp_line_starts;
    /// let lm = compute_lsp_line_starts(b"ab\ncd");
    /// use tsgo_core::text::TextPos;
    /// assert_eq!(lm.compute_index_of_line_start(TextPos(4)), 1);
    /// assert_eq!(lm.compute_index_of_line_start(TextPos(1)), 0);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/lsconv/linemap.go:ComputeIndexOfLineStart
    pub fn compute_index_of_line_start(&self, target_pos: TextPos) -> i32 {
        // Mirrors Go `slices.BinarySearchFunc`: on a miss the search yields the
        // insertion index; we step back one to the containing line start.
        match self.line_starts.binary_search(&target_pos) {
            Ok(line_number) => line_number as i32,
            Err(insertion) => {
                if insertion > 0 {
                    (insertion - 1) as i32
                } else {
                    0
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "linemap_test.rs"]
mod tests;
