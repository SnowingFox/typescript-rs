use super::*;

use std::cmp::Ordering;

// Go's `util.go` has no `*_test.go`; these are behavior-level tests over the
// public comparators (PORTING.md §8.5), with expectations derived from Go's
// documented `cmp.Compare` equivalence.

fn pos(line: u32, character: u32) -> Position {
    Position { line, character }
}

fn range(s: Position, e: Position) -> Range {
    Range { start: s, end: e }
}

// Go: internal/lsp/lsproto/util.go:ComparePositions
#[test]
fn compare_positions_equal() {
    assert_eq!(compare_positions(&pos(1, 5), &pos(1, 5)), Ordering::Equal);
}

// Go: internal/lsp/lsproto/util.go:ComparePositions
#[test]
fn compare_positions_character_breaks_tie() {
    assert_eq!(compare_positions(&pos(1, 2), &pos(1, 5)), Ordering::Less);
    assert_eq!(compare_positions(&pos(1, 9), &pos(1, 2)), Ordering::Greater);
}

// Go: internal/lsp/lsproto/util.go:ComparePositions
#[test]
fn compare_positions_line_dominates() {
    // A later line is greater even with a smaller character.
    assert_eq!(compare_positions(&pos(2, 0), &pos(1, 9)), Ordering::Greater);
}

// Go: internal/lsp/lsproto/util.go:CompareRanges
#[test]
fn compare_ranges_equal() {
    let r = range(pos(1, 2), pos(3, 4));
    assert_eq!(compare_ranges(&r, &r.clone()), Ordering::Equal);
}

// Go: internal/lsp/lsproto/util.go:CompareRanges
#[test]
fn compare_ranges_start_dominates() {
    let a = range(pos(1, 0), pos(9, 9));
    let b = range(pos(2, 0), pos(0, 0));
    assert_eq!(compare_ranges(&a, &b), Ordering::Less);
}

// Go: internal/lsp/lsproto/util.go:CompareRanges
#[test]
fn compare_ranges_end_breaks_tie() {
    let a = range(pos(1, 2), pos(3, 4));
    let b = range(pos(1, 2), pos(3, 9));
    assert_eq!(compare_ranges(&a, &b), Ordering::Less);
}
