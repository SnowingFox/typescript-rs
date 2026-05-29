use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

use regex::Regex;
use thiserror::Error;

// https://semver.org/#spec-item-2
// > A normal version number MUST take the form X.Y.Z where X, Y, and Z are
// > non-negative integers, and MUST NOT contain leading zeroes.
//
// NOTE: We differ here in that we allow X and X.Y, with missing parts having
// the default value of `0`.
static VERSION_REGEXP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(0|[1-9]\d*)(?:\.(0|[1-9]\d*)(?:\.(0|[1-9]\d*)(?:-([a-z0-9-.]+))?(?:\+([a-z0-9-.]+))?)?)?$").unwrap()
});

// https://semver.org/#spec-item-9
// > A pre-release version MAY be denoted by appending a hyphen and a series of
// > dot separated identifiers immediately following the patch version.
//
// NOTE: Go also declares `prereleasePartRegexp`/`buildPartRegExp` but never
// references them; they are omitted here to satisfy `-D warnings`.
static PRERELEASE_REGEXP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:0|[1-9]\d*|[a-z-][a-z0-9-]*)(?:\.(?:0|[1-9]\d*|[a-zA-Z-][a-zA-Z0-9-]*))*$")
        .unwrap()
});

// https://semver.org/#spec-item-10
// > Build metadata MAY be denoted by appending a plus sign and a series of dot
// > separated identifiers immediately following the patch or pre-release version.
static BUILD_REGEXP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^[a-z0-9-]+(?:\.[a-z0-9-]+)*$").unwrap());

// https://semver.org/#spec-item-9
// > Numeric identifiers MUST NOT include leading zeroes.
static NUMERIC_IDENTIFIER_REGEXP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:0|[1-9]\d*)$").unwrap());

/// A parsed semantic version of the form `major.minor.patch[-prerelease][+build]`.
///
/// This is the npm-flavored variant TypeScript relies on: unlike strict
/// semver, a missing `minor` or `patch` segment is allowed and defaults to `0`
/// (so `"1"` and `"1.2"` parse). Ordering follows semver precedence rules
/// (numeric major/minor/patch, then dot-separated prerelease identifiers),
/// and build metadata never affects precedence.
///
/// Construct one with [`try_parse_version`] or [`must_parse`].
///
/// # Examples
/// ```
/// use tsgo_semver::try_parse_version;
/// let v = try_parse_version("1.2.3-pre.4+build.5").unwrap();
/// assert_eq!(v.to_string(), "1.2.3-pre.4+build.5");
/// assert!(try_parse_version("1.0.0-pre").unwrap() < try_parse_version("1.0.0").unwrap());
/// ```
///
/// Side effects: none (pure).
#[derive(Debug, Clone)]
pub struct Version {
    pub(crate) major: u32,
    pub(crate) minor: u32,
    pub(crate) patch: u32,
    pub(crate) prerelease: Vec<String>,
    pub(crate) build: Vec<String>,
}

impl Version {
    // Go: internal/semver/version.go:incrementMajor
    pub(crate) fn increment_major(&self) -> Version {
        Version {
            major: self.major + 1,
            minor: 0,
            patch: 0,
            prerelease: Vec::new(),
            build: Vec::new(),
        }
    }

    // Go: internal/semver/version.go:incrementMinor
    pub(crate) fn increment_minor(&self) -> Version {
        Version {
            major: self.major,
            minor: self.minor + 1,
            patch: 0,
            prerelease: Vec::new(),
            build: Vec::new(),
        }
    }

    // Go: internal/semver/version.go:incrementPatch
    pub(crate) fn increment_patch(&self) -> Version {
        Version {
            major: self.major,
            minor: self.minor,
            patch: self.patch + 1,
            prerelease: Vec::new(),
            build: Vec::new(),
        }
    }

    // https://semver.org/#spec-item-11
    // > Precedence is determined by the first difference when comparing each of
    // > these identifiers from left to right: major, minor, and patch are always
    // > compared numerically; build metadata does not figure into precedence.
    // Go: internal/semver/version.go:Compare
    fn compare(&self, other: &Version) -> Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
            .then_with(|| compare_prerelease_identifiers(&self.prerelease, &other.prerelease))
    }
}

// Go: internal/semver/version.go:comparePreReleaseIdentifiers
fn compare_prerelease_identifiers(left: &[String], right: &[String]) -> Ordering {
    // > When major, minor, and patch are equal, a pre-release version has lower
    // > precedence than a normal version.
    if left.is_empty() {
        return if right.is_empty() {
            Ordering::Equal
        } else {
            Ordering::Greater
        };
    } else if right.is_empty() {
        return Ordering::Less;
    }

    // Compare each dot separated identifier from left to right; if one set is a
    // prefix of the other, the shorter set has the lower precedence.
    for (l, r) in left.iter().zip(right.iter()) {
        let ordering = compare_prerelease_identifier(l, r);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

// Go: internal/semver/version.go:comparePreReleaseIdentifier
fn compare_prerelease_identifier(left: &str, right: &str) -> Ordering {
    let compare_result = left.cmp(right);
    if compare_result == Ordering::Equal {
        return compare_result;
    }

    let left_is_numeric = NUMERIC_IDENTIFIER_REGEXP.is_match(left);
    let right_is_numeric = NUMERIC_IDENTIFIER_REGEXP.is_match(right);

    if left_is_numeric || right_is_numeric {
        // > Numeric identifiers always have lower precedence than non-numeric ones.
        if !right_is_numeric {
            return Ordering::Less;
        }
        if !left_is_numeric {
            return Ordering::Greater;
        }

        // > Identifiers consisting of only digits are compared numerically.
        match (get_uint_component(left), get_uint_component(right)) {
            (Ok(left_number), Ok(right_number)) => left_number.cmp(&right_number),
            _ => {
                // This should only happen in the event of an overflow. If so,
                // use the lengths or fall back to string comparison.
                let len_compare = left.len().cmp(&right.len());
                if len_compare == Ordering::Equal {
                    compare_result
                } else {
                    len_compare
                }
            }
        }
    } else {
        // > Identifiers with letters or hyphens are compared lexically in ASCII order.
        compare_result
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.compare(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare(other)
    }
}

// Go: internal/semver/version.go:(*Version).String
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.prerelease.is_empty() {
            write!(f, "-{}", self.prerelease.join("."))?;
        }
        if !self.build.is_empty() {
            write!(f, "+{}", self.build.join("."))?;
        }
        Ok(())
    }
}

/// Error returned by [`try_parse_version`] when a string cannot be parsed as a
/// [`Version`].
///
/// The message mirrors the Go upstream: `Could not parse version string from "<input>"`.
///
/// # Examples
/// ```
/// use tsgo_semver::try_parse_version;
/// let err = try_parse_version("not a version").unwrap_err();
/// assert!(err.to_string().contains("Could not parse version string"));
/// ```
///
/// Side effects: none (pure).
#[derive(Debug, Error)]
#[error("Could not parse version string from {orig_input:?}")]
pub struct SemverParseError {
    orig_input: String,
}

/// Parses `text` into a [`Version`], or returns a [`SemverParseError`].
///
/// Accepts the relaxed npm/TypeScript grammar where `minor`/`patch` may be
/// omitted (defaulting to `0`), with optional `-prerelease` and `+build`
/// suffixes that are validated against the semver identifier grammar.
///
/// # Examples
/// ```
/// use tsgo_semver::try_parse_version;
/// assert_eq!(try_parse_version("1").unwrap().to_string(), "1.0.0");
/// assert!(try_parse_version("1.2.3+only-build").is_ok());
/// assert!(try_parse_version("01.2.3").is_err());
/// ```
///
/// Side effects: none (pure).
// Go: internal/semver/version.go:TryParseVersion
pub fn try_parse_version(text: &str) -> Result<Version, SemverParseError> {
    let parse_error = || SemverParseError {
        orig_input: text.to_string(),
    };

    let captures = VERSION_REGEXP.captures(text).ok_or_else(parse_error)?;

    let group = |i: usize| captures.get(i).map_or("", |m| m.as_str());
    let major_str = group(1);
    let minor_str = group(2);
    let patch_str = group(3);
    let prerelease_str = group(4);
    let build_str = group(5);

    let mut result = Version {
        major: 0,
        minor: 0,
        patch: 0,
        prerelease: Vec::new(),
        build: Vec::new(),
    };

    result.major = get_uint_component(major_str).map_err(|_| parse_error())?;

    if !minor_str.is_empty() {
        result.minor = get_uint_component(minor_str).map_err(|_| parse_error())?;
    }

    if !patch_str.is_empty() {
        result.patch = get_uint_component(patch_str).map_err(|_| parse_error())?;
    }

    if !prerelease_str.is_empty() {
        if !PRERELEASE_REGEXP.is_match(prerelease_str) {
            return Err(parse_error());
        }
        result.prerelease = prerelease_str.split('.').map(str::to_string).collect();
    }

    if !build_str.is_empty() {
        if !BUILD_REGEXP.is_match(build_str) {
            return Err(parse_error());
        }
        result.build = build_str.split('.').map(str::to_string).collect();
    }

    Ok(result)
}

/// Parses `text` into a [`Version`], panicking if it is not a valid version.
///
/// Use this only for known-good literals (e.g. constants in tests); prefer
/// [`try_parse_version`] for untrusted input.
///
/// # Examples
/// ```
/// use tsgo_semver::must_parse;
/// assert_eq!(must_parse("4.9.0").to_string(), "4.9.0");
/// ```
///
/// Side effects: none (pure); panics on invalid input.
// Go: internal/semver/version.go:MustParse
pub fn must_parse(text: &str) -> Version {
    match try_parse_version(text) {
        Ok(version) => version,
        Err(err) => panic!("{err}"),
    }
}

// Go: internal/semver/version.go:getUintComponent
pub(crate) fn get_uint_component(text: &str) -> Result<u32, <u32 as FromStr>::Err> {
    u32::from_str(text)
}

// Go: internal/semver/version.go:versionZero
pub(crate) fn version_zero() -> Version {
    Version {
        major: 0,
        minor: 0,
        patch: 0,
        prerelease: vec!["0".to_string()],
        build: Vec::new(),
    }
}

#[cfg(test)]
#[path = "version_test.rs"]
mod tests;
