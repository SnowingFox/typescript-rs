//! Port of Go `internal/lsp/lsproto/util.go` (position/range comparators).

use std::cmp::Ordering;

use crate::{Position, Range};

/// Compares two [`Position`]s, ordering by line first and then character.
///
/// Mirrors Go's `ComparePositions`, whose result equals `cmp.Compare(pos,
/// other)`: it returns [`Ordering::Less`]/`Equal`/`Greater` analogous to Go's
/// `-1`/`0`/`+1`.
///
/// # Examples
/// ```
/// use std::cmp::Ordering;
/// let a: tsgo_lsproto::Position = serde_json::from_str(r#"{"line":1,"character":2}"#).unwrap();
/// let b: tsgo_lsproto::Position = serde_json::from_str(r#"{"line":1,"character":5}"#).unwrap();
/// assert_eq!(tsgo_lsproto::compare_positions(&a, &b), Ordering::Less);
/// ```
///
/// Side effects: none (pure).
// Go: internal/lsp/lsproto/util.go:ComparePositions
pub fn compare_positions(pos: &Position, other: &Position) -> Ordering {
    pos.line
        .cmp(&other.line)
        .then_with(|| pos.character.cmp(&other.character))
}

/// Compares two [`Range`]s, ordering by start position first and then end
/// position.
///
/// Mirrors Go's `CompareRanges`, whose result equals `cmp.Compare(lsRange,
/// other)` with `Range.Start` compared before `Range.End`.
///
/// Side effects: none (pure).
// Go: internal/lsp/lsproto/util.go:CompareRanges
pub fn compare_ranges(ls_range: &Range, other: &Range) -> Ordering {
    compare_positions(&ls_range.start, &other.start)
        .then_with(|| compare_positions(&ls_range.end, &other.end))
}

#[cfg(test)]
#[path = "util_test.rs"]
mod tests;
