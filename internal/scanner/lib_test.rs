//! Behavior-level tests for `tsgo_scanner`.
//!
//! Go `internal/scanner` ships no `*_test.go`; per `PORTING.md` §8.5 the scanner
//! is verified end-to-end by the P10 conformance/fourslash parity suite. These
//! tests cover the key lexical paths through the public `scan` API and the
//! position/UTF-16 helpers, with `expected` values taken from the ECMAScript/TS
//! spec and the Go implementation's observable semantics. Each block carries a
//! `// Go:` anchor pointing at the implementation item it exercises.

use super::*;
use tsgo_ast::Kind;
use tsgo_core::text::TextPos;
use tsgo_core::Utf16Offset;

/// Scans `src` with default options, returning the token `Kind` sequence
/// including the terminating `EndOfFile`.
fn scan_kinds(src: &str) -> Vec<Kind> {
    let mut s = Scanner::new();
    s.set_text(src.to_string());
    let mut out = Vec::new();
    loop {
        let k = s.scan();
        out.push(k);
        if k == Kind::EndOfFile {
            break;
        }
    }
    out
}

// Go: internal/scanner/scanner.go:Scan (punctuation single-character arms)
#[test]
fn scan_punctuation_singletons() {
    assert_eq!(
        scan_kinds("( ) { } [ ] ; , ."),
        vec![
            Kind::OpenParenToken,
            Kind::CloseParenToken,
            Kind::OpenBraceToken,
            Kind::CloseBraceToken,
            Kind::OpenBracketToken,
            Kind::CloseBracketToken,
            Kind::SemicolonToken,
            Kind::CommaToken,
            Kind::DotToken,
            Kind::EndOfFile,
        ]
    );
}

// Go: internal/scanner/scanner.go:Scan (longest-match compound operator arms)
#[test]
fn scan_compound_operators() {
    // NOTE: raw `Scan` never coalesces `>` runs; `>>`/`>>>` come from
    // `ReScanGreaterThanToken` (covered separately). These are the operators
    // the main scan loop assembles directly.
    assert_eq!(
        scan_kinds("=> === !== ?. ?? ??= **= <<="),
        vec![
            Kind::EqualsGreaterThanToken,
            Kind::EqualsEqualsEqualsToken,
            Kind::ExclamationEqualsEqualsToken,
            Kind::QuestionDotToken,
            Kind::QuestionQuestionToken,
            Kind::QuestionQuestionEqualsToken,
            Kind::AsteriskAsteriskEqualsToken,
            Kind::LessThanLessThanEqualsToken,
            Kind::EndOfFile,
        ]
    );
}

/// Scans `src`, returning `(Kind, token_value)` pairs (excluding EOF).
fn scan_kinds_values(src: &str) -> Vec<(Kind, String)> {
    let mut s = Scanner::new();
    s.set_text(src.to_string());
    let mut out = Vec::new();
    loop {
        let k = s.scan();
        if k == Kind::EndOfFile {
            break;
        }
        out.push((k, s.token_value().to_string()));
    }
    out
}

// Go: internal/scanner/scanner.go:GetIdentifierToken
#[test]
fn scan_keyword_vs_identifier() {
    assert_eq!(
        scan_kinds_values("let x const yield foo"),
        vec![
            (Kind::LetKeyword, "let".to_string()),
            (Kind::Identifier, "x".to_string()),
            (Kind::ConstKeyword, "const".to_string()),
            (Kind::YieldKeyword, "yield".to_string()),
            (Kind::Identifier, "foo".to_string()),
        ]
    );
}

// Go: internal/scanner/scanner.go:scanIdentifier ($ and _ are identifier chars)
#[test]
fn scan_identifier_dollar_underscore() {
    assert_eq!(
        scan_kinds_values("$_a _ $123"),
        vec![
            (Kind::Identifier, "$_a".to_string()),
            (Kind::Identifier, "_".to_string()),
            (Kind::Identifier, "$123".to_string()),
        ]
    );
}

// Go: internal/scanner/scanner.go:IsIdentifierStart
#[test]
fn is_identifier_start_ascii() {
    assert!(is_identifier_start('a' as i32));
    assert!(is_identifier_start('_' as i32));
    assert!(is_identifier_start('$' as i32));
    assert!(!is_identifier_start('1' as i32));
    assert!(!is_identifier_start(' ' as i32));
}

// Go: internal/scanner/scanner.go:IsIdentifierPartEx (JSX allows '-' and ':')
#[test]
fn is_identifier_part_jsx_dash() {
    use tsgo_core::languagevariant::LanguageVariant;
    assert!(is_identifier_part_ex('-' as i32, LanguageVariant::Jsx));
    assert!(is_identifier_part_ex(':' as i32, LanguageVariant::Jsx));
    assert!(!is_identifier_part_ex(
        '-' as i32,
        LanguageVariant::Standard
    ));
    assert!(is_identifier_part('a' as i32));
    assert!(!is_identifier_part('-' as i32));
}

// Go: internal/scanner/scanner.go:isInUnicodeRanges
#[test]
fn is_in_unicode_ranges_binary_search() {
    // ß (U+00DF) is an ID_Start; @ (U+0040) is not; a CJK ideograph is.
    assert!(is_identifier_start(0x00DF));
    assert!(!is_identifier_start(0x0040));
    assert!(is_identifier_start(0x4E00)); // CJK unified ideograph
    assert!(!is_identifier_part(0x0040));
}

// Go: internal/scanner/scanner.go:IsValidIdentifier
#[test]
fn is_valid_identifier_full() {
    assert!(is_valid_identifier("foo"));
    assert!(!is_valid_identifier("1foo"));
    assert!(!is_valid_identifier(""));
    assert!(!is_valid_identifier("a-b"));
}

// Go: internal/scanner/scanner.go:GetIdentifierToken (length and first-char bounds)
#[test]
fn get_identifier_token_bounds() {
    assert_eq!(get_identifier_token("let"), Kind::LetKeyword);
    assert_eq!(get_identifier_token("Let"), Kind::Identifier); // uppercase first char
    assert_eq!(get_identifier_token("abstractx"), Kind::Identifier); // not a keyword
    assert_eq!(get_identifier_token("a"), Kind::Identifier); // length < 2
}

// Go: internal/scanner/scanner.go:TokenToString / StringToToken
#[test]
fn token_to_string_roundtrip() {
    assert_eq!(token_to_string(Kind::PlusToken), "+");
    assert_eq!(string_to_token("+"), Kind::PlusToken);
    assert_eq!(string_to_token("let"), Kind::LetKeyword);
    assert_eq!(string_to_token("=>"), Kind::EqualsGreaterThanToken);
    assert_eq!(string_to_token("not-a-token"), Kind::Unknown);
}

/// Scans exactly one token and returns its kind, value, and flags.
fn scan_one(src: &str) -> (Kind, String, TokenFlags) {
    let mut s = Scanner::new();
    s.set_text(src.to_string());
    let k = s.scan();
    (k, s.token_value().to_string(), s.token_flags())
}

// Go: internal/scanner/scanner.go:scanNumber / scanBigIntSuffix / scanHexDigits
#[test]
fn scan_numeric_literals() {
    let got: Vec<(Kind, String)> = scan_kinds_values("0 0x1F 0b1010 0o17 1_000 3.14 1e3 5n");
    assert_eq!(
        got,
        vec![
            (Kind::NumericLiteral, "0".to_string()),
            (Kind::NumericLiteral, "31".to_string()),
            (Kind::NumericLiteral, "10".to_string()),
            (Kind::NumericLiteral, "15".to_string()),
            (Kind::NumericLiteral, "1000".to_string()),
            (Kind::NumericLiteral, "3.14".to_string()),
            (Kind::NumericLiteral, "1000".to_string()),
            (Kind::BigIntLiteral, "5n".to_string()),
        ]
    );
}

// Go: internal/scanner/scanner.go:scanString / scanEscapeSequence
#[test]
fn scan_string_literal_value_double_quote_escape() {
    // Source text is the four characters: " a \ n b "
    let (kind, value, flags) = scan_one(r#""a\nb""#);
    assert_eq!(kind, Kind::StringLiteral);
    assert_eq!(value, "a\nb");
    assert!(!flags.contains(TokenFlags::SINGLE_QUOTE));
}

// Go: internal/scanner/scanner.go:scanString (single-quote flag)
#[test]
fn scan_string_literal_single_quote_flag() {
    let (kind, value, flags) = scan_one("'b'");
    assert_eq!(kind, Kind::StringLiteral);
    assert_eq!(value, "b");
    assert!(flags.contains(TokenFlags::SINGLE_QUOTE));
}

// Go: internal/scanner/scanner.go:scanTemplateAndSetTokenValue
#[test]
fn scan_template_no_substitution_and_head() {
    let (kind, value, _) = scan_one("`abc`");
    assert_eq!(kind, Kind::NoSubstitutionTemplateLiteral);
    assert_eq!(value, "abc");

    let (kind, value, _) = scan_one("`a${");
    assert_eq!(kind, Kind::TemplateHead);
    assert_eq!(value, "a");
}

// Go: internal/scanner/scanner.go:Scanner.charAndSize / ContainsNonASCII
#[test]
fn contains_non_ascii_flag_set() {
    let mut s = Scanner::new();
    s.set_text("a\u{1F600}".to_string());
    while s.scan() != Kind::EndOfFile {}
    assert!(s.contains_non_ascii());

    let mut s2 = Scanner::new();
    s2.set_text("ab".to_string());
    while s2.scan() != Kind::EndOfFile {}
    assert!(!s2.contains_non_ascii());
}

// Go: internal/scanner/scanner.go:ReScanLessThanToken (splits `<<` into `<`)
#[test]
fn rescan_less_than_splits() {
    let mut s = Scanner::new();
    s.set_text("<<".to_string());
    assert_eq!(s.scan(), Kind::LessThanLessThanToken);
    assert_eq!(s.token_end(), 2);
    assert_eq!(s.re_scan_less_than_token(), Kind::LessThanToken);
    assert_eq!(s.token_end(), 1);
}

// Go: internal/scanner/scanner.go:ReScanGreaterThanToken (combines `>` runs)
#[test]
fn rescan_greater_than_combines() {
    let mut s = Scanner::new();
    s.set_text(">>".to_string());
    assert_eq!(s.scan(), Kind::GreaterThanToken); // Scan never coalesces `>`
    assert_eq!(s.token_end(), 1);
    assert_eq!(
        s.re_scan_greater_than_token(),
        Kind::GreaterThanGreaterThanToken
    );
    assert_eq!(s.token_end(), 2);

    let mut s2 = Scanner::new();
    s2.set_text(">>=".to_string());
    assert_eq!(s2.scan(), Kind::GreaterThanToken);
    assert_eq!(
        s2.re_scan_greater_than_token(),
        Kind::GreaterThanGreaterThanEqualsToken
    );
    assert_eq!(s2.token_end(), 3);
}

// Go: internal/scanner/scanner.go:ReScanQuestionToken / ReScanAsteriskEqualsToken
#[test]
fn rescan_question_and_asterisk_equals() {
    let mut s = Scanner::new();
    s.set_text("??".to_string());
    assert_eq!(s.scan(), Kind::QuestionQuestionToken);
    assert_eq!(s.re_scan_question_token(), Kind::QuestionToken);
    assert_eq!(s.token_end(), 1);

    let mut s2 = Scanner::new();
    s2.set_text("*=".to_string());
    assert_eq!(s2.scan(), Kind::AsteriskEqualsToken);
    assert_eq!(s2.re_scan_asterisk_equals_token(), Kind::EqualsToken);
}

// Go: internal/scanner/scanner.go:ReScanHashToken
#[test]
fn rescan_hash() {
    let mut s = Scanner::new();
    s.set_text("#foo".to_string());
    assert_eq!(s.scan(), Kind::PrivateIdentifier);
    assert_eq!(s.re_scan_hash_token(), Kind::HashToken);
    assert_eq!(s.token_end(), 1);
}

// Go: internal/scanner/scanner.go:ReScanSlashToken
#[test]
fn rescan_slash_to_regex() {
    let mut s = Scanner::new();
    s.set_text("/ab/g".to_string());
    assert_eq!(s.scan(), Kind::SlashToken);
    assert_eq!(s.re_scan_slash_token(false), Kind::RegularExpressionLiteral);
    assert_eq!(s.token_value(), "/ab/g");
    assert_eq!(s.token_end(), 5);
}

// Go: internal/scanner/scanner.go:ReScanTemplateToken / scanTemplateAndSetTokenValue
#[test]
fn rescan_template_tail() {
    let mut s = Scanner::new();
    s.set_text("}b`".to_string());
    assert_eq!(s.scan(), Kind::CloseBraceToken);
    assert_eq!(s.re_scan_template_token(false), Kind::TemplateTail);
    assert_eq!(s.token_value(), "b");
}

// Go: internal/scanner/scanner.go:ScanJsxTokenEx (JsxText)
#[test]
fn scan_jsx_text() {
    let mut s = Scanner::new();
    s.set_language_variant(tsgo_core::languagevariant::LanguageVariant::Jsx);
    s.set_text("hello<".to_string());
    assert_eq!(s.scan_jsx_token(), Kind::JsxText);
    assert_eq!(s.token_value(), "hello");
}

// Go: internal/scanner/scanner.go:Scan (JSX `</` token)
#[test]
fn scan_jsx_less_than_slash() {
    let mut s = Scanner::new();
    s.set_language_variant(tsgo_core::languagevariant::LanguageVariant::Jsx);
    s.set_text("</div>".to_string());
    assert_eq!(s.scan(), Kind::LessThanSlashToken);
}

// Go: internal/scanner/scanner.go:ScanJsxIdentifier (allows '-')
#[test]
fn scan_jsx_identifier_dash() {
    let mut s = Scanner::new();
    s.set_language_variant(tsgo_core::languagevariant::LanguageVariant::Jsx);
    s.set_text("data-foo=".to_string());
    assert_eq!(s.scan(), Kind::Identifier);
    assert_eq!(s.token_value(), "data");
    assert_eq!(s.scan_jsx_identifier(), Kind::Identifier);
    assert_eq!(s.token_value(), "data-foo");
}

// Go: internal/scanner/scanner.go:Scan (trivia skipped by default)
#[test]
fn scan_trivia_skipped_default() {
    let mut s = Scanner::new();
    s.set_text("  \n\t a".to_string());
    assert_eq!(s.scan(), Kind::Identifier);
    assert_eq!(s.token_value(), "a");
    assert!(s.has_preceding_line_break());
    assert_eq!(s.scan(), Kind::EndOfFile);
}

// Go: internal/scanner/scanner.go:Scan (trivia emitted when disabled)
#[test]
fn scan_trivia_emitted_when_disabled() {
    let mut s = Scanner::new();
    s.set_skip_trivia(false);
    s.set_text(" \n//c\n a".to_string());
    let mut kinds = Vec::new();
    loop {
        let k = s.scan();
        kinds.push(k);
        if k == Kind::EndOfFile {
            break;
        }
    }
    assert_eq!(
        kinds,
        vec![
            Kind::WhitespaceTrivia,
            Kind::NewLineTrivia,
            Kind::SingleLineCommentTrivia,
            Kind::NewLineTrivia,
            Kind::WhitespaceTrivia,
            Kind::Identifier,
            Kind::EndOfFile,
        ]
    );
}

// Go: internal/scanner/scanner.go:Scan / processCommentDirective
#[test]
fn comment_directive_collected() {
    let mut s = Scanner::new();
    s.set_text("//@ts-ignore\n x".to_string());
    while s.scan() != Kind::EndOfFile {}
    let directives = s.comment_directives();
    assert_eq!(directives.len(), 1);
    assert_eq!(directives[0].kind, CommentDirectiveKind::Ignore);
    assert_eq!(directives[0].loc.pos(), 0);
}

// Go: internal/scanner/scanner.go:SkipTrivia
#[test]
fn skip_trivia_basic() {
    assert_eq!(skip_trivia("  /*c*/ x", 0), 8);
}

// Go: internal/scanner/scanner.go:SkipTriviaEx (StopAtComments)
#[test]
fn skip_trivia_stop_at_comments() {
    let opts = SkipTriviaOptions {
        stop_at_comments: true,
        ..SkipTriviaOptions::default()
    };
    assert_eq!(skip_trivia_ex(" //c\n x", 0, Some(&opts)), 1);
}

// Go: internal/scanner/scanner.go:GetShebang
#[test]
fn shebang_detected_at_zero() {
    assert_eq!(get_shebang("#!/usr/bin/env node\n"), "#!/usr/bin/env node");
    assert_eq!(get_shebang("x#!/usr/bin/env node"), "");
}

// Go: internal/scanner/scanner.go:isConflictMarkerTrivia
#[test]
fn conflict_marker_trivia_detected() {
    assert!(is_conflict_marker_trivia("<<<<<<< HEAD", 0));
    // A marker run must be followed by content (the `pos+7 < len` check), so a
    // bare 7-char run is not detected, but `=======` followed by a newline is.
    assert!(is_conflict_marker_trivia("=======\n", 0));
    assert!(!is_conflict_marker_trivia("=======", 0));
    assert!(!is_conflict_marker_trivia("<<< HEAD", 0));
}

// Go: internal/scanner/scanner.go:GetLeadingCommentRanges / iterateCommentRanges
#[test]
fn leading_comment_ranges_two_block_comments() {
    let ranges = get_leading_comment_ranges("/*a*//*b*/x", 0);
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0].kind, Kind::MultiLineCommentTrivia);
    assert_eq!((ranges[0].loc.pos(), ranges[0].loc.end()), (0, 5));
    assert_eq!(ranges[1].kind, Kind::MultiLineCommentTrivia);
    assert_eq!((ranges[1].loc.pos(), ranges[1].loc.end()), (5, 10));
}

// Go: internal/scanner/scanner.go:Scan (token range / TokenStart / TokenEnd)
#[test]
fn scan_token_positions_and_full_start() {
    let mut s = Scanner::new();
    s.set_text("  (  )".to_string());
    assert_eq!(s.scan(), Kind::OpenParenToken);
    assert_eq!(s.token_full_start(), 0); // includes leading whitespace
    assert_eq!(s.token_start(), 2);
    assert_eq!(s.token_end(), 3);
    assert_eq!(s.token_text(), "(");
    assert_eq!(s.scan(), Kind::CloseParenToken);
    assert_eq!(s.token_start(), 5);
    assert_eq!(s.token_end(), 6);
}

// Go: internal/scanner/scanner.go:ComputeLineOfPosition
#[test]
fn compute_line_of_position_binary() {
    let line_starts = [TextPos(0), TextPos(5), TextPos(9)];
    assert_eq!(compute_line_of_position(&line_starts, 7), 1);
    assert_eq!(compute_line_of_position(&line_starts, 0), 0);
    assert_eq!(compute_line_of_position(&line_starts, 5), 1);
    assert_eq!(compute_line_of_position(&line_starts, 9), 2);
    assert_eq!(compute_line_of_position(&line_starts, 100), 2);
}

// Go: internal/scanner/scanner.go:GetECMALineOfPosition
#[test]
fn ecma_line_of_position_from_text() {
    // "ab\ncde\nf": line starts at bytes 0, 3, 7.
    let text = "ab\ncde\nf";
    assert_eq!(get_ecma_line_of_position(text, 0), 0);
    assert_eq!(get_ecma_line_of_position(text, 2), 0);
    assert_eq!(get_ecma_line_of_position(text, 3), 1);
    assert_eq!(get_ecma_line_of_position(text, 6), 1);
    assert_eq!(get_ecma_line_of_position(text, 7), 2);
    // CRLF is one terminator: "a\r\nb" has line 1 starting after `\r\n`.
    assert_eq!(get_ecma_line_of_position("a\r\nb", 3), 1);
}

// Go: internal/scanner/scanner.go:GetECMALineAndUTF16CharacterOfPosition
#[test]
fn line_and_utf16_char_with_astral() {
    // "a😀b": emoji is UTF-8 4 bytes / UTF-16 2 code units; 'b' starts at byte 5.
    let text = "a\u{1F600}b";
    let line_starts = tsgo_core::compute_ecma_line_starts(text);
    let line = compute_line_of_position(&line_starts, 5);
    assert_eq!(line, 0);
    let character = tsgo_core::utf16_len(&text[line_starts[line as usize].0 as usize..5]);
    // a = 1 UTF-16 unit, emoji = 2 => character 3.
    assert_eq!(character, Utf16Offset(3));
}

// Go: internal/scanner/scanner.go:GetECMALineAndByteOffsetOfPosition
#[test]
fn line_and_byte_offset_with_astral() {
    let text = "a\u{1F600}b";
    let line_starts = tsgo_core::compute_ecma_line_starts(text);
    let line = compute_line_of_position(&line_starts, 5);
    let byte_offset = 5 - line_starts[line as usize].0;
    assert_eq!((line, byte_offset), (0, 5));
}

// Go: internal/scanner/scanner.go:ComputePositionOfLineAndUTF16Character
#[test]
fn position_of_line_and_utf16_char_roundtrip() {
    let text = "a\u{1F600}b";
    let line_starts = tsgo_core::compute_ecma_line_starts(text);
    assert_eq!(
        compute_position_of_line_and_utf16_character(&line_starts, 0, Utf16Offset(3), text, false),
        5
    );
    assert_eq!(
        compute_position_of_line_and_utf16_character(&line_starts, 0, Utf16Offset(1), text, false),
        1
    );
    assert_eq!(
        compute_position_of_line_and_utf16_character(&line_starts, 0, Utf16Offset(0), text, false),
        0
    );
}

// Go: internal/scanner/scanner.go:ComputePositionOfLineAndUTF16Character
#[test]
fn position_of_line_and_utf16_char_clamp_allows_edits() {
    let text = "ab";
    let line_starts = tsgo_core::compute_ecma_line_starts(text);
    // character past end with allow_edits clamps to len(text).
    assert_eq!(
        compute_position_of_line_and_utf16_character(&line_starts, 0, Utf16Offset(99), text, true),
        2
    );
}

// Go: internal/scanner/scanner.go:ComputePositionOfLineAndUTF16Character
#[test]
#[should_panic(expected = "Bad UTF-16 character offset")]
fn position_of_line_and_utf16_char_panics_out_of_range() {
    let text = "ab";
    let line_starts = tsgo_core::compute_ecma_line_starts(text);
    compute_position_of_line_and_utf16_character(&line_starts, 0, Utf16Offset(99), text, false);
}

// Go: internal/scanner/scanner.go:ComputePositionOfLineAndByteOffset
#[test]
fn position_of_line_and_byte_offset_basic() {
    let line_starts = [TextPos(0), TextPos(5), TextPos(9)];
    assert_eq!(
        compute_position_of_line_and_byte_offset(&line_starts, 1, 2),
        7
    );
    assert_eq!(
        compute_position_of_line_and_byte_offset(&line_starts, 0, 0),
        0
    );
}

// Go: internal/core/core.go:UTF16Len (fast path used by the line/char helpers)
#[test]
fn utf16_len_ascii_and_astral() {
    assert_eq!(tsgo_core::utf16_len("abc"), Utf16Offset(3));
    assert_eq!(tsgo_core::utf16_len("\u{00E9}"), Utf16Offset(1)); // é (BMP)
    assert_eq!(tsgo_core::utf16_len("\u{1F600}"), Utf16Offset(2)); // emoji (astral)
    assert_eq!(tsgo_core::utf16_len("a\u{1F600}b"), Utf16Offset(4));
}
