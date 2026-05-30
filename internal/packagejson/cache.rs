//! Parsed `package.json` contents plus lazy `typesVersions` resolution and a
//! per-directory cache keyed by canonical [`Path`].
//!
//! 1:1 port of Go `internal/packagejson/cache.go`.
//!
//! # Divergence from Go
//! - Go uses `sync.Once` + mutable fields for `GetVersionPaths`; here a single
//!   [`OnceLock`] holds both the resolved [`VersionPaths`] and the recorded
//!   trace diagnostics, computed once and replayed on every call. Using
//!   [`OnceLock`] (rather than `OnceCell`) keeps [`PackageJson`] `Send + Sync`,
//!   matching Go's `sync.Once` + `SyncMap` so the cache can be shared across
//!   threads.
//! - Go's trace callback is `func(*Message, ...any)`; diagnostics only ever
//!   pass strings, so the recorded arguments are stored as `Vec<String>`.

use std::sync::{Arc, LazyLock, OnceLock};

use tsgo_collections::{OrderedMap, SyncMap};
use tsgo_core::version::{version, version_major_minor};
use tsgo_diagnostics::{
    Message, EXPECTED_TYPE_OF_0_FIELD_IN_PACKAGE_JSON_TO_BE_1_GOT_2,
    X_PACKAGE_JSON_DOES_NOT_HAVE_A_0_FIELD,
    X_PACKAGE_JSON_DOES_NOT_HAVE_A_TYPESVERSIONS_ENTRY_THAT_MATCHES_VERSION_0,
    X_PACKAGE_JSON_HAS_A_TYPESVERSIONS_ENTRY_0_THAT_IS_NOT_A_VALID_SEMVER_RANGE,
    X_PACKAGE_JSON_HAS_A_TYPESVERSIONS_FIELD_WITH_VERSION_SPECIFIC_PATH_MAPPINGS,
};
use tsgo_semver::{must_parse, try_parse_version_range, Version};
use tsgo_tspath::{to_path, Path};

use crate::{Fields, JsonValue, JsonValueType};

// Go: internal/packagejson/cache.go:typeScriptVersion
static TYPESCRIPT_VERSION: LazyLock<Version> = LazyLock::new(|| must_parse(version()));

/// A callback receiving each diagnostic recorded while resolving
/// `typesVersions`, as `(message, stringified args)`.
///
/// Mirrors Go's `func(m *diagnostics.Message, args ...any)`; diagnostics only
/// ever pass strings, so the arguments arrive as `&[&str]`.
pub type VersionPathsTrace<'a> = dyn FnMut(&'static Message, &[&str]) + 'a;

// Go: internal/packagejson/cache.go:diagnosticAndArgs
#[derive(Debug, Clone)]
struct DiagnosticAndArgs {
    message: &'static Message,
    args: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct ComputedVersionPaths {
    version_paths: VersionPaths,
    traces: Vec<DiagnosticAndArgs>,
}

/// Parsed `package.json` [`Fields`] plus lazily-resolved `typesVersions`.
///
/// # Examples
/// ```
/// use tsgo_packagejson::{parse, PackageJson};
/// let pj = PackageJson::new(parse(b"{}").unwrap(), true);
/// assert!(pj.parseable());
/// assert!(!pj.get_version_paths(None).exists());
/// ```
// Go: internal/packagejson/cache.go:PackageJson
#[derive(Debug, Default)]
pub struct PackageJson {
    fields: Fields,
    parseable: bool,
    computed: OnceLock<ComputedVersionPaths>,
}

impl PackageJson {
    /// Builds a [`PackageJson`] from parsed [`Fields`].
    ///
    /// `parseable` records whether the source file parsed without error.
    ///
    /// Side effects: none (pure).
    pub fn new(fields: Fields, parseable: bool) -> Self {
        PackageJson {
            fields,
            parseable,
            computed: OnceLock::new(),
        }
    }

    /// Returns the parsed fields.
    ///
    /// Side effects: none (pure).
    pub fn fields(&self) -> &Fields {
        &self.fields
    }

    /// Reports whether the source `package.json` parsed without error.
    ///
    /// Side effects: none (pure).
    pub fn parseable(&self) -> bool {
        self.parseable
    }

    /// Resolves the `typesVersions` entry matching the compiler version,
    /// computing it once and caching the result.
    ///
    /// If `trace` is supplied, the diagnostics recorded during resolution
    /// (missing field, wrong type, invalid range, no match) are replayed to it
    /// on every call.
    ///
    /// Side effects: caches the resolution on first call (interior mutability);
    /// invokes `trace` if supplied.
    // Go: internal/packagejson/cache.go:GetVersionPaths
    pub fn get_version_paths(&self, trace: Option<&mut VersionPathsTrace<'_>>) -> &VersionPaths {
        let computed = self.computed.get_or_init(|| self.compute_version_paths());
        if let Some(trace) = trace {
            for diagnostic in &computed.traces {
                let args: Vec<&str> = diagnostic.args.iter().map(String::as_str).collect();
                trace(diagnostic.message, &args);
            }
        }
        &computed.version_paths
    }

    fn compute_version_paths(&self) -> ComputedVersionPaths {
        let mut traces = Vec::new();
        let types_versions = &self.fields.path.types_versions;
        match types_versions.value_type() {
            JsonValueType::NotPresent => {
                traces.push(DiagnosticAndArgs {
                    message: &X_PACKAGE_JSON_DOES_NOT_HAVE_A_0_FIELD,
                    args: vec!["typesVersions".to_string()],
                });
                return ComputedVersionPaths {
                    traces,
                    ..Default::default()
                };
            }
            JsonValueType::Object => {}
            other => {
                traces.push(DiagnosticAndArgs {
                    message: &EXPECTED_TYPE_OF_0_FIELD_IN_PACKAGE_JSON_TO_BE_1_GOT_2,
                    args: vec![
                        "typesVersions".to_string(),
                        "object".to_string(),
                        other.to_string(),
                    ],
                });
                return ComputedVersionPaths {
                    traces,
                    ..Default::default()
                };
            }
        }

        traces.push(DiagnosticAndArgs {
            message: &X_PACKAGE_JSON_HAS_A_TYPESVERSIONS_FIELD_WITH_VERSION_SPECIFIC_PATH_MAPPINGS,
            args: vec!["typesVersions".to_string()],
        });

        for (key, value) in types_versions.as_object().entries() {
            let (key_range, ok) = try_parse_version_range(key);
            if !ok {
                traces.push(DiagnosticAndArgs {
                    message: &X_PACKAGE_JSON_HAS_A_TYPESVERSIONS_ENTRY_0_THAT_IS_NOT_A_VALID_SEMVER_RANGE,
                    args: vec![key.clone()],
                });
                continue;
            }
            if key_range.test(&TYPESCRIPT_VERSION) {
                if value.value_type() != JsonValueType::Object {
                    traces.push(DiagnosticAndArgs {
                        message: &EXPECTED_TYPE_OF_0_FIELD_IN_PACKAGE_JSON_TO_BE_1_GOT_2,
                        args: vec![
                            format!("typesVersions['{key}']"),
                            "object".to_string(),
                            value.value_type().to_string(),
                        ],
                    });
                    return ComputedVersionPaths {
                        traces,
                        ..Default::default()
                    };
                }
                return ComputedVersionPaths {
                    version_paths: VersionPaths {
                        version: key.clone(),
                        paths_json: Some(value.as_object().clone()),
                        paths: OnceLock::new(),
                    },
                    traces,
                };
            }
        }

        traces.push(DiagnosticAndArgs {
            message: &X_PACKAGE_JSON_DOES_NOT_HAVE_A_TYPESVERSIONS_ENTRY_THAT_MATCHES_VERSION_0,
            args: vec![version_major_minor().to_string()],
        });
        ComputedVersionPaths {
            traces,
            ..Default::default()
        }
    }
}

/// The `typesVersions` entry selected for the current compiler version, plus a
/// lazily-extracted `pattern -> targets` mapping.
///
/// An empty/unset value is reported by [`VersionPaths::exists`].
// Go: internal/packagejson/cache.go:VersionPaths
#[derive(Debug, Clone, Default)]
pub struct VersionPaths {
    version: String,
    paths_json: Option<OrderedMap<String, JsonValue>>,
    paths: OnceLock<OrderedMap<String, Vec<String>>>,
}

impl VersionPaths {
    /// Reports whether a matching `typesVersions` entry was found.
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/cache.go:Exists
    pub fn exists(&self) -> bool {
        !self.version.is_empty() && self.paths_json.is_some()
    }

    /// Returns the matched range key (e.g. `">=4.0"`), empty when none matched.
    ///
    /// Side effects: none (pure).
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Returns the `pattern -> targets` mapping, extracting it lazily from the
    /// raw JSON on first call. Non-array values are skipped; non-string array
    /// elements become empty strings (preserving index alignment), matching Go.
    ///
    /// Side effects: caches the extracted mapping on first call (interior
    /// mutability). Returns `None` when [`VersionPaths::exists`] is false.
    // Go: internal/packagejson/cache.go:GetPaths
    pub fn get_paths(&self) -> Option<&OrderedMap<String, Vec<String>>> {
        if !self.exists() {
            return None;
        }
        Some(self.paths.get_or_init(|| {
            let paths_json = self
                .paths_json
                .as_ref()
                .expect("exists() guarantees paths_json is Some");
            let mut paths = OrderedMap::with_size_hint(paths_json.size());
            for (key, value) in paths_json.entries() {
                if value.value_type() != JsonValueType::Array {
                    continue;
                }
                let array = value.as_array();
                let mut targets = Vec::with_capacity(array.len());
                for path in array {
                    if path.value_type() == JsonValueType::String {
                        targets.push(path.as_str().to_string());
                    } else {
                        targets.push(String::new());
                    }
                }
                paths.set(key.clone(), targets);
            }
            paths
        }))
    }
}

/// A cache entry recording the `package.json` directory, whether it exists, and
/// the parsed contents (absent when there is no readable `package.json`).
// Go: internal/packagejson/cache.go:InfoCacheEntry
#[derive(Debug)]
pub struct InfoCacheEntry {
    /// The directory that contains the `package.json`.
    pub package_directory: String,
    /// Whether the directory exists on disk.
    pub directory_exists: bool,
    /// The parsed contents, if a `package.json` was found and readable.
    pub contents: Option<PackageJson>,
}

impl InfoCacheEntry {
    /// Reports whether the entry has parsed contents.
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/cache.go:Exists
    pub fn exists(&self) -> bool {
        self.contents.is_some()
    }

    /// Returns the parsed contents, if any.
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/cache.go:GetContents
    pub fn get_contents(&self) -> Option<&PackageJson> {
        self.contents.as_ref()
    }

    /// Returns the package directory.
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/cache.go:GetDirectory
    pub fn get_directory(&self) -> &str {
        &self.package_directory
    }
}

/// A concurrent cache of [`InfoCacheEntry`] keyed by canonical `package.json`
/// [`Path`].
// Go: internal/packagejson/cache.go:InfoCache
pub struct InfoCache {
    cache: SyncMap<Path, Option<Arc<InfoCacheEntry>>>,
    current_directory: String,
    use_case_sensitive_file_names: bool,
}

impl InfoCache {
    /// Creates an empty cache rooted at `current_directory`.
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/cache.go:NewInfoCache
    pub fn new(current_directory: String, use_case_sensitive_file_names: bool) -> Self {
        InfoCache {
            cache: SyncMap::default(),
            current_directory,
            use_case_sensitive_file_names,
        }
    }

    /// Returns the cached entry for `package_json_path`, if present.
    ///
    /// Side effects: none (pure; reads the shared map).
    // Go: internal/packagejson/cache.go:Get
    pub fn get(&self, package_json_path: &str) -> Option<Arc<InfoCacheEntry>> {
        let key = self.to_key(package_json_path);
        let (value, ok) = self.cache.load(&key);
        if ok {
            value
        } else {
            None
        }
    }

    /// Stores `info` for `package_json_path` if absent, returning the value now
    /// in the cache (the existing one on a race, matching Go's `LoadOrStore`).
    ///
    /// Side effects: may insert into the shared map.
    // Go: internal/packagejson/cache.go:Set
    pub fn set(&self, package_json_path: &str, info: Arc<InfoCacheEntry>) -> Arc<InfoCacheEntry> {
        let key = self.to_key(package_json_path);
        let (actual, _) = self.cache.load_or_store(key, Some(info));
        actual.expect("Set always stores Some")
    }

    fn to_key(&self, package_json_path: &str) -> Path {
        to_path(
            package_json_path,
            &self.current_directory,
            self.use_case_sensitive_file_names,
        )
    }
}

#[cfg(test)]
#[path = "cache_test.rs"]
mod tests;
