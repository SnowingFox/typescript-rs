use std::fmt;
use std::sync::LazyLock;

use regex::Regex;

use std::cmp::Ordering;

use crate::version::{get_uint_component, version_zero, Version};

// https://github.com/npm/node-semver#range-grammar
//
// range-set    ::= range ( logical-or range ) *
// range        ::= hyphen | simple ( ' ' simple ) * | ''
// logical-or   ::= ( ' ' ) * '||' ( ' ' ) *
static LOGICAL_OR_REGEXP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\|\|").unwrap());
static WHITESPACE_REGEXP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

// partial      ::= xr ( '.' xr ( '.' xr qualifier ? )? )?
// xr           ::= 'x' | 'X' | '*' | nr
static PARTIAL_REGEXP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^([x*0]|[1-9]\d*)(?:\.([x*0]|[1-9]\d*)(?:\.([x*0]|[1-9]\d*)(?:-([a-z0-9-.]+))?(?:\+([a-z0-9-.]+))?)?)?$").unwrap()
});

// hyphen       ::= partial ' - ' partial
static HYPHEN_REGEXP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*([a-z0-9-+.*]+)\s+-\s+([a-z0-9-+.*]+)\s*$").unwrap());

// simple       ::= primitive | partial | tilde | caret
// primitive    ::= ( '<' | '>' | '>=' | '<=' | '=' ) partial
static RANGE_REGEXP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^([~^<>=]|<=|>=)?\s*([a-z0-9-+.*]+)$").unwrap());

/// An npm-style version range: a disjunction (`||`) of conjunctions of
/// comparators.
///
/// Build it with [`try_parse_version_range`], then check membership with
/// [`VersionRange::test`]. Supports primitives (`<`, `<=`, `=`, `>=`, `>`),
/// tilde (`~`), caret (`^`), hyphen (`a - b`), and `x`/`X`/`*` wildcards.
///
/// # Examples
/// ```
/// use tsgo_semver::{try_parse_version, try_parse_version_range};
/// let (range, ok) = try_parse_version_range("^1.2.3");
/// assert!(ok);
/// assert!(range.test(&try_parse_version("1.9.0").unwrap()));
/// assert!(!range.test(&try_parse_version("2.0.0").unwrap()));
/// assert_eq!(range.to_string(), ">=1.2.3 <2.0.0");
/// ```
///
/// Side effects: none (pure).
// Go: internal/semver/version_range.go:VersionRange
pub struct VersionRange {
    alternatives: Vec<Vec<VersionComparator>>,
}

// Go: internal/semver/version_range.go:versionComparator
struct VersionComparator {
    operator: ComparatorOperator,
    operand: Version,
}

// Go: internal/semver/version_range.go:comparatorOperator
#[derive(Clone, Copy)]
enum ComparatorOperator {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
}

impl fmt::Display for ComparatorOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ComparatorOperator::Lt => "<",
            ComparatorOperator::Le => "<=",
            ComparatorOperator::Eq => "=",
            ComparatorOperator::Ge => ">=",
            ComparatorOperator::Gt => ">",
        })
    }
}

// Go: internal/semver/version_range.go:(*VersionRange).String
impl fmt::Display for VersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format_disjunction(&self.alternatives))
    }
}

impl VersionRange {
    /// Reports whether `version` satisfies this range.
    ///
    /// An empty range matches every version. Each alternative (separated by
    /// `||`) is a conjunction: all of its comparators must hold for that
    /// alternative to match.
    ///
    /// # Examples
    /// ```
    /// use tsgo_semver::{try_parse_version, try_parse_version_range};
    /// let (range, _) = try_parse_version_range("~1.2.3");
    /// assert!(range.test(&try_parse_version("1.2.9").unwrap()));
    /// assert!(!range.test(&try_parse_version("1.3.0").unwrap()));
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/semver/version_range.go:(*VersionRange).Test
    pub fn test(&self, version: &Version) -> bool {
        test_disjunction(&self.alternatives, version)
    }
}

// Go: internal/semver/version_range.go:testDisjunction
fn test_disjunction(alternatives: &[Vec<VersionComparator>], version: &Version) -> bool {
    // an empty disjunction is treated as "*" (all versions)
    if alternatives.is_empty() {
        return true;
    }
    alternatives
        .iter()
        .any(|alternative| test_alternative(alternative, version))
}

// Go: internal/semver/version_range.go:testAlternative
fn test_alternative(alternative: &[VersionComparator], version: &Version) -> bool {
    alternative
        .iter()
        .all(|comparator| test_comparator(comparator, version))
}

// Go: internal/semver/version_range.go:testComparator
fn test_comparator(comparator: &VersionComparator, version: &Version) -> bool {
    let ordering = version.cmp(&comparator.operand);
    match comparator.operator {
        ComparatorOperator::Lt => ordering == Ordering::Less,
        ComparatorOperator::Le => ordering != Ordering::Greater,
        ComparatorOperator::Eq => ordering == Ordering::Equal,
        ComparatorOperator::Ge => ordering != Ordering::Less,
        ComparatorOperator::Gt => ordering == Ordering::Greater,
    }
}

// Go: internal/semver/version_range.go:formatDisjunction
fn format_disjunction(alternatives: &[Vec<VersionComparator>]) -> String {
    let mut out = String::new();
    for (i, alternative) in alternatives.iter().enumerate() {
        if i > 0 {
            out.push_str(" || ");
        }
        format_alternative(&mut out, alternative);
    }
    // an empty disjunction renders as "*" (all versions)
    if out.is_empty() {
        out.push('*');
    }
    out
}

// Go: internal/semver/version_range.go:formatAlternative
fn format_alternative(out: &mut String, comparators: &[VersionComparator]) {
    for (i, comparator) in comparators.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        format_comparator(out, comparator);
    }
}

// Go: internal/semver/version_range.go:formatComparator
fn format_comparator(out: &mut String, comparator: &VersionComparator) {
    out.push_str(&comparator.operator.to_string());
    out.push_str(&comparator.operand.to_string());
}

/// Parses an npm-style range set into a [`VersionRange`].
///
/// Returns the parsed range together with a success flag: when the flag is
/// `false` the input was syntactically invalid and the returned range should
/// be ignored (this mirrors the Go `(VersionRange, bool)` shape).
///
/// # Examples
/// ```
/// use tsgo_semver::try_parse_version_range;
/// assert!(try_parse_version_range(">=1.0.0 <2.0.0 || >=3.0.0 <4.0.0").1);
/// assert!(!try_parse_version_range("<<<").1);
/// ```
///
/// Side effects: none (pure).
// Go: internal/semver/version_range.go:TryParseVersionRange
pub fn try_parse_version_range(text: &str) -> (VersionRange, bool) {
    let (alternatives, ok) = parse_alternatives(text);
    (VersionRange { alternatives }, ok)
}

// Go: internal/semver/version_range.go:parseAlternatives
fn parse_alternatives(text: &str) -> (Vec<Vec<VersionComparator>>, bool) {
    let mut alternatives: Vec<Vec<VersionComparator>> = Vec::new();

    let text = text.trim();
    for range in LOGICAL_OR_REGEXP.split(text) {
        let range = range.trim();
        if range.is_empty() {
            continue;
        }

        let mut comparators: Vec<VersionComparator> = Vec::new();

        if let Some(hyphen_match) = HYPHEN_REGEXP.captures(range) {
            let left = hyphen_match.get(1).map_or("", |m| m.as_str());
            let right = hyphen_match.get(2).map_or("", |m| m.as_str());
            match parse_hyphen(left, right) {
                Some(parsed) => comparators.extend(parsed),
                None => return (Vec::new(), false),
            }
        } else {
            for simple in WHITESPACE_REGEXP.split(range) {
                let Some(captures) = RANGE_REGEXP.captures(simple.trim()) else {
                    return (Vec::new(), false);
                };
                let op = captures.get(1).map_or("", |m| m.as_str());
                let operand = captures.get(2).map_or("", |m| m.as_str());
                match parse_comparator(op, operand) {
                    Some(parsed) => comparators.extend(parsed),
                    None => return (Vec::new(), false),
                }
            }
        }

        alternatives.push(comparators);
    }

    (alternatives, true)
}

// Go: internal/semver/version_range.go:parseHyphen
fn parse_hyphen(left: &str, right: &str) -> Option<Vec<VersionComparator>> {
    let left_result = parse_partial(left)?;
    let right_result = parse_partial(right)?;

    let mut comparators: Vec<VersionComparator> = Vec::new();

    if !is_wildcard(&left_result.major_str) {
        // `MAJOR.*.*-...` gives us `>=MAJOR.0.0 ...`
        comparators.push(VersionComparator {
            operator: ComparatorOperator::Ge,
            operand: left_result.version,
        });
    }

    if !is_wildcard(&right_result.major_str) {
        let mut operand = right_result.version;
        let operator = if is_wildcard(&right_result.minor_str) {
            // `...-MAJOR.*.*` gives us `... <(MAJOR+1).0.0`
            operand = operand.increment_major();
            ComparatorOperator::Lt
        } else if is_wildcard(&right_result.patch_str) {
            // `...-MAJOR.MINOR.*` gives us `... <MAJOR.(MINOR+1).0`
            operand = operand.increment_minor();
            ComparatorOperator::Lt
        } else {
            // `...-MAJOR.MINOR.PATCH` gives us `... <=MAJOR.MINOR.PATCH`
            ComparatorOperator::Le
        };

        comparators.push(VersionComparator { operator, operand });
    }

    Some(comparators)
}

// Go: internal/semver/version_range.go:partialVersion
struct PartialVersion {
    version: Version,
    major_str: String,
    minor_str: String,
    patch_str: String,
}

// Go: internal/semver/version_range.go:parsePartial
fn parse_partial(text: &str) -> Option<PartialVersion> {
    let captures = PARTIAL_REGEXP.captures(text)?;

    let major_str = captures.get(1).map_or("", |m| m.as_str()).to_string();
    let mut minor_str = captures.get(2).map_or("", |m| m.as_str()).to_string();
    let mut patch_str = captures.get(3).map_or("", |m| m.as_str()).to_string();
    let prerelease_str = captures.get(4).map_or("", |m| m.as_str());
    let build_str = captures.get(5).map_or("", |m| m.as_str());

    if minor_str.is_empty() {
        minor_str = "*".to_string();
    }
    if patch_str.is_empty() {
        patch_str = "*".to_string();
    }

    let mut major_numeric = 0;
    let mut minor_numeric = 0;
    let mut patch_numeric = 0;

    if !is_wildcard(&major_str) {
        major_numeric = get_uint_component(&major_str).ok()?;
        if !is_wildcard(&minor_str) {
            minor_numeric = get_uint_component(&minor_str).ok()?;
            if !is_wildcard(&patch_str) {
                patch_numeric = get_uint_component(&patch_str).ok()?;
            }
        }
    }

    let prerelease = if prerelease_str.is_empty() {
        Vec::new()
    } else {
        prerelease_str.split('.').map(str::to_string).collect()
    };
    let build = if build_str.is_empty() {
        Vec::new()
    } else {
        build_str.split('.').map(str::to_string).collect()
    };

    Some(PartialVersion {
        version: Version {
            major: major_numeric,
            minor: minor_numeric,
            patch: patch_numeric,
            prerelease,
            build,
        },
        major_str,
        minor_str,
        patch_str,
    })
}

// Go: internal/semver/version_range.go:parseComparator
fn parse_comparator(op: &str, text: &str) -> Option<Vec<VersionComparator>> {
    let result = parse_partial(text)?;

    let mut comparators_result: Vec<VersionComparator> = Vec::new();

    if !is_wildcard(&result.major_str) {
        match op {
            "~" => {
                let first = VersionComparator {
                    operator: ComparatorOperator::Ge,
                    operand: result.version.clone(),
                };

                let second_version = if is_wildcard(&result.minor_str) {
                    result.version.increment_major()
                } else {
                    result.version.increment_minor()
                };
                let second = VersionComparator {
                    operator: ComparatorOperator::Lt,
                    operand: second_version,
                };
                comparators_result = vec![first, second];
            }
            "^" => {
                let first = VersionComparator {
                    operator: ComparatorOperator::Ge,
                    operand: result.version.clone(),
                };

                let second_version = if result.version.major > 0 || is_wildcard(&result.minor_str) {
                    result.version.increment_major()
                } else if result.version.minor > 0 || is_wildcard(&result.patch_str) {
                    result.version.increment_minor()
                } else {
                    result.version.increment_patch()
                };
                let second = VersionComparator {
                    operator: ComparatorOperator::Lt,
                    operand: second_version,
                };
                comparators_result = vec![first, second];
            }
            "<" | ">=" => {
                let operator = if op == "<" {
                    ComparatorOperator::Lt
                } else {
                    ComparatorOperator::Ge
                };
                let mut version = result.version;
                if is_wildcard(&result.minor_str) || is_wildcard(&result.patch_str) {
                    version.prerelease = vec!["0".to_string()];
                }
                comparators_result = vec![VersionComparator {
                    operator,
                    operand: version,
                }];
            }
            "<=" | ">" => {
                let mut operator = if op == "<=" {
                    ComparatorOperator::Le
                } else {
                    ComparatorOperator::Gt
                };
                let mut version = result.version;

                if is_wildcard(&result.minor_str) {
                    operator = if op == "<=" {
                        ComparatorOperator::Lt
                    } else {
                        ComparatorOperator::Ge
                    };
                    version = version.increment_major();
                    version.prerelease = vec!["0".to_string()];
                } else if is_wildcard(&result.patch_str) {
                    operator = if op == "<=" {
                        ComparatorOperator::Lt
                    } else {
                        ComparatorOperator::Ge
                    };
                    version = version.increment_minor();
                    version.prerelease = vec!["0".to_string()];
                }

                comparators_result = vec![VersionComparator {
                    operator,
                    operand: version,
                }];
            }
            "=" | "" => {
                // normalize the empty string to `=`
                if is_wildcard(&result.minor_str) || is_wildcard(&result.patch_str) {
                    let original_version = result.version;

                    let mut first_version = original_version.clone();
                    first_version.prerelease = vec!["0".to_string()];

                    let mut second_version = if is_wildcard(&result.minor_str) {
                        original_version.increment_major()
                    } else {
                        original_version.increment_minor()
                    };
                    second_version.prerelease = vec!["0".to_string()];

                    comparators_result = vec![
                        VersionComparator {
                            operator: ComparatorOperator::Ge,
                            operand: first_version,
                        },
                        VersionComparator {
                            operator: ComparatorOperator::Lt,
                            operand: second_version,
                        },
                    ];
                } else {
                    comparators_result = vec![VersionComparator {
                        operator: ComparatorOperator::Eq,
                        operand: result.version,
                    }];
                }
            }
            other => panic!("Unexpected operator: {other}"),
        }
    } else if op == "<" || op == ">" {
        // `<` or `>` against a wildcard major: `< 0.0.0-0`
        comparators_result = vec![VersionComparator {
            operator: ComparatorOperator::Lt,
            operand: version_zero(),
        }];
    }

    Some(comparators_result)
}

// Go: internal/semver/version_range.go:isWildcard
fn is_wildcard(text: &str) -> bool {
    text == "*" || text == "x" || text == "X"
}

#[cfg(test)]
#[path = "version_range_test.rs"]
mod tests;
