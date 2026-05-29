use super::*;

// Go: internal/core/version.go:VersionMajorMinor
#[test]
fn version_major_minor_is_prefix() {
    assert_eq!(version_major_minor(), "7.0");
    assert!(version().starts_with(version_major_minor()));
}
