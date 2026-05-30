//! `package.json` `exports`/`imports` resolution: self-name references, the
//! `#imports` map, the `exports` map, conditional targets, and mapping emitted
//! outputs back to input files (`tryLoadInputFileForPath`).
//!
//! Split from Go `internal/module/resolver.go` (PORTING.md §2).

use tsgo_collections::OrderedMap;
use tsgo_diagnostics::{
    DIRECTORY_0_HAS_NO_CONTAINING_PACKAGE_JSON_SCOPE_IMPORTS_WILL_NOT_RESOLVE,
    ENTERING_CONDITIONAL_EXPORTS, EXITING_CONDITIONAL_EXPORTS,
    EXPORT_SPECIFIER_0_DOES_NOT_EXIST_IN_PACKAGE_JSON_SCOPE_AT_PATH_1,
    FAILED_TO_RESOLVE_UNDER_CONDITION_0,
    IMPORT_SPECIFIER_0_DOES_NOT_EXIST_IN_PACKAGE_JSON_SCOPE_AT_PATH_1,
    INVALID_IMPORT_SPECIFIER_0_HAS_NO_POSSIBLE_RESOLUTIONS, MATCHED_0_CONDITION_1,
    RESOLVED_UNDER_CONDITION_0, RESOLVING_MODULE_0_FROM_1, SAW_NON_MATCHING_CONDITION_0,
    THE_PROJECT_ROOT_IS_AMBIGUOUS_BUT_IS_REQUIRED_TO_RESOLVE_EXPORT_MAP_ENTRY_0_IN_FILE_1_SUPPLY_THE_ROOTDIR_COMPILER_OPTION_TO_DISAMBIGUATE,
    THE_PROJECT_ROOT_IS_AMBIGUOUS_BUT_IS_REQUIRED_TO_RESOLVE_IMPORT_MAP_ENTRY_0_IN_FILE_1_SUPPLY_THE_ROOTDIR_COMPILER_OPTION_TO_DISAMBIGUATE,
    USING_0_SUBPATH_1_WITH_TARGET_2, X_PACKAGE_JSON_SCOPE_0_EXPLICITLY_MAPS_SPECIFIER_1_TO_NULL,
    X_PACKAGE_JSON_SCOPE_0_HAS_INVALID_TYPE_FOR_TARGET_OF_SPECIFIER_1,
    X_PACKAGE_JSON_SCOPE_0_HAS_NO_IMPORTS_DEFINED,
};
use tsgo_packagejson::{ExportsOrImports, JsonValueType};
use tsgo_tspath::{
    self as tspath, ComparePathsOptions, EXTENSION_CJS, EXTENSION_DCTS, EXTENSION_DMTS,
    EXTENSION_DTS, EXTENSION_JS, EXTENSION_JSON, EXTENSION_MJS,
};

use crate::state::{PackageJsonInfo, ResolutionState};
use crate::{
    continue_searching, extension_is_ok, matches_pattern_with_trailer, unresolved, write_trace,
    Extensions, ResolutionDiagnostic, Resolved, ResolvedExt, ResolvedInner,
};

impl ResolutionState<'_> {
    // Go: internal/module/resolver.go:resolutionState.loadModuleFromSelfNameReference
    pub(crate) fn load_module_from_self_name_reference(&mut self) -> Resolved {
        let directory_path = tspath::get_normalized_absolute_path(
            &self.containing_directory,
            self.resolver.host.get_current_directory(),
        );
        let Some(scope) = self.get_package_scope_for_path(&directory_path) else {
            return continue_searching();
        };
        if !scope.exists() || scope.contents().fields().path.exports.is_falsy() {
            return continue_searching();
        }
        let (name, ok) = scope.contents().fields().header.name.get_value();
        if !ok {
            return continue_searching();
        }
        let name = name.clone();
        let parts = tspath::get_path_components(&self.name, "");
        let name_parts = tspath::get_path_components(&name, "");
        if parts.len() < name_parts.len() || name_parts[..] != parts[..name_parts.len()] {
            return continue_searching();
        }
        let trailing_parts = &parts[name_parts.len()..];
        let subpath = if !trailing_parts.is_empty() {
            let refs: Vec<&str> = trailing_parts.iter().map(String::as_str).collect();
            tspath::combine_paths(".", &refs)
        } else {
            ".".to_string()
        };
        // See selfNameModuleAugmentation.ts: a single pass with all extensions
        // when allowJs is set and the package is not in node_modules.
        if self.compiler_options.get_allow_js()
            && !self.containing_directory.contains("/node_modules/")
        {
            let ext = self.extensions;
            return self.load_module_from_exports(&scope, ext, &subpath);
        }
        let priority_extensions =
            self.extensions & (Extensions::TYPE_SCRIPT | Extensions::DECLARATION);
        let secondary_extensions =
            self.extensions & !(Extensions::TYPE_SCRIPT | Extensions::DECLARATION);
        let resolved = self.load_module_from_exports(&scope, priority_extensions, &subpath);
        if !resolved.should_continue_searching() {
            return resolved;
        }
        self.load_module_from_exports(&scope, secondary_extensions, &subpath)
    }

    // Go: internal/module/resolver.go:resolutionState.loadModuleFromImports
    pub(crate) fn load_module_from_imports(&mut self) -> Resolved {
        if self.name == "#"
            || (self.name.starts_with("#/")
                && !self
                    .features
                    .contains(crate::NodeResolutionFeatures::IMPORTS_PATTERN_ROOT))
        {
            if self.tracer.is_some() {
                let name = self.name.clone();
                write_trace(
                    &mut self.tracer,
                    &INVALID_IMPORT_SPECIFIER_0_HAS_NO_POSSIBLE_RESOLUTIONS,
                    &[&name],
                );
            }
            return continue_searching();
        }
        let directory_path = tspath::get_normalized_absolute_path(
            &self.containing_directory,
            self.resolver.host.get_current_directory(),
        );
        let Some(scope) = self.get_package_scope_for_path(&directory_path) else {
            write_trace(
                &mut self.tracer,
                &DIRECTORY_0_HAS_NO_CONTAINING_PACKAGE_JSON_SCOPE_IMPORTS_WILL_NOT_RESOLVE,
                &[&directory_path],
            );
            return continue_searching();
        };
        if scope.contents().fields().path.imports.value_type() != JsonValueType::Object {
            if self.tracer.is_some() {
                let dir = scope.package_directory().to_string();
                write_trace(
                    &mut self.tracer,
                    &X_PACKAGE_JSON_SCOPE_0_HAS_NO_IMPORTS_DEFINED,
                    &[&dir],
                );
            }
            return continue_searching();
        }
        let name = self.name.clone();
        let ext = self.extensions;
        let imports = scope.contents().fields().path.imports.as_object();
        let result = self.load_module_from_exports_or_imports(ext, &name, imports, &scope, true);
        if !result.should_continue_searching() {
            return result;
        }
        if self.tracer.is_some() {
            let dir = scope.package_directory().to_string();
            write_trace(
                &mut self.tracer,
                &IMPORT_SPECIFIER_0_DOES_NOT_EXIST_IN_PACKAGE_JSON_SCOPE_AT_PATH_1,
                &[&name, &dir],
            );
        }
        continue_searching()
    }

    // Go: internal/module/resolver.go:resolutionState.loadModuleFromExports
    pub(crate) fn load_module_from_exports(
        &mut self,
        package_info: &PackageJsonInfo,
        ext: Extensions,
        subpath: &str,
    ) -> Resolved {
        if !package_info.exists() || package_info.contents().fields().path.exports.is_falsy() {
            return continue_searching();
        }

        if subpath == "." {
            let exports = &package_info.contents().fields().path.exports;
            let main_export: Option<ExportsOrImports> = match exports.value_type() {
                JsonValueType::String | JsonValueType::Array => Some(exports.clone()),
                JsonValueType::Object => {
                    if exports.is_conditions() {
                        Some(exports.clone())
                    } else {
                        exports.as_object().get(&".".to_string()).cloned()
                    }
                }
                _ => None,
            };
            if let Some(main_export) = main_export {
                if main_export.value_type() != JsonValueType::NotPresent {
                    return self.load_module_from_target_export_or_import(
                        ext,
                        subpath,
                        package_info,
                        false,
                        &main_export,
                        "",
                        false,
                        ".",
                    );
                }
            }
        } else if package_info.contents().fields().path.exports.value_type()
            == JsonValueType::Object
            && package_info.contents().fields().path.exports.is_subpaths()
        {
            let exports = package_info.contents().fields().path.exports.as_object();
            let result = self.load_module_from_exports_or_imports(
                ext,
                subpath,
                exports,
                package_info,
                false,
            );
            if !result.should_continue_searching() {
                return result;
            }
        }

        if self.tracer.is_some() {
            let dir = package_info.package_directory().to_string();
            write_trace(
                &mut self.tracer,
                &EXPORT_SPECIFIER_0_DOES_NOT_EXIST_IN_PACKAGE_JSON_SCOPE_AT_PATH_1,
                &[subpath, &dir],
            );
        }
        continue_searching()
    }

    // Go: internal/module/resolver.go:resolutionState.loadModuleFromExportsOrImports
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn load_module_from_exports_or_imports(
        &mut self,
        extensions: Extensions,
        module_name: &str,
        lookup_table: &OrderedMap<String, ExportsOrImports>,
        scope: &PackageJsonInfo,
        is_imports: bool,
    ) -> Resolved {
        if !module_name.ends_with('/') && !module_name.contains('*') {
            if let Some(target) = lookup_table.get(&module_name.to_string()) {
                return self.load_module_from_target_export_or_import(
                    extensions,
                    module_name,
                    scope,
                    is_imports,
                    target,
                    "",
                    false,
                    module_name,
                );
            }
        }

        let mut expanding_keys: Vec<String> = Vec::new();
        for key in lookup_table.keys() {
            if key.matches('*').count() == 1 || key.ends_with('/') {
                expanding_keys.push(key.clone());
            }
        }
        expanding_keys.sort_by(|a, b| crate::compare_pattern_keys(a, b));

        for potential_target in &expanding_keys {
            if self
                .features
                .contains(crate::NodeResolutionFeatures::EXPORTS_PATTERN_TRAILERS)
                && matches_pattern_with_trailer(potential_target, module_name)
            {
                let target = lookup_table.get(potential_target).expect("key from keys()");
                let star_pos = potential_target.find('*').expect("trailer contains '*'");
                let subpath = &module_name
                    [star_pos..module_name.len() - (potential_target.len() - 1 - star_pos)];
                return self.load_module_from_target_export_or_import(
                    extensions,
                    module_name,
                    scope,
                    is_imports,
                    target,
                    subpath,
                    true,
                    potential_target,
                );
            } else if potential_target.ends_with('*')
                && module_name.starts_with(&potential_target[..potential_target.len() - 1])
            {
                let target = lookup_table.get(potential_target).expect("key from keys()");
                let subpath = &module_name[potential_target.len() - 1..];
                return self.load_module_from_target_export_or_import(
                    extensions,
                    module_name,
                    scope,
                    is_imports,
                    target,
                    subpath,
                    true,
                    potential_target,
                );
            } else if module_name.starts_with(potential_target.as_str()) {
                let target = lookup_table.get(potential_target).expect("key from keys()");
                let subpath = &module_name[potential_target.len()..];
                return self.load_module_from_target_export_or_import(
                    extensions,
                    module_name,
                    scope,
                    is_imports,
                    target,
                    subpath,
                    false,
                    potential_target,
                );
            }
        }

        continue_searching()
    }

    // Go: internal/module/resolver.go:resolutionState.loadModuleFromTargetExportOrImport
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn load_module_from_target_export_or_import(
        &mut self,
        extensions: Extensions,
        module_name: &str,
        scope: &PackageJsonInfo,
        is_imports: bool,
        target: &ExportsOrImports,
        subpath: &str,
        is_pattern: bool,
        key: &str,
    ) -> Resolved {
        match target.value_type() {
            JsonValueType::String => {
                let target_string = target.as_str();
                if !is_pattern && !subpath.is_empty() && !target_string.ends_with('/') {
                    self.trace_invalid_target(scope, module_name);
                    return continue_searching();
                }
                if !target_string.starts_with("./") {
                    if is_imports
                        && !target_string.starts_with("../")
                        && !target_string.starts_with('/')
                        && !tspath::is_rooted_disk_path(target_string)
                    {
                        let combined_lookup = if is_pattern {
                            target_string.replace('*', subpath)
                        } else {
                            format!("{target_string}{subpath}")
                        };
                        let scope_containing_directory =
                            tspath::ensure_trailing_directory_separator(scope.package_directory());
                        if self.tracer.is_some() {
                            write_trace(
                                &mut self.tracer,
                                &USING_0_SUBPATH_1_WITH_TARGET_2,
                                &["imports", key, &combined_lookup],
                            );
                            write_trace(
                                &mut self.tracer,
                                &RESOLVING_MODULE_0_FROM_1,
                                &[&combined_lookup, &scope_containing_directory],
                            );
                        }
                        let saved_name = std::mem::replace(&mut self.name, combined_lookup);
                        let saved_dir = std::mem::replace(
                            &mut self.containing_directory,
                            scope_containing_directory,
                        );
                        let result = self.resolve_node_like();
                        self.name = saved_name;
                        self.containing_directory = saved_dir;
                        if result.is_resolved() {
                            return Some(ResolvedInner {
                                path: result.resolved_file_name,
                                extension: result.extension,
                                package_id: result.package_id,
                                original_path: result.original_path,
                                resolved_using_ts_extension: result.resolved_using_ts_extension,
                            });
                        }
                        return continue_searching();
                    }
                    self.trace_invalid_target(scope, module_name);
                    return continue_searching();
                }

                let components = tspath::get_path_components(target_string, "");
                let parts: &[String] = if tspath::path_is_relative(target_string) {
                    &components[1..]
                } else {
                    &components
                };
                let parts_after_first = &parts[1..];
                if parts_after_first
                    .iter()
                    .any(|p| p == ".." || p == "." || p == "node_modules")
                {
                    self.trace_invalid_target(scope, module_name);
                    return continue_searching();
                }
                let resolved_target =
                    tspath::combine_paths(scope.package_directory(), &[target_string]);
                let subpath_components = tspath::get_path_components(subpath, "");
                if subpath_components
                    .iter()
                    .any(|p| p == ".." || p == "." || p == "node_modules")
                {
                    self.trace_invalid_target(scope, module_name);
                    return continue_searching();
                }

                if self.tracer.is_some() {
                    let message_target = if is_pattern {
                        target_string.replace('*', subpath)
                    } else {
                        format!("{target_string}{subpath}")
                    };
                    let label = if is_imports { "imports" } else { "exports" };
                    write_trace(
                        &mut self.tracer,
                        &USING_0_SUBPATH_1_WITH_TARGET_2,
                        &[label, key, &message_target],
                    );
                }

                let cwd = self.resolver.host.get_current_directory();
                let final_path = if is_pattern {
                    tspath::get_normalized_absolute_path(
                        &resolved_target.replace('*', subpath),
                        cwd,
                    )
                } else {
                    tspath::get_normalized_absolute_path(
                        &format!("{resolved_target}{subpath}"),
                        cwd,
                    )
                };
                let package_path =
                    tspath::combine_paths(scope.package_directory(), &["package.json"]);
                let mut input_link = self.try_load_input_file_for_path(
                    &final_path,
                    subpath,
                    &package_path,
                    is_imports,
                );
                if !input_link.should_continue_searching() {
                    let path = input_link.as_ref().expect("some").path.clone();
                    let pkg_id = self.get_package_id(&path, Some(scope));
                    input_link.as_mut().expect("some").package_id = pkg_id;
                    return input_link;
                }
                let target_string_owned = target_string.to_string();
                let mut result = self.load_file_name_from_package_json_field(
                    extensions,
                    &final_path,
                    &target_string_owned,
                );
                if !result.should_continue_searching() {
                    let path = result.as_ref().expect("some").path.clone();
                    let pkg_id = self.get_package_id(&path, Some(scope));
                    result.as_mut().expect("some").package_id = pkg_id;
                    return result;
                }
                continue_searching()
            }
            JsonValueType::Object => {
                write_trace(&mut self.tracer, &ENTERING_CONDITIONAL_EXPORTS, &[]);
                let conditions: Vec<String> = target.as_object().keys().cloned().collect();
                for condition in &conditions {
                    if self.condition_matches(condition) {
                        if self.tracer.is_some() {
                            let label = if is_imports { "imports" } else { "exports" };
                            write_trace(
                                &mut self.tracer,
                                &MATCHED_0_CONDITION_1,
                                &[label, condition],
                            );
                        }
                        let sub_target = target.as_object().get(condition).expect("key").clone();
                        let result = self.load_module_from_target_export_or_import(
                            extensions,
                            module_name,
                            scope,
                            is_imports,
                            &sub_target,
                            subpath,
                            is_pattern,
                            key,
                        );
                        if !result.should_continue_searching() {
                            if result.is_resolved() && self.tracer.is_some() {
                                write_trace(
                                    &mut self.tracer,
                                    &RESOLVED_UNDER_CONDITION_0,
                                    &[condition],
                                );
                            }
                            write_trace(&mut self.tracer, &EXITING_CONDITIONAL_EXPORTS, &[]);
                            return result;
                        } else if self.tracer.is_some() {
                            write_trace(
                                &mut self.tracer,
                                &FAILED_TO_RESOLVE_UNDER_CONDITION_0,
                                &[condition],
                            );
                        }
                    } else if self.tracer.is_some() {
                        write_trace(
                            &mut self.tracer,
                            &SAW_NON_MATCHING_CONDITION_0,
                            &[condition],
                        );
                    }
                }
                write_trace(&mut self.tracer, &EXITING_CONDITIONAL_EXPORTS, &[]);
                continue_searching()
            }
            JsonValueType::Array => {
                if target.as_array().is_empty() {
                    self.trace_invalid_target(scope, module_name);
                    return continue_searching();
                }
                let elements: Vec<ExportsOrImports> = target.as_array().to_vec();
                for elem in &elements {
                    let result = self.load_module_from_target_export_or_import(
                        extensions,
                        module_name,
                        scope,
                        is_imports,
                        elem,
                        subpath,
                        is_pattern,
                        key,
                    );
                    if !result.should_continue_searching() {
                        return result;
                    }
                }
                // Falls through to the invalid-target trace below.
                self.trace_invalid_target(scope, module_name);
                continue_searching()
            }
            JsonValueType::Null => {
                if self.tracer.is_some() {
                    let dir = scope.package_directory().to_string();
                    write_trace(
                        &mut self.tracer,
                        &X_PACKAGE_JSON_SCOPE_0_EXPLICITLY_MAPS_SPECIFIER_1_TO_NULL,
                        &[&dir, module_name],
                    );
                }
                unresolved()
            }
            _ => {
                self.trace_invalid_target(scope, module_name);
                continue_searching()
            }
        }
    }

    fn trace_invalid_target(&mut self, scope: &PackageJsonInfo, module_name: &str) {
        if self.tracer.is_some() {
            let dir = scope.package_directory().to_string();
            write_trace(
                &mut self.tracer,
                &X_PACKAGE_JSON_SCOPE_0_HAS_INVALID_TYPE_FOR_TARGET_OF_SPECIFIER_1,
                &[&dir, module_name],
            );
        }
    }

    // Go: internal/module/resolver.go:resolutionState.tryLoadInputFileForPath
    pub(crate) fn try_load_input_file_for_path(
        &mut self,
        final_path: &str,
        entry: &str,
        package_path: &str,
        is_imports: bool,
    ) -> Resolved {
        let opts = ComparePathsOptions {
            use_case_sensitive_file_names: self.resolver.host.fs().use_case_sensitive_file_names(),
            current_directory: self.resolver.host.get_current_directory().to_string(),
        };
        let config_file_path = &self.compiler_options.config_file_path;
        let within_config = config_file_path.is_empty()
            || tspath::contains_path(
                &tspath::get_directory_path(package_path),
                config_file_path,
                &opts,
            );
        if !self.is_config_lookup
            && (!self.compiler_options.declaration_dir.is_empty()
                || !self.compiler_options.out_dir.is_empty())
            && !final_path.contains("/node_modules/")
            && within_config
        {
            let root_dir = if !self.compiler_options.root_dir.is_empty() {
                self.compiler_options.root_dir.clone()
            } else if !self.compiler_options.config_file_path.is_empty() {
                tspath::get_directory_path(&self.compiler_options.config_file_path)
            } else {
                let message = if is_imports {
                    &THE_PROJECT_ROOT_IS_AMBIGUOUS_BUT_IS_REQUIRED_TO_RESOLVE_IMPORT_MAP_ENTRY_0_IN_FILE_1_SUPPLY_THE_ROOTDIR_COMPILER_OPTION_TO_DISAMBIGUATE
                } else {
                    &THE_PROJECT_ROOT_IS_AMBIGUOUS_BUT_IS_REQUIRED_TO_RESOLVE_EXPORT_MAP_ENTRY_0_IN_FILE_1_SUPPLY_THE_ROOTDIR_COMPILER_OPTION_TO_DISAMBIGUATE
                };
                let entry_arg = if entry.is_empty() { "." } else { entry };
                self.diagnostics.push(ResolutionDiagnostic {
                    message,
                    args: vec![entry_arg.to_string(), package_path.to_string()],
                });
                return unresolved();
            };

            let candidate_directories = self.get_output_directories_for_base_directory(&root_dir);
            for candidate_dir in &candidate_directories {
                if tspath::contains_path(candidate_dir, final_path, &opts) {
                    // +1 to also remove the directory separator.
                    let path_fragment = &final_path[candidate_dir.len() + 1..];
                    let possible_input_base = tspath::combine_paths(&root_dir, &[path_fragment]);
                    let js_and_dts_extensions = [
                        EXTENSION_MJS,
                        EXTENSION_CJS,
                        EXTENSION_JS,
                        EXTENSION_JSON,
                        EXTENSION_DMTS,
                        EXTENSION_DCTS,
                        EXTENSION_DTS,
                    ];
                    for ext in js_and_dts_extensions {
                        if tspath::file_extension_is(&possible_input_base, ext) {
                            let input_exts =
                                tspath::get_possible_original_input_extension_for_extension(
                                    &possible_input_base,
                                );
                            for possible_ext in &input_exts {
                                if !extension_is_ok(self.extensions, possible_ext) {
                                    continue;
                                }
                                let possible_input_with_input_extension =
                                    tspath::change_extension(&possible_input_base, possible_ext);
                                if self
                                    .resolver
                                    .host
                                    .fs()
                                    .file_exists(&possible_input_with_input_extension)
                                {
                                    let ext_set = self.extensions;
                                    let resolved = self.load_file_name_from_package_json_field(
                                        ext_set,
                                        &possible_input_with_input_extension,
                                        "",
                                    );
                                    if !resolved.should_continue_searching() {
                                        return resolved;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        continue_searching()
    }

    // Go: internal/module/resolver.go:resolutionState.getOutputDirectoriesForBaseDirectory
    pub(crate) fn get_output_directories_for_base_directory(
        &self,
        common_source_dir_guess: &str,
    ) -> Vec<String> {
        let cwd = self.resolver.host.get_current_directory();
        let current_dir = if !self.compiler_options.config_file_path.is_empty() {
            cwd.to_string()
        } else {
            common_source_dir_guess.to_string()
        };
        let mut candidate_directories: Vec<String> = Vec::new();
        if !self.compiler_options.declaration_dir.is_empty() {
            candidate_directories.push(tspath::get_normalized_absolute_path(
                &tspath::combine_paths(&current_dir, &[&self.compiler_options.declaration_dir]),
                cwd,
            ));
        }
        if !self.compiler_options.out_dir.is_empty()
            && self.compiler_options.out_dir != self.compiler_options.declaration_dir
        {
            candidate_directories.push(tspath::get_normalized_absolute_path(
                &tspath::combine_paths(&current_dir, &[&self.compiler_options.out_dir]),
                cwd,
            ));
        }
        candidate_directories
    }
}

#[cfg(test)]
#[path = "node_resolution_test.rs"]
mod tests;
