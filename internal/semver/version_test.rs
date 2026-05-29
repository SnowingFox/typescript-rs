use super::*;
use std::cmp::Ordering;

fn version(major: u32, minor: u32, patch: u32, prerelease: &[&str], build: &[&str]) -> Version {
    Version {
        major,
        minor,
        patch,
        prerelease: prerelease.iter().map(|s| s.to_string()).collect(),
        build: build.iter().map(|s| s.to_string()).collect(),
    }
}

fn assert_version(actual: &Version, expected: &Version) {
    assert_eq!(actual.major, expected.major);
    assert_eq!(actual.minor, expected.minor);
    assert_eq!(actual.patch, expected.patch);
    assert_eq!(actual.prerelease, expected.prerelease);
    assert_eq!(actual.build, expected.build);
}

// Go: internal/semver/version_test.go:TestTryParseSemver
#[test]
fn test_try_parse_semver() {
    let cases = [
        // Go: .../1.2.3-pre.4+build.5
        (
            "1.2.3-pre.4+build.5",
            version(1, 2, 3, &["pre", "4"], &["build", "5"]),
        ),
        // Go: .../1.2.3-pre.4
        ("1.2.3-pre.4", version(1, 2, 3, &["pre", "4"], &[])),
        // Go: .../1.2.3+build.4
        ("1.2.3+build.4", version(1, 2, 3, &[], &["build", "4"])),
        // Go: .../1.2.3
        ("1.2.3", version(1, 2, 3, &[], &[])),
    ];
    for (input, expected) in &cases {
        let parsed = try_parse_version(input).expect(input);
        assert_version(&parsed, expected);
    }
}

// Go: internal/semver/version_test.go:TestVersionString
#[test]
fn test_version_string() {
    let cases = [
        // Go: .../1.2.3-pre.4+build.5
        (
            version(1, 2, 3, &["pre", "4"], &["build", "5"]),
            "1.2.3-pre.4+build.5",
        ),
        // Go: .../1.2.3-pre.4+build
        (
            version(1, 2, 3, &["pre", "4"], &["build"]),
            "1.2.3-pre.4+build",
        ),
        // Go: .../1.2.3+build
        (version(1, 2, 3, &[], &["build"]), "1.2.3+build"),
        // Go: .../1.2.3-pre.4
        (version(1, 2, 3, &["pre", "4"], &[]), "1.2.3-pre.4"),
        // Go: .../1.2.3+build.4
        (version(1, 2, 3, &[], &["build", "4"]), "1.2.3+build.4"),
        // Go: .../1.2.3
        (version(1, 2, 3, &[], &[]), "1.2.3"),
    ];
    for (input, expected) in &cases {
        assert_eq!(input.to_string(), *expected);
    }
}

// Go: internal/semver/version_test.go:TestVersionCompare
#[test]
fn test_version_compare() {
    use Ordering::{Equal, Greater, Less};
    let cases: &[(&str, &str, Ordering)] = &[
        // Major, minor, and patch versions are always compared numerically.
        ("1.0.0", "2.0.0", Less),
        ("1.0.0", "1.1.0", Less),
        ("1.0.0", "1.0.1", Less),
        ("2.0.0", "1.0.0", Greater),
        ("1.1.0", "1.0.0", Greater),
        ("1.0.1", "1.0.0", Greater),
        ("1.0.0", "1.0.0", Equal),
        // A pre-release version has lower precedence than a normal version.
        ("1.0.0", "1.0.0-pre", Greater),
        ("1.0.1-pre", "1.0.0", Greater),
        ("1.0.0-pre", "1.0.0", Less),
        // Identifiers consisting of only digits are compared numerically.
        ("1.0.0-0", "1.0.0-1", Less),
        ("1.0.0-1", "1.0.0-0", Greater),
        ("1.0.0-2", "1.0.0-10", Less),
        ("1.0.0-10", "1.0.0-2", Greater),
        ("1.0.0-0", "1.0.0-0", Equal),
        // Identifiers with letters or hyphens are compared lexically in ASCII order.
        ("1.0.0-a", "1.0.0-b", Less),
        ("1.0.0-a-2", "1.0.0-a-10", Greater),
        ("1.0.0-b", "1.0.0-a", Greater),
        ("1.0.0-a", "1.0.0-a", Equal),
        ("1.0.0-A", "1.0.0-a", Less),
        // Numeric identifiers always have lower precedence than non-numeric identifiers.
        ("1.0.0-0", "1.0.0-alpha", Less),
        ("1.0.0-alpha", "1.0.0-0", Greater),
        ("1.0.0-0", "1.0.0-0", Equal),
        ("1.0.0-alpha", "1.0.0-alpha", Equal),
        // A larger set of pre-release fields has a higher precedence than a smaller set.
        ("1.0.0-alpha", "1.0.0-alpha.0", Less),
        ("1.0.0-alpha.0", "1.0.0-alpha", Greater),
        // Compare each dot separated identifier from left to right.
        ("1.0.0-a.0.b.1", "1.0.0-a.0.b.2", Less),
        ("1.0.0-a.0.b.1", "1.0.0-b.0.a.1", Less),
        ("1.0.0-a.0.b.2", "1.0.0-a.0.b.1", Greater),
        ("1.0.0-b.0.a.1", "1.0.0-a.0.b.1", Greater),
        // Build metadata does not figure into precedence.
        ("1.0.0+build", "1.0.0", Equal),
        ("1.0.0+build.stuff", "1.0.0", Equal),
        ("1.0.0", "1.0.0+build", Equal),
        ("1.0.0+build", "1.0.0+stuff", Equal),
        // Edge cases for numeric and lexical comparison of prerelease identifiers.
        ("1.0.0-alpha.99999", "1.0.0-alpha.100000", Less),
        ("1.0.0-alpha.beta", "1.0.0-alpha.alpha", Greater),
    ];
    for (v1, v2, want) in cases {
        let a = try_parse_version(v1).expect(v1);
        let b = try_parse_version(v2).expect(v2);
        assert_eq!(a.cmp(&b), *want, "{v1} <=> {v2}");
    }
}
