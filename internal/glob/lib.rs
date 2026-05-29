//! `tsgo_glob` — 1:1 Rust port of Go `internal/glob`.
//!
//! An LSP-compliant glob pattern matcher, as defined by the spec
//! (`documentFilter`). A pattern is parsed into a sequence of pattern elements
//! and matched against an input path with a recursive backtracking matcher.
//!
//! Supported syntax:
//! - `*` matches one or more characters in a path segment
//! - `?` matches one character in a path segment
//! - `**` matches any number of path segments (must be adjacent to `/`)
//! - `{}` groups sub-patterns into an OR expression
//! - `[]` declares a range of characters; `[!...]` negates it
//! - `/` matches one or more literal slashes; any other character is literal
//!
//! # Divergence from Go
//! - Go models pattern elements with the `element fmt.Stringer` interface plus a
//!   type switch; this port uses a discriminated `Element` enum + `match`.
//! - The Go matcher is byte-oriented (`input[0]`, byte slicing, `HasPrefix`),
//!   so the matcher here also operates on bytes; only `[x-y]` ranges decode a
//!   full Unicode scalar value, mirroring `utf8.DecodeRuneInString`.
//! - Go parses `[!x-y]` (negation) but its matcher never reads the `negate`
//!   flag, so `[!x-y]` behaves exactly like `[x-y]`. This port reproduces that
//!   upstream behavior faithfully; LSP-spec negation is deferred to P10 parity.

use std::fmt;

/// Errors produced while parsing a glob pattern.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GlobError {
    /// A `[` range was malformed (not of the form `[x-y]`).
    #[error("'[' patterns must be of the form [x-y]")]
    BadRange,
    /// A range endpoint was not valid UTF-8.
    ///
    /// Unreachable when parsing from `&str` (always valid UTF-8); kept for
    /// parity with Go's `utf8.RuneError` size-1 branch.
    #[error("invalid UTF-8 encoding")]
    InvalidUtf8,
    /// `**` appeared somewhere other than adjacent to `/`.
    #[error("** may only be adjacent to '/'")]
    DoubleStarAdjacency,
    /// A `{` group was never closed.
    #[error("unmatched '{{'")]
    UnmatchedBrace,
}

/// A parsed LSP glob pattern.
///
/// Build one with [`parse`] and test inputs with [`Glob::match_input`].
///
/// # Examples
/// ```
/// let g = tsgo_glob::parse("**/*.ts").unwrap();
/// assert!(g.match_input("src/app.ts"));
/// assert_eq!(g.to_string(), "**/*.ts");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Glob {
    elems: Vec<Element>,
}

#[derive(Debug, Clone, PartialEq)]
enum Element {
    Slash,
    Literal(String),
    Star,
    AnyChar,
    StarStar,
    Group(Vec<Glob>),
    CharRange { negate: bool, low: char, high: char },
}

/// Parses `pattern` into a [`Glob`], returning a [`GlobError`] if it is invalid.
///
/// # Examples
/// ```
/// assert!(tsgo_glob::parse("example.[0-9]").is_ok());
/// assert!(tsgo_glob::parse("a**b").is_err());
/// ```
///
/// Side effects: none (pure).
// Go: internal/glob/glob.go:Parse
pub fn parse(pattern: &str) -> Result<Glob, GlobError> {
    let (g, _rest) = parse_inner(pattern, false)?;
    Ok(g)
}

// Go: internal/glob/glob.go:parse
fn parse_inner(mut pattern: &str, nested: bool) -> Result<(Glob, &str), GlobError> {
    let mut g = Glob { elems: Vec::new() };
    while !pattern.is_empty() {
        let bytes = pattern.as_bytes();
        match bytes[0] {
            b'/' => {
                pattern = &pattern[1..];
                g.elems.push(Element::Slash);
            }
            b'*' => {
                if bytes.len() > 1 && bytes[1] == b'*' {
                    let last_not_slash = g.elems.last().is_some_and(|e| *e != Element::Slash);
                    if last_not_slash || (bytes.len() > 2 && bytes[2] != b'/') {
                        return Err(GlobError::DoubleStarAdjacency);
                    }
                    pattern = &pattern[2..];
                    g.elems.push(Element::StarStar);
                } else {
                    pattern = &pattern[1..];
                    g.elems.push(Element::Star);
                }
            }
            b'?' => {
                pattern = &pattern[1..];
                g.elems.push(Element::AnyChar);
            }
            b'{' => {
                let mut gs: Vec<Glob> = Vec::new();
                loop {
                    match pattern.as_bytes().first() {
                        Some(b'}') => break,
                        // Defensive: Go indexes `pattern[0]` here and would panic
                        // on an empty remainder; that path is unreachable because a
                        // successful nested parse always leaves `}` or `,`.
                        None => return Err(GlobError::UnmatchedBrace),
                        Some(_) => {
                            pattern = &pattern[1..];
                            let (group_g, pat) = parse_inner(pattern, true)?;
                            if pat.is_empty() {
                                return Err(GlobError::UnmatchedBrace);
                            }
                            pattern = pat;
                            gs.push(group_g);
                        }
                    }
                }
                pattern = &pattern[1..];
                g.elems.push(Element::Group(gs));
            }
            b'}' | b',' => {
                if nested {
                    return Ok((g, pattern));
                }
                pattern = g.parse_literal(pattern, false);
            }
            b'[' => {
                pattern = &pattern[1..];
                if pattern.is_empty() {
                    return Err(GlobError::BadRange);
                }
                let mut negate = false;
                if pattern.as_bytes()[0] == b'!' {
                    pattern = &pattern[1..];
                    negate = true;
                }
                let (low, sz) = read_range_rune(pattern)?;
                pattern = &pattern[sz..];
                if pattern.is_empty() || pattern.as_bytes()[0] != b'-' {
                    return Err(GlobError::BadRange);
                }
                pattern = &pattern[1..];
                let (high, sz) = read_range_rune(pattern)?;
                pattern = &pattern[sz..];
                if pattern.is_empty() || pattern.as_bytes()[0] != b']' {
                    return Err(GlobError::BadRange);
                }
                pattern = &pattern[1..];
                g.elems.push(Element::CharRange { negate, low, high });
            }
            _ => {
                pattern = g.parse_literal(pattern, nested);
            }
        }
    }
    Ok((g, ""))
}

/// Decodes a single Unicode scalar value for a `[x-y]` range endpoint, returning
/// it with its byte width. Mirrors Go's `readRangeRune`/`utf8.DecodeRuneInString`:
/// an empty input is a [`GlobError::BadRange`].
// Go: internal/glob/glob.go:readRangeRune
fn read_range_rune(input: &str) -> Result<(char, usize), GlobError> {
    match input.chars().next() {
        None => Err(GlobError::BadRange),
        Some(c) => Ok((c, c.len_utf8())),
    }
}

impl Glob {
    // Go: internal/glob/glob.go:(*Glob).parseLiteral
    fn parse_literal<'a>(&mut self, pattern: &'a str, nested: bool) -> &'a str {
        let special: &[u8] = if nested { b"*?{[/}," } else { b"*?{[/" };
        let end = pattern
            .as_bytes()
            .iter()
            .position(|b| special.contains(b))
            .unwrap_or(pattern.len());
        self.elems
            .push(Element::Literal(pattern[..end].to_string()));
        &pattern[end..]
    }
}

impl fmt::Display for Glob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for e in &self.elems {
            write!(f, "{e}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Element {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Element::Slash => f.write_str("/"),
            Element::Literal(s) => f.write_str(s),
            Element::Star => f.write_str("*"),
            Element::AnyChar => f.write_str("?"),
            Element::StarStar => f.write_str("**"),
            Element::Group(globs) => {
                f.write_str("{")?;
                for (i, g) in globs.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{g}")?;
                }
                f.write_str("}")
            }
            // Go's `charRange.String()` omits the negate flag, so do we.
            Element::CharRange { low, high, .. } => write!(f, "[{low}-{high}]"),
        }
    }
}

impl Glob {
    /// Reports whether `input` matches this glob pattern.
    ///
    /// # Examples
    /// ```
    /// let g = tsgo_glob::parse("*.{ts,js}").unwrap();
    /// assert!(g.match_input("main.ts"));
    /// assert!(!g.match_input("main.go"));
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/glob/glob.go:(*Glob).Match
    pub fn match_input(&self, input: &str) -> bool {
        match_elems(&self.elems, input.as_bytes())
    }
}

// Go: internal/glob/glob.go:match
fn match_elems(mut elems: &[Element], mut input: &[u8]) -> bool {
    while let Some((elem, rest)) = elems.split_first() {
        elems = rest;
        match elem {
            Element::Slash => {
                if input.first() != Some(&b'/') {
                    return false;
                }
                // Consume one or more slashes. Guards against the empty-input
                // panic latent in Go's `for input[0] == '/'`.
                while input.first() == Some(&b'/') {
                    input = &input[1..];
                }
            }
            Element::StarStar => {
                // `**` is always followed by `/` (enforced by parse); drop it.
                if !elems.is_empty() {
                    elems = &elems[1..];
                }
                // A trailing `**` matches anything.
                if elems.is_empty() {
                    return true;
                }
                // Backtrack across path segments until the rest matches.
                while !input.is_empty() {
                    if match_elems(elems, input) {
                        return true;
                    }
                    let (_, rest) = split(input);
                    input = rest;
                }
                return false;
            }
            Element::Literal(s) => {
                let lit = s.as_bytes();
                if !input.starts_with(lit) {
                    return false;
                }
                input = &input[lit.len()..];
            }
            Element::Star => {
                let (seg_input, rest_input) = split(input);
                input = rest_input;

                let mut elem_end = elems.len();
                for (i, e) in elems.iter().enumerate() {
                    if *e == Element::Slash {
                        elem_end = i;
                        break;
                    }
                }
                let seg_elems = &elems[..elem_end];
                elems = &elems[elem_end..];

                // A trailing `*` matches the entire segment.
                if seg_elems.is_empty() {
                    continue;
                }

                // Backtrack over byte offsets until the subpattern matches.
                let mut matched = false;
                for i in 0..seg_input.len() {
                    if match_elems(seg_elems, &seg_input[i..]) {
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    return false;
                }
            }
            Element::AnyChar => {
                // Go advances a single byte here, not a full rune.
                if input.is_empty() || input[0] == b'/' {
                    return false;
                }
                input = &input[1..];
            }
            Element::Group(members) => {
                for m in members {
                    let mut branch: Vec<Element> = Vec::with_capacity(m.elems.len() + elems.len());
                    branch.extend_from_slice(&m.elems);
                    branch.extend_from_slice(elems);
                    if match_elems(&branch, input) {
                        return true;
                    }
                }
                return false;
            }
            Element::CharRange { low, high, .. } => {
                // Go's matcher never consults `negate`, so neither do we.
                if input.is_empty() || input[0] == b'/' {
                    return false;
                }
                let (c, sz) = decode_first_rune(input);
                if c < *low || c > *high {
                    return false;
                }
                input = &input[sz..];
            }
        }
    }
    input.is_empty()
}

/// Splits `input` at the first slash (or run of consecutive slashes), returning
/// the portion before and after. With no slash it returns `(input, b"")`.
// Go: internal/glob/glob.go:split
fn split(input: &[u8]) -> (&[u8], &[u8]) {
    match input.iter().position(|&b| b == b'/') {
        None => (input, b""),
        Some(i) => {
            let first = &input[..i];
            let mut j = i;
            while j < input.len() {
                if input[j] != b'/' {
                    return (first, &input[j..]);
                }
                j += 1;
            }
            (first, b"")
        }
    }
}

/// Decodes the first UTF-8 scalar value of `input` with its byte width,
/// mirroring `utf8.DecodeRuneInString` (invalid bytes -> `U+FFFD`, width 1).
fn decode_first_rune(input: &[u8]) -> (char, usize) {
    let max = input.len().min(4);
    for n in 1..=max {
        if let Ok(s) = std::str::from_utf8(&input[..n]) {
            if let Some(c) = s.chars().next() {
                return (c, n);
            }
        }
    }
    ('\u{FFFD}', 1)
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
