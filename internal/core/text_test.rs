use super::*;

// Go: internal/core/text.go:Contains/ContainsInclusive/ContainsExclusive
#[test]
fn text_range_contains() {
    let r = TextRange::new(0, 5);
    assert!(r.contains(0));
    assert!(!r.contains(5));
    assert!(r.contains_inclusive(5));
    assert!(!r.contains_exclusive(0));
    assert!(r.contains_exclusive(3));
}

// Go: internal/core/text.go:Overlaps/Intersects
#[test]
fn text_range_overlaps_intersects() {
    let a = TextRange::new(0, 5);
    let b = TextRange::new(5, 10);
    assert!(!a.overlaps(b));
    assert!(a.intersects(b));
    assert!(a.overlaps(TextRange::new(3, 8)));
}

// Go: internal/core/text.go:UndefinedTextRange/IsValid/Pos/End/Len
#[test]
fn text_range_undefined() {
    let u = TextRange::undefined();
    assert_eq!(u.pos(), -1);
    assert_eq!(u.end(), -1);
    assert!(!u.is_valid());
    let r = TextRange::new(0, 5);
    assert!(r.is_valid());
    assert_eq!(r.len(), 5);
}

// Go: internal/core/text.go:CompareTextRanges/WithPos/WithEnd/ContainedBy
#[test]
fn text_range_compare_and_with() {
    assert!(compare_text_ranges(TextRange::new(0, 5), TextRange::new(0, 6)) < 0);
    assert!(compare_text_ranges(TextRange::new(1, 5), TextRange::new(0, 6)) > 0);
    assert_eq!(
        compare_text_ranges(TextRange::new(0, 5), TextRange::new(0, 5)),
        0
    );

    let r = TextRange::new(2, 8);
    assert_eq!(r.with_pos(3), TextRange::new(3, 8));
    assert_eq!(r.with_end(9), TextRange::new(2, 9));
    assert!(r.contained_by(TextRange::new(0, 10)));
    assert!(!r.contained_by(TextRange::new(3, 10)));
}
