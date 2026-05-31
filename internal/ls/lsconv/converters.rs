//! Port of Go `internal/ls/lsconv/converters.go`.
//!
//! Converts between the compiler's internal UTF-8 byte offsets / [`TextRange`]s
//! and the LSP protocol's 0-based `(line, UTF-16 character)` positions, and
//! between internal file names and LSP `DocumentUri`s.

use std::rc::Rc;

use tsgo_bundled::is_bundled;
use tsgo_core::text::{TextPos, TextRange};
use tsgo_lsproto::{DocumentUri, Location, Position, Range};
use tsgo_tspath::{is_dynamic_file_name, split_volume_path};

use crate::linemap::LSPLineMap;

/// A source document the converters operate on: its file name and raw text.
///
/// `text` is bytes (not `&str`) because Go models source as a byte `string`
/// and the position conversions are defined over raw bytes, which may contain
/// invalid UTF-8 (see `TestConvertersInvalidUTF8`).
// Go: internal/ls/lsconv/converters.go:Script
pub trait Script {
    /// The document's file name (an internal, normalized path).
    fn file_name(&self) -> &str;
    /// The document's raw text, as UTF-8 bytes (possibly invalid UTF-8).
    fn text(&self) -> &[u8];
}

/// The encoding used to measure LSP character offsets within a line.
///
/// TODO(port): this type belongs in `tsgo_lsproto`
/// (Go: `internal/lsp/lsproto/lsp_generated.go:PositionEncodingKind`). It is
/// mirrored here as a temporary shim because lsproto has not ported it yet;
/// move it to lsproto and re-export once available.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PositionEncodingKind(pub String);

impl PositionEncodingKind {
    /// Character offsets measured in UTF-8 code units (`"utf-8"`).
    pub fn utf8() -> PositionEncodingKind {
        PositionEncodingKind("utf-8".to_string())
    }
    /// Character offsets measured in UTF-16 code units (`"utf-16"`).
    pub fn utf16() -> PositionEncodingKind {
        PositionEncodingKind("utf-16".to_string())
    }
    /// Character offsets measured in UTF-32 code units (`"utf-32"`).
    pub fn utf32() -> PositionEncodingKind {
        PositionEncodingKind("utf-32".to_string())
    }
}

/// Callback that returns the [`LSPLineMap`] for a given file name.
///
/// Mirrors Go's `getLineMap func(fileName string) *LSPLineMap`; an `Rc` stands
/// in for the shared `*LSPLineMap` pointer.
type GetLineMap = Box<dyn Fn(&str) -> Rc<LSPLineMap>>;

/// Converts between internal byte offsets and LSP positions for a given
/// position encoding, looking up per-file line maps on demand.
// Go: internal/ls/lsconv/converters.go:Converters
pub struct Converters {
    get_line_map: GetLineMap,
    position_encoding: PositionEncodingKind,
}

impl Converters {
    /// Creates a [`Converters`] with the given position `encoding` and a
    /// `get_line_map` callback that returns the line map for a file name.
    ///
    /// Side effects: none (pure); `get_line_map` is invoked lazily on each
    /// conversion.
    // Go: internal/ls/lsconv/converters.go:NewConverters
    pub fn new<F>(encoding: PositionEncodingKind, get_line_map: F) -> Converters
    where
        F: Fn(&str) -> Rc<LSPLineMap> + 'static,
    {
        Converters {
            get_line_map: Box::new(get_line_map),
            position_encoding: encoding,
        }
    }

    /// Converts an internal [`TextRange`] to an LSP [`Range`].
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/lsconv/converters.go:ToLSPRange
    pub fn to_lsp_range(&self, script: &dyn Script, text_range: TextRange) -> Range {
        Range {
            start: self.position_to_line_and_character(script, TextPos(text_range.pos())),
            end: self.position_to_line_and_character(script, TextPos(text_range.end())),
        }
    }

    /// Converts an LSP [`Range`] to an internal [`TextRange`].
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/lsconv/converters.go:FromLSPRange
    pub fn from_lsp_range(&self, script: &dyn Script, range: Range) -> TextRange {
        TextRange::new(
            self.line_and_character_to_position(script, range.start).0,
            self.line_and_character_to_position(script, range.end).0,
        )
    }

    /// Converts an internal [`TextRange`] to an LSP [`Location`] for `script`.
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/lsconv/converters.go:ToLSPLocation
    pub fn to_lsp_location(&self, script: &dyn Script, rng: TextRange) -> Location {
        Location {
            uri: file_name_to_document_uri(script.file_name()),
            range: self.to_lsp_range(script, rng),
        }
    }

    /// Converts a 0-based LSP `(line, character)` to an internal UTF-8 byte
    /// offset.
    ///
    /// The character offset is interpreted in UTF-8 code units when the text is
    /// ASCII-only or the encoding is UTF-8, otherwise in UTF-16 code units
    /// (rescanning the line with a port of Go `utf8.DecodeRuneInString`).
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/lsconv/converters.go:LineAndCharacterToPosition
    pub fn line_and_character_to_position(
        &self,
        script: &dyn Script,
        line_and_character: Position,
    ) -> TextPos {
        let text = script.text();
        let text_len = text.len() as i32;

        let line_map = (self.get_line_map)(script.file_name());

        let line = line_and_character.line as usize;
        let char = line_and_character.character as i32;

        // Clamp line to valid range.
        if line >= line_map.line_starts.len() {
            return TextPos(text_len);
        }

        let start = line_map.line_starts[line].0;

        // Determine the end of this line (start of next line, or end of text).
        let line_end = if line + 1 < line_map.line_starts.len() {
            line_map.line_starts[line + 1].0
        } else {
            text_len
        };

        if line_map.ascii_only || self.position_encoding == PositionEncodingKind::utf8() {
            return TextPos(start.max((start + char).min(line_end)));
        }

        // Scan from line start counting UTF-16 code units to find the byte
        // position. Uses decode_rune (not range + RuneLen) so that invalid
        // UTF-8 bytes advance by their actual size (1) rather than
        // RuneLen(RuneError) == 3.
        let mut utf16_char = 0i32;
        let mut pos = start as usize;
        let end = line_end as usize;
        while pos < end {
            let (r, size) = decode_rune(&text[pos..]);
            let u16_len = r.len_utf16() as i32;
            if utf16_char + u16_len > char {
                break;
            }
            utf16_char += u16_len;
            pos += size;
        }

        TextPos(pos as i32)
    }

    /// Converts an internal UTF-8 byte offset to a 0-based LSP
    /// `(line, character)`.
    ///
    /// The character offset is reported in UTF-8 code units when the text is
    /// ASCII-only or the encoding is UTF-8, otherwise in UTF-16 code units.
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/lsconv/converters.go:PositionToLineAndCharacter
    pub fn position_to_line_and_character(
        &self,
        script: &dyn Script,
        position: TextPos,
    ) -> Position {
        let text = script.text();
        let text_len = text.len() as i32;

        // Clamp position into the valid byte range.
        let position = position.0.clamp(0, text_len);

        let line_map = (self.get_line_map)(script.file_name());

        let (mut line, is_line_start) = match line_map.line_starts.binary_search(&TextPos(position))
        {
            Ok(i) => (i as i64, true),
            Err(i) => (i as i64, false),
        };
        if !is_line_start {
            line -= 1;
        }
        let max_line = line_map.line_starts.len() as i64 - 1;
        let line = line.clamp(0, max_line) as usize;

        // The current line ranges from line_starts[line] to line_starts[line+1]
        // (or len(text)).
        let start = line_map.line_starts[line].0;

        let character =
            if line_map.ascii_only || self.position_encoding == PositionEncodingKind::utf8() {
                position - start
            } else {
                // Rescan the text as UTF-16 to find the character offset.
                let mut c = 0i32;
                let sub = &text[start as usize..position as usize];
                let mut i = 0usize;
                while i < sub.len() {
                    let (r, size) = decode_rune(&sub[i..]);
                    c += r.len_utf16() as i32;
                    i += size;
                }
                c
            };

        Position {
            line: line as u32,
            character: character as u32,
        }
    }
}

/// Converts an internal file name to an LSP `DocumentUri`.
///
/// Bundled paths pass through unchanged; dynamic (`^/scheme/authority/...`)
/// file names are rebuilt as `scheme:path` (for the `ts-nul-authority`) or
/// `scheme://authority/path`; ordinary paths become `file://` URIs with each
/// path segment percent-encoded.
///
/// # Examples
/// ```
/// use tsgo_ls_lsconv::file_name_to_document_uri;
/// assert_eq!(file_name_to_document_uri("/path/to/file.ts").0, "file:///path/to/file.ts");
/// assert_eq!(file_name_to_document_uri("c:/test/me").0, "file:///c%3A/test/me");
/// ```
///
/// # Panics
/// Panics on a malformed dynamic file name (missing `/` separators).
///
/// Side effects: none (pure).
// Go: internal/ls/lsconv/converters.go:FileNameToDocumentURI
pub fn file_name_to_document_uri(file_name: &str) -> DocumentUri {
    if is_bundled(file_name) {
        return DocumentUri(file_name.to_string());
    }
    if is_dynamic_file_name(file_name) {
        let after = &file_name[2..];
        let (scheme, rest) = after
            .split_once('/')
            .unwrap_or_else(|| panic!("invalid file name: {file_name}"));
        let (authority, path) = rest
            .split_once('/')
            .unwrap_or_else(|| panic!("invalid file name: {file_name}"));
        if authority == "ts-nul-authority" {
            return DocumentUri(format!("{scheme}:{path}"));
        }
        return DocumentUri(format!("{scheme}://{authority}/{path}"));
    }

    let (mut volume, rest, _) = split_volume_path(file_name);
    if !volume.is_empty() {
        volume = format!("/{}", extra_escape_replace(&volume));
    }

    let trimmed = rest.strip_prefix("//").unwrap_or(&rest);

    let parts: Vec<String> = trimmed
        .split('/')
        .map(|part| extra_escape_replace(&path_escape(part)))
        .collect();

    DocumentUri(format!("file://{volume}{}", parts.join("/")))
}

const UPPER_HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Reports whether `c` must be percent-escaped within a URL path segment.
///
/// Mirrors Go `net/url.shouldEscape(c, encodePathSegment)`: unreserved
/// characters and the reserved subset `$ & + : = @` are left as-is; everything
/// else (including `/ ; , ?`, controls, and non-ASCII bytes) is escaped.
fn should_escape_path_segment(c: u8) -> bool {
    if c.is_ascii_alphanumeric() {
        return false;
    }
    !matches!(
        c,
        b'-' | b'_' | b'.' | b'~' | b'$' | b'&' | b'+' | b':' | b'=' | b'@'
    )
}

/// Percent-encodes `s` as a single URL path segment.
///
/// Mirrors Go `net/url.PathEscape`, iterating over UTF-8 bytes and emitting
/// `%XX` (uppercase hex) for each byte that [`should_escape_path_segment`].
fn path_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &c in s.as_bytes() {
        if should_escape_path_segment(c) {
            out.push('%');
            out.push(UPPER_HEX[(c >> 4) as usize] as char);
            out.push(UPPER_HEX[(c & 0x0f) as usize] as char);
        } else {
            out.push(c as char);
        }
    }
    out
}

/// Applies the vscode-uri "extra escape" replacements, percent-encoding the
/// path-significant characters that [`path_escape`] leaves intact.
///
/// Mirrors the Go `extraEscapeReplacer` (`strings.NewReplacer`).
fn extra_escape_replace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            ':' => out.push_str("%3A"),
            '/' => out.push_str("%2F"),
            '?' => out.push_str("%3F"),
            '#' => out.push_str("%23"),
            '[' => out.push_str("%5B"),
            ']' => out.push_str("%5D"),
            '@' => out.push_str("%40"),
            '!' => out.push_str("%21"),
            '$' => out.push_str("%24"),
            '&' => out.push_str("%26"),
            '\'' => out.push_str("%27"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '*' => out.push_str("%2A"),
            '+' => out.push_str("%2B"),
            ',' => out.push_str("%2C"),
            ';' => out.push_str("%3B"),
            '=' => out.push_str("%3D"),
            ' ' => out.push_str("%20"),
            other => out.push(other),
        }
    }
    out
}

/// Decodes the first rune of `b`, returning `(char, byte_width)`.
///
/// Mirrors Go `utf8.DecodeRuneInString`: an empty input decodes as the
/// replacement character `U+FFFD` with width 0, and any invalid lead /
/// continuation byte decodes as `U+FFFD` advancing by exactly 1 byte (so
/// `utf16` length of an invalid byte is 1, matching `utf16.RuneLen(RuneError)`).
///
/// Side effects: none (pure).
pub(crate) fn decode_rune(b: &[u8]) -> (char, usize) {
    if b.is_empty() {
        return ('\u{FFFD}', 0);
    }
    let max = b.len().min(4);
    for n in 1..=max {
        if let Ok(s) = std::str::from_utf8(&b[..n]) {
            if let Some(c) = s.chars().next() {
                return (c, n);
            }
        }
    }
    ('\u{FFFD}', 1)
}

#[cfg(test)]
#[path = "converters_test.rs"]
mod tests;
