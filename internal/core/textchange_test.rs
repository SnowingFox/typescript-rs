use super::*;
use crate::text::TextRange;

// Go: internal/core/textchange.go:ApplyTo
#[test]
fn apply_to_single() {
    let c = TextChange::new(TextRange::new(1, 3), "X");
    assert_eq!(c.apply_to("abcd"), "aXd");
}

// Go: internal/core/textchange.go:ApplyBulkEdits
#[test]
fn apply_bulk_edits_splices() {
    let edits = vec![
        TextChange::new(TextRange::new(1, 3), "X"),
        TextChange::new(TextRange::new(4, 5), "YY"),
    ];
    assert_eq!(apply_bulk_edits("abcdef", &edits), "aXdYYf");

    // No edits -> unchanged.
    assert_eq!(apply_bulk_edits("abcdef", &[]), "abcdef");
}
