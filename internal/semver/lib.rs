//! `tsgo_semver` is a 1:1 Rust port of Go `internal/semver`.
//!
//! It implements the npm-flavored subset of semantic versioning used by
//! TypeScript: a relaxed [`Version`] value type (missing `minor`/`patch`
//! default to `0`) plus npm-style [`VersionRange`] parsing and matching
//! (`~`, `^`, hyphen ranges, `||`, and `x`/`X`/`*` wildcards).

mod version;
mod version_range;

pub use version::*;
pub use version_range::*;
