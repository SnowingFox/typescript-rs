//! Text positions and ranges (`TextPos`, `TextRange`).
//!
//! 1:1 port of Go `internal/core/text.go`.

/// A position into source text, measured in UTF-8 byte offsets.
///
/// A value of `-1` denotes an undefined position.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextPos(pub i32);

/// A half-open range `[pos, end)` into source text.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextRange {
    pos: TextPos,
    end: TextPos,
}

impl TextRange {
    /// Creates a range from `pos` and `end` byte offsets.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/text.go:NewTextRange
    pub fn new(pos: i32, end: i32) -> TextRange {
        TextRange {
            pos: TextPos(pos),
            end: TextPos(end),
        }
    }

    /// Returns the undefined range `(-1, -1)`.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/text.go:UndefinedTextRange
    pub fn undefined() -> TextRange {
        TextRange {
            pos: TextPos(-1),
            end: TextPos(-1),
        }
    }

    /// Returns the start offset.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/text.go:Pos
    pub fn pos(self) -> i32 {
        self.pos.0
    }

    /// Returns the end offset.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/text.go:End
    pub fn end(self) -> i32 {
        self.end.0
    }

    /// Returns the length `end - pos`.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/text.go:Len
    pub fn len(self) -> i32 {
        self.end.0 - self.pos.0
    }

    /// Reports whether the range is empty (`len == 0`).
    ///
    /// Side effects: none (pure).
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// Reports whether this is a defined range (either endpoint `>= 0`).
    ///
    /// Side effects: none (pure).
    // Go: internal/core/text.go:IsValid
    pub fn is_valid(self) -> bool {
        self.pos.0 >= 0 || self.end.0 >= 0
    }

    /// Reports whether `pos` lies within `[pos, end)`.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/text.go:Contains
    pub fn contains(self, pos: i32) -> bool {
        pos >= self.pos.0 && pos < self.end.0
    }

    /// Reports whether `pos` lies within `[pos, end]` (end inclusive).
    ///
    /// Side effects: none (pure).
    // Go: internal/core/text.go:ContainsInclusive
    pub fn contains_inclusive(self, pos: i32) -> bool {
        pos >= self.pos.0 && pos <= self.end.0
    }

    /// Reports whether `pos` lies strictly within `(pos, end)`.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/text.go:ContainsExclusive
    pub fn contains_exclusive(self, pos: i32) -> bool {
        self.pos.0 < pos && pos < self.end.0
    }

    /// Reports whether this range overlaps `other` (shared interior point).
    ///
    /// Side effects: none (pure).
    // Go: internal/core/text.go:Overlaps
    pub fn overlaps(self, other: TextRange) -> bool {
        let start = self.pos.0.max(other.pos.0);
        let end = self.end.0.min(other.end.0);
        start < end
    }

    /// Reports whether this range intersects `other`, treating touching ranges
    /// as intersecting (e.g. `[0, 5)` intersects `[5, 10)`).
    ///
    /// Side effects: none (pure).
    // Go: internal/core/text.go:Intersects
    pub fn intersects(self, other: TextRange) -> bool {
        let start = self.pos.0.max(other.pos.0);
        let end = self.end.0.min(other.end.0);
        start <= end
    }

    /// Returns a copy with the start offset replaced by `pos`.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/text.go:WithPos
    pub fn with_pos(self, pos: i32) -> TextRange {
        TextRange {
            pos: TextPos(pos),
            end: self.end,
        }
    }

    /// Returns a copy with the end offset replaced by `end`.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/text.go:WithEnd
    pub fn with_end(self, end: i32) -> TextRange {
        TextRange {
            pos: self.pos,
            end: TextPos(end),
        }
    }

    /// Reports whether this range is fully contained by `other`.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/text.go:ContainedBy
    pub fn contained_by(self, other: TextRange) -> bool {
        other.pos.0 <= self.pos.0 && other.end.0 >= self.end.0
    }
}

/// Compares two ranges by `pos`, then by `end`.
///
/// Returns a negative, zero, or positive value when `r1` orders before, equal
/// to, or after `r2`.
///
/// # Examples
/// ```
/// use tsgo_core::text::{compare_text_ranges, TextRange};
/// assert!(compare_text_ranges(TextRange::new(0, 5), TextRange::new(0, 6)) < 0);
/// ```
///
/// Side effects: none (pure).
// Go: internal/core/text.go:CompareTextRanges
pub fn compare_text_ranges(r1: TextRange, r2: TextRange) -> i32 {
    let c = r1.pos.0 - r2.pos.0;
    if c != 0 {
        return c;
    }
    r1.end.0 - r2.end.0
}

#[cfg(test)]
#[path = "text_test.rs"]
mod tests;
