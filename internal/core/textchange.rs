//! Text edits (`TextChange`) and bulk application.
//!
//! 1:1 port of Go `internal/core/textchange.go`.

use crate::text::TextRange;

/// A replacement of the text within `range` by `new_text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextChange {
    /// The half-open range of the original text being replaced.
    pub range: TextRange,
    /// The replacement text.
    pub new_text: String,
}

impl TextChange {
    /// Creates a [`TextChange`] replacing `range` with `new_text`.
    ///
    /// Side effects: none (pure).
    pub fn new(range: TextRange, new_text: impl Into<String>) -> TextChange {
        TextChange {
            range,
            new_text: new_text.into(),
        }
    }

    /// Applies this single edit to `text`, returning the edited string.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/textchange.go:ApplyTo
    pub fn apply_to(&self, text: &str) -> String {
        let pos = self.range.pos() as usize;
        let end = self.range.end() as usize;
        format!("{}{}{}", &text[..pos], self.new_text, &text[end..])
    }
}

/// Applies a sequence of non-overlapping `edits` (in ascending order) to
/// `text`, splicing in the replacement text between untouched spans.
///
/// Side effects: none (pure).
// Go: internal/core/textchange.go:ApplyBulkEdits
pub fn apply_bulk_edits(text: &str, edits: &[TextChange]) -> String {
    let mut b = String::with_capacity(text.len());
    let mut last_end = 0usize;
    for e in edits {
        let start = e.range.pos() as usize;
        if start != last_end {
            b.push_str(&text[last_end..start]);
        }
        b.push_str(&e.new_text);
        last_end = e.range.end() as usize;
    }
    b.push_str(&text[last_end..]);
    b
}

#[cfg(test)]
#[path = "textchange_test.rs"]
mod tests;
