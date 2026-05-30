//! Pure emit helpers: string escaping, triple-slash/pinned comment recognition,
//! and generated-name formatting.
//!
//! This file ports the self-contained, AST-free helpers of Go
//! `internal/printer/utilities.go`. The range/line helpers and `getLiteralText`
//! (which need the arena, scanner, and source map) are added alongside the emit
//! loop in later slices.

use tsgo_ast::Kind;
use tsgo_core::text::{TextPos, TextRange};
use tsgo_scanner::{compute_line_of_position, skip_trivia_ex, SkipTriviaOptions};
use tsgo_stringutil::{is_digit, is_white_space_single_line};

bitflags::bitflags! {
    /// Flags steering literal text production / escaping.
    ///
    /// Side effects: none (pure value type).
    // Go: internal/printer/utilities.go:getLiteralTextFlags
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct GetLiteralTextFlags: u32 {
        /// No flags.
        const NONE = 0;
        /// Never emit non-ASCII characters as `\uXXXX` escapes.
        const NEVER_ASCII_ESCAPE = 1 << 0;
        /// Escape for a JSX attribute value (HTML entities, no backslash escaping).
        const JSX_ATTRIBUTE_ESCAPE = 1 << 1;
        /// Terminate unterminated literals.
        const TERMINATE_UNTERMINATED_LITERALS = 1 << 2;
        /// Permit numeric separators to remain in the original text.
        const ALLOW_NUMERIC_SEPARATOR = 1 << 3;
    }
}

/// The quote character used to wrap (and escape within) a string-like literal.
///
/// # Examples
/// ```
/// use tsgo_printer::utilities::{escape_string, QuoteChar};
/// assert_eq!(escape_string("a'b", QuoteChar::SingleQuote), "a\\'b");
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/printer/utilities.go:QuoteChar
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuoteChar {
    /// A single quote (`'`).
    SingleQuote,
    /// A double quote (`"`).
    DoubleQuote,
    /// A backtick (`` ` ``).
    Backtick,
}

impl QuoteChar {
    /// Returns the underlying quote character.
    ///
    /// Side effects: none (pure).
    pub fn ch(self) -> char {
        match self {
            QuoteChar::SingleQuote => '\'',
            QuoteChar::DoubleQuote => '"',
            QuoteChar::Backtick => '`',
        }
    }
}

/// A lexical comment range: a [`TextRange`] tagged with its trivia kind.
///
/// Mirrors Go `ast.CommentRange`. It is defined here until `tsgo_ast` ports the
/// type; the printer is its only current consumer.
///
/// Side effects: none (pure value type).
// Go: internal/ast/ast.go:CommentRange
#[derive(Clone, Copy, Debug)]
pub struct CommentRange {
    /// The comment's source range.
    pub loc: TextRange,
    /// The trivia kind (`SingleLineCommentTrivia` / `MultiLineCommentTrivia`).
    pub kind: Kind,
    /// Whether a newline follows the comment.
    pub has_trailing_new_line: bool,
}

impl CommentRange {
    /// Builds a comment range over `[pos, end)`.
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/ast.go:NodeFactory.NewCommentRange
    pub fn new(kind: Kind, pos: i32, end: i32, has_trailing_new_line: bool) -> CommentRange {
        CommentRange {
            loc: TextRange::new(pos, end),
            kind,
            has_trailing_new_line,
        }
    }

    /// Returns the range start offset.
    ///
    /// Side effects: none (pure).
    pub fn pos(self) -> i32 {
        self.loc.pos()
    }

    /// Returns the range end offset.
    ///
    /// Side effects: none (pure).
    pub fn end(self) -> i32 {
        self.loc.end()
    }

    /// Returns the range length.
    ///
    /// Side effects: none (pure).
    pub fn len(self) -> i32 {
        self.loc.len()
    }

    /// Reports whether the range is empty.
    ///
    /// Side effects: none (pure).
    pub fn is_empty(self) -> bool {
        self.loc.is_empty()
    }
}

/// Returns the canonical `\`-style escape for a control/canonical character, if
/// one exists.
fn escaped_char(ch: char) -> Option<&'static str> {
    Some(match ch {
        '\t' => "\\t",
        '\u{b}' => "\\v",
        '\u{c}' => "\\f",
        '\u{8}' => "\\b",
        '\r' => "\\r",
        '\n' => "\\n",
        '\\' => "\\\\",
        '"' => "\\\"",
        '\'' => "\\'",
        '`' => "\\`",
        '$' => "\\$",
        '\u{2028}' => "\\u2028",
        '\u{2029}' => "\\u2029",
        '\u{85}' => "\\u0085",
        _ => return None,
    })
}

/// Returns the HTML entity for a JSX-escaped quote character, if one exists.
fn jsx_escaped_char(ch: char) -> Option<&'static str> {
    match ch {
        '"' => Some("&quot;"),
        '\'' => Some("&apos;"),
        _ => None,
    }
}

/// Appends `&#xHH;` for `char_code` to `b`.
fn encode_jsx_character_entity(b: &mut String, char_code: u32) {
    b.push_str("&#x");
    b.push_str(&format!("{char_code:X}"));
    b.push(';');
}

/// Appends `\uHHHH` (min 4 hex digits, uppercase) for `char_code` to `b`.
fn encode_utf16_escape_sequence(b: &mut String, char_code: u32) {
    b.push_str("\\u");
    b.push_str(&format!("{char_code:04X}"));
}

/// Escapes the contents of a string-like literal per ECMA-262 `QuoteJSONString`,
/// augmented for line/paragraph separators and next-line.
///
/// Does not wrap the input in quote characters.
// Go: internal/printer/utilities.go:escapeStringWorker
pub(crate) fn escape_string_worker(
    s: &str,
    quote_char: QuoteChar,
    flags: GetLiteralTextFlags,
    b: &mut String,
) {
    let bytes = s.as_bytes();
    let quote = quote_char.ch();
    let jsx = flags.contains(GetLiteralTextFlags::JSX_ATTRIBUTE_ESCAPE);
    let never_ascii = flags.contains(GetLiteralTextFlags::NEVER_ASCII_ESCAPE);
    let mut pos = 0usize;
    let mut i = 0usize;
    while i < s.len() {
        let ch = s[i..].chars().next().unwrap();
        let mut size = ch.len_utf8();

        let escape = match ch {
            '\\' => !jsx,
            '$' => quote_char == QuoteChar::Backtick && i + 1 < s.len() && bytes[i + 1] == b'{',
            '\u{2028}' | '\u{2029}' | '\u{85}' | '\r' => true,
            '\n' => quote_char != QuoteChar::Backtick,
            _ => ch == quote || ch <= '\u{1f}' || (!never_ascii && ch > '\u{7f}'),
        };

        if escape {
            if pos < i {
                b.push_str(&s[pos..i]);
            }

            if jsx {
                if ch == '\0' {
                    b.push_str("&#0;");
                } else if let Some(m) = jsx_escaped_char(ch) {
                    b.push_str(m);
                } else {
                    encode_jsx_character_entity(b, ch as u32);
                }
            } else if ch == '\r'
                && quote_char == QuoteChar::Backtick
                && i + 1 < s.len()
                && bytes[i + 1] == b'\n'
            {
                // Template strings preserve simple LF newlines but must still escape CRLF.
                size += 1;
                b.push_str("\\r\\n");
            } else if (ch as u32) > 0xffff {
                let c = (ch as u32) - 0x10000;
                encode_utf16_escape_sequence(b, ((c & 0b1111_1111_1100_0000_0000) >> 10) + 0xD800);
                encode_utf16_escape_sequence(b, (c & 0b0000_0000_0011_1111_1111) + 0xDC00);
            } else if ch == '\0' {
                if i + 1 < s.len() && is_digit(bytes[i + 1] as char) {
                    // A null followed by a digit must be a hex escape to avoid an octal parse.
                    b.push_str("\\x00");
                } else {
                    b.push_str("\\0");
                }
            } else if let Some(m) = escaped_char(ch) {
                b.push_str(m);
            } else {
                encode_utf16_escape_sequence(b, ch as u32);
            }
            pos = i + size;
        }

        i += size;
    }

    if pos < s.len() {
        b.push_str(&s[pos..]);
    }
}

/// Escapes a string for emission, keeping non-ASCII characters verbatim.
///
/// # Examples
/// ```
/// use tsgo_printer::utilities::{escape_string, QuoteChar};
/// assert_eq!(escape_string("a\tb", QuoteChar::DoubleQuote), "a\\tb");
/// ```
///
/// Side effects: none (pure).
// Go: internal/printer/utilities.go:EscapeString
pub fn escape_string(s: &str, quote_char: QuoteChar) -> String {
    let mut b = String::with_capacity(s.len() + 2);
    escape_string_worker(
        s,
        quote_char,
        GetLiteralTextFlags::NEVER_ASCII_ESCAPE,
        &mut b,
    );
    b
}

/// Escapes a string for emission, escaping non-ASCII characters as `\uXXXX`.
///
/// # Examples
/// ```
/// use tsgo_printer::utilities::{escape_non_ascii_string, QuoteChar};
/// assert_eq!(escape_non_ascii_string("\u{8f}", QuoteChar::DoubleQuote), "\\u008F");
/// ```
///
/// Side effects: none (pure).
// Go: internal/printer/utilities.go:escapeNonAsciiString
pub fn escape_non_ascii_string(s: &str, quote_char: QuoteChar) -> String {
    let mut b = String::with_capacity(s.len() + 2);
    escape_string_worker(s, quote_char, GetLiteralTextFlags::NONE, &mut b);
    b
}

/// Escapes a string for a JSX attribute value (HTML entities, no backslash escaping).
///
/// # Examples
/// ```
/// use tsgo_printer::utilities::{escape_jsx_attribute_string, QuoteChar};
/// assert_eq!(escape_jsx_attribute_string("\"", QuoteChar::DoubleQuote), "&quot;");
/// ```
///
/// Side effects: none (pure).
// Go: internal/printer/utilities.go:escapeJsxAttributeString
pub fn escape_jsx_attribute_string(s: &str, quote_char: QuoteChar) -> String {
    let mut b = String::with_capacity(s.len() + 2);
    escape_string_worker(
        s,
        quote_char,
        GetLiteralTextFlags::JSX_ATTRIBUTE_ESCAPE | GetLiteralTextFlags::NEVER_ASCII_ESCAPE,
        &mut b,
    );
    b
}

/// Advances `pos` over single-line whitespace within `text`.
fn skip_white_space_single_line(text: &str, pos: &mut usize) {
    while *pos < text.len() {
        let ch = text[*pos..].chars().next().unwrap();
        if !is_white_space_single_line(ch) {
            break;
        }
        *pos += ch.len_utf8();
    }
}

/// Reports whether at least one single-line whitespace char was skipped.
fn match_white_space_single_line(text: &str, pos: &mut usize) -> bool {
    let start = *pos;
    skip_white_space_single_line(text, pos);
    *pos != start
}

/// Consumes `expected` at `pos` if present, advancing `pos`.
fn match_rune(text: &str, pos: &mut usize, expected: char) -> bool {
    if *pos >= text.len() {
        return false;
    }
    let ch = text[*pos..].chars().next().unwrap();
    if ch == expected {
        *pos += ch.len_utf8();
        true
    } else {
        false
    }
}

/// Consumes the literal `expected` at `pos` if present, advancing `pos`.
fn match_string(text: &str, pos: &mut usize, expected: &str) -> bool {
    let mut text_pos = *pos;
    for expected_rune in expected.chars() {
        if text_pos >= text.len() {
            return false;
        }
        if !match_rune(text, &mut text_pos, expected_rune) {
            return false;
        }
    }
    *pos = text_pos;
    true
}

/// Consumes a single- or double-quoted string at `pos`, advancing past it.
fn match_quoted_string(text: &str, pos: &mut usize) -> bool {
    let mut text_pos = *pos;
    let quote_char = if match_rune(text, &mut text_pos, '\'') {
        '\''
    } else if match_rune(text, &mut text_pos, '"') {
        '"'
    } else {
        return false;
    };
    while text_pos < text.len() {
        let ch = text[text_pos..].chars().next().unwrap();
        text_pos += ch.len_utf8();
        if ch == quote_char {
            *pos = text_pos;
            return true;
        }
    }
    false
}

/// Reports whether `text[commentRange]` is a recognized triple-slash directive
/// comment (`/// <reference ... />`, `/// <amd-dependency ... />`, `/// <amd-module />`).
///
/// # Examples
/// ```
/// use tsgo_ast::Kind;
/// use tsgo_printer::utilities::{is_recognized_triple_slash_comment, CommentRange};
/// let s = "///<reference path=\"foo\" />";
/// let cr = CommentRange::new(Kind::SingleLineCommentTrivia, 0, s.len() as i32, false);
/// assert!(is_recognized_triple_slash_comment(s, cr));
/// ```
///
/// Side effects: none (pure).
// Go: internal/printer/utilities.go:IsRecognizedTripleSlashComment
pub fn is_recognized_triple_slash_comment(text: &str, comment_range: CommentRange) -> bool {
    let bytes = text.as_bytes();
    let cr_pos = comment_range.pos() as usize;
    let cr_end = comment_range.end() as usize;
    if comment_range.kind == Kind::SingleLineCommentTrivia
        && comment_range.len() > 2
        && bytes[cr_pos + 1] == b'/'
        && bytes[cr_pos + 2] == b'/'
    {
        let text = &text[cr_pos + 3..cr_end];
        let mut pos = 0usize;
        skip_white_space_single_line(text, &mut pos);
        if !match_rune(text, &mut pos, '<') {
            return false;
        }
        if match_string(text, &mut pos, "reference") {
            if !match_white_space_single_line(text, &mut pos) {
                return false;
            }
            if !match_string(text, &mut pos, "path")
                && !match_string(text, &mut pos, "types")
                && !match_string(text, &mut pos, "lib")
                && !match_string(text, &mut pos, "no-default-lib")
            {
                return false;
            }
            skip_white_space_single_line(text, &mut pos);
            if !match_rune(text, &mut pos, '=') {
                return false;
            }
            skip_white_space_single_line(text, &mut pos);
            if !match_quoted_string(text, &mut pos) {
                return false;
            }
        } else if match_string(text, &mut pos, "amd-dependency") {
            if !match_white_space_single_line(text, &mut pos) {
                return false;
            }
            if !match_string(text, &mut pos, "path") {
                return false;
            }
            skip_white_space_single_line(text, &mut pos);
            if !match_rune(text, &mut pos, '=') {
                return false;
            }
            skip_white_space_single_line(text, &mut pos);
            if !match_quoted_string(text, &mut pos) {
                return false;
            }
        } else if match_string(text, &mut pos, "amd-module") {
            skip_white_space_single_line(text, &mut pos);
        } else {
            return false;
        }
        return text[pos..].contains("/>");
    }

    false
}

/// Reports whether `comment` is a "pinned" (`/*! ... */`) multi-line comment.
///
/// # Examples
/// ```
/// use tsgo_ast::Kind;
/// use tsgo_printer::utilities::{is_pinned_comment, CommentRange};
/// let s = "/*! keep */";
/// let cr = CommentRange::new(Kind::MultiLineCommentTrivia, 0, s.len() as i32, false);
/// assert!(is_pinned_comment(s, cr));
/// ```
///
/// Side effects: none (pure).
// Go: internal/printer/utilities.go:IsPinnedComment
pub fn is_pinned_comment(text: &str, comment: CommentRange) -> bool {
    let bytes = text.as_bytes();
    comment.kind == Kind::MultiLineCommentTrivia
        && comment.len() > 5
        && bytes[comment.pos() as usize + 2] == b'!'
}

/// Reports whether `text` starts with `#`.
fn has_leading_hash(text: &str) -> bool {
    text.starts_with('#')
}

/// Removes a single leading `#` from `text`, if present.
pub(crate) fn remove_leading_hash(text: &str) -> &str {
    if has_leading_hash(text) {
        &text[1..]
    } else {
        text
    }
}

/// Ensures `text` starts with a single `#`.
pub(crate) fn ensure_leading_hash(text: &str) -> String {
    if has_leading_hash(text) {
        text.to_string()
    } else {
        format!("#{text}")
    }
}

/// Joins `prefix`/`base`/`suffix` into a generated name, stripping each part's
/// leading `#` and re-adding one iff `private_name` is set.
///
/// # Examples
/// ```
/// use tsgo_printer::utilities::format_generated_name;
/// assert_eq!(format_generated_name(true, "#a", "#b", "#c"), "#abc");
/// ```
///
/// Side effects: none (pure).
// Go: internal/printer/utilities.go:FormatGeneratedName
pub fn format_generated_name(private_name: bool, prefix: &str, base: &str, suffix: &str) -> String {
    let name = format!(
        "{}{}{}",
        remove_leading_hash(prefix),
        remove_leading_hash(base),
        remove_leading_hash(suffix)
    );
    if private_name {
        ensure_leading_hash(&name)
    } else {
        name
    }
}

/// Returns the signed number of source lines between `pos1` and `pos2`
/// (negative when `pos2` precedes `pos1`), measured against `line_starts`.
///
/// Side effects: none (pure).
// Go: internal/printer/utilities.go:GetLinesBetweenPositions
pub(crate) fn get_lines_between_positions(line_starts: &[TextPos], pos1: i32, pos2: i32) -> i32 {
    if pos1 == pos2 {
        return 0;
    }
    let lower = pos1.min(pos2);
    let is_negative = lower == pos2;
    let upper = if is_negative { pos1 } else { pos2 };
    let lower_line = compute_line_of_position(line_starts, lower);
    let upper_line =
        lower_line + compute_line_of_position(&line_starts[lower_line as usize..], upper);
    if is_negative {
        lower_line - upper_line
    } else {
        upper_line - lower_line
    }
}

/// Reports whether `pos1` and `pos2` fall on the same source line.
///
/// Side effects: none (pure).
// Go: internal/printer/utilities.go:PositionsAreOnSameLine
pub(crate) fn positions_are_on_same_line(line_starts: &[TextPos], pos1: i32, pos2: i32) -> bool {
    get_lines_between_positions(line_starts, pos1, pos2) == 0
}

/// Returns the first non-trivia position of a range starting at `pos`
/// (`-1` for a synthesized position).
///
/// Side effects: none (pure).
// Go: internal/printer/utilities.go:getStartPositionOfRange
pub(crate) fn get_start_position_of_range(text: &str, pos: i32, include_comments: bool) -> i32 {
    if pos < 0 {
        return -1;
    }
    skip_trivia_ex(
        text,
        pos,
        Some(&SkipTriviaOptions {
            stop_after_line_break: false,
            stop_at_comments: include_comments,
            in_jsdoc: false,
        }),
    )
}

/// Reports whether `range`'s start and end are on the same line.
///
/// Side effects: none (pure).
// Go: internal/printer/utilities.go:RangeIsOnSingleLine / rangeStartIsOnSameLineAsRangeEnd
pub(crate) fn range_is_on_single_line(
    text: &str,
    line_starts: &[TextPos],
    range: TextRange,
) -> bool {
    range_start_is_on_same_line_as_range_end(text, line_starts, range, range)
}

/// Reports whether `range1`'s start line equals `range2`'s end line.
///
/// Side effects: none (pure).
// Go: internal/printer/utilities.go:rangeStartIsOnSameLineAsRangeEnd
pub(crate) fn range_start_is_on_same_line_as_range_end(
    text: &str,
    line_starts: &[TextPos],
    range1: TextRange,
    range2: TextRange,
) -> bool {
    positions_are_on_same_line(
        line_starts,
        get_start_position_of_range(text, range1.pos(), false),
        range2.end(),
    )
}

/// Reports whether `range1`'s end line equals `range2`'s start line.
///
/// Side effects: none (pure).
// Go: internal/printer/utilities.go:rangeEndIsOnSameLineAsRangeStart
pub(crate) fn range_end_is_on_same_line_as_range_start(
    text: &str,
    line_starts: &[TextPos],
    range1: TextRange,
    range2: TextRange,
) -> bool {
    positions_are_on_same_line(
        line_starts,
        range1.end(),
        get_start_position_of_range(text, range2.pos(), false),
    )
}

/// Reports whether `range1` and `range2` end on the same line.
///
/// Side effects: none (pure).
// Go: internal/printer/utilities.go:rangeEndPositionsAreOnSameLine
pub(crate) fn range_end_positions_are_on_same_line(
    line_starts: &[TextPos],
    range1: TextRange,
    range2: TextRange,
) -> bool {
    positions_are_on_same_line(line_starts, range1.end(), range2.end())
}

/// Reports whether `range1` and `range2` start on the same line.
///
/// Side effects: none (pure).
// Go: internal/printer/utilities.go:RangeStartPositionsAreOnSameLine
pub(crate) fn range_start_positions_are_on_same_line(
    text: &str,
    line_starts: &[TextPos],
    range1: TextRange,
    range2: TextRange,
) -> bool {
    positions_are_on_same_line(
        line_starts,
        get_start_position_of_range(text, range1.pos(), false),
        get_start_position_of_range(text, range2.pos(), false),
    )
}

#[cfg(test)]
#[path = "utilities_test.rs"]
mod tests;
