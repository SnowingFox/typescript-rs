use super::*;

// Go: internal/printer/utilities_test.go:TestEscapeString
#[test]
fn escape_string_table() {
    let cases: &[(&str, QuoteChar, &str)] = &[
        ("", QuoteChar::DoubleQuote, ""),
        ("abc", QuoteChar::DoubleQuote, "abc"),
        ("ab\"c", QuoteChar::DoubleQuote, "ab\\\"c"),
        ("ab\tc", QuoteChar::DoubleQuote, "ab\\tc"),
        ("ab\nc", QuoteChar::DoubleQuote, "ab\\nc"),
        ("ab'c", QuoteChar::DoubleQuote, "ab'c"),
        ("ab'c", QuoteChar::SingleQuote, "ab\\'c"),
        ("ab\"c", QuoteChar::SingleQuote, "ab\"c"),
        ("ab`c", QuoteChar::Backtick, "ab\\`c"),
        ("\u{1f}", QuoteChar::Backtick, "\\u001F"),
    ];
    for (i, (s, q, expected)) in cases.iter().enumerate() {
        assert_eq!(escape_string(s, *q), *expected, "case [{i}]");
    }
}

// Go: internal/printer/utilities_test.go:TestEscapeNonAsciiString
#[test]
fn escape_non_ascii_string_table() {
    let cases: &[(&str, QuoteChar, &str)] = &[
        ("", QuoteChar::DoubleQuote, ""),
        ("abc", QuoteChar::DoubleQuote, "abc"),
        ("ab\"c", QuoteChar::DoubleQuote, "ab\\\"c"),
        ("ab\tc", QuoteChar::DoubleQuote, "ab\\tc"),
        ("ab\nc", QuoteChar::DoubleQuote, "ab\\nc"),
        ("ab'c", QuoteChar::DoubleQuote, "ab'c"),
        ("ab'c", QuoteChar::SingleQuote, "ab\\'c"),
        ("ab\"c", QuoteChar::SingleQuote, "ab\"c"),
        ("ab`c", QuoteChar::Backtick, "ab\\`c"),
        ("ab\u{8f}c", QuoteChar::DoubleQuote, "ab\\u008Fc"),
        ("𝟘𝟙", QuoteChar::DoubleQuote, "\\uD835\\uDFD8\\uD835\\uDFD9"),
    ];
    for (i, (s, q, expected)) in cases.iter().enumerate() {
        assert_eq!(escape_non_ascii_string(s, *q), *expected, "case [{i}]");
    }
}

// Go: internal/printer/utilities_test.go:TestEscapeJsxAttributeString
#[test]
fn escape_jsx_attribute_string_table() {
    let cases: &[(&str, QuoteChar, &str)] = &[
        ("", QuoteChar::DoubleQuote, ""),
        ("abc", QuoteChar::DoubleQuote, "abc"),
        ("ab\"c", QuoteChar::DoubleQuote, "ab&quot;c"),
        ("ab\tc", QuoteChar::DoubleQuote, "ab&#x9;c"),
        ("ab\nc", QuoteChar::DoubleQuote, "ab&#xA;c"),
        ("ab'c", QuoteChar::DoubleQuote, "ab'c"),
        ("ab'c", QuoteChar::SingleQuote, "ab&apos;c"),
        ("ab\"c", QuoteChar::SingleQuote, "ab\"c"),
        ("ab\u{8f}c", QuoteChar::DoubleQuote, "ab\u{8f}c"),
        ("𝟘𝟙", QuoteChar::DoubleQuote, "𝟘𝟙"),
    ];
    for (i, (s, q, expected)) in cases.iter().enumerate() {
        assert_eq!(escape_jsx_attribute_string(s, *q), *expected, "case [{i}]");
    }
}

// Go: internal/printer/utilities_test.go:TestIsRecognizedTripleSlashComment
#[test]
fn is_recognized_triple_slash_comment_table() {
    struct Case {
        s: &'static str,
        explicit_kind: Option<Kind>,
        expected: bool,
    }
    let cases = [
        Case {
            s: "",
            explicit_kind: Some(Kind::MultiLineCommentTrivia),
            expected: false,
        },
        Case {
            s: "",
            explicit_kind: Some(Kind::SingleLineCommentTrivia),
            expected: false,
        },
        Case {
            s: "/a",
            explicit_kind: None,
            expected: false,
        },
        Case {
            s: "//",
            explicit_kind: None,
            expected: false,
        },
        Case {
            s: "//a",
            explicit_kind: None,
            expected: false,
        },
        Case {
            s: "///",
            explicit_kind: None,
            expected: false,
        },
        Case {
            s: "///a",
            explicit_kind: None,
            expected: false,
        },
        Case {
            s: "///<reference path=\"foo\" />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "///<reference types=\"foo\" />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "///<reference lib=\"foo\" />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "///<reference no-default-lib=\"foo\" />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "///<amd-dependency path=\"foo\" />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "///<amd-module />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference path=\"foo\" />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference types=\"foo\" />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference lib=\"foo\" />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference no-default-lib=\"foo\" />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <amd-dependency path=\"foo\" />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <amd-module />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference path=\"foo\"/>",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference types=\"foo\"/>",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference lib=\"foo\"/>",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference no-default-lib=\"foo\"/>",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <amd-dependency path=\"foo\"/>",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <amd-module/>",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference path='foo' />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference types='foo' />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference lib='foo' />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference no-default-lib='foo' />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <amd-dependency path='foo' />",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference path=\"foo\" />  ",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference types=\"foo\" />  ",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference lib=\"foo\" />  ",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <reference no-default-lib=\"foo\" />  ",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <amd-dependency path=\"foo\" />  ",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <amd-module />  ",
            explicit_kind: None,
            expected: true,
        },
        Case {
            s: "/// <foo />",
            explicit_kind: None,
            expected: false,
        },
        Case {
            s: "/// <reference />",
            explicit_kind: None,
            expected: false,
        },
        Case {
            s: "/// <amd-dependency />",
            explicit_kind: None,
            expected: false,
        },
    ];
    for (i, c) in cases.iter().enumerate() {
        let comment_range = match c.explicit_kind {
            Some(kind) => CommentRange::new(kind, 0, c.s.len() as i32, false),
            None => CommentRange::new(Kind::SingleLineCommentTrivia, 0, c.s.len() as i32, false),
        };
        assert_eq!(
            is_recognized_triple_slash_comment(c.s, comment_range),
            c.expected,
            "case [{i}] {:?}",
            c.s
        );
    }
}

// Go: internal/printer/utilities.go:IsPinnedComment
#[test]
fn is_pinned_comment_behavior() {
    let text = "/*! keep */";
    let cr = CommentRange::new(Kind::MultiLineCommentTrivia, 0, text.len() as i32, false);
    assert!(is_pinned_comment(text, cr));

    let text2 = "/* plain */";
    let cr2 = CommentRange::new(Kind::MultiLineCommentTrivia, 0, text2.len() as i32, false);
    assert!(!is_pinned_comment(text2, cr2));

    // Single-line comments are never pinned.
    let text3 = "//! x";
    let cr3 = CommentRange::new(Kind::SingleLineCommentTrivia, 0, text3.len() as i32, false);
    assert!(!is_pinned_comment(text3, cr3));
}

// Go: internal/printer/utilities.go:GetLinesBetweenPositions
#[test]
fn lines_between_positions_and_same_line() {
    let text = "a\nbb\nccc";
    let line_starts = tsgo_core::compute_ecma_line_starts(text);
    // "a" at 0 (line 0), "bb" at 2 (line 1), "ccc" at 5 (line 2).
    assert_eq!(get_lines_between_positions(&line_starts, 0, 2), 1);
    assert_eq!(get_lines_between_positions(&line_starts, 0, 5), 2);
    assert_eq!(get_lines_between_positions(&line_starts, 5, 0), -2);
    assert!(positions_are_on_same_line(&line_starts, 0, 1));
    assert!(!positions_are_on_same_line(&line_starts, 0, 2));
}

// Go: internal/printer/utilities.go:RangeIsOnSingleLine
#[test]
fn range_single_line_and_cross_line() {
    let text = "a\n.b";
    let line_starts = tsgo_core::compute_ecma_line_starts(text);
    // "a" occupies [0,1) on a single line.
    assert!(range_is_on_single_line(
        text,
        &line_starts,
        tsgo_core::text::TextRange::new(0, 1)
    ));
    // A range spanning the newline is not single-line.
    assert!(!range_is_on_single_line(
        text,
        &line_starts,
        tsgo_core::text::TextRange::new(0, 4)
    ));
    // `a`(end=1) and `.b`(start=2) are on different lines.
    assert!(!range_end_is_on_same_line_as_range_start(
        text,
        &line_starts,
        tsgo_core::text::TextRange::new(0, 1),
        tsgo_core::text::TextRange::new(2, 4)
    ));
}

// Go: internal/printer/utilities.go:FormatGeneratedName
#[test]
fn format_generated_name_behavior() {
    assert_eq!(format_generated_name(false, "", "foo", ""), "foo");
    assert_eq!(format_generated_name(false, "p_", "foo", "_s"), "p_foo_s");
    // Leading hashes are stripped from each part, then re-added for private names.
    assert_eq!(format_generated_name(true, "", "#foo", ""), "#foo");
    assert_eq!(format_generated_name(false, "#a", "#b", "#c"), "abc");
    assert_eq!(format_generated_name(true, "#a", "#b", "#c"), "#abc");
}
