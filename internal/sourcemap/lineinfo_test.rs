use super::*;
use tsgo_core::text::TextPos;

// Go: internal/sourcemap/lineinfo.go:LineText
#[test]
fn ecma_line_info_line_text() {
    let text = "abc\ndef";
    let line_starts = vec![TextPos(0), TextPos(4)];
    let info = create_ecma_line_info(text, line_starts);
    assert_eq!(info.line_count(), 2);
    assert_eq!(info.line_text(0), "abc\n");
    assert_eq!(info.line_text(1), "def");
}
