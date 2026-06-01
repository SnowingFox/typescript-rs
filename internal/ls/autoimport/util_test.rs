use super::*;

// Go: internal/ls/autoimport/util_test.go:TestWordIndices
//
// Each case maps the input through `word_indices` and reconstructs the word
// slices (`s[idx:]`) to compare against the expected words, exactly like the Go
// table test.
fn check(input: &str, expected_words: &[&str]) {
    let indices = word_indices(input);
    let actual: Vec<&str> = indices.iter().map(|&i| &input[i..]).collect();
    assert_eq!(actual, expected_words, "word_indices({input:?})");
}

#[test]
fn camel_case() {
    check("camelCase", &["camelCase", "Case"]);
}

#[test]
fn snake_case() {
    check("snake_case", &["snake_case", "case"]);
}

#[test]
fn parse_url() {
    check("ParseURL", &["ParseURL", "URL"]);
}

#[test]
fn xml_http_request() {
    check(
        "XMLHttpRequest",
        &["XMLHttpRequest", "HttpRequest", "Request"],
    );
}

#[test]
fn single_word_lowercase() {
    check("hello", &["hello"]);
}

#[test]
fn single_word_uppercase() {
    check("HELLO", &["HELLO"]);
}

#[test]
fn mixed_with_numbers() {
    check(
        "parseHTML5Parser",
        &["parseHTML5Parser", "HTML5Parser", "Parser"],
    );
}

#[test]
fn double_underscore_proto() {
    check("__proto__", &["__proto__", "proto__"]);
}

#[test]
fn underscore_private_member() {
    check("_private_member", &["_private_member", "member"]);
}

#[test]
fn single_char_lower() {
    check("a", &["a"]);
}

#[test]
fn single_char_upper() {
    check("A", &["A"]);
}

#[test]
fn consecutive_underscores() {
    check(
        "test__double__underscore",
        &[
            "test__double__underscore",
            "double__underscore",
            "underscore",
        ],
    );
}
