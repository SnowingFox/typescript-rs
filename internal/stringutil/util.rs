//! Character/string primitives: whitespace and line-break checks, line
//! splitting, URI encoding, BOM handling, quote stripping, and so on.
//!
//! 1:1 port of Go `internal/stringutil/util.go`.

use std::sync::OnceLock;

use regex::Regex;

const UPPERHEX: &[u8; 16] = b"0123456789ABCDEF";

/// Reports whether `ch` is an ECMAScript single-line whitespace or line break
/// (the "wide" whitespace check).
///
/// # Examples
/// ```
/// use tsgo_stringutil::is_white_space_like;
/// assert!(is_white_space_like(' '));
/// assert!(is_white_space_like('\n'));
/// assert!(!is_white_space_like('a'));
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:IsWhiteSpaceLike
pub fn is_white_space_like(ch: char) -> bool {
    is_white_space_single_line(ch) || is_line_break(ch)
}

/// Reports whether `ch` is an ECMAScript single-line whitespace (excludes line
/// breaks).
///
/// Covers 21 code points (including `0x0085` nextLine and `0xFEFF` BOM).
/// `nextLine` is in category `Zs`, treated as whitespace but *not* a line break.
///
/// # Examples
/// ```
/// use tsgo_stringutil::is_white_space_single_line;
/// assert!(is_white_space_single_line('\u{00A0}'));
/// assert!(!is_white_space_single_line('\n'));
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:IsWhiteSpaceSingleLine
pub fn is_white_space_single_line(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '\t'
            | '\u{000B}' // verticalTab
            | '\u{000C}' // formFeed
            | '\u{0085}' // nextLine
            | '\u{00A0}' // nonBreakingSpace
            | '\u{1680}' // ogham
            | '\u{2000}' // enQuad
            | '\u{2001}' // emQuad
            | '\u{2002}' // enSpace
            | '\u{2003}' // emSpace
            | '\u{2004}' // threePerEmSpace
            | '\u{2005}' // fourPerEmSpace
            | '\u{2006}' // sixPerEmSpace
            | '\u{2007}' // figureSpace
            | '\u{2008}' // punctuationEmSpace
            | '\u{2009}' // thinSpace
            | '\u{200A}' // hairSpace
            | '\u{200B}' // zeroWidthSpace
            | '\u{202F}' // narrowNoBreakSpace
            | '\u{205F}' // mathematicalSpace
            | '\u{3000}' // ideographicSpace
            | '\u{FEFF}' // byteOrderMark
    )
}

/// Reports whether `ch` is an ECMAScript line break (`\n` `\r` `\u2028`
/// `\u2029`).
///
/// # Examples
/// ```
/// use tsgo_stringutil::is_line_break;
/// assert!(is_line_break('\u{2028}'));
/// assert!(!is_line_break(' '));
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:IsLineBreak
pub fn is_line_break(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// Reports whether `ch` is a decimal digit `'0'..='9'`.
///
/// # Examples
/// ```
/// use tsgo_stringutil::is_digit;
/// assert!(is_digit('5'));
/// assert!(!is_digit('a'));
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:IsDigit
pub fn is_digit(ch: char) -> bool {
    ch.is_ascii_digit()
}

/// Reports whether `ch` is an octal digit `'0'..='7'`.
///
/// # Examples
/// ```
/// use tsgo_stringutil::is_octal_digit;
/// assert!(is_octal_digit('7'));
/// assert!(!is_octal_digit('8'));
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:IsOctalDigit
pub fn is_octal_digit(ch: char) -> bool {
    ('0'..='7').contains(&ch)
}

/// Reports whether `ch` is a hexadecimal digit (`0-9A-Fa-f`).
///
/// # Examples
/// ```
/// use tsgo_stringutil::is_hex_digit;
/// assert!(is_hex_digit('f'));
/// assert!(!is_hex_digit('g'));
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:IsHexDigit
pub fn is_hex_digit(ch: char) -> bool {
    ch.is_ascii_hexdigit()
}

/// Reports whether `ch` is an ASCII letter (`A-Za-z`).
///
/// # Examples
/// ```
/// use tsgo_stringutil::is_ascii_letter;
/// assert!(is_ascii_letter('Z'));
/// assert!(!is_ascii_letter('5'));
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:IsASCIILetter
pub fn is_ascii_letter(ch: char) -> bool {
    ch.is_ascii_alphabetic()
}

/// Decodes the first rune of `s`, returning `(char, byte_len)`; empty input
/// yields `('\0', 0)`.
///
/// Mirrors Go `utf8.DecodeRuneInString` (input is assumed valid UTF-8, so there
/// is no `RuneError` branch).
fn decode_first_char(s: &str) -> (char, usize) {
    match s.chars().next() {
        Some(c) => (c, c.len_utf8()),
        None => ('\0', 0),
    }
}

/// Splits `text` into lines on `\r\n` / `\r` / `\n` (borrowing the source,
/// zero-copy); keeps a trailing non-empty segment.
///
/// # Examples
/// ```
/// use tsgo_stringutil::split_lines;
/// assert_eq!(split_lines("a\r\nb\nc\rd"), vec!["a", "b", "c", "d"]);
/// assert_eq!(split_lines("a\n"), vec!["a"]);
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:SplitLines
pub fn split_lines(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut lines: Vec<&str> = Vec::with_capacity(text.matches('\n').count() + 1);
    let mut start = 0usize;
    let mut pos = 0usize;
    while pos < bytes.len() {
        match bytes[pos] {
            b'\r' => {
                if pos + 1 < bytes.len() && bytes[pos + 1] == b'\n' {
                    lines.push(&text[start..pos]);
                    pos += 2;
                    start = pos;
                    continue;
                }
                lines.push(&text[start..pos]);
                pos += 1;
                start = pos;
                continue;
            }
            b'\n' => {
                lines.push(&text[start..pos]);
                pos += 1;
                start = pos;
                continue;
            }
            _ => {}
        }
        pos += 1;
    }
    if start < bytes.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Infers the indentation width of a group of lines: the minimum count of
/// leading whitespace bytes over all non-empty lines.
///
/// Returns immediately on hitting 0; returns 0 if there are no non-empty lines
/// or all are empty.
///
/// # Examples
/// ```
/// use tsgo_stringutil::guess_indentation;
/// assert_eq!(guess_indentation(&["  a", "    b", "   c"]), 2);
/// assert_eq!(guess_indentation(&["a"]), 0);
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:GuessIndentation
pub fn guess_indentation(lines: &[&str]) -> usize {
    const MAX_SMI_X86: usize = 0x3fff_ffff;
    let mut indentation = MAX_SMI_X86;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let mut i = 0usize;
        while i < line.len() && i < indentation {
            let (ch, size) = decode_first_char(&line[i..]);
            if !is_white_space_like(ch) {
                break;
            }
            i += size;
        }
        if i < indentation {
            indentation = i;
        }
        if indentation == 0 {
            return 0;
        }
    }
    if indentation == MAX_SMI_X86 {
        return 0;
    }
    indentation
}

/// Reports whether byte `b` must be percent-escaped under `encodeURI`
/// semantics.
///
/// Preserves ASCII alphanumerics and the unreserved set `;/?:@&=+$,#-_.!~*'()`;
/// everything else is escaped.
fn should_escape_for_encode_uri(b: u8) -> bool {
    !matches!(b,
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
        | b';' | b'/' | b'?' | b':' | b'@' | b'&' | b'=' | b'+' | b'$' | b',' | b'#' | b'-'
        | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')')
}

/// Percent-encodes a string per ECMAScript `encodeURI`.
///
/// Processed byte by byte (not rune by rune): multibyte UTF-8 characters are
/// emitted as a sequence of `%XX`, matching the Go implementation.
///
/// # Examples
/// ```
/// use tsgo_stringutil::encode_uri;
/// assert_eq!(encode_uri("a b"), "a%20b");
/// assert_eq!(encode_uri(";/?:@&=+$,#"), ";/?:@&=+$,#");
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:EncodeURI
pub fn encode_uri(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if !should_escape_for_encode_uri(b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(UPPERHEX[(b >> 4) as usize] as char);
            out.push(UPPERHEX[(b & 0x0f) as usize] as char);
        }
    }
    out
}

/// Detects a BOM and returns its length: UTF16BE/LE -> 2, UTF8 -> 3, none -> 0.
///
/// Note: a Rust `&str` is always valid UTF-8, so the UTF-16 BOM branches
/// starting with 0xFE/0xFF are unreachable in practice; only the UTF-8 BOM
/// (`EF BB BF`, i.e. `\u{FEFF}`) matches.
fn get_byte_order_mark_length(text: &str) -> usize {
    let b = text.as_bytes();
    if let Some(&ch0) = b.first() {
        if ch0 == 0xfe {
            if b.len() >= 2 && b[1] == 0xff {
                return 2; // utf16be
            }
            return 0;
        }
        if ch0 == 0xff {
            if b.len() >= 2 && b[1] == 0xfe {
                return 2; // utf16le
            }
            return 0;
        }
        if ch0 == 0xef {
            if b.len() >= 3 && b[1] == 0xbb && b[2] == 0xbf {
                return 3; // utf8
            }
            return 0;
        }
    }
    0
}

/// Strips a leading BOM (if any) from `text`.
///
/// # Examples
/// ```
/// use tsgo_stringutil::remove_byte_order_mark;
/// assert_eq!(remove_byte_order_mark("\u{FEFF}abc"), "abc");
/// assert_eq!(remove_byte_order_mark("abc"), "abc");
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:RemoveByteOrderMark
pub fn remove_byte_order_mark(text: &str) -> &str {
    let length = get_byte_order_mark_length(text);
    if length > 0 {
        &text[length..]
    } else {
        text
    }
}

/// Prepends a UTF-8 BOM (`\u{FEFF}`) when `text` has none; otherwise returns it
/// unchanged.
///
/// # Examples
/// ```
/// use tsgo_stringutil::add_utf8_byte_order_mark;
/// assert_eq!(add_utf8_byte_order_mark("abc"), "\u{FEFF}abc");
/// assert_eq!(add_utf8_byte_order_mark("\u{FEFF}abc"), "\u{FEFF}abc");
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:AddUTF8ByteOrderMark
pub fn add_utf8_byte_order_mark(text: &str) -> String {
    if get_byte_order_mark_length(text) == 0 {
        let mut s = String::with_capacity(3 + text.len());
        s.push('\u{FEFF}');
        s.push_str(text);
        s
    } else {
        text.to_string()
    }
}

/// Strips matching surrounding quotes (`'` `"` `` ` ``) when present; otherwise
/// returns `name` unchanged.
///
/// # Examples
/// ```
/// use tsgo_stringutil::strip_quotes;
/// assert_eq!(strip_quotes("\"x\""), "x");
/// assert_eq!(strip_quotes("x"), "x");
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:StripQuotes
pub fn strip_quotes(name: &str) -> &str {
    if name.len() < 2 {
        return name;
    }
    let (first_char, _) = decode_first_char(name);
    let last_char = name.chars().next_back().unwrap_or('\0');
    if first_char == last_char && (first_char == '\'' || first_char == '"' || first_char == '`') {
        &name[1..name.len() - 1]
    } else {
        name
    }
}

/// Regex matching `\.` (backslash + any non-newline char) used by
/// [`unquote_string`] (lazily initialized).
fn match_slash_something() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\\.").unwrap())
}

/// After stripping quotes, replaces each `\X` (backslash + char) with `X`
/// (faithfully reproducing the questionable-but-established Go/strada behavior).
///
/// # Examples
/// ```
/// use tsgo_stringutil::unquote_string;
/// assert_eq!(unquote_string("\"a\\nb\""), "anb");
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:UnquoteString
pub fn unquote_string(s: &str) -> String {
    let inner = strip_quotes(s);
    match_slash_something()
        .replace_all(inner, |caps: &regex::Captures| caps[0][1..].to_string())
        .into_owned()
}

/// Lowercases the first rune, leaving the rest unchanged.
///
/// # Examples
/// ```
/// use tsgo_stringutil::lower_first_char;
/// assert_eq!(lower_first_char("Foo"), "foo");
/// assert_eq!(lower_first_char(""), "");
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:LowerFirstChar
pub fn lower_first_char(s: &str) -> String {
    let (ch, size) = decode_first_char(s);
    if size > 0 {
        // PERF(port): Go uses unicode.ToLower (simple 1:1 folding); Rust's
        // char::to_lowercase is full folding, so a few code points (e.g.
        // U+0130) may expand to more than one char. Common cases match.
        let lower: String = ch.to_lowercase().collect();
        lower + &s[size..]
    } else {
        s.to_string()
    }
}

/// Truncates `s` to at most `max_length` runes (byte length < max returns as-is;
/// max == 0 returns empty).
///
/// # Examples
/// ```
/// use tsgo_stringutil::truncate_by_runes;
/// assert_eq!(truncate_by_runes("hello", 3), "hel");
/// assert_eq!(truncate_by_runes("hi", 5), "hi");
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/util.go:TruncateByRunes
pub fn truncate_by_runes(s: &str, max_length: usize) -> &str {
    if s.len() < max_length {
        return s;
    }
    if max_length == 0 {
        return "";
    }
    let mut rune_count = 0usize;
    for (i, _) in s.char_indices() {
        rune_count += 1;
        if rune_count > max_length {
            return &s[..i];
        }
    }
    s
}

#[cfg(test)]
#[path = "util_test.rs"]
mod tests;
