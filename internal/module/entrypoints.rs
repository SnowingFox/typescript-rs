//! Enumerating a package's entrypoints from its `exports` map (or `main`/index
//! plus a directory scan), used by module-specifier generation.
//!
//! Split from Go `internal/module/resolver.go` (PORTING.md §2).
//!
//! # Divergence from Go
//! Go's `loadEntrypointsFromExportMap` uses a recursive closure that captures
//! and mutates a shared `entrypoints` slice; here it is a recursive method
//! threading the accumulator explicitly. Condition sets are passed by owned
//! `Option<Set<String>>` clones (Go passes nilable `*Set`).

use tsgo_collections::Set;
use tsgo_packagejson::{ExportsOrImports, JsonValueType};
use tsgo_tspath::{self as tspath, ComparePathsOptions};
use tsgo_vfs::vfsmatch::{read_directory, UNLIMITED_DEPTH};

use crate::state::{PackageJsonInfo, ResolutionState};
use crate::util::{is_applicable_versioned_types_key, try_get_js_extension_for_file};
use crate::{ResolvedExt, Resolver};

/// Describes how much of an entrypoint's module specifier may be rewritten.
// Go: internal/module/resolver.go:Ending
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// The specifier cannot be changed without changing resolution.
    // Go: internal/module/resolver.go:EndingFixed
    Fixed,
    /// The extension portion was inferred from disk and is interchangeable.
    // Go: internal/module/resolver.go:EndingExtensionChangeable
    ExtensionChangeable,
    /// The whole file name and extension were inferred and can be changed.
    // Go: internal/module/resolver.go:EndingChangeable
    Changeable,
}

/// A package entrypoint discovered via its `exports` map or directory scan.
///
/// Side effects: none (plain data).
// Go: internal/module/resolver.go:ResolvedEntrypoint
#[derive(Debug, Clone)]
pub struct ResolvedEntrypoint {
    /// The symlink path the entrypoint was discovered at, or empty.
    pub original_file_name: String,
    /// The real path to the entrypoint file.
    pub resolved_file_name: String,
    /// The module specifier that reaches this entrypoint.
    pub module_specifier: String,
    /// How much of the specifier is fixed vs changeable.
    pub ending: Ending,
    /// Conditions a resolver must have to reach this entrypoint.
    pub include_conditions: Option<Set<String>>,
    /// Conditions a resolver must not have to reach this entrypoint.
    pub exclude_conditions: Option<Set<String>>,
}

impl ResolvedEntrypoint {
    /// Returns the symlink path if present, otherwise the resolved real path.
    ///
    /// Side effects: none (pure).
    // Go: internal/module/resolver.go:ResolvedEntrypoint.SymlinkOrRealpath
    pub fn symlink_or_realpath(&self) -> &str {
        if !self.original_file_name.is_empty() {
            &self.original_file_name
        } else {
            &self.resolved_file_name
        }
    }
}

impl Resolver {
    /// Enumerates the entrypoints declared by `package_json`, optionally also
    /// scanning the package directory for additional files.
    ///
    /// Side effects: reads the file system.
    // Go: internal/module/resolver.go:Resolver.GetEntrypointsFromPackageJsonInfo
    pub fn get_entrypoints_from_package_json_info(
        &self,
        package_json: &PackageJsonInfo,
        package_name: &str,
        enable_directory_search: bool,
    ) -> Vec<ResolvedEntrypoint> {
        let mut state = ResolutionState::for_entrypoints(self);
        if package_json.exists() && package_json.contents().fields().path.exports.is_present() {
            let exports = package_json.contents().fields().path.exports.clone();
            return state.load_entrypoints_from_export_map(package_json, package_name, &exports);
        }

        let mut result: Vec<ResolvedEntrypoint> = Vec::new();
        let extensions = state.extensions;
        let main_resolution = state.load_node_module_from_directory_worker(
            extensions,
            package_json.package_directory(),
            Some(package_json),
        );

        if main_resolution.is_resolved() {
            let path = main_resolution.as_ref().expect("resolved").path.clone();
            result.push(self.create_resolved_entrypoint_handling_symlink(
                &path,
                package_name.to_string(),
                None,
                None,
                Ending::Fixed,
            ));
        }

        if enable_directory_search {
            let other_files = read_directory(
                self.host.fs(),
                self.host.get_current_directory(),
                package_json.package_directory(),
                &extensions.to_array(),
                &["node_modules".to_string()],
                &["**/*".to_string()],
                UNLIMITED_DEPTH,
            );
            let opts = ComparePathsOptions {
                use_case_sensitive_file_names: self.host.fs().use_case_sensitive_file_names(),
                current_directory: String::new(),
            };
            let main_path = main_resolution.as_ref().map(|r| r.path.clone());
            for file in &other_files {
                if let Some(main_path) = &main_path {
                    if tspath::compare_paths(file, main_path, &opts) == std::cmp::Ordering::Equal {
                        continue;
                    }
                }
                let module_specifier = tspath::resolve_path(
                    package_name,
                    &[&tspath::get_relative_path_from_directory(
                        package_json.package_directory(),
                        file,
                        &opts,
                    )],
                );
                result.push(self.create_resolved_entrypoint_handling_symlink(
                    file,
                    module_specifier,
                    None,
                    None,
                    Ending::Changeable,
                ));
            }
        }

        result
    }

    // Go: internal/module/resolver.go:Resolver.createResolvedEntrypointHandlingSymlink
    pub(crate) fn create_resolved_entrypoint_handling_symlink(
        &self,
        file_name: &str,
        module_specifier: String,
        include_conditions: Option<Set<String>>,
        exclude_conditions: Option<Set<String>>,
        ending: Ending,
    ) -> ResolvedEntrypoint {
        let mut original_file_name = String::new();
        let mut resolved_file_name = file_name.to_string();
        let real_path = self.host.fs().realpath(file_name);
        if real_path != file_name {
            original_file_name = file_name.to_string();
            resolved_file_name = real_path;
        }
        ResolvedEntrypoint {
            original_file_name,
            resolved_file_name,
            module_specifier,
            ending,
            include_conditions,
            exclude_conditions,
        }
    }
}

impl ResolutionState<'_> {
    // Go: internal/module/resolver.go:resolutionState.loadEntrypointsFromExportMap
    pub(crate) fn load_entrypoints_from_export_map(
        &mut self,
        package_json: &PackageJsonInfo,
        package_name: &str,
        exports: &ExportsOrImports,
    ) -> Vec<ResolvedEntrypoint> {
        let mut entrypoints: Vec<ResolvedEntrypoint> = Vec::new();
        match exports.value_type() {
            JsonValueType::Array => {
                let elements: Vec<ExportsOrImports> = exports.as_array().to_vec();
                for element in &elements {
                    self.load_entrypoints_from_target_exports(
                        package_json,
                        package_name,
                        &mut entrypoints,
                        ".",
                        None,
                        None,
                        element,
                    );
                }
            }
            JsonValueType::Object => {
                if exports.is_subpaths() {
                    let subpaths: Vec<(String, ExportsOrImports)> = exports
                        .as_object()
                        .entries()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    for (subpath, export) in &subpaths {
                        self.load_entrypoints_from_target_exports(
                            package_json,
                            package_name,
                            &mut entrypoints,
                            subpath,
                            None,
                            None,
                            export,
                        );
                    }
                } else {
                    self.load_entrypoints_from_target_exports(
                        package_json,
                        package_name,
                        &mut entrypoints,
                        ".",
                        None,
                        None,
                        exports,
                    );
                }
            }
            _ => {
                self.load_entrypoints_from_target_exports(
                    package_json,
                    package_name,
                    &mut entrypoints,
                    ".",
                    None,
                    None,
                    exports,
                );
            }
        }
        entrypoints
    }

    #[allow(clippy::too_many_arguments)]
    fn load_entrypoints_from_target_exports(
        &mut self,
        package_json: &PackageJsonInfo,
        package_name: &str,
        entrypoints: &mut Vec<ResolvedEntrypoint>,
        subpath: &str,
        include_conditions: Option<Set<String>>,
        exclude_conditions: Option<Set<String>>,
        exports: &ExportsOrImports,
    ) {
        if exports.value_type() == JsonValueType::String && exports.as_str().starts_with("./") {
            let export_str = exports.as_str();
            if export_str.contains('*') {
                if export_str.find('*') != export_str.rfind('*') {
                    return;
                }
                let pattern_path =
                    tspath::resolve_path(package_json.package_directory(), &[export_str]);
                let (leading_slice, trailing_slice) = match pattern_path.split_once('*') {
                    Some((a, b)) => (a.to_string(), b.to_string()),
                    None => (pattern_path.clone(), String::new()),
                };
                let case_sensitive = self.resolver.host.fs().use_case_sensitive_file_names();
                let include_glob =
                    tspath::change_full_extension(&export_str.replacen('*', "**/*", 1), ".*");
                let files = read_directory(
                    self.resolver.host.fs(),
                    self.resolver.host.get_current_directory(),
                    package_json.package_directory(),
                    &self.extensions.to_array(),
                    &[],
                    &[include_glob],
                    UNLIMITED_DEPTH,
                );
                let ending = if export_str.ends_with('*') {
                    Ending::ExtensionChangeable
                } else {
                    Ending::Fixed
                };
                for file in &files {
                    let Some(matched_star) = self.get_matched_star_for_pattern_entrypoint(
                        file,
                        &leading_slice,
                        &trailing_slice,
                        case_sensitive,
                    ) else {
                        continue;
                    };
                    let module_specifier = tspath::resolve_path(
                        package_name,
                        &[&subpath.replacen('*', &matched_star, 1)],
                    );
                    entrypoints.push(self.resolver.create_resolved_entrypoint_handling_symlink(
                        file,
                        module_specifier,
                        include_conditions.clone(),
                        exclude_conditions.clone(),
                        ending,
                    ));
                }
            } else {
                let components = tspath::get_path_components(export_str, "");
                let parts_after_first = if components.len() > 2 {
                    &components[2..]
                } else {
                    &[]
                };
                if parts_after_first
                    .iter()
                    .any(|p| p == ".." || p == "." || p == "node_modules")
                {
                    return;
                }
                let resolved_target =
                    tspath::resolve_path(package_json.package_directory(), &[export_str]);
                let ext = self.extensions;
                let export_str_owned = export_str.to_string();
                let result = self.load_file_name_from_package_json_field(
                    ext,
                    &resolved_target,
                    &export_str_owned,
                );
                if result.is_resolved() {
                    let path = result.as_ref().expect("resolved").path.clone();
                    let module_specifier = tspath::resolve_path(package_name, &[subpath]);
                    let ending = if export_str_owned.ends_with('*') {
                        Ending::ExtensionChangeable
                    } else {
                        Ending::Fixed
                    };
                    entrypoints.push(self.resolver.create_resolved_entrypoint_handling_symlink(
                        &path,
                        module_specifier,
                        include_conditions.clone(),
                        exclude_conditions.clone(),
                        ending,
                    ));
                }
            }
        } else if exports.value_type() == JsonValueType::Array {
            let elements: Vec<ExportsOrImports> = exports.as_array().to_vec();
            for element in &elements {
                self.load_entrypoints_from_target_exports(
                    package_json,
                    package_name,
                    entrypoints,
                    subpath,
                    include_conditions.clone(),
                    exclude_conditions.clone(),
                    element,
                );
            }
        } else if exports.value_type() == JsonValueType::Object {
            let mut prev_conditions: Vec<String> = Vec::new();
            let mut exclude_conditions = exclude_conditions;
            let entries: Vec<(String, ExportsOrImports)> = exports
                .as_object()
                .entries()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (condition, export) in &entries {
                if exclude_conditions
                    .as_ref()
                    .is_some_and(|s| s.has(condition))
                {
                    continue;
                }
                let condition_always_matches = condition == "default"
                    || condition == "types"
                    || is_applicable_versioned_types_key(condition);
                let mut new_include_conditions = include_conditions.clone();
                if !condition_always_matches {
                    let mut inc = new_include_conditions.unwrap_or_default();
                    inc.add(condition.clone());
                    new_include_conditions = Some(inc);
                    let mut exc = exclude_conditions.clone().unwrap_or_default();
                    for prev_condition in &prev_conditions {
                        exc.add(prev_condition.clone());
                    }
                    exclude_conditions = Some(exc);
                }
                prev_conditions.push(condition.clone());
                self.load_entrypoints_from_target_exports(
                    package_json,
                    package_name,
                    entrypoints,
                    subpath,
                    new_include_conditions,
                    exclude_conditions.clone(),
                    export,
                );
                if condition_always_matches {
                    break;
                }
            }
        }
    }

    // Go: internal/module/resolver.go:resolutionState.getMatchedStarForPatternEntrypoint
    fn get_matched_star_for_pattern_entrypoint(
        &self,
        file: &str,
        leading_slice: &str,
        trailing_slice: &str,
        case_sensitive: bool,
    ) -> Option<String> {
        if tsgo_stringutil::has_prefix_and_suffix_without_overlap(
            file,
            leading_slice,
            trailing_slice,
            case_sensitive,
        ) {
            return Some(file[leading_slice.len()..file.len() - trailing_slice.len()].to_string());
        }

        let js_extension = try_get_js_extension_for_file(file, &self.compiler_options);
        if !js_extension.is_empty() {
            let swapped = tspath::change_full_extension(file, js_extension);
            if tsgo_stringutil::has_prefix_and_suffix_without_overlap(
                &swapped,
                leading_slice,
                trailing_slice,
                case_sensitive,
            ) {
                return Some(
                    swapped[leading_slice.len()..swapped.len() - trailing_slice.len()].to_string(),
                );
            }
        }

        None
    }
}

#[cfg(test)]
#[path = "entrypoints_test.rs"]
mod tests;
