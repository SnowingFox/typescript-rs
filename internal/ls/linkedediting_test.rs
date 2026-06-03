use tsgo_lsproto::{LinkedEditingRanges, Position, Range};

use crate::test_support::build_service;

/// Builds an LSP [`Position`].
fn pos(line: u32, character: u32) -> Position {
    Position { line, character }
}

/// Builds an LSP [`Range`] from `(start_line, start_char)` to `(end_line, end_char)`.
fn rng(sl: u32, sc: u32, el: u32, ec: u32) -> Range {
    Range {
        start: pos(sl, sc),
        end: pos(el, ec),
    }
}

/// The JSX-tag-name word pattern Go returns verbatim (`jsxTagWordPattern`).
fn word_pattern() -> Option<String> {
    Some(r"[a-zA-Z0-9:\-\._$]*".to_string())
}

// Go: internal/ls/linkedediting.go:ProvideLinkedEditingRange — a cursor inside
// the OPENING JSX tag name returns both the opening and the closing tag-name
// ranges (so the editor renames the pair together) plus the word pattern.
#[test]
fn provide_linked_editing_ranges_cursor_in_opening_tag_name() {
    let ls = build_service(&[("/m.tsx", "<div></div>")], "/", &["/m.tsx"]);
    // Position inside the opening `div` tag name (byte 2).
    let result = ls.provide_linked_editing_ranges("/m.tsx", pos(0, 2));
    assert_eq!(
        result,
        Some(LinkedEditingRanges {
            ranges: vec![rng(0, 1, 0, 4), rng(0, 7, 0, 10)],
            word_pattern: word_pattern(),
        })
    );
}

// Go: internal/ls/linkedediting.go:ProvideLinkedEditingRange — a cursor inside
// the CLOSING tag name returns the same opening + closing tag-name ranges
// (the `closeTagNameStart <= position <= closeTagNameEnd` arm).
#[test]
fn provide_linked_editing_ranges_cursor_in_closing_tag_name() {
    let ls = build_service(&[("/m.tsx", "<div></div>")], "/", &["/m.tsx"]);
    // Position inside the closing `div` tag name (byte 8).
    let result = ls.provide_linked_editing_ranges("/m.tsx", pos(0, 8));
    assert_eq!(
        result,
        Some(LinkedEditingRanges {
            ranges: vec![rng(0, 1, 0, 4), rng(0, 7, 0, 10)],
            word_pattern: word_pattern(),
        })
    );
}

// Go: internal/ls/linkedediting.go:ProvideLinkedEditingRange — the word pattern
// is Go's `jsxTagWordPattern` verbatim (`[a-zA-Z0-9:\-\._$]*`).
#[test]
fn provide_linked_editing_ranges_word_pattern_is_go_jsx_tag_pattern() {
    let ls = build_service(&[("/m.tsx", "<div></div>")], "/", &["/m.tsx"]);
    let result = ls
        .provide_linked_editing_ranges("/m.tsx", pos(0, 2))
        .unwrap();
    assert_eq!(result.word_pattern.as_deref(), Some(r"[a-zA-Z0-9:\-\._$]*"));
}

// Go: internal/ls/linkedediting.go:ProvideLinkedEditingRange — a multi-character
// tag name's ranges scale to its full width, on both the opening and the
// closing tag.
#[test]
fn provide_linked_editing_ranges_multichar_tag_name() {
    let ls = build_service(&[("/m.tsx", "<span>x</span>")], "/", &["/m.tsx"]);
    // Position inside the opening `span` tag name (byte 3).
    let result = ls.provide_linked_editing_ranges("/m.tsx", pos(0, 3));
    assert_eq!(
        result,
        Some(LinkedEditingRanges {
            ranges: vec![rng(0, 1, 0, 5), rng(0, 9, 0, 13)],
            word_pattern: word_pattern(),
        })
    );
}

// Go: internal/ls/linkedediting.go:ProvideLinkedEditingRange — opening and
// closing tags on different lines map to LSP positions on their own lines.
#[test]
fn provide_linked_editing_ranges_multiline_element() {
    let ls = build_service(&[("/m.tsx", "<div>\n</div>")], "/", &["/m.tsx"]);
    // Position inside the opening `div` tag name (line 0, byte 2).
    let result = ls.provide_linked_editing_ranges("/m.tsx", pos(0, 2));
    assert_eq!(
        result,
        Some(LinkedEditingRanges {
            ranges: vec![rng(0, 1, 0, 4), rng(1, 2, 1, 5)],
            word_pattern: word_pattern(),
        })
    );
}

// Guard: a self-closing element `<div/>` has no closing tag, so `FindAncestor`
// finds no opening/closing element (a self-closing element is neither) and the
// result is `None`.
// Go: internal/ls/linkedediting.go:ProvideLinkedEditingRange (tag == nil)
#[test]
fn provide_linked_editing_ranges_self_closing_element_is_none() {
    let ls = build_service(&[("/m.tsx", "<div/>")], "/", &["/m.tsx"]);
    // Position inside the `div` tag name (byte 2).
    assert_eq!(ls.provide_linked_editing_ranges("/m.tsx", pos(0, 2)), None);
}

// Guard: a cursor in a non-JSX construct (a variable name) is not inside a JSX
// tag name, so the result is `None`.
// Go: internal/ls/linkedediting.go:ProvideLinkedEditingRange (tag == nil)
#[test]
fn provide_linked_editing_ranges_non_jsx_position_is_none() {
    let ls = build_service(&[("/m.tsx", "const x = 1;")], "/", &["/m.tsx"]);
    // Position on the `x` identifier (byte 6).
    assert_eq!(ls.provide_linked_editing_ranges("/m.tsx", pos(0, 6)), None);
}

// Guard: a cursor on an element's body text (between the tags, not on a tag
// name) yields `None`.
// Go: internal/ls/linkedediting.go:ProvideLinkedEditingRange (not within a tag name)
#[test]
fn provide_linked_editing_ranges_cursor_on_body_is_none() {
    let ls = build_service(&[("/m.tsx", "<div>x</div>")], "/", &["/m.tsx"]);
    // Position on the body text `x` (byte 5).
    assert_eq!(ls.provide_linked_editing_ranges("/m.tsx", pos(0, 5)), None);
}

// Guard: an unknown file yields `None` (no panic).
// Go: internal/ls/linkedediting.go:ProvideLinkedEditingRange (sourceFile == nil)
#[test]
fn provide_linked_editing_ranges_unknown_file_is_none() {
    let ls = build_service(&[("/m.tsx", "<div></div>")], "/", &["/m.tsx"]);
    assert_eq!(
        ls.provide_linked_editing_ranges("/missing.tsx", pos(0, 2)),
        None
    );
}
