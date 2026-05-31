use super::*;

// Go: internal/testutil/stringtestutil/stringtestutil.go:Dedent
// The Go package ships no `_test.go`, so these are behavior-level supplements
// (PORTING §8.6). Every expected value is captured from the real Go `Dedent`.

/// Slice 1 (identity): a single line with no surrounding blanks and no common
/// indentation is returned unchanged.
#[test]
fn single_line_without_indentation_is_unchanged() {
    // Go ground truth: Dedent("hello") == "hello"
    assert_eq!(dedent("hello"), "hello");
}

/// Slice 2 (strip surrounding blanks): leading/trailing blank lines are
/// removed even when there is no common indentation to strip.
#[test]
fn strips_leading_and_trailing_blank_lines() {
    // Go ground truth: Dedent("\nhello\n") == "hello"
    assert_eq!(dedent("\nhello\n"), "hello");
}

/// Slice 3 (remove common indentation): the minimum leading indentation across
/// non-blank lines is removed, preserving relative nesting.
#[test]
fn removes_common_leading_indentation() {
    // Go ground truth:
    // Dedent("\n    function f() {\n        return 1;\n    }\n")
    //   == "function f() {\n    return 1;\n}"
    assert_eq!(
        dedent("\n    function f() {\n        return 1;\n    }\n"),
        "function f() {\n    return 1;\n}"
    );
}

/// Leading tabs are expanded to 4 spaces before the common indentation is
/// measured, so a tab-indented fixture dedents like a space-indented one.
#[test]
fn expands_leading_tabs_to_four_spaces() {
    // Go ground truth: Dedent("\n\tfoo\n\t\tbar\n") == "foo\n    bar"
    assert_eq!(dedent("\n\tfoo\n\t\tbar\n"), "foo\n    bar");
}

/// Blank lines inside the content are preserved (emitted as empty lines) while
/// the common indentation of the surrounding non-blank lines is removed.
#[test]
fn preserves_interior_blank_lines() {
    // Go ground truth: Dedent("\n  a\n\n  b\n") == "a\n\nb"
    assert_eq!(dedent("\n  a\n\n  b\n"), "a\n\nb");
}

/// Multiple lines with zero common indentation are returned unchanged.
#[test]
fn multi_line_without_indentation_is_unchanged() {
    // Go ground truth: Dedent("a\nb\nc") == "a\nb\nc"
    assert_eq!(dedent("a\nb\nc"), "a\nb\nc");
}

/// Mixed tab+space indentation: tabs expand to 4 spaces, then the common
/// width is removed.
#[test]
fn handles_mixed_tab_and_space_indentation() {
    // Go ground truth: Dedent("\n\t  x\n\t\t  y\n") == "x\n    y"
    assert_eq!(dedent("\n\t  x\n\t\t  y\n"), "x\n    y");
}
