//! `tsgo_packagejson` — 1:1 Rust port of Go `internal/packagejson`.
//!
//! Strongly-typed parsing and querying of `package.json`: it keeps each
//! field's "actual JSON type vs expected type" so module resolution can emit
//! precise diagnostics, and exposes structured access to `exports`/`imports`/
//! `typesVersions` plus per-directory caching.
//!
//! # Divergence from Go
//! Go composes the field groups with struct embedding (`Fields` embeds
//! `HeaderFields`/`PathFields`/`DependencyFields`). Rust has no embedding, so
//! [`Fields`] holds the three groups as named members (`header`/`path`/`deps`)
//! and provides a hand-written [`Deserialize`] that mirrors Go's
//! `json.AllowDuplicateNames(true)` (last value wins, insertion order kept).

mod cache;
mod expected;
mod exportsorimports;
mod jsonvalue;
mod validated;

pub use cache::*;
pub use expected::*;
pub use exportsorimports::*;
pub use jsonvalue::*;
pub use validated::*;

use std::collections::HashMap;
use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use tsgo_collections::Set;

/// The `name`/`version`/`type` header fields of a `package.json`.
///
/// # Examples
/// ```
/// let f = tsgo_packagejson::parse(br#"{"name":"x","version":"1.0.0"}"#).unwrap();
/// assert_eq!(f.header.name.get_value().0, "x");
/// ```
// Go: internal/packagejson/packagejson.go:HeaderFields
#[derive(Debug, Clone, Default)]
pub struct HeaderFields {
    /// The package `name`.
    pub name: Expected<String>,
    /// The package `version`.
    pub version: Expected<String>,
    /// The module `type` (`"module"` / `"commonjs"`).
    pub type_: Expected<String>,
}

/// The path-related fields of a `package.json` (`main`, `types`, `exports`, …).
// Go: internal/packagejson/packagejson.go:PathFields
#[derive(Debug, Clone, Default)]
pub struct PathFields {
    /// The `tsconfig` field.
    pub tsconfig: Expected<String>,
    /// The `main` entry point.
    pub main: Expected<String>,
    /// The `types` entry point.
    pub types: Expected<String>,
    /// The legacy `typings` entry point.
    pub typings: Expected<String>,
    /// The raw `typesVersions` mapping (parsed lazily by `cache`).
    pub types_versions: JsonValue,
    /// The `imports` field.
    pub imports: ExportsOrImports,
    /// The `exports` field.
    pub exports: ExportsOrImports,
}

/// The dependency maps of a `package.json`.
// Go: internal/packagejson/packagejson.go:DependencyFields
#[derive(Debug, Clone, Default)]
pub struct DependencyFields {
    /// Runtime `dependencies`.
    pub dependencies: Expected<HashMap<String, String>>,
    /// Development-only `devDependencies`.
    pub dev_dependencies: Expected<HashMap<String, String>>,
    /// `peerDependencies`.
    pub peer_dependencies: Expected<HashMap<String, String>>,
    /// `optionalDependencies`.
    pub optional_dependencies: Expected<HashMap<String, String>>,
}

impl DependencyFields {
    /// Reports whether `name` appears under any dependency field.
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/packagejson.go:HasDependency
    pub fn has_dependency(&self, name: &str) -> bool {
        for field in [
            &self.dependencies,
            &self.dev_dependencies,
            &self.peer_dependencies,
            &self.optional_dependencies,
        ] {
            let (deps, ok) = field.get_value();
            if ok && deps.contains_key(name) {
                return true;
            }
        }
        false
    }

    /// Invokes `f(name, version, dependency_field)` for every dependency across
    /// all four fields, stopping early when `f` returns `false`.
    ///
    /// Side effects: invokes `f`.
    // Go: internal/packagejson/packagejson.go:RangeDependencies
    pub fn range_dependencies(&self, mut f: impl FnMut(&str, &str, &str) -> bool) {
        for (field, label) in [
            (&self.dependencies, "dependencies"),
            (&self.dev_dependencies, "devDependencies"),
            (&self.peer_dependencies, "peerDependencies"),
            (&self.optional_dependencies, "optionalDependencies"),
        ] {
            let (deps, ok) = field.get_value();
            if ok {
                for (name, version) in deps {
                    if !f(name, version, label) {
                        return;
                    }
                }
            }
        }
    }

    /// Returns the names of runtime dependencies: `dependencies`,
    /// `peerDependencies`, and `optionalDependencies` (deliberately excluding
    /// `devDependencies`).
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/packagejson.go:GetRuntimeDependencyNames
    pub fn get_runtime_dependency_names(&self) -> Set<String> {
        let deps = self.dependencies.get_value().0;
        let peer_deps = self.peer_dependencies.get_value().0;
        let opt_deps = self.optional_dependencies.get_value().0;
        let count = deps.len() + peer_deps.len() + opt_deps.len();
        let mut names = Set::with_size_hint(count);
        for map in [deps, peer_deps, opt_deps] {
            for name in map.keys() {
                names.add(name.clone());
            }
        }
        names
    }
}

/// The complete set of recognized `package.json` fields.
///
/// # Examples
/// ```
/// let f = tsgo_packagejson::parse(br#"{"type":"module"}"#).unwrap();
/// assert_eq!(f.header.type_.get_value().0, "module");
/// ```
// Go: internal/packagejson/packagejson.go:Fields
#[derive(Debug, Clone, Default)]
pub struct Fields {
    /// The header fields (`name`/`version`/`type`).
    pub header: HeaderFields,
    /// The path-related fields.
    pub path: PathFields,
    /// The dependency fields.
    pub deps: DependencyFields,
}

struct FieldsVisitor;

impl<'de> Visitor<'de> for FieldsVisitor {
    type Value = Fields;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a package.json object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Fields, A::Error> {
        let mut fields = Fields::default();
        // Last value wins on duplicate keys (Go `AllowDuplicateNames(true)`).
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "name" => fields.header.name = map.next_value()?,
                "version" => fields.header.version = map.next_value()?,
                "type" => fields.header.type_ = map.next_value()?,
                "tsconfig" => fields.path.tsconfig = map.next_value()?,
                "main" => fields.path.main = map.next_value()?,
                "types" => fields.path.types = map.next_value()?,
                "typings" => fields.path.typings = map.next_value()?,
                "typesVersions" => fields.path.types_versions = map.next_value()?,
                "imports" => fields.path.imports = map.next_value()?,
                "exports" => fields.path.exports = map.next_value()?,
                "dependencies" => fields.deps.dependencies = map.next_value()?,
                "devDependencies" => fields.deps.dev_dependencies = map.next_value()?,
                "peerDependencies" => fields.deps.peer_dependencies = map.next_value()?,
                "optionalDependencies" => fields.deps.optional_dependencies = map.next_value()?,
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }
        Ok(fields)
    }
}

impl<'de> Deserialize<'de> for Fields {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(FieldsVisitor)
    }
}

/// Parses `package.json` bytes into [`Fields`].
///
/// Duplicate keys keep the last value (mirroring Go's
/// `json.AllowDuplicateNames(true)`); object insertion order is preserved for
/// `typesVersions`/`exports`/`imports`.
///
/// # Examples
/// ```
/// let f = tsgo_packagejson::parse(br#"{"name":"pkg","version":"1.2.3"}"#).unwrap();
/// assert_eq!(f.header.version.get_value().0, "1.2.3");
/// ```
///
/// Side effects: none (pure).
// Go: internal/packagejson/packagejson.go:Parse
pub fn parse(data: &[u8]) -> Result<Fields, tsgo_json::Error> {
    tsgo_json::unmarshal(data)
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
