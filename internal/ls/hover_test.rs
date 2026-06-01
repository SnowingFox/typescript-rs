use tsgo_core::text::TextRange;
use tsgo_lsproto::{MarkupKind, Position, Range};

use crate::test_support::build_service;

// Go: internal/ls/hover.go:ProvideHover — hovering the use of `x` in
// `const x: number = 1; x` resolves its symbol and renders the type `number`.
#[test]
fn get_quick_info_at_position_resolves_identifier_type() {
    let mut ls = build_service(&[("/m.ts", "const x: number = 1; x")], "/", &["/m.ts"]);
    // The use of `x` is at byte/character 21 (ASCII single line).
    let quick_info = ls
        .get_quick_info_at_position(
            "/m.ts",
            Position {
                line: 0,
                character: 21,
            },
        )
        .expect("quick info for the `x` use");
    assert_eq!(quick_info.text, "number");
    assert_eq!(quick_info.text_range, TextRange::new(21, 22));
}

// `provide_hover` wraps the quick info in an `lsproto::Hover` with a plain-text
// markup content and the token's UTF-16 range.
// Go: internal/ls/hover.go:ProvideHover (lsproto.Hover shape)
#[test]
fn provide_hover_wraps_quick_info_in_lsp_hover() {
    let mut ls = build_service(&[("/m.ts", "const x: number = 1; x")], "/", &["/m.ts"]);
    let hover = ls
        .provide_hover(
            "/m.ts",
            Position {
                line: 0,
                character: 21,
            },
        )
        .expect("a hover for the `x` use");
    let content = hover
        .contents
        .markup_content
        .expect("a markup-content hover body");
    assert_eq!(content.kind, MarkupKind::PLAIN_TEXT);
    assert_eq!(content.value, "number");
    assert_eq!(
        hover.range,
        Some(Range {
            start: Position {
                line: 0,
                character: 21
            },
            end: Position {
                line: 0,
                character: 22
            },
        })
    );
}

// A token with no resolvable symbol (the `const` keyword) yields no quick info.
// Go: internal/ls/hover.go:ProvideHover (nil symbol -> no hover)
#[test]
fn get_quick_info_at_position_is_none_without_a_symbol() {
    let mut ls = build_service(&[("/m.ts", "const x: number = 1;")], "/", &["/m.ts"]);
    // Position 0 is the `const` keyword.
    assert!(ls
        .get_quick_info_at_position(
            "/m.ts",
            Position {
                line: 0,
                character: 0
            }
        )
        .is_none());
}

// An unknown file yields no quick info (no panic).
// Go: internal/ls/languageservice.go:getProgramAndFile (missing file)
#[test]
fn get_quick_info_at_position_unknown_file_is_none() {
    let mut ls = build_service(&[("/m.ts", "const x: number = 1;")], "/", &["/m.ts"]);
    assert!(ls
        .get_quick_info_at_position(
            "/missing.ts",
            Position {
                line: 0,
                character: 0
            }
        )
        .is_none());
}

// Position-conversion slice: an astral (non-BMP) character earlier on the line
// makes the UTF-16 character offset of the target token differ from its UTF-8
// byte offset, in both directions. `𐐷` (U+10437) is 4 UTF-8 bytes but 2 UTF-16
// code units, so the trailing `x` sits at byte 39 yet UTF-16 character 37.
//
// Hovering at UTF-16 `(0, 37)` must convert to byte 39 to resolve `x`, and the
// reported hover range must convert byte `[39, 40)` back to UTF-16 `[37, 38)`.
// Go: internal/ls/lsconv/converters.go:LineAndCharacterToPosition / PositionToLineAndCharacter
#[test]
fn quick_info_position_conversion_is_utf16_correct_on_multibyte_line() {
    let src = "const x: number = 1; const s = \"\u{10437}\"; x";
    // Sanity: the target `x` is the last byte, and its UTF-16 column is 37.
    assert_eq!(src.len(), 40);
    assert_eq!(src.encode_utf16().count(), 38);

    let mut ls = build_service(&[("/m.ts", src)], "/", &["/m.ts"]);
    let quick_info = ls
        .get_quick_info_at_position(
            "/m.ts",
            Position {
                line: 0,
                character: 37,
            },
        )
        .expect("quick info for the trailing `x` use");
    assert_eq!(quick_info.text, "number");
    // The token's internal byte span is [39, 40).
    assert_eq!(quick_info.text_range, TextRange::new(39, 40));

    let hover = ls
        .provide_hover(
            "/m.ts",
            Position {
                line: 0,
                character: 37,
            },
        )
        .expect("a hover for the trailing `x` use");
    assert_eq!(
        hover.range,
        Some(Range {
            start: Position {
                line: 0,
                character: 37
            },
            end: Position {
                line: 0,
                character: 38
            },
        })
    );
}
