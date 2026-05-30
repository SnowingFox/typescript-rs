use super::*;
use crate::emittextwriter::EmitTextWriter;
use tsgo_core::Utf16Offset;

// Go: internal/printer/textwriter.go:GetDefaultIndentSize
#[test]
fn default_indent_size_is_four() {
    assert_eq!(get_default_indent_size(), 4);
}

// Go: internal/printer/textwriter.go:NewTextWriter
#[test]
fn new_text_writer_defaults_indent_size() {
    let mut w = new_text_writer("\n", 0);
    w.increase_indent();
    w.write("a");
    // Indent size defaulted to 4.
    assert_eq!(w.get_text(), "    a");
}

// Go: internal/printer/textwriter.go:textWriter.Write
#[test]
fn write_applies_indent_at_line_start() {
    let mut w = new_text_writer("\n", 4);
    w.increase_indent();
    w.write("a");
    assert_eq!(w.get_text(), "    a");
    assert_eq!(w.get_column(), Utf16Offset(5));
    assert!(!w.is_at_start_of_line());
    assert_eq!(w.get_indent(), 1);
}

// Go: internal/printer/textwriter.go:textWriter.WriteLine
#[test]
fn write_line_then_indented_write() {
    let mut w = new_text_writer("\n", 4);
    w.increase_indent();
    w.write("a");
    w.write_line();
    assert!(w.is_at_start_of_line());
    assert_eq!(w.get_line(), 1);
    w.write("b");
    assert_eq!(w.get_text(), "    a\n    b");
    assert_eq!(w.get_line(), 1);
}

// Go: internal/printer/textwriter.go:textWriter.WriteLine (no-op when at line start)
#[test]
fn write_line_is_noop_at_start_of_line() {
    let mut w = new_text_writer("\n", 4);
    w.write_line();
    assert_eq!(w.get_text(), "");
    assert_eq!(w.get_line(), 0);
}

// Go: internal/printer/textwriter.go:textWriter.WriteLineForce
#[test]
fn write_line_force_emits_at_start_of_line() {
    let mut w = new_text_writer("\n", 4);
    w.write_line_force(true);
    assert_eq!(w.get_text(), "\n");
    assert_eq!(w.get_line(), 1);
}

// Go: internal/printer/textwriter.go:textWriter.RawWrite
#[test]
fn raw_write_ignores_indent() {
    let mut w = new_text_writer("\n", 4);
    w.increase_indent();
    w.raw_write("x");
    assert_eq!(w.get_text(), "x");
    assert!(!w.is_at_start_of_line());
}

// Go: internal/printer/textwriter.go:textWriter.RawWrite (multi-line updates line state)
#[test]
fn raw_write_multiline_updates_line_count() {
    let mut w = new_text_writer("\n", 4);
    w.raw_write("ab\ncd");
    assert_eq!(w.get_line(), 1);
    assert_eq!(w.get_column(), Utf16Offset(2));
    assert!(!w.is_at_start_of_line());
}

// Go: internal/printer/textwriter.go:textWriter.GetTextPos
#[test]
fn get_text_pos_returns_byte_length() {
    let mut w = new_text_writer("\n", 4);
    w.write("abc");
    assert_eq!(w.get_text_pos(), 3);
}

// Go: internal/printer/textwriter.go:textWriter.HasTrailingWhitespace
#[test]
fn has_trailing_whitespace_tracks_last_write() {
    let mut w = new_text_writer("\n", 4);
    assert!(!w.has_trailing_whitespace());
    w.write("a ");
    assert!(w.has_trailing_whitespace());
    w.write("b");
    assert!(!w.has_trailing_whitespace());
}

// Go: internal/printer/textwriter.go:textWriter.HasTrailingComment / WriteComment
#[test]
fn has_trailing_comment_tracks_comment_writes() {
    let mut w = new_text_writer("\n", 4);
    assert!(!w.has_trailing_comment());
    w.write_comment("/*x*/");
    assert!(w.has_trailing_comment());
    w.write("y");
    assert!(!w.has_trailing_comment());
}

// Go: internal/printer/textwriter.go:textWriter.Clear
#[test]
fn clear_resets_state_but_keeps_config() {
    let mut w = new_text_writer("\n", 4);
    w.increase_indent();
    w.write("a");
    w.clear();
    assert_eq!(w.get_text(), "");
    assert_eq!(w.get_indent(), 0);
    assert!(w.is_at_start_of_line());
    // Configured indent size is preserved.
    w.increase_indent();
    w.write("b");
    assert_eq!(w.get_text(), "    b");
}

// Go: internal/printer/textwriter.go:textWriter.GetColumn (column reflects indent at line start)
#[test]
fn column_at_line_start_reflects_indent() {
    let mut w = new_text_writer("\n", 4);
    w.increase_indent();
    w.increase_indent();
    assert!(w.is_at_start_of_line());
    assert_eq!(w.get_column(), Utf16Offset(8));
}

// Go: internal/printer/emittextwriter.go:EmitTextWriter (usable as a trait object)
#[test]
fn usable_as_trait_object() {
    let mut w = new_text_writer("\n", 4);
    let dyn_w: &mut dyn EmitTextWriter = &mut w;
    dyn_w.write_keyword("return");
    dyn_w.write_space(" ");
    dyn_w.write_punctuation(";");
    assert_eq!(dyn_w.get_text(), "return ;");
}
