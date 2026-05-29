//! String comparator family: case-sensitive/insensitive equality and three-way
//! comparison, plus an ESLint-compatible comparator.
//!
//! 1:1 port of Go `internal/stringutil/compare.go`. Go's `Comparison = int`
//! (-1/0/1) is represented in Rust with [`std::cmp::Ordering`] (a
//! structurally-equivalent, allowed divergence).

use std::cmp::Ordering;

/// Three-way comparison result. Mirrors Go's `type Comparison = int`; Rust uses
/// [`Ordering`].
pub type Comparison = Ordering;

/// Folds `c` to simple lowercase (the first char of its full lowercase folding).
///
/// PERF(port): Go uses `unicode.ToLower` (simple 1:1 folding); this approximates
/// with the first char of `char::to_lowercase`. Common code points match.
fn to_lower_simple(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Case-insensitive equality (mirrors Go `strings.EqualFold`'s per-rune
/// folding).
///
/// # Examples
/// ```
/// use tsgo_stringutil::equate_string_case_insensitive;
/// assert!(equate_string_case_insensitive("ABC", "abc"));
/// assert!(!equate_string_case_insensitive("abc", "abd"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/compare.go:EquateStringCaseInsensitive
pub fn equate_string_case_insensitive(a: &str, b: &str) -> bool {
    // PERF(port): not Go's simpleFold byte-by-byte implementation; approximates
    // EqualFold with per-rune lowercase folding.
    let mut ai = a.chars();
    let mut bi = b.chars();
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return true,
            (Some(ca), Some(cb)) => {
                if ca == cb {
                    continue;
                }
                if to_lower_simple(ca) != to_lower_simple(cb) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// Case-sensitive equality (i.e. `a == b`).
///
/// # Examples
/// ```
/// use tsgo_stringutil::equate_string_case_sensitive;
/// assert!(equate_string_case_sensitive("abc", "abc"));
/// assert!(!equate_string_case_sensitive("abc", "ABC"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/compare.go:EquateStringCaseSensitive
pub fn equate_string_case_sensitive(a: &str, b: &str) -> bool {
    a == b
}

/// Returns the equality comparator function pointer selected by `ignore_case`.
///
/// # Examples
/// ```
/// use tsgo_stringutil::get_string_equality_comparer;
/// let eq = get_string_equality_comparer(true);
/// assert!(eq("Foo", "foo"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/compare.go:GetStringEqualityComparer
pub fn get_string_equality_comparer(ignore_case: bool) -> fn(&str, &str) -> bool {
    if ignore_case {
        equate_string_case_insensitive
    } else {
        equate_string_case_sensitive
    }
}

/// Case-insensitive three-way comparison: per-rune lowercase comparison; when
/// one is a prefix of the other, the shorter sorts first.
///
/// # Examples
/// ```
/// use tsgo_stringutil::compare_strings_case_insensitive;
/// use std::cmp::Ordering;
/// assert_eq!(compare_strings_case_insensitive("ABC", "abc"), Ordering::Equal);
/// assert_eq!(compare_strings_case_insensitive("a", "B"), Ordering::Less);
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/compare.go:CompareStringsCaseInsensitive
pub fn compare_strings_case_insensitive(a: &str, b: &str) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }
    let mut ai = a.chars();
    let mut bi = b.chars();
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) => {
                let lca = to_lower_simple(ca);
                let lcb = to_lower_simple(cb);
                if lca != lcb {
                    return lca.cmp(&lcb);
                }
            }
        }
    }
}

/// Case-sensitive three-way comparison (byte order, equivalent to Go
/// `strings.Compare`).
///
/// # Examples
/// ```
/// use tsgo_stringutil::compare_strings_case_sensitive;
/// use std::cmp::Ordering;
/// assert_eq!(compare_strings_case_sensitive("a", "b"), Ordering::Less);
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/compare.go:CompareStringsCaseSensitive
pub fn compare_strings_case_sensitive(a: &str, b: &str) -> Ordering {
    a.cmp(b)
}

/// Returns the three-way comparator function pointer selected by `ignore_case`.
///
/// # Examples
/// ```
/// use tsgo_stringutil::get_string_comparer;
/// use std::cmp::Ordering;
/// let cmp = get_string_comparer(false);
/// assert_eq!(cmp("a", "b"), Ordering::Less);
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/compare.go:GetStringComparer
pub fn get_string_comparer(ignore_case: bool) -> fn(&str, &str) -> Ordering {
    if ignore_case {
        compare_strings_case_insensitive
    } else {
        compare_strings_case_sensitive
    }
}

/// Reports whether `s` starts with `prefix` (`case_sensitive=false` uses
/// case folding).
///
/// # Examples
/// ```
/// use tsgo_stringutil::has_prefix;
/// assert!(has_prefix("Foo", "fo", false));
/// assert!(!has_prefix("Foo", "fo", true));
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/compare.go:HasPrefix
pub fn has_prefix(s: &str, prefix: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        return s.starts_with(prefix);
    }
    if prefix.len() > s.len() {
        return false;
    }
    if !s.is_char_boundary(prefix.len()) {
        return false;
    }
    equate_string_case_insensitive(&s[..prefix.len()], prefix)
}

/// Reports whether `s` ends with `suffix` (`case_sensitive=false` uses case
/// folding).
///
/// # Examples
/// ```
/// use tsgo_stringutil::has_suffix;
/// assert!(has_suffix("Foo", "OO", false));
/// assert!(!has_suffix("Foo", "OO", true));
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/compare.go:HasSuffix
pub fn has_suffix(s: &str, suffix: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        return s.ends_with(suffix);
    }
    if suffix.len() > s.len() {
        return false;
    }
    let start = s.len() - suffix.len();
    if !s.is_char_boundary(start) {
        return false;
    }
    equate_string_case_insensitive(&s[start..], suffix)
}

/// Reports whether `s` has both a `prefix` and a `suffix` that do not overlap.
///
/// # Examples
/// ```
/// use tsgo_stringutil::has_prefix_and_suffix_without_overlap;
/// assert!(has_prefix_and_suffix_without_overlap("foobar", "foo", "bar", true));
/// assert!(!has_prefix_and_suffix_without_overlap("foo", "foo", "foo", true));
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/compare.go:HasPrefixAndSuffixWithoutOverlap
pub fn has_prefix_and_suffix_without_overlap(
    s: &str,
    prefix: &str,
    suffix: &str,
    case_sensitive: bool,
) -> bool {
    if prefix.len() + suffix.len() > s.len() {
        return false;
    }
    has_prefix(s, prefix, case_sensitive) && has_suffix(s, suffix, case_sensitive)
}

/// Compares case-insensitively first; if equal, compares case-sensitively.
///
/// # Examples
/// ```
/// use tsgo_stringutil::compare_strings_case_insensitive_then_sensitive;
/// use std::cmp::Ordering;
/// assert_eq!(compare_strings_case_insensitive_then_sensitive("a", "a"), Ordering::Equal);
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/compare.go:CompareStringsCaseInsensitiveThenSensitive
pub fn compare_strings_case_insensitive_then_sensitive(a: &str, b: &str) -> Ordering {
    let cmp = compare_strings_case_insensitive(a, b);
    if cmp != Ordering::Equal {
        return cmp;
    }
    compare_strings_case_sensitive(a, b)
}

/// ESLint-compatible case-insensitive comparison: uses `to_lowercase` (not
/// uppercase) to match eslint `sort-imports`.
///
/// This affects the relative order of letters and ASCII 91-96 (including the
/// identifier-legal `_`).
///
/// # Examples
/// ```
/// use tsgo_stringutil::compare_strings_case_insensitive_eslint_compatible;
/// use std::cmp::Ordering;
/// assert_eq!(compare_strings_case_insensitive_eslint_compatible("__String", "Foo"), Ordering::Less);
/// ```
///
/// Side effects: none (pure).
// Go: internal/stringutil/compare.go:CompareStringsCaseInsensitiveEslintCompatible
pub fn compare_strings_case_insensitive_eslint_compatible(a: &str, b: &str) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }
    let la = a.to_lowercase();
    let lb = b.to_lowercase();
    la.cmp(&lb)
}

#[cfg(test)]
#[path = "compare_test.rs"]
mod tests;
