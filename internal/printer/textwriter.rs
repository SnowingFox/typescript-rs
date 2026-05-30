//! [`TextWriter`]: the default in-memory [`EmitTextWriter`] implementation.

use crate::emittextwriter::EmitTextWriter;
use tsgo_ast::Symbol;
use tsgo_core::{compute_ecma_line_starts_seq, utf16_len, Utf16Offset};
use tsgo_stringutil::is_white_space_like;

const DEFAULT_INDENT_SIZE: i32 = 4;

/// Returns the default indent size (4 spaces) used when no specific indent size
/// is configured.
///
/// # Examples
/// ```
/// use tsgo_printer::textwriter::get_default_indent_size;
/// assert_eq!(get_default_indent_size(), 4);
/// ```
///
/// Side effects: none (pure).
// Go: internal/printer/textwriter.go:GetDefaultIndentSize
pub fn get_default_indent_size() -> i32 {
    DEFAULT_INDENT_SIZE
}

/// Returns `indent * indent_size` spaces (empty when `indent == 0`).
fn get_indent_string(indent: i32, indent_size: i32) -> String {
    if indent == 0 {
        return String::new();
    }
    " ".repeat((indent * indent_size) as usize)
}

/// An in-memory [`EmitTextWriter`] that accumulates output into a `String` while
/// tracking indentation, line/column, and trailing comment/whitespace state.
///
/// # Examples
/// ```
/// use tsgo_printer::textwriter::new_text_writer;
/// use tsgo_printer::emittextwriter::EmitTextWriter;
/// let mut w = new_text_writer("\n", 4);
/// w.increase_indent();
/// w.write("x");
/// assert_eq!(w.get_text(), "    x");
/// ```
///
/// Side effects: mutating methods append to or reset the internal buffer.
// Go: internal/printer/textwriter.go:textWriter
#[derive(Clone, Debug)]
pub struct TextWriter {
    new_line: String,
    indent_size: i32,
    builder: String,
    last_written: String,
    indent: i32,
    line_start: bool,
    line_count: i32,
    line_pos: usize,
    has_trailing_comment_state: bool,
}

/// Creates a [`TextWriter`] using `new_line` for line breaks and `indent_size`
/// spaces per level (`<= 0` defaults to 4).
///
/// # Examples
/// ```
/// use tsgo_printer::textwriter::new_text_writer;
/// use tsgo_printer::emittextwriter::EmitTextWriter;
/// let w = new_text_writer("\n", 0);
/// assert!(w.is_at_start_of_line());
/// ```
///
/// Side effects: none (allocates a fresh writer).
// Go: internal/printer/textwriter.go:NewTextWriter
pub fn new_text_writer(new_line: &str, indent_size: i32) -> TextWriter {
    let indent_size = if indent_size <= 0 {
        DEFAULT_INDENT_SIZE
    } else {
        indent_size
    };
    TextWriter {
        new_line: new_line.to_string(),
        indent_size,
        builder: String::new(),
        last_written: String::new(),
        indent: 0,
        line_start: true,
        line_count: 0,
        line_pos: 0,
        has_trailing_comment_state: false,
    }
}

impl TextWriter {
    /// Updates `line_count`, `line_pos`, and `line_start` after appending `s`.
    // Go: internal/printer/textwriter.go:textWriter.updateLineCountAndPosFor
    fn update_line_count_and_pos_for(&mut self, s: &str) {
        let mut count: i32 = 0;
        let mut last_line_start: i32 = 0;
        for line_start in compute_ecma_line_starts_seq(s) {
            count += 1;
            last_line_start = line_start.0;
        }

        if count > 1 {
            self.line_count += count - 1;
            let cur_len = self.builder.len() as i64;
            let line_pos = cur_len - s.len() as i64 + last_line_start as i64;
            self.line_pos = line_pos as usize;
            self.line_start = (line_pos - cur_len) == 0;
            return;
        }
        self.line_start = false;
    }

    /// Appends `s` (applying pending indentation) and updates line state.
    // Go: internal/printer/textwriter.go:textWriter.writeText
    fn write_text(&mut self, s: &str) {
        if !s.is_empty() {
            if self.line_start {
                self.builder
                    .push_str(&get_indent_string(self.indent, self.indent_size));
                self.line_start = false;
            }
            self.builder.push_str(s);
            self.last_written = s.to_string();
            self.update_line_count_and_pos_for(s);
        }
    }

    /// Appends a raw newline and resets line state.
    // Go: internal/printer/textwriter.go:textWriter.writeLineRaw
    fn write_line_raw(&mut self) {
        self.builder.push_str(&self.new_line.clone());
        self.last_written = self.new_line.clone();
        self.line_count += 1;
        self.line_pos = self.builder.len();
        self.line_start = true;
        self.has_trailing_comment_state = false;
    }
}

impl EmitTextWriter for TextWriter {
    // Go: internal/printer/textwriter.go:textWriter.Write
    fn write(&mut self, s: &str) {
        if !s.is_empty() {
            self.has_trailing_comment_state = false;
        }
        self.write_text(s);
    }

    // Go: internal/printer/textwriter.go:textWriter.WriteTrailingSemicolon
    fn write_trailing_semicolon(&mut self, text: &str) {
        self.write(text);
    }

    // Go: internal/printer/textwriter.go:textWriter.WriteComment
    fn write_comment(&mut self, text: &str) {
        if !text.is_empty() {
            self.has_trailing_comment_state = true;
        }
        self.write_text(text);
    }

    // Go: internal/printer/textwriter.go:textWriter.WriteKeyword
    fn write_keyword(&mut self, text: &str) {
        self.write(text);
    }

    // Go: internal/printer/textwriter.go:textWriter.WriteOperator
    fn write_operator(&mut self, text: &str) {
        self.write(text);
    }

    // Go: internal/printer/textwriter.go:textWriter.WritePunctuation
    fn write_punctuation(&mut self, text: &str) {
        self.write(text);
    }

    // Go: internal/printer/textwriter.go:textWriter.WriteSpace
    fn write_space(&mut self, text: &str) {
        self.write(text);
    }

    // Go: internal/printer/textwriter.go:textWriter.WriteStringLiteral
    fn write_string_literal(&mut self, text: &str) {
        self.write(text);
    }

    // Go: internal/printer/textwriter.go:textWriter.WriteParameter
    fn write_parameter(&mut self, text: &str) {
        self.write(text);
    }

    // Go: internal/printer/textwriter.go:textWriter.WriteProperty
    fn write_property(&mut self, text: &str) {
        self.write(text);
    }

    // Go: internal/printer/textwriter.go:textWriter.WriteSymbol
    fn write_symbol(&mut self, text: &str, _symbol: &Symbol) {
        self.write(text);
    }

    // Go: internal/printer/textwriter.go:textWriter.WriteLine
    fn write_line(&mut self) {
        if !self.line_start {
            self.write_line_raw();
        }
    }

    // Go: internal/printer/textwriter.go:textWriter.WriteLineForce
    fn write_line_force(&mut self, force: bool) {
        if !self.line_start || force {
            self.write_line_raw();
        }
    }

    // Go: internal/printer/textwriter.go:textWriter.IncreaseIndent
    fn increase_indent(&mut self) {
        self.indent += 1;
    }

    // Go: internal/printer/textwriter.go:textWriter.DecreaseIndent
    fn decrease_indent(&mut self) {
        self.indent -= 1;
    }

    // Go: internal/printer/textwriter.go:textWriter.Clear
    fn clear(&mut self) {
        *self = TextWriter {
            new_line: std::mem::take(&mut self.new_line),
            indent_size: self.indent_size,
            builder: String::new(),
            last_written: String::new(),
            indent: 0,
            line_start: true,
            line_count: 0,
            line_pos: 0,
            has_trailing_comment_state: false,
        };
    }

    // Go: internal/printer/textwriter.go:textWriter.String
    fn get_text(&self) -> &str {
        &self.builder
    }

    // Go: internal/printer/textwriter.go:textWriter.RawWrite
    fn raw_write(&mut self, s: &str) {
        if !s.is_empty() {
            self.builder.push_str(s);
            self.last_written = s.to_string();
            self.has_trailing_comment_state = false;
        }
        self.update_line_count_and_pos_for(s);
    }

    // Go: internal/printer/textwriter.go:textWriter.WriteLiteral
    fn write_literal(&mut self, s: &str) {
        self.write(s);
    }

    // Go: internal/printer/textwriter.go:textWriter.GetTextPos
    fn get_text_pos(&self) -> i32 {
        self.builder.len() as i32
    }

    // Go: internal/printer/textwriter.go:textWriter.GetLine
    fn get_line(&self) -> i32 {
        self.line_count
    }

    // Go: internal/printer/textwriter.go:textWriter.GetColumn
    fn get_column(&self) -> Utf16Offset {
        if self.line_start {
            Utf16Offset(self.indent * self.indent_size)
        } else {
            utf16_len(&self.builder[self.line_pos..])
        }
    }

    // Go: internal/printer/textwriter.go:textWriter.GetIndent
    fn get_indent(&self) -> i32 {
        self.indent
    }

    // Go: internal/printer/textwriter.go:textWriter.IsAtStartOfLine
    fn is_at_start_of_line(&self) -> bool {
        self.line_start
    }

    // Go: internal/printer/textwriter.go:textWriter.HasTrailingComment
    fn has_trailing_comment(&self) -> bool {
        self.has_trailing_comment_state
    }

    // Go: internal/printer/textwriter.go:textWriter.HasTrailingWhitespace
    fn has_trailing_whitespace(&self) -> bool {
        if self.builder.is_empty() {
            return false;
        }
        match self.last_written.chars().next_back() {
            Some(ch) => is_white_space_like(ch),
            None => false,
        }
    }
}

#[cfg(test)]
#[path = "textwriter_test.rs"]
mod tests;
