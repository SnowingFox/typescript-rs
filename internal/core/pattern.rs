//! Wildcard pattern matching (`Pattern`) with a single `*`.
//!
//! 1:1 port of Go `internal/core/pattern.go`.

/// A pattern with at most one `*` wildcard. `star_index == -1` means an exact
/// match.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pattern {
    /// The pattern text (including the `*` if present).
    pub text: String,
    /// Byte index of the `*`, or `-1` for an exact match.
    pub star_index: i32,
}

/// Parses `pattern`; returns an empty pattern if it contains more than one `*`.
///
/// # Examples
/// ```
/// use tsgo_core::pattern::try_parse_pattern;
/// let p = try_parse_pattern("a*c");
/// assert_eq!(p.star_index, 1);
/// ```
///
/// Side effects: none (pure).
// Go: internal/core/pattern.go:TryParsePattern
pub fn try_parse_pattern(pattern: &str) -> Pattern {
    match pattern.find('*') {
        None => Pattern {
            text: pattern.to_string(),
            star_index: -1,
        },
        Some(star_index) => {
            if pattern[star_index + 1..].contains('*') {
                Pattern::default()
            } else {
                Pattern {
                    text: pattern.to_string(),
                    star_index: star_index as i32,
                }
            }
        }
    }
}

impl Pattern {
    /// Reports whether the pattern is valid.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/pattern.go:IsValid
    pub fn is_valid(&self) -> bool {
        self.star_index == -1 || (self.star_index as usize) < self.text.len()
    }

    /// Reports whether `candidate` matches this pattern.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/pattern.go:Matches
    pub fn matches(&self, candidate: &str) -> bool {
        if self.star_index == -1 {
            return self.text == candidate;
        }
        let star = self.star_index as usize;
        candidate.len() >= star
            && candidate.starts_with(&self.text[..star])
            && candidate.ends_with(&self.text[star + 1..])
    }

    /// Returns the substring of `candidate` matched by the `*`.
    ///
    /// # Panics
    /// Panics if `candidate` does not match this pattern.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/pattern.go:MatchedText
    pub fn matched_text(&self, candidate: &str) -> String {
        if !self.matches(candidate) {
            panic!("candidate does not match pattern");
        }
        if self.star_index == -1 {
            return String::new();
        }
        let star = self.star_index as usize;
        candidate[star..candidate.len() - self.text.len() + star + 1].to_string()
    }
}

/// Finds the value whose pattern best matches `candidate` (longest fixed
/// prefix wins).
///
/// Side effects: none (pure).
// Go: internal/core/pattern.go:FindBestPatternMatch
pub fn find_best_pattern_match<T>(
    values: &[T],
    get_pattern: impl Fn(&T) -> Pattern,
    candidate: &str,
) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut longest_match_prefix_length = -1i32;
    for (i, value) in values.iter().enumerate() {
        let pattern = get_pattern(value);
        if (pattern.star_index == -1 || pattern.star_index > longest_match_prefix_length)
            && pattern.matches(candidate)
        {
            best = Some(i);
            longest_match_prefix_length = pattern.star_index;
        }
    }
    best
}

#[cfg(test)]
#[path = "pattern_test.rs"]
mod tests;
