use super::*;

// Go: internal/stringutil/util_test.go:TestEncodeURI/encodes spaces as percent20
#[test]
fn encode_uri_encodes_spaces_as_percent20() {
    assert_eq!(encode_uri("a b"), "a%20b");
}

// Go: internal/stringutil/util_test.go:TestEncodeURI/preserves reserved uri characters
#[test]
fn encode_uri_preserves_reserved_uri_characters() {
    assert_eq!(encode_uri(";/?:@&=+$,#"), ";/?:@&=+$,#");
}

// Go: internal/stringutil/util_test.go:TestEncodeURI/encodes brackets and unicode using utf8 bytes
#[test]
fn encode_uri_encodes_brackets_and_unicode_using_utf8_bytes() {
    assert_eq!(
        encode_uri("①Ⅻㄨㄩ U1[abc]"),
        "%E2%91%A0%E2%85%AB%E3%84%A8%E3%84%A9%20U1%5Babc%5D"
    );
}

// Go: internal/stringutil/util.go:IsWhiteSpaceLike (behavior-level supplement)
#[test]
fn is_white_space_like_basics() {
    assert!(is_white_space_like(' '));
    assert!(is_white_space_like('\t'));
    assert!(is_white_space_like('\n'));
    assert!(!is_white_space_like('a'));
    assert!(is_white_space_like('\u{FEFF}'));
}

// Go: internal/stringutil/util.go:IsLineBreak (behavior-level supplement)
#[test]
fn is_line_break_set() {
    assert!(is_line_break('\n'));
    assert!(is_line_break('\r'));
    assert!(is_line_break('\u{2028}'));
    assert!(is_line_break('\u{2029}'));
    assert!(!is_line_break(' '));
}

// Go: internal/stringutil/util.go:IsDigit/IsOctalDigit/IsHexDigit/IsASCIILetter (behavior-level supplement)
#[test]
fn is_digit_octal_hex_ascii() {
    assert!(is_octal_digit('7'));
    assert!(!is_octal_digit('8'));
    assert!(is_hex_digit('f'));
    assert!(!is_hex_digit('g'));
    assert!(is_ascii_letter('Z'));
    assert!(is_digit('0'));
}

// Go: internal/stringutil/util.go:SplitLines (behavior-level supplement)
#[test]
fn split_lines_crlf_lf_cr() {
    assert_eq!(split_lines("a\r\nb\nc\rd"), vec!["a", "b", "c", "d"]);
}

// Go: internal/stringutil/util.go:SplitLines (behavior-level supplement)
#[test]
fn split_lines_trailing() {
    assert_eq!(split_lines("a\n"), vec!["a"]);
    assert_eq!(split_lines("a\nb"), vec!["a", "b"]);
}

// Go: internal/stringutil/util.go:GuessIndentation (behavior-level supplement)
#[test]
fn guess_indentation_min() {
    assert_eq!(guess_indentation(&["  a", "    b", "   c"]), 2);
    assert_eq!(guess_indentation(&["", "  a", "    b"]), 2);
}

// Go: internal/stringutil/util.go:GuessIndentation (behavior-level supplement)
#[test]
fn guess_indentation_zero_and_empty() {
    assert_eq!(guess_indentation(&["a"]), 0);
    assert_eq!(guess_indentation(&[]), 0);
    assert_eq!(guess_indentation(&["", ""]), 0);
}

// Go: internal/stringutil/util.go:RemoveByteOrderMark (behavior-level supplement)
#[test]
fn remove_byte_order_mark_utf8() {
    assert_eq!(remove_byte_order_mark("\u{FEFF}abc"), "abc");
    assert_eq!(remove_byte_order_mark("abc"), "abc");
}

// Go: internal/stringutil/util.go:AddUTF8ByteOrderMark (behavior-level supplement)
#[test]
fn add_utf8_byte_order_mark_test() {
    assert_eq!(add_utf8_byte_order_mark("abc"), "\u{FEFF}abc");
    assert_eq!(add_utf8_byte_order_mark("\u{FEFF}abc"), "\u{FEFF}abc");
}

// Go: internal/stringutil/util.go:StripQuotes (behavior-level supplement)
#[test]
fn strip_quotes_pairs() {
    assert_eq!(strip_quotes("\"x\""), "x");
    assert_eq!(strip_quotes("'x'"), "x");
    assert_eq!(strip_quotes("`x`"), "x");
    assert_eq!(strip_quotes("x"), "x");
    assert_eq!(strip_quotes("\""), "\"");
}

// Go: internal/stringutil/util.go:UnquoteString (behavior-level supplement)
#[test]
fn unquote_string_backslash() {
    assert_eq!(unquote_string("\"a\\nb\""), "anb");
}

// Go: internal/stringutil/util.go:LowerFirstChar (behavior-level supplement)
#[test]
fn lower_first_char_cases() {
    assert_eq!(lower_first_char("Foo"), "foo");
    assert_eq!(lower_first_char(""), "");
    assert_eq!(lower_first_char("Ünder"), "ünder");
}

// Go: internal/stringutil/util.go:TruncateByRunes (behavior-level supplement)
#[test]
fn truncate_by_runes_basic() {
    assert_eq!(truncate_by_runes("hello", 3), "hel");
    assert_eq!(truncate_by_runes("hi", 5), "hi");
    assert_eq!(truncate_by_runes("x", 0), "");
}
