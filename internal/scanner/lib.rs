//! `tsgo_scanner` — 1:1 Rust port of Go `internal/scanner`.
//!
//! The scanner is the compiler's lexical layer: it turns UTF-8 source text into
//! a stream of tokens (`Kind` + value + `TokenFlags` + `[pos, end)` byte range)
//! and provides the byte <-> UTF-16 <-> (line, character) conversions, trivia
//! skipping, comment scanning, and the family of context-sensitive `re_scan_*`
//! routines that the parser drives.
//!
//! # Position semantics (read this first)
//!
//! Internally every offset (`pos`, `token_start`, `full_start_pos`, `end`) is a
//! **UTF-8 byte** offset, mirroring Go where the source is a byte string. Only
//! when communicating with the outside world (LSP, diagnostics columns,
//! sourcemaps) are offsets converted to **UTF-16** code units. The
//! [`Scanner::contains_non_ascii`] fast-path flag records whether any non-ASCII
//! byte was seen, so consumers can skip the conversion entirely for ASCII text.
//!
//! # Divergences from Go
//!
//! - `rune` (Go `int32` code point) is modeled as `i32`; the single-byte
//!   `char` reader returns `-1` at end-of-file, exactly like Go.
//! - `ScannerState` is `Clone` (not `Copy`): Go embeds a `tokenValue string`
//!   and a `commentDirectives` slice. `token_value` stays a `String`, and the
//!   comment directives live on the [`Scanner`] (rolled back by length on
//!   `rewind`), so the snapshot stays cheap to clone.
//! - The `ast.SourceFile`-based helpers (`GetScannerForSourceFile`,
//!   `GetECMALine*`, comment ranges keyed on `NodeFactory`, ...) and the regular
//!   expression validator are deferred until the `ast` SourceFile surface and
//!   the parser error interface land; see the package `impl.md`.

use std::sync::OnceLock;

use rustc_hash::FxHashMap;
use tsgo_ast::{Kind, TokenFlags};
use tsgo_core::compileroptions::ScriptTarget;
use tsgo_core::languagevariant::LanguageVariant;
use tsgo_core::text::{TextPos, TextRange};
use tsgo_core::Utf16Offset;
use tsgo_diagnostics as diagnostics;
use tsgo_diagnostics::Message;
use tsgo_stringutil as stringutil;

mod utilities;

use utilities::{
    code_point_is_high_surrogate, code_point_is_low_surrogate, encode_surrogate,
    surrogate_pair_to_codepoint, token_is_identifier_or_keyword,
};
pub use utilities::{is_identifier_text, is_intrinsic_jsx_name};

bitflags::bitflags! {
    /// Controls how [`Scanner`] interprets an escape sequence (string literal vs
    /// regular expression mode, error reporting, unicode mode, ...).
    ///
    /// Mirrors Go `EscapeSequenceScanningFlags` (an `int32` bit set).
    ///
    /// # Examples
    /// ```
    /// use tsgo_scanner::EscapeSequenceScanningFlags as F;
    /// assert_eq!(F::REPORT_INVALID_ESCAPE_ERRORS, F::REGULAR_EXPRESSION | F::REPORT_ERRORS);
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/scanner/scanner.go:EscapeSequenceScanningFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct EscapeSequenceScanningFlags: i32 {
        /// Scanning a string literal.
        const STRING = 1 << 0;
        /// Report diagnostics for malformed escapes.
        const REPORT_ERRORS = 1 << 1;
        /// Scanning a regular expression literal.
        const REGULAR_EXPRESSION = 1 << 2;
        /// Annex B (web compatibility) semantics apply.
        const ANNEX_B = 1 << 3;
        /// Any unicode mode (`u` or `v` flag) is active.
        const ANY_UNICODE_MODE = 1 << 4;
        /// Scanning an atom escape (regex character class context).
        const ATOM_ESCAPE = 1 << 5;
        /// Report invalid-escape errors (regex + report errors).
        const REPORT_INVALID_ESCAPE_ERRORS =
            Self::REGULAR_EXPRESSION.bits() | Self::REPORT_ERRORS.bits();
        /// Allow extended `\u{...}` escapes (string + any-unicode mode).
        const ALLOW_EXTENDED_UNICODE_ESCAPE = Self::STRING.bits() | Self::ANY_UNICODE_MODE.bits();
    }
}

/// Formats a code point like Go's `string(rune)`: a valid scalar becomes its
/// UTF-8 character, while a surrogate / out-of-range value becomes U+FFFD.
fn rune_to_string(r: i32) -> String {
    match u32::try_from(r).ok().and_then(char::from_u32) {
        Some(c) => c.to_string(),
        None => '\u{FFFD}'.to_string(),
    }
}

/// Length of a git merge-conflict marker run (`<<<<<<<`), 7 characters.
// Go: internal/scanner/scanner.go:mergeConflictMarkerLength
const MERGE_CONFLICT_MARKER_LENGTH: usize = 7;

/// Converts a single byte / code point sentinel (`-1` for EOF) into a `char`
/// suitable for the ASCII-only `stringutil` predicates. Out-of-range values map
/// to `U+FFFD`, which every predicate rejects (mirroring Go feeding `rune(-1)`
/// or a lone byte to `IsDigit`/`IsLineBreak`/...).
fn rune_char(ch: i32) -> char {
    if (0..=0x10FFFF).contains(&ch) {
        char::from_u32(ch as u32).unwrap_or('\u{FFFD}')
    } else {
        '\u{FFFD}'
    }
}

/// Decodes the last `char` of `s`, or `'\0'` when empty (mirrors Go
/// `utf8.DecodeLastRuneInString` returning `(RuneError, 0)` only for invalid
/// input, which a Rust `&str` never contains).
fn decode_last_char(s: &str) -> char {
    s.chars().next_back().unwrap_or('\0')
}

/// Decodes the last `char` of `s` together with its byte length, or
/// `('\u{FFFD}', 0)` when empty (mirrors Go `utf8.DecodeLastRuneInString`).
fn decode_last_char_with_size(s: &str) -> (char, usize) {
    match s.chars().next_back() {
        Some(c) => (c, c.len_utf8()),
        None => ('\u{FFFD}', 0),
    }
}

/// The kind of an inline compiler directive comment.
///
/// Mirrors Go `ast.CommentDirectiveKind`.
///
/// TODO(port): move to `tsgo_ast` once `ast.CommentDirective` is ported (P2);
/// the `ast` crate currently exposes only a representative node subset.
///
/// Side effects: none (pure value type).
// Go: internal/ast/ast.go:CommentDirectiveKind
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CommentDirectiveKind {
    /// Unrecognized directive.
    #[default]
    Unknown,
    /// `@ts-expect-error`.
    ExpectError,
    /// `@ts-ignore`.
    Ignore,
}

/// A `@ts-ignore` / `@ts-expect-error` directive collected while scanning.
///
/// Mirrors Go `ast.CommentDirective`.
///
/// TODO(port): move to `tsgo_ast` once `ast.CommentDirective` is ported (P2).
///
/// Side effects: none (pure value type).
// Go: internal/ast/ast.go:CommentDirective
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommentDirective {
    /// The comment's source range.
    pub loc: TextRange,
    /// The directive kind.
    pub kind: CommentDirectiveKind,
}

/// Error-reporting callback invoked by the scanner.
///
/// The parser injects this to collect diagnostics. The precise argument-passing
/// interface (Go's variadic `args ...any` for message substitution) is finalized
/// alongside the parser; for now formatting arguments are dropped.
// Go: internal/scanner/scanner.go:ErrorCallback
pub type ErrorCallback = Box<dyn FnMut(&'static Message, i32, i32)>;

/// Returns the 0-based line index containing byte position `pos`, given the
/// 0-based line-start byte offsets `line_starts`.
///
/// Performs a binary search; if `pos` falls between two line starts the lower
/// line is returned.
///
/// # Examples
/// ```
/// use tsgo_core::text::TextPos;
/// use tsgo_scanner::compute_line_of_position;
/// let line_starts = [TextPos(0), TextPos(5), TextPos(9)];
/// assert_eq!(compute_line_of_position(&line_starts, 7), 1);
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:ComputeLineOfPosition
pub fn compute_line_of_position(line_starts: &[TextPos], pos: i32) -> i32 {
    let mut low: i32 = 0;
    let mut high: i32 = line_starts.len() as i32 - 1;
    while low <= high {
        let middle = low + ((high - low) >> 1);
        let value = line_starts[middle as usize].0;
        if value < pos {
            low = middle + 1;
        } else if value > pos {
            high = middle - 1;
        } else {
            return middle;
        }
    }
    low - 1
}

/// Returns the 0-based line index containing byte position `pos` in `text`,
/// using the ECMAScript line-terminator rules (`\r`, `\n`, `\r\n`, U+2028,
/// U+2029).
///
/// Go threads a `SourceFileLike` whose `ECMALineMap()` is cached on the file;
/// the navigation contexts in `tsgo_astnav` / `tsgo_ls_lsutil` carry no cached
/// line map, so this `&str`-based form recomputes the line starts on each call.
///
/// # Examples
/// ```
/// use tsgo_scanner::get_ecma_line_of_position;
/// assert_eq!(get_ecma_line_of_position("a\nb\nc", 0), 0);
/// assert_eq!(get_ecma_line_of_position("a\nb\nc", 2), 1);
/// assert_eq!(get_ecma_line_of_position("a\nb\nc", 4), 2);
/// ```
///
/// Side effects: none (pure).
// PERF(port): Go caches the line map on the source file; this recomputes it.
// Go: internal/scanner/scanner.go:GetECMALineOfPosition
pub fn get_ecma_line_of_position(text: &str, pos: i32) -> i32 {
    let line_starts = tsgo_core::compute_ecma_line_starts(text);
    compute_line_of_position(&line_starts, pos)
}

/// Converts a 0-based `line` and raw byte offset within that line to an absolute
/// byte position.
///
/// # Panics
/// Panics if `line` is out of range, mirroring Go.
///
/// # Examples
/// ```
/// use tsgo_core::text::TextPos;
/// use tsgo_scanner::compute_position_of_line_and_byte_offset;
/// let line_starts = [TextPos(0), TextPos(5), TextPos(9)];
/// assert_eq!(compute_position_of_line_and_byte_offset(&line_starts, 1, 2), 7);
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:ComputePositionOfLineAndByteOffset
pub fn compute_position_of_line_and_byte_offset(
    line_starts: &[TextPos],
    line: i32,
    byte_offset: i32,
) -> i32 {
    if line < 0 || line >= line_starts.len() as i32 {
        panic!(
            "Bad line number. Line: {}, lineStarts.length: {}.",
            line,
            line_starts.len()
        );
    }
    line_starts[line as usize].0 + byte_offset
}

/// Converts a 0-based `line` and UTF-16 code-unit `character` offset back to an
/// absolute byte position, scanning the line so multi-byte characters count
/// correctly.
///
/// When `allow_edits` is true, out-of-range `line`/`character` values are
/// clamped to the text bounds instead of panicking.
///
/// # Panics
/// With `allow_edits == false`, panics on an out-of-range line or a `character`
/// offset past the end of the line.
///
/// # Examples
/// ```
/// use tsgo_core::text::TextPos;
/// use tsgo_core::Utf16Offset;
/// use tsgo_scanner::compute_position_of_line_and_utf16_character;
/// // "a😀b": the emoji is UTF-8 4 bytes / UTF-16 2 code units.
/// let text = "a\u{1F600}b";
/// let line_starts = [TextPos(0)];
/// // character 3 (a=1 + emoji=2) lands on the byte before 'b' (byte 5).
/// assert_eq!(
///     compute_position_of_line_and_utf16_character(&line_starts, 0, Utf16Offset(3), text, false),
///     5
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:ComputePositionOfLineAndUTF16Character
pub fn compute_position_of_line_and_utf16_character(
    line_starts: &[TextPos],
    line: i32,
    character: Utf16Offset,
    text: &str,
    allow_edits: bool,
) -> i32 {
    let mut line = line;
    if line < 0 || line >= line_starts.len() as i32 {
        if allow_edits {
            if line < 0 {
                line = 0;
            } else if line >= line_starts.len() as i32 {
                line = line_starts.len() as i32 - 1;
            }
        } else {
            panic!(
                "Bad line number. Line: {}, lineStarts.length: {}.",
                line,
                line_starts.len()
            );
        }
    }

    let line_start = line_starts[line as usize].0;

    if character.0 > 0 {
        // UTF-16 character offset: scan from line start counting UTF-16 code units.
        let text_len = text.len() as i32;
        let mut line_end = text_len;
        if line + 1 < line_starts.len() as i32 {
            line_end = line_starts[(line + 1) as usize].0;
        }
        let mut utf16_count: i32 = 0;
        let mut pos = line_start;
        while pos < line_end {
            if utf16_count >= character.0 {
                break;
            }
            let r = text[pos as usize..].chars().next().unwrap();
            utf16_count += r.len_utf16() as i32;
            pos += r.len_utf8() as i32;
        }
        if !allow_edits {
            if pos == line_end && utf16_count < character.0 {
                panic!(
                    "Bad UTF-16 character offset. Line: {}, character: {}.",
                    line, character.0
                );
            }
            tsgo_debug::assert(pos <= text_len, None);
            return pos;
        }
        if pos > text_len {
            return text_len;
        }
        return pos;
    }

    // Character is 0: line start position.
    let res = line_start;
    if allow_edits {
        if res > text.len() as i32 {
            return text.len() as i32;
        }
        return res;
    }
    tsgo_debug::assert(res <= text.len() as i32, None);
    res
}

/// Mutable scanning state captured by [`Scanner::mark`] and restored by
/// [`Scanner::rewind`] (the parser's lookahead/speculation primitive).
///
/// Unlike Go's value-semantics `ScannerState` this is `Clone`, not `Copy`: the
/// `token_value` `String` (and the comment-directive rollback handled on the
/// owning [`Scanner`]) preclude `Copy`. Cloning a snapshot is still cheap.
///
/// Side effects: none (pure value type).
// Go: internal/scanner/scanner.go:ScannerState
#[derive(Clone, Debug)]
struct ScannerState {
    /// Current position in text (and ending position of current token).
    pos: i32,
    /// Starting position of current token including preceding whitespace.
    full_start_pos: i32,
    /// Starting position of the non-whitespace part of the current token.
    token_start: i32,
    /// Kind of the current token.
    token: Kind,
    /// Parsed value of the current token.
    token_value: String,
    /// Flags for the current token.
    token_flags: TokenFlags,
    /// Length of `Scanner::comment_directives` at this snapshot (rollback point).
    comment_directives_len: usize,
    /// Leading asterisks to skip when scanning types inside JSDoc (0 outside).
    skip_jsdoc_leading_asterisks: i32,
}

impl Default for ScannerState {
    fn default() -> ScannerState {
        ScannerState {
            pos: 0,
            full_start_pos: 0,
            token_start: 0,
            token: Kind::Unknown,
            token_value: String::new(),
            token_flags: TokenFlags::NONE,
            comment_directives_len: 0,
            skip_jsdoc_leading_asterisks: 0,
        }
    }
}

/// A handwritten lexical state machine over UTF-8 source text.
///
/// Construct with [`Scanner::new`], feed text with [`Scanner::set_text`], then
/// call [`Scanner::scan`] repeatedly to advance through the token stream. The
/// `re_scan_*`/`scan_jsx_*` methods re-interpret the current token under a
/// parser-supplied syntactic context.
///
/// # Examples
/// ```
/// use tsgo_scanner::Scanner;
/// use tsgo_ast::Kind;
/// let mut s = Scanner::new();
/// s.set_text("( )".to_string());
/// assert_eq!(s.scan(), Kind::OpenParenToken);
/// assert_eq!(s.scan(), Kind::CloseParenToken);
/// assert_eq!(s.scan(), Kind::EndOfFile);
/// ```
///
/// Side effects: `scan`/`re_scan_*`/`set_*` mutate scanner state; nothing leaves
/// the scanner (no I/O).
// Go: internal/scanner/scanner.go:Scanner
pub struct Scanner {
    text: String,
    end: i32,
    language_variant: LanguageVariant,
    script_target: ScriptTarget,
    on_error: Option<ErrorCallback>,
    skip_trivia: bool,
    state: ScannerState,
    contains_non_ascii: bool,
    comment_directives: Vec<CommentDirective>,
    number_cache: FxHashMap<String, String>,
    hex_number_cache: FxHashMap<String, String>,
    hex_digit_cache: FxHashMap<String, String>,
}

impl Default for Scanner {
    fn default() -> Scanner {
        Scanner {
            text: String::new(),
            end: 0,
            language_variant: LanguageVariant::Standard,
            script_target: ScriptTarget::None,
            on_error: None,
            skip_trivia: true,
            state: ScannerState::default(),
            contains_non_ascii: false,
            comment_directives: Vec::new(),
            number_cache: FxHashMap::default(),
            hex_number_cache: FxHashMap::default(),
            hex_digit_cache: FxHashMap::default(),
        }
    }
}

impl Scanner {
    /// Creates a scanner with default options (`skip_trivia = true`).
    ///
    /// Side effects: none.
    // Go: internal/scanner/scanner.go:NewScanner
    pub fn new() -> Scanner {
        Scanner::default()
    }

    /// Resets the scanner to defaults while reusing the (cleared) number caches.
    ///
    /// Side effects: clears all scanner state and the cache maps.
    // Go: internal/scanner/scanner.go:Reset
    pub fn reset(&mut self) {
        self.text = String::new();
        self.end = 0;
        self.language_variant = LanguageVariant::Standard;
        self.script_target = ScriptTarget::None;
        self.on_error = None;
        self.skip_trivia = true;
        self.state = ScannerState::default();
        self.contains_non_ascii = false;
        self.comment_directives.clear();
        self.number_cache.clear();
        self.hex_number_cache.clear();
        self.hex_digit_cache.clear();
    }

    /// Returns the source text being scanned.
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.Text
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the kind of the most recently scanned token.
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.Token
    pub fn token(&self) -> Kind {
        self.state.token
    }

    /// Returns the flags of the most recently scanned token.
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.TokenFlags
    pub fn token_flags(&self) -> TokenFlags {
        self.state.token_flags
    }

    /// Returns the start offset of the current token including preceding trivia.
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.TokenFullStart
    pub fn token_full_start(&self) -> i32 {
        self.state.full_start_pos
    }

    /// Returns the start offset of the non-trivia part of the current token.
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.TokenStart
    pub fn token_start(&self) -> i32 {
        self.state.token_start
    }

    /// Returns the end offset (exclusive) of the current token.
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.TokenEnd
    pub fn token_end(&self) -> i32 {
        self.state.pos
    }

    /// Returns the raw source slice spanning the current token.
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.TokenText
    pub fn token_text(&self) -> &str {
        &self.text[self.state.token_start as usize..self.state.pos as usize]
    }

    /// Returns the parsed value of the current token (unescaped string, numeric
    /// value normalized, ...).
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.TokenValue
    pub fn token_value(&self) -> &str {
        &self.state.token_value
    }

    /// Returns the source range `[token_start, token_end)` of the current token.
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.TokenRange
    pub fn token_range(&self) -> TextRange {
        TextRange::new(self.state.token_start, self.state.pos)
    }

    /// Returns the comment directives collected so far.
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.CommentDirectives
    pub fn comment_directives(&self) -> &[CommentDirective] {
        &self.comment_directives
    }

    /// Reports whether any non-ASCII byte has been seen, i.e. whether UTF-8 byte
    /// offsets may diverge from UTF-16 code-unit offsets.
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.ContainsNonASCII
    pub fn contains_non_ascii(&self) -> bool {
        self.contains_non_ascii
    }

    /// Snapshots the current scanning state for later [`Scanner::rewind`].
    ///
    /// Side effects: none (clones state).
    // Go: internal/scanner/scanner.go:Scanner.Mark
    pub fn mark(&self) -> ScannerStateSnapshot {
        ScannerStateSnapshot(self.state.clone())
    }

    /// Restores a state previously captured by [`Scanner::mark`], rolling back any
    /// comment directives appended since the snapshot.
    ///
    /// Side effects: overwrites scanner state and truncates comment directives.
    // Go: internal/scanner/scanner.go:Scanner.Rewind
    pub fn rewind(&mut self, state: ScannerStateSnapshot) {
        self.state = state.0;
        self.comment_directives
            .truncate(self.state.comment_directives_len);
    }

    /// Reports whether the current token had a unicode escape (`\u00a0`).
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.HasUnicodeEscape
    pub fn has_unicode_escape(&self) -> bool {
        self.state.token_flags.contains(TokenFlags::UNICODE_ESCAPE)
    }

    /// Reports whether the current token had an extended unicode escape
    /// (`\u{10ffff}`).
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.HasExtendedUnicodeEscape
    pub fn has_extended_unicode_escape(&self) -> bool {
        self.state
            .token_flags
            .contains(TokenFlags::EXTENDED_UNICODE_ESCAPE)
    }

    /// Reports whether a line break precedes the current token.
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.HasPrecedingLineBreak
    pub fn has_preceding_line_break(&self) -> bool {
        self.state
            .token_flags
            .contains(TokenFlags::PRECEDING_LINE_BREAK)
    }

    /// Reports whether a JSDoc comment precedes the current token.
    ///
    /// Side effects: none (pure).
    // Go: internal/scanner/scanner.go:Scanner.HasPrecedingJSDocComment
    pub fn has_preceding_jsdoc_comment(&self) -> bool {
        self.state
            .token_flags
            .contains(TokenFlags::PRECEDING_JSDOC_COMMENT)
    }

    /// Sets the source text and resets token state.
    ///
    /// Side effects: replaces the text buffer and resets scanning state.
    // Go: internal/scanner/scanner.go:Scanner.SetText
    pub fn set_text(&mut self, text: String) {
        self.end = text.len() as i32;
        self.text = text;
        self.state = ScannerState::default();
        self.comment_directives.clear();
    }

    /// Installs the parser's error-reporting callback.
    ///
    /// Side effects: stores the callback.
    // Go: internal/scanner/scanner.go:Scanner.SetOnError
    pub fn set_on_error(&mut self, callback: Option<ErrorCallback>) {
        self.on_error = callback;
    }

    /// Selects standard vs JSX lexing.
    ///
    /// Side effects: stores the variant.
    // Go: internal/scanner/scanner.go:Scanner.SetLanguageVariant
    pub fn set_language_variant(&mut self, language_variant: LanguageVariant) {
        self.language_variant = language_variant;
    }

    /// Sets the script target (affects regex flag availability checks).
    ///
    /// Side effects: stores the target.
    // Go: internal/scanner/scanner.go:Scanner.SetScriptTarget
    pub fn set_script_target(&mut self, script_target: ScriptTarget) {
        self.script_target = script_target;
    }

    /// Enables or disables trivia tokens (whitespace/comments).
    ///
    /// Side effects: toggles the flag.
    // Go: internal/scanner/scanner.go:Scanner.SetSkipTrivia
    pub fn set_skip_trivia(&mut self, skip: bool) {
        self.skip_trivia = skip;
    }

    /// Increments (or decrements) the JSDoc leading-asterisk skip depth.
    ///
    /// Side effects: adjusts the skip depth.
    // Go: internal/scanner/scanner.go:Scanner.SetSkipJSDocLeadingAsterisks
    pub fn set_skip_jsdoc_leading_asterisks(&mut self, skip: bool) {
        if skip {
            self.state.skip_jsdoc_leading_asterisks += 1;
        } else {
            self.state.skip_jsdoc_leading_asterisks -= 1;
        }
    }

    /// Resets `pos`/`full_start_pos`/`token_start` to `pos`.
    ///
    /// # Panics
    /// Panics if `pos < 0`.
    ///
    /// Side effects: moves the scan position.
    // Go: internal/scanner/scanner.go:Scanner.ResetPos
    pub fn reset_pos(&mut self, pos: i32) {
        if pos < 0 {
            panic!("Cannot reset token state to negative position");
        }
        self.state.pos = pos;
        self.state.full_start_pos = pos;
        self.state.token_start = pos;
    }

    /// Resets the position and clears the current token kind/value/flags.
    ///
    /// Side effects: moves the scan position and clears token state.
    // Go: internal/scanner/scanner.go:Scanner.ResetTokenState
    pub fn reset_token_state(&mut self, pos: i32) {
        self.reset_pos(pos);
        self.state.token = Kind::Unknown;
        self.state.token_value = String::new();
        self.state.token_flags = TokenFlags::NONE;
    }

    /// Returns the effective language version (defaults to latest when unset).
    // Go: internal/scanner/scanner.go:Scanner.languageVersion
    #[allow(dead_code)]
    fn language_version(&self) -> ScriptTarget {
        if self.script_target == ScriptTarget::None {
            ScriptTarget::LATEST
        } else {
            self.script_target
        }
    }

    /// Reports the diagnostic at the current position with zero length.
    // Go: internal/scanner/scanner.go:Scanner.error
    fn error(&mut self, message: &'static Message) {
        let pos = self.state.pos;
        self.error_at(message, pos, 0);
    }

    /// Reports the diagnostic at `pos` with `length`.
    ///
    /// Substitution arguments are intentionally dropped until the parser error
    /// interface lands (see [`ErrorCallback`]).
    // Go: internal/scanner/scanner.go:Scanner.errorAt
    fn error_at(&mut self, message: &'static Message, pos: i32, length: i32) {
        if let Some(cb) = self.on_error.as_mut() {
            cb(message, pos, length);
        }
    }

    /// Returns the byte at the current position as an `i32`, or `-1` at EOF.
    ///
    /// Note: this only decodes a single byte; callers must check it against
    /// `0x80` and fall back to [`Scanner::char_and_size`] for multi-byte input.
    // Go: internal/scanner/scanner.go:Scanner.char
    fn char(&self) -> i32 {
        if self.state.pos < self.end {
            self.text.as_bytes()[self.state.pos as usize] as i32
        } else {
            -1
        }
    }

    /// Returns the byte at `pos + offset` as an `i32`, or `-1` past the end.
    // Go: internal/scanner/scanner.go:Scanner.charAt
    fn char_at(&self, offset: i32) -> i32 {
        if self.state.pos + offset < self.end {
            self.text.as_bytes()[(self.state.pos + offset) as usize] as i32
        } else {
            -1
        }
    }

    /// Decodes the full UTF-8 character at the current position, returning the
    /// character and its byte length (`('\u{FFFD}', 0)` at EOF). Sets
    /// `contains_non_ascii` when a multi-byte character is decoded.
    // Go: internal/scanner/scanner.go:Scanner.charAndSize
    fn char_and_size(&mut self) -> (char, usize) {
        match self.text[self.state.pos as usize..].chars().next() {
            Some(c) => {
                let size = c.len_utf8();
                if size > 1 {
                    self.contains_non_ascii = true;
                }
                (c, size)
            }
            None => ('\u{FFFD}', 0),
        }
    }

    /// Scans the next token, returning its [`Kind`] (also stored as
    /// [`Scanner::token`]).
    ///
    /// Trivia (whitespace, comments) is skipped when `skip_trivia` is set; the
    /// preceding-line-break and JSDoc flags are recorded on the produced token.
    ///
    /// Side effects: advances the scan position and updates token state; may push
    /// comment directives and report diagnostics.
    // Go: internal/scanner/scanner.go:Scanner.Scan
    pub fn scan(&mut self) -> Kind {
        self.state.full_start_pos = self.state.pos;
        self.state.token_flags = TokenFlags::NONE;
        loop {
            self.state.token_start = self.state.pos;
            let ch = self.char();
            if ch < 0 {
                self.state.token = Kind::EndOfFile;
                return self.state.token;
            }
            match ch as u8 {
                b'\t' | 0x0B | 0x0C | b' ' => {
                    self.state.pos += 1;
                    if self.skip_trivia {
                        continue;
                    }
                    loop {
                        let (c, size) = self.char_and_size();
                        if !stringutil::is_white_space_single_line(c) {
                            break;
                        }
                        self.state.pos += size as i32;
                    }
                    self.state.token = Kind::WhitespaceTrivia;
                }
                b'\n' | b'\r' => {
                    self.state.token_flags |= TokenFlags::PRECEDING_LINE_BREAK;
                    if self.skip_trivia {
                        self.state.pos += 1;
                        continue;
                    }
                    if ch == b'\r' as i32 && self.char_at(1) == b'\n' as i32 {
                        self.state.pos += 2;
                    } else {
                        self.state.pos += 1;
                    }
                    self.state.token = Kind::NewLineTrivia;
                }
                b'!' => {
                    if self.char_at(1) == b'=' as i32 {
                        if self.char_at(2) == b'=' as i32 {
                            self.state.pos += 3;
                            self.state.token = Kind::ExclamationEqualsEqualsToken;
                        } else {
                            self.state.pos += 2;
                            self.state.token = Kind::ExclamationEqualsToken;
                        }
                    } else {
                        self.state.pos += 1;
                        self.state.token = Kind::ExclamationToken;
                    }
                }
                b'"' | b'\'' => {
                    self.state.token_value = self.scan_string(false);
                    self.state.token = Kind::StringLiteral;
                }
                b'`' => {
                    self.state.token = self.scan_template_and_set_token_value(false);
                }
                b'%' => {
                    if self.char_at(1) == b'=' as i32 {
                        self.state.pos += 2;
                        self.state.token = Kind::PercentEqualsToken;
                    } else {
                        self.state.pos += 1;
                        self.state.token = Kind::PercentToken;
                    }
                }
                b'&' => {
                    if self.char_at(1) == b'&' as i32 {
                        if self.char_at(2) == b'=' as i32 {
                            self.state.pos += 3;
                            self.state.token = Kind::AmpersandAmpersandEqualsToken;
                        } else {
                            self.state.pos += 2;
                            self.state.token = Kind::AmpersandAmpersandToken;
                        }
                    } else if self.char_at(1) == b'=' as i32 {
                        self.state.pos += 2;
                        self.state.token = Kind::AmpersandEqualsToken;
                    } else {
                        self.state.pos += 1;
                        self.state.token = Kind::AmpersandToken;
                    }
                }
                b'(' => {
                    self.state.pos += 1;
                    self.state.token = Kind::OpenParenToken;
                }
                b')' => {
                    self.state.pos += 1;
                    self.state.token = Kind::CloseParenToken;
                }
                b'*' => {
                    if self.char_at(1) == b'=' as i32 {
                        self.state.pos += 2;
                        self.state.token = Kind::AsteriskEqualsToken;
                    } else if self.char_at(1) == b'*' as i32 {
                        if self.char_at(2) == b'=' as i32 {
                            self.state.pos += 3;
                            self.state.token = Kind::AsteriskAsteriskEqualsToken;
                        } else {
                            self.state.pos += 2;
                            self.state.token = Kind::AsteriskAsteriskToken;
                        }
                    } else {
                        self.state.pos += 1;
                        if self.state.skip_jsdoc_leading_asterisks != 0
                            && !self
                                .state
                                .token_flags
                                .contains(TokenFlags::PRECEDING_JSDOC_LEADING_ASTERISKS)
                            && self
                                .state
                                .token_flags
                                .contains(TokenFlags::PRECEDING_LINE_BREAK)
                        {
                            self.state.token_flags |= TokenFlags::PRECEDING_JSDOC_LEADING_ASTERISKS;
                            continue;
                        }
                        self.state.token = Kind::AsteriskToken;
                    }
                }
                b'+' => {
                    if self.char_at(1) == b'=' as i32 {
                        self.state.pos += 2;
                        self.state.token = Kind::PlusEqualsToken;
                    } else if self.char_at(1) == b'+' as i32 {
                        self.state.pos += 2;
                        self.state.token = Kind::PlusPlusToken;
                    } else {
                        self.state.pos += 1;
                        self.state.token = Kind::PlusToken;
                    }
                }
                b',' => {
                    self.state.pos += 1;
                    self.state.token = Kind::CommaToken;
                }
                b'-' => {
                    if self.char_at(1) == b'=' as i32 {
                        self.state.pos += 2;
                        self.state.token = Kind::MinusEqualsToken;
                    } else if self.char_at(1) == b'-' as i32 {
                        self.state.pos += 2;
                        self.state.token = Kind::MinusMinusToken;
                    } else {
                        self.state.pos += 1;
                        self.state.token = Kind::MinusToken;
                    }
                }
                b'.' => {
                    if stringutil::is_digit(rune_char(self.char_at(1))) {
                        self.state.token = self.scan_number();
                    } else if self.char_at(1) == b'.' as i32 && self.char_at(2) == b'.' as i32 {
                        self.state.pos += 3;
                        self.state.token = Kind::DotDotDotToken;
                    } else {
                        self.state.pos += 1;
                        self.state.token = Kind::DotToken;
                    }
                }
                b'/' => {
                    if self.char_at(1) == b'/' as i32 {
                        self.state.pos += 2;
                        loop {
                            let (c, size) = self.char_and_size();
                            if size == 0 || stringutil::is_line_break(c) {
                                break;
                            }
                            self.state.pos += size as i32;
                        }
                        let start = self.state.token_start;
                        let end = self.state.pos;
                        self.process_comment_directive(start, end, false);
                        if self.skip_trivia {
                            continue;
                        }
                        self.state.token = Kind::SingleLineCommentTrivia;
                        return self.state.token;
                    }
                    if self.char_at(1) == b'*' as i32 {
                        self.state.pos += 2;
                        let is_jsdoc = self.char() == b'*' as i32 && self.char_at(1) != b'/' as i32;
                        let mut comment_closed = false;
                        let mut last_line_start = self.state.token_start;
                        loop {
                            let (c, size) = self.char_and_size();
                            if size == 0 {
                                break;
                            }
                            if c == '*' && self.char_at(1) == b'/' as i32 {
                                self.state.pos += 2;
                                comment_closed = true;
                                break;
                            }
                            self.state.pos += size as i32;
                            if stringutil::is_line_break(c) {
                                last_line_start = self.state.pos;
                                self.state.token_flags |= TokenFlags::PRECEDING_LINE_BREAK;
                            }
                        }
                        if is_jsdoc {
                            self.state.token_flags |= TokenFlags::PRECEDING_JSDOC_COMMENT;
                            let text = self.text
                                [self.state.token_start as usize..self.state.pos as usize]
                                .to_string();
                            self.scan_jsdoc_comment_for_tags(&text);
                        }
                        self.process_comment_directive(last_line_start, self.state.pos, true);
                        if !comment_closed {
                            self.error(&diagnostics::ASTERISK_SLASH_EXPECTED);
                        }
                        if self.skip_trivia {
                            continue;
                        }
                        if !comment_closed {
                            self.state.token_flags |= TokenFlags::UNTERMINATED;
                        }
                        self.state.token = Kind::MultiLineCommentTrivia;
                        return self.state.token;
                    }
                    if self.char_at(1) == b'=' as i32 {
                        self.state.pos += 2;
                        self.state.token = Kind::SlashEqualsToken;
                    } else {
                        self.state.pos += 1;
                        self.state.token = Kind::SlashToken;
                    }
                }
                b'0' => {
                    if self.char_at(1) == b'X' as i32 || self.char_at(1) == b'x' as i32 {
                        let start = self.state.pos;
                        self.state.pos += 2;
                        let mut digits = self.scan_hex_digits(1, true, true);
                        if digits.is_empty() {
                            self.error(&diagnostics::HEXADECIMAL_DIGIT_EXPECTED);
                            digits = "0".to_string();
                        }
                        if let Some(cached) = self.hex_number_cache.get(&digits) {
                            self.state.token_value = cached.clone();
                        } else {
                            let raw_text =
                                self.text[start as usize..self.state.pos as usize].to_string();
                            if let Some(stripped) = raw_text.strip_prefix("0x") {
                                if stripped == digits {
                                    self.state.token_value = raw_text.clone();
                                } else {
                                    self.state.token_value = format!("0x{digits}");
                                }
                            } else {
                                self.state.token_value = format!("0x{digits}");
                            }
                            self.hex_number_cache
                                .insert(digits.clone(), self.state.token_value.clone());
                        }
                        self.state.token_flags |= TokenFlags::HEX_SPECIFIER;
                        self.state.token = self.scan_big_int_suffix();
                    } else if self.char_at(1) == b'B' as i32 || self.char_at(1) == b'b' as i32 {
                        self.state.pos += 2;
                        let mut digits = self.scan_binary_or_octal_digits(2);
                        if digits.is_empty() {
                            self.error(&diagnostics::BINARY_DIGIT_EXPECTED);
                            digits = "0".to_string();
                        }
                        self.state.token_value = format!("0b{digits}");
                        self.state.token_flags |= TokenFlags::BINARY_SPECIFIER;
                        self.state.token = self.scan_big_int_suffix();
                    } else if self.char_at(1) == b'O' as i32 || self.char_at(1) == b'o' as i32 {
                        self.state.pos += 2;
                        let mut digits = self.scan_binary_or_octal_digits(8);
                        if digits.is_empty() {
                            self.error(&diagnostics::OCTAL_DIGIT_EXPECTED);
                            digits = "0".to_string();
                        }
                        self.state.token_value = format!("0o{digits}");
                        self.state.token_flags |= TokenFlags::OCTAL_SPECIFIER;
                        self.state.token = self.scan_big_int_suffix();
                    } else {
                        self.state.token = self.scan_number();
                    }
                }
                b'1'..=b'9' => {
                    self.state.token = self.scan_number();
                }
                b':' => {
                    self.state.pos += 1;
                    self.state.token = Kind::ColonToken;
                }
                b';' => {
                    self.state.pos += 1;
                    self.state.token = Kind::SemicolonToken;
                }
                b'<' => {
                    if is_conflict_marker_trivia(&self.text, self.state.pos) {
                        self.state.pos = scan_conflict_marker_trivia(&self.text, self.state.pos);
                        if self.skip_trivia {
                            continue;
                        }
                        self.state.token = Kind::ConflictMarkerTrivia;
                        return self.state.token;
                    }
                    if self.char_at(1) == b'<' as i32 {
                        if self.char_at(2) == b'=' as i32 {
                            self.state.pos += 3;
                            self.state.token = Kind::LessThanLessThanEqualsToken;
                        } else {
                            self.state.pos += 2;
                            self.state.token = Kind::LessThanLessThanToken;
                        }
                    } else if self.char_at(1) == b'=' as i32 {
                        self.state.pos += 2;
                        self.state.token = Kind::LessThanEqualsToken;
                    } else if self.language_variant == LanguageVariant::Jsx
                        && self.char_at(1) == b'/' as i32
                        && self.char_at(2) != b'*' as i32
                    {
                        self.state.pos += 2;
                        self.state.token = Kind::LessThanSlashToken;
                    } else {
                        self.state.pos += 1;
                        self.state.token = Kind::LessThanToken;
                    }
                }
                b'=' => {
                    if is_conflict_marker_trivia(&self.text, self.state.pos) {
                        self.state.pos = scan_conflict_marker_trivia(&self.text, self.state.pos);
                        if self.skip_trivia {
                            continue;
                        }
                        self.state.token = Kind::ConflictMarkerTrivia;
                        return self.state.token;
                    }
                    if self.char_at(1) == b'=' as i32 {
                        if self.char_at(2) == b'=' as i32 {
                            self.state.pos += 3;
                            self.state.token = Kind::EqualsEqualsEqualsToken;
                        } else {
                            self.state.pos += 2;
                            self.state.token = Kind::EqualsEqualsToken;
                        }
                    } else if self.char_at(1) == b'>' as i32 {
                        self.state.pos += 2;
                        self.state.token = Kind::EqualsGreaterThanToken;
                    } else {
                        self.state.pos += 1;
                        self.state.token = Kind::EqualsToken;
                    }
                }
                b'>' => {
                    if is_conflict_marker_trivia(&self.text, self.state.pos) {
                        self.state.pos = scan_conflict_marker_trivia(&self.text, self.state.pos);
                        if self.skip_trivia {
                            continue;
                        }
                        self.state.token = Kind::ConflictMarkerTrivia;
                        return self.state.token;
                    }
                    self.state.pos += 1;
                    self.state.token = Kind::GreaterThanToken;
                }
                b'?' => {
                    if self.char_at(1) == b'.' as i32
                        && !stringutil::is_digit(rune_char(self.char_at(2)))
                    {
                        self.state.pos += 2;
                        self.state.token = Kind::QuestionDotToken;
                    } else if self.char_at(1) == b'?' as i32 {
                        if self.char_at(2) == b'=' as i32 {
                            self.state.pos += 3;
                            self.state.token = Kind::QuestionQuestionEqualsToken;
                        } else {
                            self.state.pos += 2;
                            self.state.token = Kind::QuestionQuestionToken;
                        }
                    } else {
                        self.state.pos += 1;
                        self.state.token = Kind::QuestionToken;
                    }
                }
                b'[' => {
                    self.state.pos += 1;
                    self.state.token = Kind::OpenBracketToken;
                }
                b']' => {
                    self.state.pos += 1;
                    self.state.token = Kind::CloseBracketToken;
                }
                b'^' => {
                    if self.char_at(1) == b'=' as i32 {
                        self.state.pos += 2;
                        self.state.token = Kind::CaretEqualsToken;
                    } else {
                        self.state.pos += 1;
                        self.state.token = Kind::CaretToken;
                    }
                }
                b'{' => {
                    self.state.pos += 1;
                    self.state.token = Kind::OpenBraceToken;
                }
                b'|' => {
                    if is_conflict_marker_trivia(&self.text, self.state.pos) {
                        self.state.pos = scan_conflict_marker_trivia(&self.text, self.state.pos);
                        if self.skip_trivia {
                            continue;
                        }
                        self.state.token = Kind::ConflictMarkerTrivia;
                        return self.state.token;
                    }
                    if self.char_at(1) == b'|' as i32 {
                        if self.char_at(2) == b'=' as i32 {
                            self.state.pos += 3;
                            self.state.token = Kind::BarBarEqualsToken;
                        } else {
                            self.state.pos += 2;
                            self.state.token = Kind::BarBarToken;
                        }
                    } else if self.char_at(1) == b'=' as i32 {
                        self.state.pos += 2;
                        self.state.token = Kind::BarEqualsToken;
                    } else {
                        self.state.pos += 1;
                        self.state.token = Kind::BarToken;
                    }
                }
                b'}' => {
                    self.state.pos += 1;
                    self.state.token = Kind::CloseBraceToken;
                }
                b'~' => {
                    self.state.pos += 1;
                    self.state.token = Kind::TildeToken;
                }
                b'@' => {
                    self.state.pos += 1;
                    self.state.token = Kind::AtToken;
                }
                b'\\' => {
                    let cp = self.peek_unicode_escape();
                    if cp >= 0 && is_identifier_start(cp) {
                        let escaped = self.scan_unicode_escape(true);
                        let mut value = String::new();
                        if let Some(c) = char::from_u32(escaped as u32) {
                            value.push(c);
                        }
                        value.push_str(&self.scan_identifier_parts());
                        self.state.token_value = value;
                        self.state.token = get_identifier_token(&self.state.token_value);
                    } else {
                        self.scan_invalid_character();
                    }
                }
                b'#' => {
                    if self.char_at(1) == b'!' as i32 {
                        if self.state.pos == 0 {
                            self.state.pos += 2;
                            loop {
                                let (c, size) = self.char_and_size();
                                if size == 0 || stringutil::is_line_break(c) {
                                    break;
                                }
                                self.state.pos += size as i32;
                            }
                            continue;
                        }
                        self.error_at(
                            &diagnostics::X_CAN_ONLY_BE_USED_AT_THE_START_OF_A_FILE,
                            self.state.pos,
                            2,
                        );
                        self.state.pos += 1;
                        self.state.token = Kind::Unknown;
                    } else {
                        if self.char_at(1) == b'\\' as i32 {
                            self.state.pos += 1;
                            let cp = self.peek_unicode_escape();
                            if cp >= 0 && is_identifier_start(cp) {
                                let escaped = self.scan_unicode_escape(true);
                                let mut value = String::from("#");
                                if let Some(c) = char::from_u32(escaped as u32) {
                                    value.push(c);
                                }
                                value.push_str(&self.scan_identifier_parts());
                                self.state.token_value = value;
                                self.state.token = Kind::PrivateIdentifier;
                                return self.state.token;
                            }
                            self.state.pos -= 1;
                        }
                        if !self.scan_identifier(1) {
                            self.error_at(&diagnostics::INVALID_CHARACTER, self.state.pos - 1, 1);
                            self.state.token_value = "#".to_string();
                        }
                        self.state.token = Kind::PrivateIdentifier;
                    }
                }
                _ => {
                    if self.scan_identifier(0) {
                        self.state.token = get_identifier_token(&self.state.token_value);
                        return self.state.token;
                    }
                    let (c, size) = self.char_and_size();
                    if c == '\u{FFFD}' && size == 1 {
                        self.error_at(&diagnostics::FILE_APPEARS_TO_BE_BINARY, 0, 0);
                        self.state.pos = self.text.len() as i32;
                        self.state.token = Kind::NonTextFileMarkerTrivia;
                        return self.state.token;
                    }
                    if stringutil::is_white_space_single_line(c) {
                        self.state.pos += size as i32;
                        if c == '\u{0085}' || self.skip_trivia {
                            continue;
                        }
                        loop {
                            let (c2, size2) = self.char_and_size();
                            if !stringutil::is_white_space_single_line(c2) {
                                break;
                            }
                            self.state.pos += size2 as i32;
                        }
                        self.state.token = Kind::WhitespaceTrivia;
                        return self.state.token;
                    }
                    if stringutil::is_line_break(c) {
                        self.state.token_flags |= TokenFlags::PRECEDING_LINE_BREAK;
                        self.state.pos += size as i32;
                        continue;
                    }
                    self.scan_invalid_character();
                }
            }
            return self.state.token;
        }
    }

    /// Re-interprets a `<<` token as a single `<` (used when the parser needs to
    /// split a left-shift in a type-argument context).
    ///
    /// Side effects: rewinds `pos` and updates the token.
    // Go: internal/scanner/scanner.go:ReScanLessThanToken
    pub fn re_scan_less_than_token(&mut self) -> Kind {
        if self.state.token == Kind::LessThanLessThanToken {
            self.state.pos = self.state.token_start + 1;
            self.state.token = Kind::LessThanToken;
        }
        self.state.token
    }

    /// Re-interprets a `>` token by greedily combining following `>`/`=` into the
    /// longest `>>`, `>>>`, `>=`, `>>=`, `>>>=` token.
    ///
    /// Side effects: advances `pos` and updates the token.
    // Go: internal/scanner/scanner.go:ReScanGreaterThanToken
    pub fn re_scan_greater_than_token(&mut self) -> Kind {
        if self.state.token == Kind::GreaterThanToken {
            self.state.pos = self.state.token_start + 1;
            if self.char() == '>' as i32 {
                if self.char_at(1) == '>' as i32 {
                    if self.char_at(2) == '=' as i32 {
                        self.state.pos += 3;
                        self.state.token = Kind::GreaterThanGreaterThanGreaterThanEqualsToken;
                    } else {
                        self.state.pos += 2;
                        self.state.token = Kind::GreaterThanGreaterThanGreaterThanToken;
                    }
                } else if self.char_at(1) == '=' as i32 {
                    self.state.pos += 2;
                    self.state.token = Kind::GreaterThanGreaterThanEqualsToken;
                } else {
                    self.state.pos += 1;
                    self.state.token = Kind::GreaterThanGreaterThanToken;
                }
            } else if self.char() == '=' as i32 {
                self.state.pos += 1;
                self.state.token = Kind::GreaterThanEqualsToken;
            }
        }
        self.state.token
    }

    /// Re-scans the current position as a template continuation (`}...`{`` or
    /// `}...` `` ` ``), producing `TemplateMiddle`/`TemplateTail`.
    ///
    /// Side effects: rewinds to `token_start` and rescans; updates token/value.
    // Go: internal/scanner/scanner.go:ReScanTemplateToken
    pub fn re_scan_template_token(&mut self, is_tagged_template: bool) -> Kind {
        self.state.pos = self.state.token_start;
        self.state.token = self.scan_template_and_set_token_value(!is_tagged_template);
        self.state.token
    }

    /// Re-interprets a `*=` token as a single `=` (generator/recovery context).
    ///
    /// # Panics
    /// Panics if the current token is not `*=`.
    ///
    /// Side effects: rewinds `pos` and updates the token.
    // Go: internal/scanner/scanner.go:ReScanAsteriskEqualsToken
    pub fn re_scan_asterisk_equals_token(&mut self) -> Kind {
        if self.state.token != Kind::AsteriskEqualsToken {
            panic!("'ReScanAsteriskEqualsToken' should only be called on a '*='");
        }
        self.state.pos = self.state.token_start + 1;
        self.state.token = Kind::EqualsToken;
        self.state.token
    }

    /// Re-scans a `/` or `/=` token as a regular-expression literal, consuming
    /// the body (respecting character classes and escapes) and trailing flags.
    ///
    /// The deep validity checks and flag diagnostics performed by Go's
    /// `regExpParser` are deferred (the validator is ported in P10); this routine
    /// still produces the correct token range and value.
    ///
    /// Side effects: advances `pos`, sets `token_value`/token; may flag
    /// `UNTERMINATED`.
    // Go: internal/scanner/scanner.go:ReScanSlashToken
    pub fn re_scan_slash_token(&mut self, report_errors: bool) -> Kind {
        // DEFER(phase-3-parser / P10): `report_errors` gates the regExpParser-based
        // validity diagnostics, which land with the regex validator in P10.
        // blocked-by: regexp.rs validator + parser error interface not yet ported.
        let _ = report_errors;
        if self.state.token == Kind::SlashToken || self.state.token == Kind::SlashEqualsToken {
            let start_of_reg_exp_body = self.state.token_start + 1;
            let mut p = start_of_reg_exp_body;
            let mut in_escape = false;
            let mut in_character_class = false;
            let bytes = self.text.as_bytes();
            loop {
                if p >= self.end {
                    self.state.token_flags |= TokenFlags::UNTERMINATED;
                    break;
                }
                let ch = bytes[p as usize] as i32;
                if stringutil::is_line_break(rune_char(ch)) {
                    self.state.token_flags |= TokenFlags::UNTERMINATED;
                    break;
                } else if in_escape {
                    in_escape = false;
                } else if ch == '/' as i32 && !in_character_class {
                    break;
                } else if ch == '[' as i32 {
                    in_character_class = true;
                } else if ch == '\\' as i32 {
                    in_escape = true;
                } else if ch == ']' as i32 {
                    in_character_class = false;
                }
                p += 1;
            }
            let end_of_reg_exp_body = p;
            if self.state.token_flags.contains(TokenFlags::UNTERMINATED) {
                // Recover by finding the nearest unbalanced bracket.
                p = start_of_reg_exp_body;
                in_escape = false;
                let mut character_class_depth = 0;
                let mut in_decimal_quantifier = false;
                let mut group_depth = 0;
                while p < end_of_reg_exp_body {
                    let ch = bytes[p as usize] as i32;
                    if in_escape {
                        in_escape = false;
                    } else if ch == '\\' as i32 {
                        in_escape = true;
                    } else if ch == '[' as i32 {
                        character_class_depth += 1;
                    } else if ch == ']' as i32 && character_class_depth != 0 {
                        character_class_depth -= 1;
                    } else if character_class_depth == 0 {
                        if ch == '{' as i32 {
                            in_decimal_quantifier = true;
                        } else if ch == '}' as i32 && in_decimal_quantifier {
                            in_decimal_quantifier = false;
                        } else if !in_decimal_quantifier {
                            if ch == '(' as i32 {
                                group_depth += 1;
                            } else if ch == ')' as i32 && group_depth != 0 {
                                group_depth -= 1;
                            } else if ch == ')' as i32 || ch == ']' as i32 || ch == '}' as i32 {
                                break;
                            }
                        }
                    }
                    p += 1;
                }
                // Trailing whitespace and semicolons are unlikely to be regex.
                while p > start_of_reg_exp_body {
                    let (ch, size) = decode_last_char_with_size(&self.text[..p as usize]);
                    if stringutil::is_white_space_like(ch) || ch == ';' {
                        p -= size as i32;
                    } else {
                        break;
                    }
                }
                self.error_at(
                    &diagnostics::UNTERMINATED_REGULAR_EXPRESSION_LITERAL,
                    self.state.token_start,
                    p - self.state.token_start,
                );
            } else {
                // Consume the slash, then any trailing identifier-part flags.
                p += 1;
                while p < self.end {
                    let (c, size) = next_char(&self.text, p);
                    if size == 0 || !is_identifier_part(c as i32) {
                        break;
                    }
                    p += size as i32;
                }
            }
            self.state.pos = p;
            self.state.token_value =
                self.text[self.state.token_start as usize..self.state.pos as usize].to_string();
            self.state.token = Kind::RegularExpressionLiteral;
        }
        self.state.token
    }

    /// Re-scans a JSX text/element token starting from the previous token's full
    /// start (used after the parser commits to JSX context).
    ///
    /// Side effects: rewinds to `full_start_pos` and rescans.
    // Go: internal/scanner/scanner.go:ReScanJsxToken
    pub fn re_scan_jsx_token(&mut self, allow_multiline_jsx_text: bool) -> Kind {
        self.state.pos = self.state.full_start_pos;
        self.state.token_start = self.state.full_start_pos;
        self.state.token = self.scan_jsx_token_ex(allow_multiline_jsx_text);
        self.state.token
    }

    /// Re-interprets a `#name` private identifier as a bare `#` token.
    ///
    /// Side effects: rewinds `pos` and updates the token.
    // Go: internal/scanner/scanner.go:ReScanHashToken
    pub fn re_scan_hash_token(&mut self) -> Kind {
        if self.state.token == Kind::PrivateIdentifier {
            self.state.pos = self.state.token_start + 1;
            self.state.token = Kind::HashToken;
        }
        self.state.token
    }

    /// Re-interprets a `??` token as a single `?`.
    ///
    /// # Panics
    /// Panics if the current token is not `??`.
    ///
    /// Side effects: rewinds `pos` and updates the token.
    // Go: internal/scanner/scanner.go:ReScanQuestionToken
    pub fn re_scan_question_token(&mut self) -> Kind {
        if self.state.token != Kind::QuestionQuestionToken {
            panic!("'reScanQuestionToken' should only be called on a '??'");
        }
        self.state.pos = self.state.token_start + 1;
        self.state.token = Kind::QuestionToken;
        self.state.token
    }

    /// Scans a single JSX token (allowing multiline `JsxText`).
    ///
    /// Side effects: advances `pos` and updates token state.
    // Go: internal/scanner/scanner.go:ScanJsxToken
    pub fn scan_jsx_token(&mut self) -> Kind {
        self.scan_jsx_token_ex(true)
    }

    /// Scans a single JSX token: `<`, `</`, `{`, or a run of `JsxText`.
    ///
    /// Side effects: advances `pos`/updates token state; may report diagnostics
    /// for stray `>`/`}`.
    // Go: internal/scanner/scanner.go:ScanJsxTokenEx
    pub fn scan_jsx_token_ex(&mut self, allow_multiline_jsx_text: bool) -> Kind {
        self.state.full_start_pos = self.state.pos;
        self.state.token_start = self.state.pos;
        let ch = self.char();
        if ch < 0 {
            self.state.token = Kind::EndOfFile;
        } else if ch == '<' as i32 {
            if self.char_at(1) == '/' as i32 {
                self.state.pos += 2;
                self.state.token = Kind::LessThanSlashToken;
            } else {
                self.state.pos += 1;
                self.state.token = Kind::LessThanToken;
            }
        } else if ch == '{' as i32 {
            self.state.pos += 1;
            self.state.token = Kind::OpenBraceToken;
        } else {
            // `first_non_whitespace == 0` means only whitespace seen so far.
            let mut first_non_whitespace: i32 = 0;
            loop {
                let (c, size) = self.char_and_size();
                if size == 0 || c == '{' {
                    break;
                }
                if c == '<' {
                    if is_conflict_marker_trivia(&self.text, self.state.pos) {
                        self.state.pos = scan_conflict_marker_trivia(&self.text, self.state.pos);
                        self.state.token = Kind::ConflictMarkerTrivia;
                        return self.state.token;
                    }
                    break;
                }
                if c == '>' {
                    self.error_at(
                        &diagnostics::UNEXPECTED_TOKEN_DID_YOU_MEAN_OR_GT,
                        self.state.pos,
                        1,
                    );
                } else if c == '}' {
                    self.error_at(
                        &diagnostics::UNEXPECTED_TOKEN_DID_YOU_MEAN_OR_RBRACE,
                        self.state.pos,
                        1,
                    );
                }
                if stringutil::is_line_break(c) && first_non_whitespace == 0 {
                    first_non_whitespace = -1;
                } else if !allow_multiline_jsx_text
                    && stringutil::is_line_break(c)
                    && first_non_whitespace > 0
                {
                    break;
                } else if !stringutil::is_white_space_like(c) {
                    first_non_whitespace = self.state.pos;
                }
                self.state.pos += size as i32;
            }
            self.state.token_value =
                self.text[self.state.full_start_pos as usize..self.state.pos as usize].to_string();
            self.state.token = if first_non_whitespace == -1 {
                Kind::JsxTextAllWhiteSpaces
            } else {
                Kind::JsxText
            };
        }
        self.state.token
    }

    /// Extends an already-scanned identifier/keyword into a JSX identifier,
    /// absorbing `-` (and unicode-escape parts).
    ///
    /// Side effects: advances `pos` and rewrites `token_value`/token in place.
    // Go: internal/scanner/scanner.go:ScanJsxIdentifier
    pub fn scan_jsx_identifier(&mut self) -> Kind {
        if token_is_identifier_or_keyword(self.state.token) {
            loop {
                let ch = self.char();
                if ch < 0 {
                    break;
                }
                if ch == '-' as i32 {
                    self.state.token_value.push('-');
                    self.state.pos += 1;
                    continue;
                }
                let old_pos = self.state.pos;
                let parts = self.scan_identifier_parts();
                self.state.token_value.push_str(&parts);
                if self.state.pos == old_pos {
                    break;
                }
            }
            self.state.token = get_identifier_token(&self.state.token_value);
        }
        self.state.token
    }

    /// Scans a JSX attribute value: a quoted string literal, or (delegating to
    /// [`Scanner::scan`]) `{` to begin an expression.
    ///
    /// Side effects: skips whitespace then advances `pos`/updates token state.
    // Go: internal/scanner/scanner.go:ScanJsxAttributeValue
    pub fn scan_jsx_attribute_value(&mut self) -> Kind {
        self.state.full_start_pos = self.state.pos;
        // Skip whitespace between '=' and the value.
        loop {
            let (c, size) = self.char_and_size();
            if size == 0 || !stringutil::is_white_space_like(c) {
                break;
            }
            self.state.pos += size as i32;
        }
        self.state.token_start = self.state.pos;
        let ch = self.char();
        if ch == '"' as i32 || ch == '\'' as i32 {
            self.state.token_value = self.scan_string(true);
            self.state.token = Kind::StringLiteral;
            self.state.token
        } else {
            self.scan()
        }
    }

    /// Re-scans the JSX attribute value from the previous token's full start.
    ///
    /// Side effects: rewinds to `full_start_pos` and rescans.
    // Go: internal/scanner/scanner.go:ReScanJsxAttributeValue
    pub fn re_scan_jsx_attribute_value(&mut self) -> Kind {
        self.state.pos = self.state.full_start_pos;
        self.state.token_start = self.state.full_start_pos;
        self.scan_jsx_attribute_value()
    }

    // ----- leaf sub-scanners (filled in subsequent TDD slices) -----

    // Go: internal/scanner/scanner.go:Scanner.processCommentDirective
    fn process_comment_directive(&mut self, start: i32, end: i32, multiline: bool) {
        let bytes = self.text.as_bytes();
        let end_u = end as usize;
        let mut pos = start as usize;
        if multiline {
            // Skip whitespace, then any combination of '/' and '*'.
            while pos < end_u && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
                pos += 1;
            }
            while pos < end_u && (bytes[pos] == b'/' || bytes[pos] == b'*') {
                pos += 1;
            }
        } else {
            // Skip the opening '//' and any extra '/'.
            pos += 2;
            while pos < end_u && bytes[pos] == b'/' {
                pos += 1;
            }
        }
        while pos < end_u && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
            pos += 1;
        }
        if !(pos < end_u && bytes[pos] == b'@') {
            return;
        }
        pos += 1;
        let kind = {
            let rest = &self.text[pos..];
            if rest.starts_with("ts-expect-error") {
                CommentDirectiveKind::ExpectError
            } else if rest.starts_with("ts-ignore") {
                CommentDirectiveKind::Ignore
            } else {
                return;
            }
        };
        self.comment_directives.push(CommentDirective {
            loc: TextRange::new(start, end),
            kind,
        });
        self.state.comment_directives_len = self.comment_directives.len();
    }

    // Go: internal/scanner/scanner.go:Scanner.scanJSDocCommentForTags
    fn scan_jsdoc_comment_for_tags(&mut self, comment_text: &str) {
        let mut text = comment_text;
        loop {
            let i = match text.find('@') {
                Some(i) => i,
                None => return,
            };
            text = &text[i + 1..];
            if !self
                .state
                .token_flags
                .contains(TokenFlags::PRECEDING_JSDOC_WITH_DEPRECATED)
                && has_jsdoc_tag(text, &["deprecated"])
            {
                self.state.token_flags |= TokenFlags::PRECEDING_JSDOC_WITH_DEPRECATED;
            }
            if !self
                .state
                .token_flags
                .contains(TokenFlags::PRECEDING_JSDOC_WITH_SEE_OR_LINK)
                && has_jsdoc_tag(text, &["see", "link", "linkcode", "linkplain"])
            {
                self.state.token_flags |= TokenFlags::PRECEDING_JSDOC_WITH_SEE_OR_LINK;
            }
            if self
                .state
                .token_flags
                .contains(TokenFlags::PRECEDING_JSDOC_WITH_DEPRECATED)
                && self
                    .state
                    .token_flags
                    .contains(TokenFlags::PRECEDING_JSDOC_WITH_SEE_OR_LINK)
            {
                return;
            }
        }
    }

    // Go: internal/scanner/scanner.go:Scanner.scanIdentifier
    fn scan_identifier(&mut self, prefix_length: i32) -> bool {
        let start = self.state.pos;
        self.state.pos += prefix_length;
        let mut ch = self.char();
        // Fast path for simple ASCII identifiers.
        if stringutil::is_ascii_letter(rune_char(ch)) || ch == '_' as i32 || ch == '$' as i32 {
            loop {
                self.state.pos += 1;
                ch = self.char();
                if !(is_word_character(ch) || ch == '$' as i32) {
                    break;
                }
            }
            if ch < 0x80 && ch != '\\' as i32 {
                self.state.token_value =
                    self.text[start as usize..self.state.pos as usize].to_string();
                return true;
            }
            self.state.pos = start + prefix_length;
        }
        let (mut c, mut size) = self.char_and_size();
        if is_identifier_start(c as i32) {
            loop {
                self.state.pos += size as i32;
                let (c2, s2) = self.char_and_size();
                c = c2;
                size = s2;
                if !is_identifier_part(c as i32) {
                    break;
                }
            }
            self.state.token_value = self.text[start as usize..self.state.pos as usize].to_string();
            if c == '\\' {
                let rest = self.scan_identifier_parts();
                self.state.token_value.push_str(&rest);
            }
            return true;
        }
        false
    }

    // Go: internal/scanner/scanner.go:Scanner.scanIdentifierParts
    fn scan_identifier_parts(&mut self) -> String {
        let mut sb = String::new();
        let mut start = self.state.pos;
        loop {
            let (ch, size) = self.char_and_size();
            if is_identifier_part(ch as i32) {
                self.state.pos += size as i32;
                continue;
            }
            if ch == '\\' {
                let escaped = self.peek_unicode_escape();
                if escaped >= 0 && is_identifier_part(escaped) {
                    sb.push_str(&self.text[start as usize..self.state.pos as usize]);
                    let u = self.scan_unicode_escape(true);
                    sb.push_str(&rune_to_string(u));
                    start = self.state.pos;
                    continue;
                }
            }
            break;
        }
        sb.push_str(&self.text[start as usize..self.state.pos as usize]);
        sb
    }

    // Go: internal/scanner/scanner.go:Scanner.scanString
    fn scan_string(&mut self, jsx_attribute_string: bool) -> String {
        let quote = self.char();
        if quote == '\'' as i32 {
            self.state.token_flags |= TokenFlags::SINGLE_QUOTE;
        }
        self.state.pos += 1;
        let quote_char = rune_char(quote);
        // Fast path for simple strings without escape sequences.
        let rest_start = self.state.pos as usize;
        if let Some(n) = self.text[rest_start..].find(quote_char) {
            if n == 0 {
                self.state.pos += 1;
                return String::new();
            }
            let str_slice = &self.text[rest_start..rest_start + n];
            if !jsx_attribute_string
                && !str_slice
                    .bytes()
                    .any(|b| b == b'\r' || b == b'\n' || b == b'\\')
            {
                let result = str_slice.to_string();
                self.state.pos += n as i32 + 1;
                return result;
            }
        }
        let mut sb = String::new();
        let mut start = self.state.pos;
        loop {
            let ch = self.char();
            if ch < 0 {
                sb.push_str(&self.text[start as usize..self.state.pos as usize]);
                self.state.token_flags |= TokenFlags::UNTERMINATED;
                self.error(&diagnostics::UNTERMINATED_STRING_LITERAL);
                break;
            }
            if ch == quote {
                sb.push_str(&self.text[start as usize..self.state.pos as usize]);
                self.state.pos += 1;
                break;
            }
            if ch == '\\' as i32 && !jsx_attribute_string {
                sb.push_str(&self.text[start as usize..self.state.pos as usize]);
                let esc = self.scan_escape_sequence(
                    EscapeSequenceScanningFlags::STRING
                        | EscapeSequenceScanningFlags::REPORT_ERRORS,
                );
                sb.push_str(&esc);
                start = self.state.pos;
                continue;
            }
            if (ch == '\n' as i32 || ch == '\r' as i32) && !jsx_attribute_string {
                sb.push_str(&self.text[start as usize..self.state.pos as usize]);
                self.state.token_flags |= TokenFlags::UNTERMINATED;
                self.error(&diagnostics::UNTERMINATED_STRING_LITERAL);
                break;
            }
            self.state.pos += 1;
        }
        sb
    }

    // Go: internal/scanner/scanner.go:Scanner.scanTemplateAndSetTokenValue
    fn scan_template_and_set_token_value(
        &mut self,
        should_emit_invalid_escape_error: bool,
    ) -> Kind {
        let started_with_backtick = self.char() == '`' as i32;
        self.state.pos += 1;
        let mut start = self.state.pos;
        let mut parts: Vec<String> = Vec::with_capacity(4);
        let token;
        loop {
            let ch = self.char();
            if ch < 0 || ch == '`' as i32 {
                parts.push(self.text[start as usize..self.state.pos as usize].to_string());
                if ch == '`' as i32 {
                    self.state.pos += 1;
                } else {
                    self.state.token_flags |= TokenFlags::UNTERMINATED;
                    self.error(&diagnostics::UNTERMINATED_TEMPLATE_LITERAL);
                }
                token = if started_with_backtick {
                    Kind::NoSubstitutionTemplateLiteral
                } else {
                    Kind::TemplateTail
                };
                break;
            }
            if ch == '$' as i32 && self.char_at(1) == '{' as i32 {
                parts.push(self.text[start as usize..self.state.pos as usize].to_string());
                self.state.pos += 2;
                token = if started_with_backtick {
                    Kind::TemplateHead
                } else {
                    Kind::TemplateMiddle
                };
                break;
            }
            if ch == '\\' as i32 {
                parts.push(self.text[start as usize..self.state.pos as usize].to_string());
                let report = if should_emit_invalid_escape_error {
                    EscapeSequenceScanningFlags::REPORT_ERRORS
                } else {
                    EscapeSequenceScanningFlags::empty()
                };
                let esc = self.scan_escape_sequence(EscapeSequenceScanningFlags::STRING | report);
                parts.push(esc);
                start = self.state.pos;
                continue;
            }
            // <CR><LF> and <CR> are normalized to <LF> for template values.
            if ch == '\r' as i32 {
                parts.push(self.text[start as usize..self.state.pos as usize].to_string());
                self.state.pos += 1;
                if self.char() == '\n' as i32 {
                    self.state.pos += 1;
                }
                parts.push("\n".to_string());
                start = self.state.pos;
                continue;
            }
            self.state.pos += 1;
        }
        self.state.token_value = parts.concat();
        token
    }

    // Go: internal/scanner/scanner.go:Scanner.scanEscapeSequence
    fn scan_escape_sequence(&mut self, flags: EscapeSequenceScanningFlags) -> String {
        let start = self.state.pos;
        self.state.pos += 1;
        let ch = self.char();
        if ch < 0 {
            self.error(&diagnostics::UNEXPECTED_END_OF_TEXT);
            return String::new();
        }
        self.state.pos += 1;
        match ch as u8 {
            c0 @ (b'0'..=b'7') => {
                // '\0' not followed by a digit is the NUL escape.
                if c0 == b'0' && !stringutil::is_digit(rune_char(self.char())) {
                    return "\u{0}".to_string();
                }
                // '0'..='3' may consume a second octal digit (Go fallthrough into
                // the '1'..'3' arm); all of '0'..='7' then attempt one more.
                if matches!(c0, b'0' | b'1' | b'2' | b'3')
                    && stringutil::is_octal_digit(rune_char(self.char()))
                {
                    self.state.pos += 1;
                }
                if stringutil::is_octal_digit(rune_char(self.char())) {
                    self.state.pos += 1;
                }
                self.state.token_flags |= TokenFlags::CONTAINS_INVALID_ESCAPE;
                if flags.contains(EscapeSequenceScanningFlags::REPORT_INVALID_ESCAPE_ERRORS) {
                    let code = i64::from_str_radix(
                        &self.text[(start + 1) as usize..self.state.pos as usize],
                        8,
                    )
                    .unwrap_or(0);
                    if flags.contains(EscapeSequenceScanningFlags::REGULAR_EXPRESSION)
                        && !flags.contains(EscapeSequenceScanningFlags::ATOM_ESCAPE)
                        && ch != '0' as i32
                    {
                        self.error_at(
                            &diagnostics::OCTAL_ESCAPE_SEQUENCES_AND_BACKREFERENCES_ARE_NOT_ALLOWED_IN_A_CHARACTER_CLASS_IF_THIS_WAS_INTENDED_AS_AN_ESCAPE_SEQUENCE_USE_THE_SYNTAX_0_INSTEAD,
                            start,
                            self.state.pos - start,
                        );
                    } else {
                        self.error_at(
                            &diagnostics::OCTAL_ESCAPE_SEQUENCES_ARE_NOT_ALLOWED_USE_THE_SYNTAX_0,
                            start,
                            self.state.pos - start,
                        );
                    }
                    return rune_to_string(code as i32);
                }
                self.text[start as usize..self.state.pos as usize].to_string()
            }
            b'8' | b'9' => {
                self.state.token_flags |= TokenFlags::CONTAINS_INVALID_ESCAPE;
                if flags.contains(EscapeSequenceScanningFlags::REPORT_INVALID_ESCAPE_ERRORS) {
                    if flags.contains(EscapeSequenceScanningFlags::REGULAR_EXPRESSION)
                        && !flags.contains(EscapeSequenceScanningFlags::ATOM_ESCAPE)
                    {
                        self.error_at(
                            &diagnostics::DECIMAL_ESCAPE_SEQUENCES_AND_BACKREFERENCES_ARE_NOT_ALLOWED_IN_A_CHARACTER_CLASS,
                            start,
                            self.state.pos - start,
                        );
                    } else {
                        self.error_at(
                            &diagnostics::ESCAPE_SEQUENCE_0_IS_NOT_ALLOWED,
                            start,
                            self.state.pos - start,
                        );
                    }
                    return rune_to_string(ch);
                }
                self.text[start as usize..self.state.pos as usize].to_string()
            }
            b'b' => "\u{8}".to_string(),
            b't' => "\t".to_string(),
            b'n' => "\n".to_string(),
            b'v' => "\u{B}".to_string(),
            b'f' => "\u{C}".to_string(),
            b'r' => "\r".to_string(),
            b'\'' => "'".to_string(),
            b'"' => "\"".to_string(),
            b'u' => {
                let extended = self.char() == '{' as i32;
                self.state.pos -= 2;
                let code_point = self.scan_unicode_escape(
                    flags.contains(EscapeSequenceScanningFlags::REPORT_INVALID_ESCAPE_ERRORS),
                );
                if extended {
                    if !flags.contains(EscapeSequenceScanningFlags::ALLOW_EXTENDED_UNICODE_ESCAPE) {
                        self.state.token_flags |= TokenFlags::CONTAINS_INVALID_ESCAPE;
                        if flags.contains(EscapeSequenceScanningFlags::REPORT_INVALID_ESCAPE_ERRORS)
                        {
                            self.error_at(
                                &diagnostics::UNICODE_ESCAPE_SEQUENCES_ARE_ONLY_AVAILABLE_WHEN_THE_UNICODE_U_FLAG_OR_THE_UNICODE_SETS_V_FLAG_IS_SET,
                                start,
                                self.state.pos - start,
                            );
                        }
                    }
                    if code_point < 0 {
                        return self.text[start as usize..self.state.pos as usize].to_string();
                    }
                    return rune_to_string(code_point);
                }
                if code_point < 0 {
                    return self.text[start as usize..self.state.pos as usize].to_string();
                } else if code_point_is_high_surrogate(code_point)
                    && (!flags.contains(EscapeSequenceScanningFlags::REGULAR_EXPRESSION)
                        || flags.contains(EscapeSequenceScanningFlags::ANY_UNICODE_MODE))
                    && self.char() == '\\' as i32
                    && self.char_at(1) == 'u' as i32
                    && self.char_at(2) != '{' as i32
                {
                    let saved_pos = self.state.pos;
                    let next_code_point = self.scan_unicode_escape(
                        flags.contains(EscapeSequenceScanningFlags::REPORT_INVALID_ESCAPE_ERRORS),
                    );
                    if code_point_is_low_surrogate(next_code_point) {
                        return rune_to_string(surrogate_pair_to_codepoint(
                            code_point,
                            next_code_point,
                        ));
                    }
                    self.state.pos = saved_pos;
                    if flags.contains(EscapeSequenceScanningFlags::REGULAR_EXPRESSION) {
                        return encode_surrogate(code_point);
                    }
                } else if (code_point_is_high_surrogate(code_point)
                    || code_point_is_low_surrogate(code_point))
                    && flags.contains(EscapeSequenceScanningFlags::REGULAR_EXPRESSION)
                {
                    return encode_surrogate(code_point);
                }
                rune_to_string(code_point)
            }
            b'x' => {
                while self.state.pos < start + 4 {
                    if !stringutil::is_hex_digit(rune_char(self.char())) {
                        self.state.token_flags |= TokenFlags::CONTAINS_INVALID_ESCAPE;
                        if flags.contains(EscapeSequenceScanningFlags::REPORT_INVALID_ESCAPE_ERRORS)
                        {
                            self.error(&diagnostics::HEXADECIMAL_DIGIT_EXPECTED);
                        }
                        return self.text[start as usize..self.state.pos as usize].to_string();
                    }
                    self.state.pos += 1;
                }
                self.state.token_flags |= TokenFlags::HEX_ESCAPE;
                let escaped_value = i64::from_str_radix(
                    &self.text[(start + 2) as usize..self.state.pos as usize],
                    16,
                )
                .unwrap_or(0);
                rune_to_string(escaped_value as i32)
            }
            b'\r' => {
                // Line continuation: a backslash + line terminator is the empty
                // code-unit sequence (consume an immediately following '\n').
                if self.char() == '\n' as i32 {
                    self.state.pos += 1;
                }
                String::new()
            }
            b'\n' => String::new(),
            _ => {
                // `ch` was read as a single byte; decode the full rune for
                // multi-byte characters and advance past all its bytes.
                let mut ch_full = ch;
                if ch >= 0x80 {
                    self.state.pos -= 1;
                    let (c, size) = self.char_and_size();
                    ch_full = c as i32;
                    self.state.pos += size as i32;
                    self.contains_non_ascii = true;
                }
                // LineContinuation for U+2028 / U+2029.
                if ch_full == 0x2028 || ch_full == 0x2029 {
                    return String::new();
                }
                if flags.contains(EscapeSequenceScanningFlags::ANY_UNICODE_MODE)
                    || (flags.contains(EscapeSequenceScanningFlags::REGULAR_EXPRESSION)
                        && !flags.contains(EscapeSequenceScanningFlags::ANNEX_B)
                        && is_identifier_part(ch_full))
                {
                    self.error_at(
                        &diagnostics::THIS_CHARACTER_CANNOT_BE_ESCAPED_IN_A_REGULAR_EXPRESSION,
                        start,
                        self.state.pos - start,
                    );
                }
                rune_to_string(ch_full)
            }
        }
    }

    // Go: internal/scanner/scanner.go:Scanner.scanUnicodeEscape
    fn scan_unicode_escape(&mut self, should_emit_invalid_escape_error: bool) -> i32 {
        self.state.pos += 2;
        let start = self.state.pos;
        let extended = self.char() == '{' as i32;
        let hex_digits = if extended {
            self.state.pos += 1;
            self.scan_hex_digits(1, true, false)
        } else {
            self.state.token_flags |= TokenFlags::UNICODE_ESCAPE;
            self.scan_hex_digits(4, false, false)
        };
        if hex_digits.is_empty() {
            self.state.token_flags |= TokenFlags::CONTAINS_INVALID_ESCAPE;
            if should_emit_invalid_escape_error {
                self.error(&diagnostics::HEXADECIMAL_DIGIT_EXPECTED);
            }
            return -1;
        }
        let hex_value = i64::from_str_radix(&hex_digits, 16).unwrap_or(i64::MAX);
        if extended {
            let mut is_invalid_extended_escape = false;
            if hex_value > 0x10FFFF {
                if should_emit_invalid_escape_error {
                    self.error_at(
                        &diagnostics::AN_EXTENDED_UNICODE_ESCAPE_VALUE_MUST_BE_BETWEEN_0X0_AND_0X10FFFF_INCLUSIVE,
                        start + 1,
                        self.state.pos - start - 1,
                    );
                }
                is_invalid_extended_escape = true;
            }
            if self.state.pos >= self.end {
                if should_emit_invalid_escape_error {
                    self.error(&diagnostics::UNEXPECTED_END_OF_TEXT);
                }
                is_invalid_extended_escape = true;
            } else if self.char() == '}' as i32 {
                self.state.pos += 1;
            } else {
                if should_emit_invalid_escape_error {
                    self.error(&diagnostics::UNTERMINATED_UNICODE_ESCAPE_SEQUENCE);
                }
                is_invalid_extended_escape = true;
            }
            if is_invalid_extended_escape {
                self.state.token_flags |= TokenFlags::CONTAINS_INVALID_ESCAPE;
                return -1;
            }
            self.state.token_flags |= TokenFlags::EXTENDED_UNICODE_ESCAPE;
        }
        hex_value as i32
    }

    // Go: internal/scanner/scanner.go:Scanner.peekUnicodeEscape
    fn peek_unicode_escape(&mut self) -> i32 {
        if self.char_at(1) == 'u' as i32 {
            let save_pos = self.state.pos;
            let save_token_flags = self.state.token_flags;
            let code_point = self.scan_unicode_escape(false);
            self.state.pos = save_pos;
            self.state.token_flags = save_token_flags;
            return code_point;
        }
        -1
    }

    // Go: internal/scanner/scanner.go:Scanner.scanNumber
    fn scan_number(&mut self) -> Kind {
        let start = self.state.pos;
        let fixed_part: String;
        if self.char() == '0' as i32 {
            self.state.pos += 1;
            if self.char() == '_' as i32 {
                self.state.token_flags |=
                    TokenFlags::CONTAINS_SEPARATOR | TokenFlags::CONTAINS_INVALID_SEPARATOR;
                self.error_at(
                    &diagnostics::NUMERIC_SEPARATORS_ARE_NOT_ALLOWED_HERE,
                    self.state.pos,
                    1,
                );
                self.state.pos = start;
                fixed_part = self.scan_number_fragment();
            } else {
                let (digits, is_octal) = self.scan_digits();
                if digits.is_empty() {
                    fixed_part = "0".to_string();
                } else if !is_octal {
                    self.state.token_flags |= TokenFlags::CONTAINS_LEADING_ZERO;
                    fixed_part = digits;
                } else {
                    let val = i64::from_str_radix(&digits, 8).unwrap_or(0);
                    self.state.token_value = val.to_string();
                    self.state.token_flags |= TokenFlags::OCTAL;
                    let with_minus = self.state.token == Kind::MinusToken;
                    let err_start = if with_minus { start - 1 } else { start };
                    self.error_at(
                        &diagnostics::OCTAL_LITERALS_ARE_NOT_ALLOWED_USE_THE_SYNTAX_0,
                        err_start,
                        self.state.pos - err_start,
                    );
                    return Kind::NumericLiteral;
                }
            }
        } else {
            fixed_part = self.scan_number_fragment();
        }
        let fixed_part_end = self.state.pos;
        let mut fractional_part = String::new();
        let mut exponent_preamble = String::new();
        let mut exponent_part = String::new();
        if self.char() == '.' as i32 {
            self.state.pos += 1;
            fractional_part = self.scan_number_fragment();
        }
        let mut end = self.state.pos;
        if self.char() == 'E' as i32 || self.char() == 'e' as i32 {
            self.state.pos += 1;
            self.state.token_flags |= TokenFlags::SCIENTIFIC;
            if self.char() == '+' as i32 || self.char() == '-' as i32 {
                self.state.pos += 1;
            }
            let start_numeric_part = self.state.pos;
            exponent_part = self.scan_number_fragment();
            if exponent_part.is_empty() {
                self.error(&diagnostics::DIGIT_EXPECTED);
            } else {
                exponent_preamble =
                    self.text[end as usize..start_numeric_part as usize].to_string();
                end = self.state.pos;
            }
        }
        if self
            .state
            .token_flags
            .contains(TokenFlags::CONTAINS_SEPARATOR)
        {
            self.state.token_value = fixed_part;
            if !fractional_part.is_empty() {
                self.state.token_value.push('.');
                self.state.token_value.push_str(&fractional_part);
            }
            if !exponent_part.is_empty() {
                self.state.token_value.push_str(&exponent_preamble);
                self.state.token_value.push_str(&exponent_part);
            }
        } else {
            self.state.token_value = self.text[start as usize..end as usize].to_string();
        }
        if self
            .state
            .token_flags
            .contains(TokenFlags::CONTAINS_LEADING_ZERO)
        {
            self.error_at(
                &diagnostics::DECIMALS_WITH_LEADING_ZEROS_ARE_NOT_ALLOWED,
                start,
                self.state.pos - start,
            );
            self.state.token_value = tsgo_jsnum::from_string(&self.state.token_value).to_string();
            return Kind::NumericLiteral;
        }
        let result;
        if fixed_part_end == self.state.pos {
            result = self.scan_big_int_suffix();
        } else {
            self.state.token_value = tsgo_jsnum::from_string(&self.state.token_value).to_string();
            result = Kind::NumericLiteral;
        }
        let (ch, _) = self.char_and_size();
        if is_identifier_start(ch as i32) {
            let id_start = self.state.pos;
            let id = self.scan_identifier_parts();
            if result != Kind::BigIntLiteral
                && id.len() == 1
                && self.text.as_bytes()[id_start as usize] == b'n'
            {
                if self.state.token_flags.contains(TokenFlags::SCIENTIFIC) {
                    self.error_at(
                        &diagnostics::A_BIGINT_LITERAL_CANNOT_USE_EXPONENTIAL_NOTATION,
                        start,
                        self.state.pos - start,
                    );
                    return result;
                }
                if fixed_part_end < id_start {
                    self.error_at(
                        &diagnostics::A_BIGINT_LITERAL_MUST_BE_AN_INTEGER,
                        start,
                        self.state.pos - start,
                    );
                    return result;
                }
            }
            self.error_at(
                &diagnostics::AN_IDENTIFIER_OR_KEYWORD_CANNOT_IMMEDIATELY_FOLLOW_A_NUMERIC_LITERAL,
                id_start,
                self.state.pos - id_start,
            );
            self.state.pos = id_start;
        }
        result
    }

    // Go: internal/scanner/scanner.go:Scanner.scanNumberFragment
    fn scan_number_fragment(&mut self) -> String {
        let mut start = self.state.pos;
        let mut allow_separator = false;
        let mut is_previous_token_separator = false;
        let mut result = String::new();
        loop {
            let ch = self.char();
            if ch == '_' as i32 {
                self.state.token_flags |= TokenFlags::CONTAINS_SEPARATOR;
                if allow_separator {
                    allow_separator = false;
                    is_previous_token_separator = true;
                    result.push_str(&self.text[start as usize..self.state.pos as usize]);
                } else {
                    self.state.token_flags |= TokenFlags::CONTAINS_INVALID_SEPARATOR;
                    if is_previous_token_separator {
                        self.error_at(
                            &diagnostics::MULTIPLE_CONSECUTIVE_NUMERIC_SEPARATORS_ARE_NOT_PERMITTED,
                            self.state.pos,
                            1,
                        );
                    } else {
                        self.error_at(
                            &diagnostics::NUMERIC_SEPARATORS_ARE_NOT_ALLOWED_HERE,
                            self.state.pos,
                            1,
                        );
                    }
                }
                self.state.pos += 1;
                start = self.state.pos;
                continue;
            }
            if stringutil::is_digit(rune_char(ch)) {
                allow_separator = true;
                is_previous_token_separator = false;
                self.state.pos += 1;
                continue;
            }
            break;
        }
        if is_previous_token_separator {
            self.state.token_flags |= TokenFlags::CONTAINS_INVALID_SEPARATOR;
            self.error_at(
                &diagnostics::NUMERIC_SEPARATORS_ARE_NOT_ALLOWED_HERE,
                self.state.pos - 1,
                1,
            );
        }
        result.push_str(&self.text[start as usize..self.state.pos as usize]);
        result
    }

    // Go: internal/scanner/scanner.go:Scanner.scanDigits
    fn scan_digits(&mut self) -> (String, bool) {
        let start = self.state.pos;
        let mut is_octal = true;
        while stringutil::is_digit(rune_char(self.char())) {
            if !stringutil::is_octal_digit(rune_char(self.char())) {
                is_octal = false;
            }
            self.state.pos += 1;
        }
        (
            self.text[start as usize..self.state.pos as usize].to_string(),
            is_octal,
        )
    }

    // Go: internal/scanner/scanner.go:Scanner.scanHexDigits
    fn scan_hex_digits(
        &mut self,
        min_count: i32,
        scan_as_many_as_possible: bool,
        can_have_separators: bool,
    ) -> String {
        let mut digit_count = 0;
        let start = self.state.pos;
        let mut allow_separator = false;
        let mut is_previous_token_separator = false;
        while digit_count < min_count || scan_as_many_as_possible {
            let ch = self.char();
            if stringutil::is_hex_digit(rune_char(ch)) {
                allow_separator = can_have_separators;
                is_previous_token_separator = false;
                digit_count += 1;
            } else if can_have_separators && ch == '_' as i32 {
                self.state.token_flags |= TokenFlags::CONTAINS_SEPARATOR;
                if allow_separator {
                    allow_separator = false;
                    is_previous_token_separator = true;
                } else if is_previous_token_separator {
                    self.error_at(
                        &diagnostics::MULTIPLE_CONSECUTIVE_NUMERIC_SEPARATORS_ARE_NOT_PERMITTED,
                        self.state.pos,
                        1,
                    );
                } else {
                    self.error_at(
                        &diagnostics::NUMERIC_SEPARATORS_ARE_NOT_ALLOWED_HERE,
                        self.state.pos,
                        1,
                    );
                }
            } else {
                break;
            }
            self.state.pos += 1;
        }
        if is_previous_token_separator {
            self.error_at(
                &diagnostics::NUMERIC_SEPARATORS_ARE_NOT_ALLOWED_HERE,
                self.state.pos - 1,
                1,
            );
        }
        if digit_count < min_count {
            return String::new();
        }
        let digits = self.text[start as usize..self.state.pos as usize].to_string();
        if let Some(cached) = self.hex_digit_cache.get(&digits) {
            return cached.clone();
        }
        let original = digits.clone();
        let mut normalized = digits;
        if self
            .state
            .token_flags
            .contains(TokenFlags::CONTAINS_SEPARATOR)
        {
            normalized = normalized.replace('_', "");
        }
        // Standardize hex literals to lowercase.
        normalized = normalized.to_lowercase();
        self.hex_digit_cache.insert(original, normalized.clone());
        normalized
    }

    // Go: internal/scanner/scanner.go:Scanner.scanBinaryOrOctalDigits
    fn scan_binary_or_octal_digits(&mut self, base: i32) -> String {
        let mut sb = String::new();
        let mut allow_separator = false;
        let mut is_previous_token_separator = false;
        loop {
            let ch = self.char();
            if stringutil::is_digit(rune_char(ch)) && ch - ('0' as i32) < base {
                sb.push(ch as u8 as char);
                allow_separator = true;
                is_previous_token_separator = false;
            } else if ch == '_' as i32 {
                self.state.token_flags |= TokenFlags::CONTAINS_SEPARATOR;
                if allow_separator {
                    allow_separator = false;
                    is_previous_token_separator = true;
                } else if is_previous_token_separator {
                    self.error_at(
                        &diagnostics::MULTIPLE_CONSECUTIVE_NUMERIC_SEPARATORS_ARE_NOT_PERMITTED,
                        self.state.pos,
                        1,
                    );
                } else {
                    self.error_at(
                        &diagnostics::NUMERIC_SEPARATORS_ARE_NOT_ALLOWED_HERE,
                        self.state.pos,
                        1,
                    );
                }
            } else {
                break;
            }
            self.state.pos += 1;
        }
        if is_previous_token_separator {
            self.error_at(
                &diagnostics::NUMERIC_SEPARATORS_ARE_NOT_ALLOWED_HERE,
                self.state.pos - 1,
                1,
            );
        }
        sb
    }

    // Go: internal/scanner/scanner.go:Scanner.scanBigIntSuffix
    fn scan_big_int_suffix(&mut self) -> Kind {
        if self.char() == 'n' as i32 {
            self.state.token_value.push('n');
            if self
                .state
                .token_flags
                .contains(TokenFlags::BINARY_OR_OCTAL_SPECIFIER)
            {
                self.state.token_value =
                    tsgo_jsnum::parse_pseudo_big_int(&self.state.token_value) + "n";
            }
            self.state.pos += 1;
            return Kind::BigIntLiteral;
        }
        if let Some(cached) = self.number_cache.get(&self.state.token_value) {
            self.state.token_value = cached.clone();
        } else {
            let key = self.state.token_value.clone();
            let token_value = tsgo_jsnum::from_string(&self.state.token_value).to_string();
            self.number_cache.insert(key, token_value.clone());
            self.state.token_value = token_value;
        }
        Kind::NumericLiteral
    }

    // Go: internal/scanner/scanner.go:Scanner.scanInvalidCharacter
    fn scan_invalid_character(&mut self) {
        let (_, size) = self.char_and_size();
        self.error_at(&diagnostics::INVALID_CHARACTER, self.state.pos, size as i32);
        self.state.pos += size as i32;
        self.state.token = Kind::Unknown;
    }
}

/// Opaque snapshot of scanner state produced by [`Scanner::mark`].
///
/// Side effects: none (pure value type).
// Go: internal/scanner/scanner.go:ScannerState
#[derive(Clone, Debug)]
pub struct ScannerStateSnapshot(ScannerState);

/// The reserved-word and contextual-keyword spellings mapped to their `Kind`.
// Go: internal/scanner/scanner.go:textToKeyword
fn keyword_pairs() -> &'static [(&'static str, Kind)] {
    &[
        ("abstract", Kind::AbstractKeyword),
        ("accessor", Kind::AccessorKeyword),
        ("any", Kind::AnyKeyword),
        ("as", Kind::AsKeyword),
        ("asserts", Kind::AssertsKeyword),
        ("assert", Kind::AssertKeyword),
        ("bigint", Kind::BigIntKeyword),
        ("boolean", Kind::BooleanKeyword),
        ("break", Kind::BreakKeyword),
        ("case", Kind::CaseKeyword),
        ("catch", Kind::CatchKeyword),
        ("class", Kind::ClassKeyword),
        ("continue", Kind::ContinueKeyword),
        ("const", Kind::ConstKeyword),
        ("constructor", Kind::ConstructorKeyword),
        ("debugger", Kind::DebuggerKeyword),
        ("declare", Kind::DeclareKeyword),
        ("default", Kind::DefaultKeyword),
        ("defer", Kind::DeferKeyword),
        ("delete", Kind::DeleteKeyword),
        ("do", Kind::DoKeyword),
        ("else", Kind::ElseKeyword),
        ("enum", Kind::EnumKeyword),
        ("export", Kind::ExportKeyword),
        ("extends", Kind::ExtendsKeyword),
        ("false", Kind::FalseKeyword),
        ("finally", Kind::FinallyKeyword),
        ("for", Kind::ForKeyword),
        ("from", Kind::FromKeyword),
        ("function", Kind::FunctionKeyword),
        ("get", Kind::GetKeyword),
        ("if", Kind::IfKeyword),
        ("immediate", Kind::ImmediateKeyword),
        ("implements", Kind::ImplementsKeyword),
        ("import", Kind::ImportKeyword),
        ("in", Kind::InKeyword),
        ("infer", Kind::InferKeyword),
        ("instanceof", Kind::InstanceOfKeyword),
        ("interface", Kind::InterfaceKeyword),
        ("intrinsic", Kind::IntrinsicKeyword),
        ("is", Kind::IsKeyword),
        ("keyof", Kind::KeyOfKeyword),
        ("let", Kind::LetKeyword),
        ("module", Kind::ModuleKeyword),
        ("namespace", Kind::NamespaceKeyword),
        ("never", Kind::NeverKeyword),
        ("new", Kind::NewKeyword),
        ("null", Kind::NullKeyword),
        ("number", Kind::NumberKeyword),
        ("object", Kind::ObjectKeyword),
        ("package", Kind::PackageKeyword),
        ("private", Kind::PrivateKeyword),
        ("protected", Kind::ProtectedKeyword),
        ("public", Kind::PublicKeyword),
        ("override", Kind::OverrideKeyword),
        ("out", Kind::OutKeyword),
        ("readonly", Kind::ReadonlyKeyword),
        ("require", Kind::RequireKeyword),
        ("global", Kind::GlobalKeyword),
        ("return", Kind::ReturnKeyword),
        ("satisfies", Kind::SatisfiesKeyword),
        ("set", Kind::SetKeyword),
        ("static", Kind::StaticKeyword),
        ("string", Kind::StringKeyword),
        ("super", Kind::SuperKeyword),
        ("switch", Kind::SwitchKeyword),
        ("symbol", Kind::SymbolKeyword),
        ("this", Kind::ThisKeyword),
        ("throw", Kind::ThrowKeyword),
        ("true", Kind::TrueKeyword),
        ("try", Kind::TryKeyword),
        ("type", Kind::TypeKeyword),
        ("typeof", Kind::TypeOfKeyword),
        ("undefined", Kind::UndefinedKeyword),
        ("unique", Kind::UniqueKeyword),
        ("unknown", Kind::UnknownKeyword),
        ("using", Kind::UsingKeyword),
        ("var", Kind::VarKeyword),
        ("void", Kind::VoidKeyword),
        ("while", Kind::WhileKeyword),
        ("with", Kind::WithKeyword),
        ("yield", Kind::YieldKeyword),
        ("async", Kind::AsyncKeyword),
        ("await", Kind::AwaitKeyword),
        ("of", Kind::OfKeyword),
    ]
}

/// The punctuation and operator spellings mapped to their `Kind`.
// Go: internal/scanner/scanner.go:textToToken
fn punctuation_pairs() -> &'static [(&'static str, Kind)] {
    &[
        ("{", Kind::OpenBraceToken),
        ("}", Kind::CloseBraceToken),
        ("(", Kind::OpenParenToken),
        (")", Kind::CloseParenToken),
        ("[", Kind::OpenBracketToken),
        ("]", Kind::CloseBracketToken),
        (".", Kind::DotToken),
        ("...", Kind::DotDotDotToken),
        (";", Kind::SemicolonToken),
        (",", Kind::CommaToken),
        ("<", Kind::LessThanToken),
        (">", Kind::GreaterThanToken),
        ("<=", Kind::LessThanEqualsToken),
        (">=", Kind::GreaterThanEqualsToken),
        ("==", Kind::EqualsEqualsToken),
        ("!=", Kind::ExclamationEqualsToken),
        ("===", Kind::EqualsEqualsEqualsToken),
        ("!==", Kind::ExclamationEqualsEqualsToken),
        ("=>", Kind::EqualsGreaterThanToken),
        ("+", Kind::PlusToken),
        ("-", Kind::MinusToken),
        ("**", Kind::AsteriskAsteriskToken),
        ("*", Kind::AsteriskToken),
        ("/", Kind::SlashToken),
        ("%", Kind::PercentToken),
        ("++", Kind::PlusPlusToken),
        ("--", Kind::MinusMinusToken),
        ("<<", Kind::LessThanLessThanToken),
        ("</", Kind::LessThanSlashToken),
        (">>", Kind::GreaterThanGreaterThanToken),
        (">>>", Kind::GreaterThanGreaterThanGreaterThanToken),
        ("&", Kind::AmpersandToken),
        ("|", Kind::BarToken),
        ("^", Kind::CaretToken),
        ("!", Kind::ExclamationToken),
        ("~", Kind::TildeToken),
        ("&&", Kind::AmpersandAmpersandToken),
        ("||", Kind::BarBarToken),
        ("?", Kind::QuestionToken),
        ("??", Kind::QuestionQuestionToken),
        ("?.", Kind::QuestionDotToken),
        (":", Kind::ColonToken),
        ("=", Kind::EqualsToken),
        ("+=", Kind::PlusEqualsToken),
        ("-=", Kind::MinusEqualsToken),
        ("*=", Kind::AsteriskEqualsToken),
        ("**=", Kind::AsteriskAsteriskEqualsToken),
        ("/=", Kind::SlashEqualsToken),
        ("%=", Kind::PercentEqualsToken),
        ("<<=", Kind::LessThanLessThanEqualsToken),
        (">>=", Kind::GreaterThanGreaterThanEqualsToken),
        (">>>=", Kind::GreaterThanGreaterThanGreaterThanEqualsToken),
        ("&=", Kind::AmpersandEqualsToken),
        ("|=", Kind::BarEqualsToken),
        ("^=", Kind::CaretEqualsToken),
        ("||=", Kind::BarBarEqualsToken),
        ("&&=", Kind::AmpersandAmpersandEqualsToken),
        ("??=", Kind::QuestionQuestionEqualsToken),
        ("@", Kind::AtToken),
        ("#", Kind::HashToken),
        ("`", Kind::BacktickToken),
    ]
}

/// Looks up `s` in the keyword table, returning [`Kind::Unknown`] if absent.
fn text_to_keyword(s: &str) -> Kind {
    static MAP: OnceLock<FxHashMap<&'static str, Kind>> = OnceLock::new();
    *MAP.get_or_init(|| keyword_pairs().iter().copied().collect())
        .get(s)
        .unwrap_or(&Kind::Unknown)
}

/// The combined punctuation + keyword spelling map (Go `textToToken`).
fn text_to_token_map() -> &'static FxHashMap<&'static str, Kind> {
    static MAP: OnceLock<FxHashMap<&'static str, Kind>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m: FxHashMap<&'static str, Kind> = punctuation_pairs().iter().copied().collect();
        for &(text, kind) in keyword_pairs() {
            m.insert(text, kind);
        }
        m
    })
}

/// The inverse of [`text_to_token_map`], indexed by `Kind`.
fn token_to_text() -> &'static [&'static str] {
    static ARR: OnceLock<Vec<&'static str>> = OnceLock::new();
    ARR.get_or_init(|| {
        let mut arr = vec![""; Kind::Count as usize];
        for (&text, &kind) in text_to_token_map().iter() {
            arr[kind as usize] = text;
        }
        arr
    })
}

/// Classifies an identifier spelling, returning the matching keyword `Kind` or
/// [`Kind::Identifier`].
///
/// Only lowercase-initial spellings of length 2..=12 can be keywords (a cheap
/// pre-filter before the table lookup).
///
/// # Examples
/// ```
/// use tsgo_scanner::get_identifier_token;
/// use tsgo_ast::Kind;
/// assert_eq!(get_identifier_token("let"), Kind::LetKeyword);
/// assert_eq!(get_identifier_token("Let"), Kind::Identifier);
/// assert_eq!(get_identifier_token("foo"), Kind::Identifier);
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:GetIdentifierToken
pub fn get_identifier_token(str: &str) -> Kind {
    let b = str.as_bytes();
    if str.len() >= 2 && str.len() <= 12 && b[0] >= b'a' && b[0] <= b'z' {
        let keyword = text_to_keyword(str);
        if keyword != Kind::Unknown {
            return keyword;
        }
    }
    Kind::Identifier
}

/// Reports whether `s` is a valid identifier (non-empty, valid start, valid
/// parts) under standard (non-JSX) rules.
///
/// # Examples
/// ```
/// use tsgo_scanner::is_valid_identifier;
/// assert!(is_valid_identifier("foo"));
/// assert!(!is_valid_identifier("1foo"));
/// assert!(!is_valid_identifier(""));
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:IsValidIdentifier
pub fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    for (i, ch) in s.char_indices() {
        if (i == 0 && !is_identifier_start(ch as i32)) || (i != 0 && !is_identifier_part(ch as i32))
        {
            return false;
        }
    }
    true
}

/// Reports whether `ch` is an ASCII word character (`[A-Za-z0-9_]`).
// Go: internal/scanner/scanner.go:isWordCharacter
fn is_word_character(ch: i32) -> bool {
    let c = rune_char(ch);
    stringutil::is_ascii_letter(c) || stringutil::is_digit(c) || ch == '_' as i32
}

/// Reports whether `ch` (a code point) may start an identifier.
///
/// ASCII letters, `_`, and `$` always qualify; non-ASCII code points are checked
/// against the Unicode `ID_Start` ranges.
///
/// # Examples
/// ```
/// use tsgo_scanner::is_identifier_start;
/// assert!(is_identifier_start('a' as i32));
/// assert!(is_identifier_start('$' as i32));
/// assert!(!is_identifier_start('1' as i32));
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:IsIdentifierStart
pub fn is_identifier_start(ch: i32) -> bool {
    let c = rune_char(ch);
    stringutil::is_ascii_letter(c)
        || ch == '_' as i32
        || ch == '$' as i32
        || (ch >= 0x80 && is_unicode_identifier_start(ch))
}

/// Reports whether `ch` may continue an identifier under standard (non-JSX)
/// rules.
///
/// # Examples
/// ```
/// use tsgo_scanner::is_identifier_part;
/// assert!(is_identifier_part('9' as i32));
/// assert!(!is_identifier_part('-' as i32));
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:IsIdentifierPart
pub fn is_identifier_part(ch: i32) -> bool {
    is_identifier_part_ex(ch, LanguageVariant::Standard)
}

/// Reports whether `ch` may continue an identifier, allowing `-` and `:` in the
/// JSX variant.
///
/// # Examples
/// ```
/// use tsgo_scanner::is_identifier_part_ex;
/// use tsgo_core::languagevariant::LanguageVariant;
/// assert!(is_identifier_part_ex('-' as i32, LanguageVariant::Jsx));
/// assert!(!is_identifier_part_ex('-' as i32, LanguageVariant::Standard));
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:IsIdentifierPartEx
pub fn is_identifier_part_ex(ch: i32, language_variant: LanguageVariant) -> bool {
    is_word_character(ch)
        || ch == '$' as i32
        || (ch >= 0x80 && is_unicode_identifier_part(ch))
        || (language_variant == LanguageVariant::Jsx && (ch == '-' as i32 || ch == ':' as i32))
}

// Go: internal/scanner/scanner.go:isUnicodeIdentifierStart
fn is_unicode_identifier_start(ch: i32) -> bool {
    is_in_unicode_ranges(ch, IDENT_START)
}

// Go: internal/scanner/scanner.go:isUnicodeIdentifierPart
fn is_unicode_identifier_part(ch: i32) -> bool {
    is_in_unicode_ranges(ch, IDENT_PART)
}

/// Binary-searches the sorted, paired `[lo, hi]` inclusive `ranges` for `cp`.
// Go: internal/scanner/scanner.go:isInUnicodeRanges
fn is_in_unicode_ranges(cp: i32, ranges: &[i32]) -> bool {
    // Bail out quickly if it couldn't possibly be in the map.
    if cp < ranges[0] {
        return false;
    }
    let mut lo: i32 = 0;
    let mut hi: i32 = ranges.len() as i32;
    while lo + 1 < hi {
        let mut mid = lo + (hi - lo) / 2;
        // mid has to be even to catch the beginning of a range.
        mid -= mid % 2;
        if ranges[mid as usize] <= cp && cp <= ranges[(mid + 1) as usize] {
            return true;
        }
        if cp < ranges[mid as usize] {
            hi = mid;
        } else {
            lo = mid + 2;
        }
    }
    false
}

/// Returns the canonical spelling of a punctuation/keyword `token`, or `""` for
/// kinds without a fixed spelling.
///
/// # Examples
/// ```
/// use tsgo_scanner::token_to_string;
/// use tsgo_ast::Kind;
/// assert_eq!(token_to_string(Kind::PlusToken), "+");
/// assert_eq!(token_to_string(Kind::LetKeyword), "let");
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:TokenToString
pub fn token_to_string(token: Kind) -> &'static str {
    token_to_text()[token as usize]
}

/// Returns the `Kind` for a punctuation/keyword spelling, or [`Kind::Unknown`].
///
/// # Examples
/// ```
/// use tsgo_scanner::string_to_token;
/// use tsgo_ast::Kind;
/// assert_eq!(string_to_token("+"), Kind::PlusToken);
/// assert_eq!(string_to_token("nope"), Kind::Unknown);
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:StringToToken
pub fn string_to_token(s: &str) -> Kind {
    *text_to_token_map().get(s).unwrap_or(&Kind::Unknown)
}

/// Returns the keyword spellings longer than two characters, used to suggest
/// fixes for misspelled keywords.
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:GetViableKeywordSuggestions
pub fn get_viable_keyword_suggestions() -> Vec<&'static str> {
    keyword_pairs()
        .iter()
        .filter(|(text, _)| text.len() > 2)
        .map(|(text, _)| *text)
        .collect()
}

/// Reports whether the text at `pos` is the start of a git merge-conflict marker
/// (`<<<<<<<`, `|||||||`, `=======`, `>>>>>>>`), which must begin a line.
///
/// # Examples
/// ```
/// use tsgo_scanner::is_conflict_marker_trivia;
/// assert!(is_conflict_marker_trivia("<<<<<<< HEAD", 0));
/// assert!(!is_conflict_marker_trivia("<< not a marker", 0));
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:isConflictMarkerTrivia
pub fn is_conflict_marker_trivia(text: &str, pos: i32) -> bool {
    if pos < 0 {
        panic!("pos < 0");
    }
    let bytes = text.as_bytes();
    let upos = pos as usize;
    let mut prev = '\0';
    if pos >= 2 {
        prev = decode_last_char(&text[..upos - 2]);
    }
    if pos == 0
        || stringutil::is_line_break(prev)
        || (pos >= 1 && stringutil::is_line_break(bytes[upos - 1] as char))
    {
        let ch = bytes[upos];
        if upos + MERGE_CONFLICT_MARKER_LENGTH < bytes.len() {
            for i in 0..MERGE_CONFLICT_MARKER_LENGTH {
                if bytes[upos + i] != ch {
                    return false;
                }
            }
            return ch == b'=' || bytes[upos + MERGE_CONFLICT_MARKER_LENGTH] == b' ';
        }
    }
    false
}

/// Consumes a git merge-conflict marker run starting at `pos`, returning the
/// position past it. Diagnostics are intentionally dropped until the parser
/// error interface lands.
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:scanConflictMarkerTrivia
pub fn scan_conflict_marker_trivia(text: &str, pos: i32) -> i32 {
    let bytes = text.as_bytes();
    let length = bytes.len() as i32;
    let mut pos = pos;
    let (mut ch, mut size) = {
        let (c, s) = next_char(text, pos);
        (c, s)
    };
    if ch == '<' || ch == '>' {
        while pos < length && !stringutil::is_line_break(ch) {
            pos += size as i32;
            let (c, s) = next_char(text, pos);
            ch = c;
            size = s;
        }
    } else {
        if ch != '|' && ch != '=' {
            panic!("Assertion failed: ch must be either '|' or '='");
        }
        // Consume from a '|||||||' or '=======' marker to the next '=======' or
        // '>>>>>>>' marker.
        while pos < length {
            let current_char = bytes[pos as usize];
            if (current_char == b'=' || current_char == b'>')
                && current_char as char != ch
                && is_conflict_marker_trivia(text, pos)
            {
                break;
            }
            pos += 1;
        }
    }
    pos
}

/// Decodes the character at byte `pos` in `text`, or `('\u{FFFD}', 0)` at/past
/// the end.
fn next_char(text: &str, pos: i32) -> (char, usize) {
    if pos < 0 || pos as usize >= text.len() {
        return ('\u{FFFD}', 0);
    }
    match text[pos as usize..].chars().next() {
        Some(c) => (c, c.len_utf8()),
        None => ('\u{FFFD}', 0),
    }
}

/// Largest ASCII byte value (`0x7F`).
// Go: internal/scanner/scanner.go:maxAsciiCharacter
const MAX_ASCII_CHARACTER: u8 = 127;

/// Reports whether `text` (already past a tag's `@name`) starts with one of the
/// JSDoc `tags` followed by a valid terminator (whitespace, `}`, `*`, or EOF).
// Go: internal/scanner/scanner.go:hasJSDocTag
fn has_jsdoc_tag(text: &str, tags: &[&str]) -> bool {
    for tag in tags {
        if !text.starts_with(tag) {
            continue;
        }
        if text.len() == tag.len() {
            return true;
        }
        let ch = text.as_bytes()[tag.len()];
        if matches!(ch, b' ' | b'\t' | b'\n' | b'\r' | b'}' | b'*') {
            return true;
        }
    }
    false
}

/// Reports whether the character at `pos` could begin trivia (whitespace, a
/// comment, a conflict marker, or a leading shebang).
///
/// Kept in sync with [`skip_trivia_ex`]; used by the parser to decide whether a
/// trivia scan is needed at a position.
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:couldStartTrivia
pub fn could_start_trivia(text: &str, pos: i32) -> bool {
    let ch = text.as_bytes()[pos as usize];
    match ch {
        b'\r' | b'\n' | b'\t' | 0x0B | 0x0C | b' ' | b'/' | b'<' | b'|' | b'=' | b'>' => true,
        // Only at the very beginning can we have `#!` shebang trivia.
        b'#' => pos == 0,
        _ => ch > MAX_ASCII_CHARACTER,
    }
}

/// Options controlling [`skip_trivia_ex`].
///
/// Side effects: none (pure value type).
// Go: internal/scanner/scanner.go:SkipTriviaOptions
#[derive(Clone, Copy, Debug, Default)]
pub struct SkipTriviaOptions {
    /// Stop immediately after the first line break.
    pub stop_after_line_break: bool,
    /// Stop at the first comment instead of skipping it.
    pub stop_at_comments: bool,
    /// Scanning inside a JSDoc comment (allows skipping leading `*`).
    pub in_jsdoc: bool,
}

/// Skips trivia (whitespace, comments, conflict markers, leading shebang)
/// starting at `pos`, returning the offset of the first non-trivia character.
///
/// # Examples
/// ```
/// use tsgo_scanner::skip_trivia;
/// assert_eq!(skip_trivia("  /*c*/ x", 0), 8);
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:SkipTrivia
pub fn skip_trivia(text: &str, pos: i32) -> i32 {
    skip_trivia_ex(text, pos, None)
}

/// Like [`skip_trivia`] but honoring `options` (stop after line break, stop at
/// comments, JSDoc leading-asterisk handling).
///
/// A synthesized (`< 0`) position is returned unchanged.
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:SkipTriviaEx
pub fn skip_trivia_ex(text: &str, pos: i32, options: Option<&SkipTriviaOptions>) -> i32 {
    // ast.PositionIsSynthesized(pos): a negative position is synthesized.
    if pos < 0 {
        return pos;
    }
    let default_options = SkipTriviaOptions::default();
    let options = options.unwrap_or(&default_options);
    let bytes = text.as_bytes();
    let text_len = text.len() as i32;
    let mut pos = pos;
    let mut can_consume_star = false;
    // Keep in sync with could_start_trivia.
    loop {
        if pos >= text_len {
            return pos;
        }
        let (ch, size) = next_char(text, pos);
        match ch {
            '\r' => {
                if pos + 1 < text_len && bytes[(pos + 1) as usize] == b'\n' {
                    pos += 1;
                }
                pos += 1;
                if options.stop_after_line_break {
                    return pos;
                }
                can_consume_star = options.in_jsdoc;
            }
            '\n' => {
                pos += 1;
                if options.stop_after_line_break {
                    return pos;
                }
                can_consume_star = options.in_jsdoc;
            }
            '\t' | '\u{000B}' | '\u{000C}' | ' ' => {
                pos += 1;
            }
            '/' => {
                if options.stop_at_comments {
                    return pos;
                }
                if pos + 1 < text_len {
                    if bytes[(pos + 1) as usize] == b'/' {
                        pos += 2;
                        while pos < text_len {
                            let (c, s) = next_char(text, pos);
                            if stringutil::is_line_break(c) {
                                break;
                            }
                            pos += s as i32;
                        }
                        can_consume_star = false;
                        continue;
                    }
                    if bytes[(pos + 1) as usize] == b'*' {
                        pos += 2;
                        while pos < text_len {
                            if bytes[pos as usize] == b'*'
                                && pos + 1 < text_len
                                && bytes[(pos + 1) as usize] == b'/'
                            {
                                pos += 2;
                                break;
                            }
                            let (_, s) = next_char(text, pos);
                            pos += s as i32;
                        }
                        can_consume_star = false;
                        continue;
                    }
                }
                return pos;
            }
            '<' | '|' | '=' | '>' => {
                if is_conflict_marker_trivia(text, pos) {
                    pos = scan_conflict_marker_trivia(text, pos);
                    can_consume_star = false;
                    continue;
                }
                return pos;
            }
            '#' => {
                if pos == 0 && is_shebang_trivia(text, pos) {
                    pos = scan_shebang_trivia(text, pos);
                    can_consume_star = false;
                    continue;
                }
                return pos;
            }
            '*' => {
                if can_consume_star {
                    pos += 1;
                    can_consume_star = false;
                    continue;
                }
                return pos;
            }
            _ => {
                if ch as u32 > MAX_ASCII_CHARACTER as u32 && stringutil::is_white_space_like(ch) {
                    pos += size as i32;
                    continue;
                }
                return pos;
            }
        }
    }
}

/// Reports whether `text` begins with a `#!` shebang.
///
/// # Panics
/// Panics if `pos != 0` (shebangs only appear at the file start).
// Go: internal/scanner/scanner.go:isShebangTrivia
fn is_shebang_trivia(text: &str, pos: i32) -> bool {
    if text.len() < 2 {
        return false;
    }
    if pos != 0 {
        panic!("Shebangs check must only be done at the start of the file");
    }
    let b = text.as_bytes();
    b[0] == b'#' && b[1] == b'!'
}

/// Consumes a shebang line starting at `pos`, returning the position of its end.
// Go: internal/scanner/scanner.go:scanShebangTrivia
fn scan_shebang_trivia(text: &str, pos: i32) -> i32 {
    let mut pos = pos + 2;
    let text_len = text.len() as i32;
    while pos < text_len {
        let (ch, size) = next_char(text, pos);
        if stringutil::is_line_break(ch) {
            break;
        }
        pos += size as i32;
    }
    pos
}

/// Returns the leading `#!` shebang line (without its line break), or `""`.
///
/// # Examples
/// ```
/// use tsgo_scanner::get_shebang;
/// assert_eq!(get_shebang("#!/usr/bin/env node\n"), "#!/usr/bin/env node");
/// assert_eq!(get_shebang("let x = 1;"), "");
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:GetShebang
pub fn get_shebang(text: &str) -> &str {
    if !is_shebang_trivia(text, 0) {
        return "";
    }
    let end = scan_shebang_trivia(text, 0);
    &text[..end as usize]
}

/// A comment range: its byte span, comment kind, and whether a line break
/// trails it.
///
/// TODO(port): move to `tsgo_ast` once `ast.CommentRange` is ported (P2). Go's
/// `iterateCommentRanges` threads a `*NodeFactory` solely to allocate these
/// values, so the port drops that parameter.
///
/// Side effects: none (pure value type).
// Go: internal/ast/ast.go:CommentRange
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommentRange {
    /// The comment's source range.
    pub loc: TextRange,
    /// `SingleLineCommentTrivia` or `MultiLineCommentTrivia`.
    pub kind: Kind,
    /// Whether a line break immediately follows the comment.
    pub has_trailing_new_line: bool,
}

/// Returns the comment ranges leading the token at `pos`.
///
/// # Examples
/// ```
/// use tsgo_scanner::get_leading_comment_ranges;
/// let ranges = get_leading_comment_ranges("/*a*/ x", 0);
/// assert_eq!(ranges.len(), 1);
/// ```
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:GetLeadingCommentRanges
pub fn get_leading_comment_ranges(text: &str, pos: i32) -> Vec<CommentRange> {
    iterate_comment_ranges(text, pos, false)
}

/// Returns the comment ranges trailing the token ending at `pos` (i.e. up to the
/// next line break).
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:GetTrailingCommentRanges
pub fn get_trailing_comment_ranges(text: &str, pos: i32) -> Vec<CommentRange> {
    iterate_comment_ranges(text, pos, true)
}

/// Collects comment ranges starting at `pos`. Single-line ranges include the
/// leading `//` but not the trailing line break; multi-line ranges include both
/// delimiters.
///
/// Divergence: Go returns a lazy `iter.Seq`; this eager `Vec` is behaviorally
/// identical for all consumers (mirrors the `tsgo_core` line-map port).
// Go: internal/scanner/scanner.go:iterateCommentRanges
fn iterate_comment_ranges(text: &str, pos: i32, trailing: bool) -> Vec<CommentRange> {
    let mut result = Vec::new();
    let bytes = text.as_bytes();
    let text_len = text.len() as i32;
    let mut pos = pos;
    // Pending range as (pos, end, kind, has_trailing_new_line).
    let mut pending: Option<(i32, i32, Kind, bool)> = None;
    let mut collecting = trailing;
    if pos == 0 {
        collecting = true;
        if is_shebang_trivia(text, pos) {
            pos = scan_shebang_trivia(text, pos);
        }
    }
    'scan: while pos >= 0 && pos < text_len {
        let (ch, size) = next_char(text, pos);
        match ch {
            '\r' | '\n' => {
                if ch == '\r' && pos + 1 < text_len && bytes[(pos + 1) as usize] == b'\n' {
                    pos += 1;
                }
                pos += 1;
                if trailing {
                    break 'scan;
                }
                collecting = true;
                if let Some(p) = pending.as_mut() {
                    p.3 = true;
                }
            }
            '\t' | '\u{000B}' | '\u{000C}' | ' ' => {
                pos += 1;
            }
            '/' => {
                let next_byte = if pos + 1 < text_len {
                    bytes[(pos + 1) as usize]
                } else {
                    0
                };
                let mut has_trailing_new_line = false;
                if next_byte == b'/' || next_byte == b'*' {
                    let kind = if next_byte == b'/' {
                        Kind::SingleLineCommentTrivia
                    } else {
                        Kind::MultiLineCommentTrivia
                    };
                    let start_pos = pos;
                    pos += 2;
                    if next_byte == b'/' {
                        while pos < text_len {
                            let (c, s) = next_char(text, pos);
                            if stringutil::is_line_break(c) {
                                has_trailing_new_line = true;
                                break;
                            }
                            pos += s as i32;
                        }
                    } else {
                        while pos < text_len {
                            let (c, s) = next_char(text, pos);
                            if c == '*' && pos + 1 < text_len && bytes[(pos + 1) as usize] == b'/' {
                                pos += 2;
                                break;
                            }
                            pos += s as i32;
                        }
                    }
                    if collecting {
                        if let Some((pp, pe, pk, pnl)) = pending {
                            result.push(CommentRange {
                                loc: TextRange::new(pp, pe),
                                kind: pk,
                                has_trailing_new_line: pnl,
                            });
                        }
                        pending = Some((start_pos, pos, kind, has_trailing_new_line));
                    }
                    continue;
                }
                break 'scan;
            }
            _ => {
                if ch as u32 > MAX_ASCII_CHARACTER as u32 && stringutil::is_white_space_like(ch) {
                    if let Some(p) = pending.as_mut() {
                        if stringutil::is_line_break(ch) {
                            p.3 = true;
                        }
                    }
                    pos += size as i32;
                } else {
                    break 'scan;
                }
            }
        }
    }
    if let Some((pp, pe, pk, pnl)) = pending {
        result.push(CommentRange {
            loc: TextRange::new(pp, pe),
            kind: pk,
            has_trailing_new_line: pnl,
        });
    }
    result
}

// Generated from Go `internal/scanner/scanner.go` unicode identifier tables
// (scripts/regenerate-unicode-identifier-parts.mjs, Unicode 15.1). Paired
// `[lo, hi]` inclusive ranges searched by `is_in_unicode_ranges`.
// Go: internal/scanner/scanner.go:unicodeESNextIdentifierStart
static IDENT_START: &[i32] = &[
    65, 90, 97, 122, 170, 170, 181, 181, 186, 186, 192, 214, 216, 246, 248, 705, 710, 721, 736,
    740, 748, 748, 750, 750, 880, 884, 886, 887, 890, 893, 895, 895, 902, 902, 904, 906, 908, 908,
    910, 929, 931, 1013, 1015, 1153, 1162, 1327, 1329, 1366, 1369, 1369, 1376, 1416, 1488, 1514,
    1519, 1522, 1568, 1610, 1646, 1647, 1649, 1747, 1749, 1749, 1765, 1766, 1774, 1775, 1786, 1788,
    1791, 1791, 1808, 1808, 1810, 1839, 1869, 1957, 1969, 1969, 1994, 2026, 2036, 2037, 2042, 2042,
    2048, 2069, 2074, 2074, 2084, 2084, 2088, 2088, 2112, 2136, 2144, 2154, 2160, 2183, 2185, 2190,
    2208, 2249, 2308, 2361, 2365, 2365, 2384, 2384, 2392, 2401, 2417, 2432, 2437, 2444, 2447, 2448,
    2451, 2472, 2474, 2480, 2482, 2482, 2486, 2489, 2493, 2493, 2510, 2510, 2524, 2525, 2527, 2529,
    2544, 2545, 2556, 2556, 2565, 2570, 2575, 2576, 2579, 2600, 2602, 2608, 2610, 2611, 2613, 2614,
    2616, 2617, 2649, 2652, 2654, 2654, 2674, 2676, 2693, 2701, 2703, 2705, 2707, 2728, 2730, 2736,
    2738, 2739, 2741, 2745, 2749, 2749, 2768, 2768, 2784, 2785, 2809, 2809, 2821, 2828, 2831, 2832,
    2835, 2856, 2858, 2864, 2866, 2867, 2869, 2873, 2877, 2877, 2908, 2909, 2911, 2913, 2929, 2929,
    2947, 2947, 2949, 2954, 2958, 2960, 2962, 2965, 2969, 2970, 2972, 2972, 2974, 2975, 2979, 2980,
    2984, 2986, 2990, 3001, 3024, 3024, 3077, 3084, 3086, 3088, 3090, 3112, 3114, 3129, 3133, 3133,
    3160, 3162, 3165, 3165, 3168, 3169, 3200, 3200, 3205, 3212, 3214, 3216, 3218, 3240, 3242, 3251,
    3253, 3257, 3261, 3261, 3293, 3294, 3296, 3297, 3313, 3314, 3332, 3340, 3342, 3344, 3346, 3386,
    3389, 3389, 3406, 3406, 3412, 3414, 3423, 3425, 3450, 3455, 3461, 3478, 3482, 3505, 3507, 3515,
    3517, 3517, 3520, 3526, 3585, 3632, 3634, 3635, 3648, 3654, 3713, 3714, 3716, 3716, 3718, 3722,
    3724, 3747, 3749, 3749, 3751, 3760, 3762, 3763, 3773, 3773, 3776, 3780, 3782, 3782, 3804, 3807,
    3840, 3840, 3904, 3911, 3913, 3948, 3976, 3980, 4096, 4138, 4159, 4159, 4176, 4181, 4186, 4189,
    4193, 4193, 4197, 4198, 4206, 4208, 4213, 4225, 4238, 4238, 4256, 4293, 4295, 4295, 4301, 4301,
    4304, 4346, 4348, 4680, 4682, 4685, 4688, 4694, 4696, 4696, 4698, 4701, 4704, 4744, 4746, 4749,
    4752, 4784, 4786, 4789, 4792, 4798, 4800, 4800, 4802, 4805, 4808, 4822, 4824, 4880, 4882, 4885,
    4888, 4954, 4992, 5007, 5024, 5109, 5112, 5117, 5121, 5740, 5743, 5759, 5761, 5786, 5792, 5866,
    5870, 5880, 5888, 5905, 5919, 5937, 5952, 5969, 5984, 5996, 5998, 6000, 6016, 6067, 6103, 6103,
    6108, 6108, 6176, 6264, 6272, 6312, 6314, 6314, 6320, 6389, 6400, 6430, 6480, 6509, 6512, 6516,
    6528, 6571, 6576, 6601, 6656, 6678, 6688, 6740, 6823, 6823, 6917, 6963, 6981, 6988, 7043, 7072,
    7086, 7087, 7098, 7141, 7168, 7203, 7245, 7247, 7258, 7293, 7296, 7304, 7312, 7354, 7357, 7359,
    7401, 7404, 7406, 7411, 7413, 7414, 7418, 7418, 7424, 7615, 7680, 7957, 7960, 7965, 7968, 8005,
    8008, 8013, 8016, 8023, 8025, 8025, 8027, 8027, 8029, 8029, 8031, 8061, 8064, 8116, 8118, 8124,
    8126, 8126, 8130, 8132, 8134, 8140, 8144, 8147, 8150, 8155, 8160, 8172, 8178, 8180, 8182, 8188,
    8305, 8305, 8319, 8319, 8336, 8348, 8450, 8450, 8455, 8455, 8458, 8467, 8469, 8469, 8472, 8477,
    8484, 8484, 8486, 8486, 8488, 8488, 8490, 8505, 8508, 8511, 8517, 8521, 8526, 8526, 8544, 8584,
    11264, 11492, 11499, 11502, 11506, 11507, 11520, 11557, 11559, 11559, 11565, 11565, 11568,
    11623, 11631, 11631, 11648, 11670, 11680, 11686, 11688, 11694, 11696, 11702, 11704, 11710,
    11712, 11718, 11720, 11726, 11728, 11734, 11736, 11742, 12293, 12295, 12321, 12329, 12337,
    12341, 12344, 12348, 12353, 12438, 12443, 12447, 12449, 12538, 12540, 12543, 12549, 12591,
    12593, 12686, 12704, 12735, 12784, 12799, 13312, 19903, 19968, 42124, 42192, 42237, 42240,
    42508, 42512, 42527, 42538, 42539, 42560, 42606, 42623, 42653, 42656, 42735, 42775, 42783,
    42786, 42888, 42891, 42954, 42960, 42961, 42963, 42963, 42965, 42969, 42994, 43009, 43011,
    43013, 43015, 43018, 43020, 43042, 43072, 43123, 43138, 43187, 43250, 43255, 43259, 43259,
    43261, 43262, 43274, 43301, 43312, 43334, 43360, 43388, 43396, 43442, 43471, 43471, 43488,
    43492, 43494, 43503, 43514, 43518, 43520, 43560, 43584, 43586, 43588, 43595, 43616, 43638,
    43642, 43642, 43646, 43695, 43697, 43697, 43701, 43702, 43705, 43709, 43712, 43712, 43714,
    43714, 43739, 43741, 43744, 43754, 43762, 43764, 43777, 43782, 43785, 43790, 43793, 43798,
    43808, 43814, 43816, 43822, 43824, 43866, 43868, 43881, 43888, 44002, 44032, 55203, 55216,
    55238, 55243, 55291, 63744, 64109, 64112, 64217, 64256, 64262, 64275, 64279, 64285, 64285,
    64287, 64296, 64298, 64310, 64312, 64316, 64318, 64318, 64320, 64321, 64323, 64324, 64326,
    64433, 64467, 64829, 64848, 64911, 64914, 64967, 65008, 65019, 65136, 65140, 65142, 65276,
    65313, 65338, 65345, 65370, 65382, 65470, 65474, 65479, 65482, 65487, 65490, 65495, 65498,
    65500, 65536, 65547, 65549, 65574, 65576, 65594, 65596, 65597, 65599, 65613, 65616, 65629,
    65664, 65786, 65856, 65908, 66176, 66204, 66208, 66256, 66304, 66335, 66349, 66378, 66384,
    66421, 66432, 66461, 66464, 66499, 66504, 66511, 66513, 66517, 66560, 66717, 66736, 66771,
    66776, 66811, 66816, 66855, 66864, 66915, 66928, 66938, 66940, 66954, 66956, 66962, 66964,
    66965, 66967, 66977, 66979, 66993, 66995, 67001, 67003, 67004, 67072, 67382, 67392, 67413,
    67424, 67431, 67456, 67461, 67463, 67504, 67506, 67514, 67584, 67589, 67592, 67592, 67594,
    67637, 67639, 67640, 67644, 67644, 67647, 67669, 67680, 67702, 67712, 67742, 67808, 67826,
    67828, 67829, 67840, 67861, 67872, 67897, 67968, 68023, 68030, 68031, 68096, 68096, 68112,
    68115, 68117, 68119, 68121, 68149, 68192, 68220, 68224, 68252, 68288, 68295, 68297, 68324,
    68352, 68405, 68416, 68437, 68448, 68466, 68480, 68497, 68608, 68680, 68736, 68786, 68800,
    68850, 68864, 68899, 69248, 69289, 69296, 69297, 69376, 69404, 69415, 69415, 69424, 69445,
    69488, 69505, 69552, 69572, 69600, 69622, 69635, 69687, 69745, 69746, 69749, 69749, 69763,
    69807, 69840, 69864, 69891, 69926, 69956, 69956, 69959, 69959, 69968, 70002, 70006, 70006,
    70019, 70066, 70081, 70084, 70106, 70106, 70108, 70108, 70144, 70161, 70163, 70187, 70207,
    70208, 70272, 70278, 70280, 70280, 70282, 70285, 70287, 70301, 70303, 70312, 70320, 70366,
    70405, 70412, 70415, 70416, 70419, 70440, 70442, 70448, 70450, 70451, 70453, 70457, 70461,
    70461, 70480, 70480, 70493, 70497, 70656, 70708, 70727, 70730, 70751, 70753, 70784, 70831,
    70852, 70853, 70855, 70855, 71040, 71086, 71128, 71131, 71168, 71215, 71236, 71236, 71296,
    71338, 71352, 71352, 71424, 71450, 71488, 71494, 71680, 71723, 71840, 71903, 71935, 71942,
    71945, 71945, 71948, 71955, 71957, 71958, 71960, 71983, 71999, 71999, 72001, 72001, 72096,
    72103, 72106, 72144, 72161, 72161, 72163, 72163, 72192, 72192, 72203, 72242, 72250, 72250,
    72272, 72272, 72284, 72329, 72349, 72349, 72368, 72440, 72704, 72712, 72714, 72750, 72768,
    72768, 72818, 72847, 72960, 72966, 72968, 72969, 72971, 73008, 73030, 73030, 73056, 73061,
    73063, 73064, 73066, 73097, 73112, 73112, 73440, 73458, 73474, 73474, 73476, 73488, 73490,
    73523, 73648, 73648, 73728, 74649, 74752, 74862, 74880, 75075, 77712, 77808, 77824, 78895,
    78913, 78918, 82944, 83526, 92160, 92728, 92736, 92766, 92784, 92862, 92880, 92909, 92928,
    92975, 92992, 92995, 93027, 93047, 93053, 93071, 93760, 93823, 93952, 94026, 94032, 94032,
    94099, 94111, 94176, 94177, 94179, 94179, 94208, 100343, 100352, 101589, 101632, 101640,
    110576, 110579, 110581, 110587, 110589, 110590, 110592, 110882, 110898, 110898, 110928, 110930,
    110933, 110933, 110948, 110951, 110960, 111355, 113664, 113770, 113776, 113788, 113792, 113800,
    113808, 113817, 119808, 119892, 119894, 119964, 119966, 119967, 119970, 119970, 119973, 119974,
    119977, 119980, 119982, 119993, 119995, 119995, 119997, 120003, 120005, 120069, 120071, 120074,
    120077, 120084, 120086, 120092, 120094, 120121, 120123, 120126, 120128, 120132, 120134, 120134,
    120138, 120144, 120146, 120485, 120488, 120512, 120514, 120538, 120540, 120570, 120572, 120596,
    120598, 120628, 120630, 120654, 120656, 120686, 120688, 120712, 120714, 120744, 120746, 120770,
    120772, 120779, 122624, 122654, 122661, 122666, 122928, 122989, 123136, 123180, 123191, 123197,
    123214, 123214, 123536, 123565, 123584, 123627, 124112, 124139, 124896, 124902, 124904, 124907,
    124909, 124910, 124912, 124926, 124928, 125124, 125184, 125251, 125259, 125259, 126464, 126467,
    126469, 126495, 126497, 126498, 126500, 126500, 126503, 126503, 126505, 126514, 126516, 126519,
    126521, 126521, 126523, 126523, 126530, 126530, 126535, 126535, 126537, 126537, 126539, 126539,
    126541, 126543, 126545, 126546, 126548, 126548, 126551, 126551, 126553, 126553, 126555, 126555,
    126557, 126557, 126559, 126559, 126561, 126562, 126564, 126564, 126567, 126570, 126572, 126578,
    126580, 126583, 126585, 126588, 126590, 126590, 126592, 126601, 126603, 126619, 126625, 126627,
    126629, 126633, 126635, 126651, 131072, 173791, 173824, 177977, 177984, 178205, 178208, 183969,
    183984, 191456, 191472, 192093, 194560, 195101, 196608, 201546, 201552, 205743,
];
// Go: internal/scanner/scanner.go:unicodeESNextIdentifierPart
static IDENT_PART: &[i32] = &[
    48, 57, 65, 90, 95, 95, 97, 122, 170, 170, 181, 181, 183, 183, 186, 186, 192, 214, 216, 246,
    248, 705, 710, 721, 736, 740, 748, 748, 750, 750, 768, 884, 886, 887, 890, 893, 895, 895, 902,
    906, 908, 908, 910, 929, 931, 1013, 1015, 1153, 1155, 1159, 1162, 1327, 1329, 1366, 1369, 1369,
    1376, 1416, 1425, 1469, 1471, 1471, 1473, 1474, 1476, 1477, 1479, 1479, 1488, 1514, 1519, 1522,
    1552, 1562, 1568, 1641, 1646, 1747, 1749, 1756, 1759, 1768, 1770, 1788, 1791, 1791, 1808, 1866,
    1869, 1969, 1984, 2037, 2042, 2042, 2045, 2045, 2048, 2093, 2112, 2139, 2144, 2154, 2160, 2183,
    2185, 2190, 2200, 2273, 2275, 2403, 2406, 2415, 2417, 2435, 2437, 2444, 2447, 2448, 2451, 2472,
    2474, 2480, 2482, 2482, 2486, 2489, 2492, 2500, 2503, 2504, 2507, 2510, 2519, 2519, 2524, 2525,
    2527, 2531, 2534, 2545, 2556, 2556, 2558, 2558, 2561, 2563, 2565, 2570, 2575, 2576, 2579, 2600,
    2602, 2608, 2610, 2611, 2613, 2614, 2616, 2617, 2620, 2620, 2622, 2626, 2631, 2632, 2635, 2637,
    2641, 2641, 2649, 2652, 2654, 2654, 2662, 2677, 2689, 2691, 2693, 2701, 2703, 2705, 2707, 2728,
    2730, 2736, 2738, 2739, 2741, 2745, 2748, 2757, 2759, 2761, 2763, 2765, 2768, 2768, 2784, 2787,
    2790, 2799, 2809, 2815, 2817, 2819, 2821, 2828, 2831, 2832, 2835, 2856, 2858, 2864, 2866, 2867,
    2869, 2873, 2876, 2884, 2887, 2888, 2891, 2893, 2901, 2903, 2908, 2909, 2911, 2915, 2918, 2927,
    2929, 2929, 2946, 2947, 2949, 2954, 2958, 2960, 2962, 2965, 2969, 2970, 2972, 2972, 2974, 2975,
    2979, 2980, 2984, 2986, 2990, 3001, 3006, 3010, 3014, 3016, 3018, 3021, 3024, 3024, 3031, 3031,
    3046, 3055, 3072, 3084, 3086, 3088, 3090, 3112, 3114, 3129, 3132, 3140, 3142, 3144, 3146, 3149,
    3157, 3158, 3160, 3162, 3165, 3165, 3168, 3171, 3174, 3183, 3200, 3203, 3205, 3212, 3214, 3216,
    3218, 3240, 3242, 3251, 3253, 3257, 3260, 3268, 3270, 3272, 3274, 3277, 3285, 3286, 3293, 3294,
    3296, 3299, 3302, 3311, 3313, 3315, 3328, 3340, 3342, 3344, 3346, 3396, 3398, 3400, 3402, 3406,
    3412, 3415, 3423, 3427, 3430, 3439, 3450, 3455, 3457, 3459, 3461, 3478, 3482, 3505, 3507, 3515,
    3517, 3517, 3520, 3526, 3530, 3530, 3535, 3540, 3542, 3542, 3544, 3551, 3558, 3567, 3570, 3571,
    3585, 3642, 3648, 3662, 3664, 3673, 3713, 3714, 3716, 3716, 3718, 3722, 3724, 3747, 3749, 3749,
    3751, 3773, 3776, 3780, 3782, 3782, 3784, 3790, 3792, 3801, 3804, 3807, 3840, 3840, 3864, 3865,
    3872, 3881, 3893, 3893, 3895, 3895, 3897, 3897, 3902, 3911, 3913, 3948, 3953, 3972, 3974, 3991,
    3993, 4028, 4038, 4038, 4096, 4169, 4176, 4253, 4256, 4293, 4295, 4295, 4301, 4301, 4304, 4346,
    4348, 4680, 4682, 4685, 4688, 4694, 4696, 4696, 4698, 4701, 4704, 4744, 4746, 4749, 4752, 4784,
    4786, 4789, 4792, 4798, 4800, 4800, 4802, 4805, 4808, 4822, 4824, 4880, 4882, 4885, 4888, 4954,
    4957, 4959, 4969, 4977, 4992, 5007, 5024, 5109, 5112, 5117, 5121, 5740, 5743, 5759, 5761, 5786,
    5792, 5866, 5870, 5880, 5888, 5909, 5919, 5940, 5952, 5971, 5984, 5996, 5998, 6000, 6002, 6003,
    6016, 6099, 6103, 6103, 6108, 6109, 6112, 6121, 6155, 6157, 6159, 6169, 6176, 6264, 6272, 6314,
    6320, 6389, 6400, 6430, 6432, 6443, 6448, 6459, 6470, 6509, 6512, 6516, 6528, 6571, 6576, 6601,
    6608, 6618, 6656, 6683, 6688, 6750, 6752, 6780, 6783, 6793, 6800, 6809, 6823, 6823, 6832, 6845,
    6847, 6862, 6912, 6988, 6992, 7001, 7019, 7027, 7040, 7155, 7168, 7223, 7232, 7241, 7245, 7293,
    7296, 7304, 7312, 7354, 7357, 7359, 7376, 7378, 7380, 7418, 7424, 7957, 7960, 7965, 7968, 8005,
    8008, 8013, 8016, 8023, 8025, 8025, 8027, 8027, 8029, 8029, 8031, 8061, 8064, 8116, 8118, 8124,
    8126, 8126, 8130, 8132, 8134, 8140, 8144, 8147, 8150, 8155, 8160, 8172, 8178, 8180, 8182, 8188,
    8204, 8205, 8255, 8256, 8276, 8276, 8305, 8305, 8319, 8319, 8336, 8348, 8400, 8412, 8417, 8417,
    8421, 8432, 8450, 8450, 8455, 8455, 8458, 8467, 8469, 8469, 8472, 8477, 8484, 8484, 8486, 8486,
    8488, 8488, 8490, 8505, 8508, 8511, 8517, 8521, 8526, 8526, 8544, 8584, 11264, 11492, 11499,
    11507, 11520, 11557, 11559, 11559, 11565, 11565, 11568, 11623, 11631, 11631, 11647, 11670,
    11680, 11686, 11688, 11694, 11696, 11702, 11704, 11710, 11712, 11718, 11720, 11726, 11728,
    11734, 11736, 11742, 11744, 11775, 12293, 12295, 12321, 12335, 12337, 12341, 12344, 12348,
    12353, 12438, 12441, 12447, 12449, 12543, 12549, 12591, 12593, 12686, 12704, 12735, 12784,
    12799, 13312, 19903, 19968, 42124, 42192, 42237, 42240, 42508, 42512, 42539, 42560, 42607,
    42612, 42621, 42623, 42737, 42775, 42783, 42786, 42888, 42891, 42954, 42960, 42961, 42963,
    42963, 42965, 42969, 42994, 43047, 43052, 43052, 43072, 43123, 43136, 43205, 43216, 43225,
    43232, 43255, 43259, 43259, 43261, 43309, 43312, 43347, 43360, 43388, 43392, 43456, 43471,
    43481, 43488, 43518, 43520, 43574, 43584, 43597, 43600, 43609, 43616, 43638, 43642, 43714,
    43739, 43741, 43744, 43759, 43762, 43766, 43777, 43782, 43785, 43790, 43793, 43798, 43808,
    43814, 43816, 43822, 43824, 43866, 43868, 43881, 43888, 44010, 44012, 44013, 44016, 44025,
    44032, 55203, 55216, 55238, 55243, 55291, 63744, 64109, 64112, 64217, 64256, 64262, 64275,
    64279, 64285, 64296, 64298, 64310, 64312, 64316, 64318, 64318, 64320, 64321, 64323, 64324,
    64326, 64433, 64467, 64829, 64848, 64911, 64914, 64967, 65008, 65019, 65024, 65039, 65056,
    65071, 65075, 65076, 65101, 65103, 65136, 65140, 65142, 65276, 65296, 65305, 65313, 65338,
    65343, 65343, 65345, 65370, 65381, 65470, 65474, 65479, 65482, 65487, 65490, 65495, 65498,
    65500, 65536, 65547, 65549, 65574, 65576, 65594, 65596, 65597, 65599, 65613, 65616, 65629,
    65664, 65786, 65856, 65908, 66045, 66045, 66176, 66204, 66208, 66256, 66272, 66272, 66304,
    66335, 66349, 66378, 66384, 66426, 66432, 66461, 66464, 66499, 66504, 66511, 66513, 66517,
    66560, 66717, 66720, 66729, 66736, 66771, 66776, 66811, 66816, 66855, 66864, 66915, 66928,
    66938, 66940, 66954, 66956, 66962, 66964, 66965, 66967, 66977, 66979, 66993, 66995, 67001,
    67003, 67004, 67072, 67382, 67392, 67413, 67424, 67431, 67456, 67461, 67463, 67504, 67506,
    67514, 67584, 67589, 67592, 67592, 67594, 67637, 67639, 67640, 67644, 67644, 67647, 67669,
    67680, 67702, 67712, 67742, 67808, 67826, 67828, 67829, 67840, 67861, 67872, 67897, 67968,
    68023, 68030, 68031, 68096, 68099, 68101, 68102, 68108, 68115, 68117, 68119, 68121, 68149,
    68152, 68154, 68159, 68159, 68192, 68220, 68224, 68252, 68288, 68295, 68297, 68326, 68352,
    68405, 68416, 68437, 68448, 68466, 68480, 68497, 68608, 68680, 68736, 68786, 68800, 68850,
    68864, 68903, 68912, 68921, 69248, 69289, 69291, 69292, 69296, 69297, 69373, 69404, 69415,
    69415, 69424, 69456, 69488, 69509, 69552, 69572, 69600, 69622, 69632, 69702, 69734, 69749,
    69759, 69818, 69826, 69826, 69840, 69864, 69872, 69881, 69888, 69940, 69942, 69951, 69956,
    69959, 69968, 70003, 70006, 70006, 70016, 70084, 70089, 70092, 70094, 70106, 70108, 70108,
    70144, 70161, 70163, 70199, 70206, 70209, 70272, 70278, 70280, 70280, 70282, 70285, 70287,
    70301, 70303, 70312, 70320, 70378, 70384, 70393, 70400, 70403, 70405, 70412, 70415, 70416,
    70419, 70440, 70442, 70448, 70450, 70451, 70453, 70457, 70459, 70468, 70471, 70472, 70475,
    70477, 70480, 70480, 70487, 70487, 70493, 70499, 70502, 70508, 70512, 70516, 70656, 70730,
    70736, 70745, 70750, 70753, 70784, 70853, 70855, 70855, 70864, 70873, 71040, 71093, 71096,
    71104, 71128, 71133, 71168, 71232, 71236, 71236, 71248, 71257, 71296, 71352, 71360, 71369,
    71424, 71450, 71453, 71467, 71472, 71481, 71488, 71494, 71680, 71738, 71840, 71913, 71935,
    71942, 71945, 71945, 71948, 71955, 71957, 71958, 71960, 71989, 71991, 71992, 71995, 72003,
    72016, 72025, 72096, 72103, 72106, 72151, 72154, 72161, 72163, 72164, 72192, 72254, 72263,
    72263, 72272, 72345, 72349, 72349, 72368, 72440, 72704, 72712, 72714, 72758, 72760, 72768,
    72784, 72793, 72818, 72847, 72850, 72871, 72873, 72886, 72960, 72966, 72968, 72969, 72971,
    73014, 73018, 73018, 73020, 73021, 73023, 73031, 73040, 73049, 73056, 73061, 73063, 73064,
    73066, 73102, 73104, 73105, 73107, 73112, 73120, 73129, 73440, 73462, 73472, 73488, 73490,
    73530, 73534, 73538, 73552, 73561, 73648, 73648, 73728, 74649, 74752, 74862, 74880, 75075,
    77712, 77808, 77824, 78895, 78912, 78933, 82944, 83526, 92160, 92728, 92736, 92766, 92768,
    92777, 92784, 92862, 92864, 92873, 92880, 92909, 92912, 92916, 92928, 92982, 92992, 92995,
    93008, 93017, 93027, 93047, 93053, 93071, 93760, 93823, 93952, 94026, 94031, 94087, 94095,
    94111, 94176, 94177, 94179, 94180, 94192, 94193, 94208, 100343, 100352, 101589, 101632, 101640,
    110576, 110579, 110581, 110587, 110589, 110590, 110592, 110882, 110898, 110898, 110928, 110930,
    110933, 110933, 110948, 110951, 110960, 111355, 113664, 113770, 113776, 113788, 113792, 113800,
    113808, 113817, 113821, 113822, 118528, 118573, 118576, 118598, 119141, 119145, 119149, 119154,
    119163, 119170, 119173, 119179, 119210, 119213, 119362, 119364, 119808, 119892, 119894, 119964,
    119966, 119967, 119970, 119970, 119973, 119974, 119977, 119980, 119982, 119993, 119995, 119995,
    119997, 120003, 120005, 120069, 120071, 120074, 120077, 120084, 120086, 120092, 120094, 120121,
    120123, 120126, 120128, 120132, 120134, 120134, 120138, 120144, 120146, 120485, 120488, 120512,
    120514, 120538, 120540, 120570, 120572, 120596, 120598, 120628, 120630, 120654, 120656, 120686,
    120688, 120712, 120714, 120744, 120746, 120770, 120772, 120779, 120782, 120831, 121344, 121398,
    121403, 121452, 121461, 121461, 121476, 121476, 121499, 121503, 121505, 121519, 122624, 122654,
    122661, 122666, 122880, 122886, 122888, 122904, 122907, 122913, 122915, 122916, 122918, 122922,
    122928, 122989, 123023, 123023, 123136, 123180, 123184, 123197, 123200, 123209, 123214, 123214,
    123536, 123566, 123584, 123641, 124112, 124153, 124896, 124902, 124904, 124907, 124909, 124910,
    124912, 124926, 124928, 125124, 125136, 125142, 125184, 125259, 125264, 125273, 126464, 126467,
    126469, 126495, 126497, 126498, 126500, 126500, 126503, 126503, 126505, 126514, 126516, 126519,
    126521, 126521, 126523, 126523, 126530, 126530, 126535, 126535, 126537, 126537, 126539, 126539,
    126541, 126543, 126545, 126546, 126548, 126548, 126551, 126551, 126553, 126553, 126555, 126555,
    126557, 126557, 126559, 126559, 126561, 126562, 126564, 126564, 126567, 126570, 126572, 126578,
    126580, 126583, 126585, 126588, 126590, 126590, 126592, 126601, 126603, 126619, 126625, 126627,
    126629, 126633, 126635, 126651, 130032, 130041, 131072, 173791, 173824, 177977, 177984, 178205,
    178208, 183969, 183984, 191456, 191472, 192093, 194560, 195101, 196608, 201546, 201552, 205743,
    917760, 917999,
];

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
