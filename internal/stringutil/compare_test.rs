use super::*;

// Go: internal/stringutil/compare.go:CompareStringsCaseSensitive (behavior-level supplement)
#[test]
fn compare_case_sensitive_order() {
    assert_eq!(compare_strings_case_sensitive("a", "b"), Ordering::Less);
    assert_eq!(compare_strings_case_sensitive("b", "a"), Ordering::Greater);
    assert_eq!(compare_strings_case_sensitive("a", "a"), Ordering::Equal);
}

// Go: internal/stringutil/compare.go:CompareStringsCaseInsensitive (behavior-level supplement)
#[test]
fn compare_case_insensitive_order() {
    assert_eq!(
        compare_strings_case_insensitive("ABC", "abc"),
        Ordering::Equal
    );
    assert_eq!(compare_strings_case_insensitive("a", "B"), Ordering::Less);
}

// Go: internal/stringutil/compare.go:HasPrefix/HasSuffix (behavior-level supplement)
#[test]
fn has_prefix_suffix_casing() {
    assert!(has_prefix("Foo", "fo", false));
    assert!(!has_prefix("Foo", "fo", true));
    assert!(has_suffix("Foo", "OO", false));
    assert!(!has_suffix("Foo", "OO", true));
}

// Go: internal/stringutil/compare.go:CompareStringsCaseInsensitiveEslintCompatible (behavior-level supplement)
#[test]
fn eslint_compatible_lowercase_order() {
    // Lowercase of "__String" is "__string" < "foo" ('_'=0x5f < 'f'=0x66).
    assert_eq!(
        compare_strings_case_insensitive_eslint_compatible("__String", "Foo"),
        Ordering::Less
    );
}

// Go: internal/stringutil/compare.go:EquateStringCaseInsensitive (behavior-level supplement)
#[test]
fn equate_case_insensitive() {
    assert!(equate_string_case_insensitive("ABC", "abc"));
    assert!(!equate_string_case_insensitive("ab", "abc"));
}
