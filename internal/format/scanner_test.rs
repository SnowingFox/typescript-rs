use super::*;
use tsgo_ast::Kind;
use tsgo_core::languagevariant::LanguageVariant;

/// A rescan context whose container is the (non-token) source file, so
/// `fixTokenKind` is a no-op and the raw scanned token is returned.
fn raw_ctx() -> TokenRescanContext {
    TokenRescanContext {
        node_kind: Kind::SourceFile,
        node_parent_kind: None,
    }
}

// Go: internal/format/scanner.go:formattingScanner.advance + readTokenInfo
#[test]
fn scans_token_sequence() {
    let text = "1+2";
    let mut s = FormattingScanner::new(text, LanguageVariant::Standard, 0, text.len() as i32);
    s.advance();
    assert!(s.is_on_token());
    let one = s.read_token_info(raw_ctx());
    assert_eq!(one.token.kind, Kind::NumericLiteral);
    assert_eq!((one.token.loc.pos(), one.token.loc.end()), (0, 1));

    s.advance();
    let plus = s.read_token_info(raw_ctx());
    assert_eq!(plus.token.kind, Kind::PlusToken);
    assert_eq!((plus.token.loc.pos(), plus.token.loc.end()), (1, 2));

    s.advance();
    let two = s.read_token_info(raw_ctx());
    assert_eq!(two.token.kind, Kind::NumericLiteral);
    assert_eq!((two.token.loc.pos(), two.token.loc.end()), (2, 3));

    s.advance();
    assert!(s.is_on_eof());
}

// Whitespace between two tokens is attached as the first token's trailing trivia.
// Go: internal/format/scanner.go:formattingScanner.readTokenInfo (trailing trivia)
#[test]
fn attaches_trailing_whitespace_trivia() {
    let text = "a b";
    let mut s = FormattingScanner::new(text, LanguageVariant::Standard, 0, text.len() as i32);
    s.advance();
    let a = s.read_token_info(raw_ctx());
    assert_eq!(a.token.kind, Kind::Identifier);
    assert_eq!(a.trailing_trivia.len(), 1);
    assert_eq!(a.trailing_trivia[0].kind, Kind::WhitespaceTrivia);
    // `was_new_line` reflects the previous token's trailing trivia after advancing.
    s.advance();
    assert!(!s.last_trailing_trivia_was_new_line());
}

// A trailing newline is recorded and flips `lastTrailingTriviaWasNewLine`.
// Go: internal/format/scanner.go:formattingScanner.advance (wasNewLine)
#[test]
fn tracks_trailing_newline() {
    let text = "a\nb";
    let mut s = FormattingScanner::new(text, LanguageVariant::Standard, 0, text.len() as i32);
    s.advance();
    let a = s.read_token_info(raw_ctx());
    assert_eq!(a.token.kind, Kind::Identifier);
    assert_eq!(a.trailing_trivia.last().unwrap().kind, Kind::NewLineTrivia);
    s.advance();
    assert!(s.last_trailing_trivia_was_new_line());
    let b = s.read_token_info(raw_ctx());
    assert_eq!(b.token.kind, Kind::Identifier);
    assert_eq!((b.token.loc.pos(), b.token.loc.end()), (2, 3));
}

// `isOnToken`/`isOnEOF` over an empty range start straight at EOF.
// Go: internal/format/scanner.go:formattingScanner.isOnEOF
#[test]
fn empty_text_is_on_eof_after_advance() {
    let mut s = FormattingScanner::new("", LanguageVariant::Standard, 0, 0);
    s.advance();
    assert!(s.is_on_eof());
    assert!(!s.is_on_token());
}

// readEOFTokenRange yields a zero-width EOF range at the end.
// Go: internal/format/scanner.go:formattingScanner.readEOFTokenRange
#[test]
fn reads_eof_token_range() {
    let text = "x";
    let mut s = FormattingScanner::new(text, LanguageVariant::Standard, 0, text.len() as i32);
    s.advance();
    let _ = s.read_token_info(raw_ctx());
    s.advance();
    assert!(s.is_on_eof());
    let eof = s.read_eof_token_range();
    assert_eq!(eof.kind, Kind::EndOfFile);
}
