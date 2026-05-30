use super::*;

// Go: internal/modulespecifiers/tests.md supplementary (compare.go:CountPathComponents)
// `CountPathComponents` counts the `/` separators after stripping a leading
// `./`, so `./a/b/c` -> `a/b/c` has 2 separators. (tests.md states 3, which
// miscounts components rather than separators; ground truth is Go behavior.)
#[test]
fn count_path_components_strips_leading_dot_slash() {
    assert_eq!(count_path_components("./a/b/c"), 2);
}

#[test]
fn count_path_components_plain_relative() {
    assert_eq!(count_path_components("a/b"), 1);
}
