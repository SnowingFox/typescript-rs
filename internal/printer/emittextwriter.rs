//! The `EmitTextWriter` trait: the opaque text sink the printer writes to.

use tsgo_ast::Symbol;
use tsgo_core::Utf16Offset;

/// An opaque text sink used by the printer, tracking indentation, line/column,
/// and trailing comment/whitespace state.
///
/// Mirrors Go's `EmitTextWriter` interface. The many `write_*` variants exist so
/// downstream writers (formatting, language service) can classify tokens; the
/// plain [`crate::textwriter::TextWriter`] treats most of them as plain writes.
///
/// Side effects: every `write_*`/`clear`/`*_indent` method mutates the sink.
// Go: internal/printer/emittextwriter.go:EmitTextWriter
pub trait EmitTextWriter {
    /// Writes `s` (applying pending indentation), clearing trailing-comment state.
    fn write(&mut self, s: &str);
    /// Writes a trailing semicolon token.
    fn write_trailing_semicolon(&mut self, text: &str);
    /// Writes comment text, marking trailing-comment state.
    fn write_comment(&mut self, text: &str);
    /// Writes a keyword token.
    fn write_keyword(&mut self, text: &str);
    /// Writes an operator token.
    fn write_operator(&mut self, text: &str);
    /// Writes a punctuation token.
    fn write_punctuation(&mut self, text: &str);
    /// Writes whitespace.
    fn write_space(&mut self, text: &str);
    /// Writes a string-literal token.
    fn write_string_literal(&mut self, text: &str);
    /// Writes a parameter name token.
    fn write_parameter(&mut self, text: &str);
    /// Writes a property name token.
    fn write_property(&mut self, text: &str);
    /// Writes a token associated with `symbol`.
    fn write_symbol(&mut self, text: &str, symbol: &Symbol);
    /// Ends the current line unless already at line start.
    fn write_line(&mut self);
    /// Ends the current line; if `force`, emits a newline even at line start.
    fn write_line_force(&mut self, force: bool);
    /// Increases the indentation level.
    fn increase_indent(&mut self);
    /// Decreases the indentation level.
    fn decrease_indent(&mut self);
    /// Clears all accumulated text and resets state.
    fn clear(&mut self);
    /// Returns the accumulated text.
    fn get_text(&self) -> &str;
    /// Writes `s` verbatim, ignoring pending indentation.
    fn raw_write(&mut self, s: &str);
    /// Writes literal text.
    fn write_literal(&mut self, s: &str);
    /// Returns the current byte length of the output.
    fn get_text_pos(&self) -> i32;
    /// Returns the current 0-based line number.
    fn get_line(&self) -> i32;
    /// Returns the current column, measured in UTF-16 code units.
    fn get_column(&self) -> Utf16Offset;
    /// Returns the current indentation level.
    fn get_indent(&self) -> i32;
    /// Reports whether the writer is at the start of a line.
    fn is_at_start_of_line(&self) -> bool;
    /// Reports whether the last write was a comment.
    fn has_trailing_comment(&self) -> bool;
    /// Reports whether the last written text ends with whitespace.
    fn has_trailing_whitespace(&self) -> bool;
}

#[cfg(test)]
#[path = "emittextwriter_test.rs"]
mod tests;
