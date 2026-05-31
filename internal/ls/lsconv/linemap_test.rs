use super::*;
use tsgo_core::text::TextPos;

// Behavior test (no direct Go test): a single newline splits two lines and
// pure-ASCII text reports ascii_only = true.
// Go: internal/ls/lsconv/linemap.go:ComputeLSPLineStarts
#[test]
fn compute_lsp_line_starts_ascii_single_newline() {
    let lm = compute_lsp_line_starts(b"hello\nworld");
    assert_eq!(lm.line_starts, vec![TextPos(0), TextPos(6)]);
    assert!(lm.ascii_only);
}

// `\r\n` is a single line break: the next line starts after both bytes.
// Go: internal/ls/lsconv/linemap.go:ComputeLSPLineStarts
#[test]
fn compute_lsp_line_starts_crlf() {
    let lm = compute_lsp_line_starts(b"a\r\nb");
    assert_eq!(lm.line_starts, vec![TextPos(0), TextPos(3)]);
    assert!(lm.ascii_only);
}

// A lone `\r` (not followed by `\n`) is also a line break.
// Go: internal/ls/lsconv/linemap.go:ComputeLSPLineStarts
#[test]
fn compute_lsp_line_starts_cr_only() {
    let lm = compute_lsp_line_starts(b"a\rb");
    assert_eq!(lm.line_starts, vec![TextPos(0), TextPos(2)]);
    assert!(lm.ascii_only);
}

// Non-ASCII bytes advance by their UTF-8 width and clear ascii_only.
// Go: internal/ls/lsconv/linemap.go:ComputeLSPLineStarts
#[test]
fn compute_lsp_line_starts_non_ascii_clears_flag() {
    let lm = compute_lsp_line_starts("α\nβ".as_bytes());
    assert_eq!(lm.line_starts, vec![TextPos(0), TextPos(3)]);
    assert!(!lm.ascii_only);
}

// Empty text has a single line starting at 0 and is ascii_only.
// Go: internal/ls/lsconv/linemap.go:ComputeLSPLineStarts
#[test]
fn compute_lsp_line_starts_empty() {
    let lm = compute_lsp_line_starts(b"");
    assert_eq!(lm.line_starts, vec![TextPos(0)]);
    assert!(lm.ascii_only);
}

fn line_map(starts: &[i32]) -> LSPLineMap {
    LSPLineMap {
        line_starts: starts.iter().map(|&p| TextPos(p)).collect(),
        ascii_only: true,
    }
}

// Behavior test (no direct Go test): an exact line-start hit returns that line;
// a position inside a line returns the line containing it; clamps at the edges.
// Go: internal/ls/lsconv/linemap.go:ComputeIndexOfLineStart
#[test]
fn compute_index_of_line_start_basic() {
    let lm = line_map(&[0, 5, 10]);
    assert_eq!(lm.compute_index_of_line_start(TextPos(5)), 1); // exact start
    assert_eq!(lm.compute_index_of_line_start(TextPos(7)), 1); // inside line 1
    assert_eq!(lm.compute_index_of_line_start(TextPos(0)), 0); // exact first
    assert_eq!(lm.compute_index_of_line_start(TextPos(3)), 0); // inside line 0
    assert_eq!(lm.compute_index_of_line_start(TextPos(12)), 2); // past last start
}
