//! Project reference model (`ProjectReference`).
//!
//! 1:1 port of Go `internal/core/projectreference.go`.

use tsgo_tspath::{combine_paths, file_extension_is, EXTENSION_JSON};

/// A reference from one project to another (a composite build edge).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectReference {
    /// The referenced project path.
    pub path: String,
    /// The original (pre-resolution) path.
    pub original_path: String,
    /// Whether this reference forms a cycle.
    pub circular: bool,
}

/// Resolves the config file path for `reference`.
///
/// Side effects: none (pure).
// Go: internal/core/projectreference.go:ResolveProjectReferencePath
pub fn resolve_project_reference_path(reference: &ProjectReference) -> String {
    resolve_config_file_name_of_project_reference(&reference.path)
}

/// Resolves a config file name for `path` (a `.json` path is returned as-is; a
/// directory gets `tsconfig.json` appended).
///
/// Side effects: none (pure).
// Go: internal/core/projectreference.go:ResolveConfigFileNameOfProjectReference
pub fn resolve_config_file_name_of_project_reference(path: &str) -> String {
    if file_extension_is(path, EXTENSION_JSON) {
        return path.to_string();
    }
    combine_paths(path, &["tsconfig.json"])
}

#[cfg(test)]
#[path = "projectreference_test.rs"]
mod tests;
