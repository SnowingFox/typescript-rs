//! File and directory probing: relative-name loading, extension substitution,
//! `tryFile`, and `package.json` directory loading (`main`/`types`/index).
//!
//! Split from Go `internal/module/resolver.go` (PORTING.md §2).

use std::cmp::Ordering;

use tsgo_diagnostics::{
    DIRECTORY_0_DOES_NOT_EXIST_SKIPPING_ALL_LOOKUPS_IN_IT, FILE_0_DOES_NOT_EXIST,
    FILE_0_EXISTS_USE_IT_AS_A_NAME_RESOLUTION_RESULT, FILE_NAME_0_HAS_A_1_EXTENSION_STRIPPING_IT,
    LOADING_MODULE_AS_FILE_SLASH_FOLDER_CANDIDATE_MODULE_LOCATION_0_TARGET_FILE_TYPES_COLON_1,
    X_PACKAGE_JSON_HAS_A_TYPESVERSIONS_ENTRY_0_THAT_MATCHES_COMPILER_VERSION_1_LOOKING_FOR_A_PATTERN_TO_MATCH_MODULE_NAME_2,
};
use tsgo_packagejson::VersionPaths;
use tsgo_tspath::{
    self as tspath, ComparePathsOptions, EXTENSION_CJS, EXTENSION_CTS, EXTENSION_DCTS,
    EXTENSION_DMTS, EXTENSION_DTS, EXTENSION_JS, EXTENSION_JSON, EXTENSION_JSX, EXTENSION_MJS,
    EXTENSION_MTS, EXTENSION_TS, EXTENSION_TSX,
};

use crate::state::{PackageJsonInfo, ResolutionState};
use crate::util::parse_node_module_from_path;
use crate::{
    continue_searching, try_parse_patterns, write_trace, Extensions, Loader, ResolvedExt,
    ResolvedInner,
};

impl ResolutionState<'_> {
    // Go: internal/module/resolver.go:resolutionState.nodeLoadModuleByRelativeName
    pub(crate) fn node_load_module_by_relative_name(
        &mut self,
        extensions: Extensions,
        candidate: &str,
        consider_package_json: bool,
    ) -> crate::Resolved {
        if self.tracer.is_some() {
            let ext = extensions.to_string();
            write_trace(
                &mut self.tracer,
                &LOADING_MODULE_AS_FILE_SLASH_FOLDER_CANDIDATE_MODULE_LOCATION_0_TARGET_FILE_TYPES_COLON_1,
                &[candidate, &ext],
            );
        }
        if !tspath::has_trailing_directory_separator(candidate) {
            let parent_of_candidate = tspath::get_directory_path(candidate);
            if !self
                .resolver
                .host
                .fs()
                .directory_exists(&parent_of_candidate)
            {
                write_trace(
                    &mut self.tracer,
                    &DIRECTORY_0_DOES_NOT_EXIST_SKIPPING_ALL_LOOKUPS_IN_IT,
                    &[&parent_of_candidate],
                );
                return continue_searching();
            }
            let mut resolved_from_file = self.load_module_from_file(extensions, candidate);
            if let Some(inner) = resolved_from_file.as_mut() {
                if consider_package_json {
                    let path = inner.path.clone();
                    let package_directory = parse_node_module_from_path(&path, false);
                    if !package_directory.is_empty() {
                        let info = self.get_package_json_info(&package_directory);
                        // `inner` borrows the local `resolved_from_file`, disjoint from `self`.
                        inner.package_id = self.get_package_id(&path, info.as_ref());
                    }
                }
                return resolved_from_file;
            }
        }
        if !self.resolver.host.fs().directory_exists(candidate) {
            write_trace(
                &mut self.tracer,
                &DIRECTORY_0_DOES_NOT_EXIST_SKIPPING_ALL_LOOKUPS_IN_IT,
                &[candidate],
            );
            return continue_searching();
        }
        // ESM relative imports do not perform directory lookups.
        if !self.esm_mode {
            return self.load_node_module_from_directory(
                extensions,
                candidate,
                consider_package_json,
            );
        }
        continue_searching()
    }

    // Go: internal/module/resolver.go:resolutionState.loadModuleFromFile
    pub(crate) fn load_module_from_file(
        &mut self,
        extensions: Extensions,
        candidate: &str,
    ) -> crate::Resolved {
        // ./foo.js -> ./foo.ts
        let resolved_by_replacing_extension =
            self.load_module_from_file_no_implicit_extensions(extensions, candidate);
        if resolved_by_replacing_extension.is_some() {
            return resolved_by_replacing_extension;
        }
        // ./foo -> ./foo.ts
        if !self.esm_mode {
            return self.try_adding_extensions(candidate, extensions, "");
        }
        continue_searching()
    }

    // Go: internal/module/resolver.go:resolutionState.loadModuleFromFileNoImplicitExtensions
    pub(crate) fn load_module_from_file_no_implicit_extensions(
        &mut self,
        extensions: Extensions,
        candidate: &str,
    ) -> crate::Resolved {
        let base = tspath::get_base_file_name(candidate);
        if !base.contains('.') {
            // extensionless import; no lookups performed
            return continue_searching();
        }
        let removed = tspath::remove_file_extension(candidate);
        let extensionless: String = if removed == candidate {
            // Arbitrary extension: strip from the last dot.
            let last_dot = candidate.rfind('.').expect("base contains '.'");
            candidate[..last_dot].to_string()
        } else {
            removed.to_string()
        };
        let extension = &candidate[extensionless.len()..];
        if self.tracer.is_some() {
            let ext = extension.to_string();
            write_trace(
                &mut self.tracer,
                &FILE_NAME_0_HAS_A_1_EXTENSION_STRIPPING_IT,
                &[candidate, &ext],
            );
        }
        self.try_adding_extensions(&extensionless, extensions, extension)
    }

    // Go: internal/module/resolver.go:resolutionState.tryAddingExtensions
    pub(crate) fn try_adding_extensions(
        &mut self,
        extensionless: &str,
        extensions: Extensions,
        original_extension: &str,
    ) -> crate::Resolved {
        let directory = tspath::get_directory_path(extensionless);
        if !directory.is_empty() && !self.resolver.host.fs().directory_exists(&directory) {
            return continue_searching();
        }

        match original_extension {
            EXTENSION_MJS | EXTENSION_MTS | EXTENSION_DMTS => {
                let ts_extension =
                    original_extension == EXTENSION_MTS || original_extension == EXTENSION_DMTS;
                if extensions.contains(Extensions::TYPE_SCRIPT) {
                    let r = self.try_extension(EXTENSION_MTS, extensionless, ts_extension);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                if extensions.contains(Extensions::DECLARATION) {
                    let r = self.try_extension(EXTENSION_DMTS, extensionless, ts_extension);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                if extensions.contains(Extensions::JAVA_SCRIPT) {
                    let r = self.try_extension(EXTENSION_MJS, extensionless, false);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                continue_searching()
            }
            EXTENSION_CJS | EXTENSION_CTS | EXTENSION_DCTS => {
                let ts_extension =
                    original_extension == EXTENSION_CTS || original_extension == EXTENSION_DCTS;
                if extensions.contains(Extensions::TYPE_SCRIPT) {
                    let r = self.try_extension(EXTENSION_CTS, extensionless, ts_extension);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                if extensions.contains(Extensions::DECLARATION) {
                    let r = self.try_extension(EXTENSION_DCTS, extensionless, ts_extension);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                if extensions.contains(Extensions::JAVA_SCRIPT) {
                    let r = self.try_extension(EXTENSION_CJS, extensionless, false);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                continue_searching()
            }
            EXTENSION_JSON => {
                if extensions.contains(Extensions::DECLARATION) {
                    let r = self.try_extension(".d.json.ts", extensionless, false);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                if extensions.contains(Extensions::JSON) {
                    let r = self.try_extension(EXTENSION_JSON, extensionless, false);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                continue_searching()
            }
            EXTENSION_TSX | EXTENSION_JSX => {
                let is_tsx = original_extension == EXTENSION_TSX;
                if extensions.contains(Extensions::TYPE_SCRIPT) {
                    let r = self.try_extension(EXTENSION_TSX, extensionless, is_tsx);
                    if !r.should_continue_searching() {
                        return r;
                    }
                    let r = self.try_extension(EXTENSION_TS, extensionless, is_tsx);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                if extensions.contains(Extensions::DECLARATION) {
                    let r = self.try_extension(EXTENSION_DTS, extensionless, is_tsx);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                if extensions.contains(Extensions::JAVA_SCRIPT) {
                    let r = self.try_extension(EXTENSION_JSX, extensionless, false);
                    if !r.should_continue_searching() {
                        return r;
                    }
                    let r = self.try_extension(EXTENSION_JS, extensionless, false);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                continue_searching()
            }
            EXTENSION_TS | EXTENSION_DTS | EXTENSION_JS | "" => {
                let ts_extension =
                    original_extension == EXTENSION_TS || original_extension == EXTENSION_DTS;
                if extensions.contains(Extensions::TYPE_SCRIPT) {
                    let r = self.try_extension(EXTENSION_TS, extensionless, ts_extension);
                    if !r.should_continue_searching() {
                        return r;
                    }
                    let r = self.try_extension(EXTENSION_TSX, extensionless, ts_extension);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                if extensions.contains(Extensions::DECLARATION) {
                    let r = self.try_extension(EXTENSION_DTS, extensionless, ts_extension);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                if extensions.contains(Extensions::JAVA_SCRIPT) {
                    let r = self.try_extension(EXTENSION_JS, extensionless, false);
                    if !r.should_continue_searching() {
                        return r;
                    }
                    let r = self.try_extension(EXTENSION_JSX, extensionless, false);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                if self.is_config_lookup {
                    let r = self.try_extension(EXTENSION_JSON, extensionless, false);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                continue_searching()
            }
            _ => {
                let full = format!("{extensionless}{original_extension}");
                if extensions.contains(Extensions::DECLARATION)
                    && !tspath::is_declaration_file_name(&full)
                {
                    let synthetic = format!(".d{original_extension}.ts");
                    let r = self.try_extension(&synthetic, extensionless, false);
                    if !r.should_continue_searching() {
                        return r;
                    }
                }
                continue_searching()
            }
        }
    }

    // Go: internal/module/resolver.go:resolutionState.tryExtension
    pub(crate) fn try_extension(
        &mut self,
        extension: &str,
        extensionless: &str,
        resolved_using_ts_extension: bool,
    ) -> crate::Resolved {
        let file_name = format!("{extensionless}{extension}");
        let (path, ok) = self.try_file(&file_name);
        if ok {
            return Some(ResolvedInner {
                path,
                extension: extension.to_string(),
                resolved_using_ts_extension: !self.candidate_ending_is_from_config
                    && resolved_using_ts_extension,
                ..Default::default()
            });
        }
        continue_searching()
    }

    // Go: internal/module/resolver.go:resolutionState.tryFile
    pub(crate) fn try_file(&mut self, file_name: &str) -> (String, bool) {
        if self.compiler_options.module_suffixes.is_empty() {
            let found = self.try_file_lookup(file_name);
            return (file_name.to_string(), found);
        }
        let ext = tspath::try_get_extension_from_path(file_name);
        let file_name_no_extension = tspath::remove_extension(file_name, ext).to_string();
        let suffixes = self.compiler_options.module_suffixes.clone();
        for suffix in &suffixes {
            let path = format!("{file_name_no_extension}{suffix}{ext}");
            if self.try_file_lookup(&path) {
                return (path, true);
            }
        }
        (file_name.to_string(), false)
    }

    // Go: internal/module/resolver.go:resolutionState.tryFileLookup
    pub(crate) fn try_file_lookup(&mut self, file_name: &str) -> bool {
        if self.resolver.host.fs().file_exists(file_name) {
            write_trace(
                &mut self.tracer,
                &FILE_0_EXISTS_USE_IT_AS_A_NAME_RESOLUTION_RESULT,
                &[file_name],
            );
            true
        } else {
            write_trace(&mut self.tracer, &FILE_0_DOES_NOT_EXIST, &[file_name]);
            false
        }
    }

    // Go: internal/module/resolver.go:resolutionState.loadNodeModuleFromDirectory
    pub(crate) fn load_node_module_from_directory(
        &mut self,
        extensions: Extensions,
        candidate: &str,
        consider_package_json: bool,
    ) -> crate::Resolved {
        let package_info = if consider_package_json {
            self.get_package_json_info(candidate)
        } else {
            None
        };
        self.load_node_module_from_directory_worker(extensions, candidate, package_info.as_ref())
    }

    // Go: internal/module/resolver.go:resolutionState.loadNodeModuleFromDirectoryWorker
    pub(crate) fn load_node_module_from_directory_worker(
        &mut self,
        ext: Extensions,
        candidate: &str,
        package_info: Option<&PackageJsonInfo>,
    ) -> crate::Resolved {
        let mut package_file = String::new();
        let mut version_paths = VersionPaths::default();
        if package_info.is_some_and(|p| p.exists()) {
            let info = package_info.expect("checked some");
            version_paths = self.version_paths_of(info.contents());
            let opts = ComparePathsOptions {
                use_case_sensitive_file_names: self
                    .resolver
                    .host
                    .fs()
                    .use_case_sensitive_file_names(),
                current_directory: String::new(),
            };
            if tspath::compare_paths(candidate, info.package_directory(), &opts) == Ordering::Equal
            {
                if let Some(file) = self.get_package_file(ext, info) {
                    package_file = file;
                }
            }
        }

        let index_path = if self.is_config_lookup {
            tspath::combine_paths(candidate, &["tsconfig"])
        } else {
            tspath::combine_paths(candidate, &["index"])
        };

        if version_paths.exists()
            && (package_file.is_empty()
                || tspath::contains_path(candidate, &package_file, &ComparePathsOptions::default()))
        {
            let module_name = if !package_file.is_empty() {
                tspath::get_relative_path_from_directory(
                    candidate,
                    &package_file,
                    &ComparePathsOptions::default(),
                )
            } else {
                tspath::get_relative_path_from_directory(
                    candidate,
                    &index_path,
                    &ComparePathsOptions::default(),
                )
            };
            if self.tracer.is_some() {
                let version = version_paths.version().to_string();
                write_trace(
                    &mut self.tracer,
                    &X_PACKAGE_JSON_HAS_A_TYPESVERSIONS_ENTRY_0_THAT_MATCHES_COMPILER_VERSION_1_LOOKING_FOR_A_PATTERN_TO_MATCH_MODULE_NAME_2,
                    &[&version, tsgo_core::version::version(), &module_name],
                );
            }
            let path_patterns = try_parse_patterns(version_paths.get_paths());
            let loader = Loader::DirectoryWorker {
                package_info,
                package_file: &package_file,
            };
            let result = self.try_load_module_using_paths(
                ext,
                &module_name,
                candidate,
                version_paths.get_paths(),
                &path_patterns,
                &loader,
            );
            if !result.should_continue_searching() {
                if !result.as_ref().expect("some").package_id.name.is_empty() {
                    panic!("expected packageId to be empty");
                }
                return result;
            }
        }

        if !package_file.is_empty() {
            let result =
                self.directory_worker_loader(ext, &package_file, package_info, &package_file);
            if !result.should_continue_searching() {
                if !result.as_ref().expect("some").package_id.name.is_empty() {
                    panic!("expected packageId to be empty");
                }
                return result;
            }
        }

        // ESM mode resolutions don't do package 'index' lookups.
        if !self.esm_mode {
            if !self.resolver.host.fs().directory_exists(candidate) {
                return continue_searching();
            }
            return self.load_module_from_file(ext, &index_path);
        }
        continue_searching()
    }

    // Go: internal/module/resolver.go:loadNodeModuleFromDirectoryWorker's inner loader
    pub(crate) fn directory_worker_loader(
        &mut self,
        extensions: Extensions,
        candidate: &str,
        package_info: Option<&PackageJsonInfo>,
        package_file: &str,
    ) -> crate::Resolved {
        let from_file =
            self.load_file_name_from_package_json_field(extensions, candidate, package_file);
        if !from_file.should_continue_searching() {
            return from_file;
        }
        // package.json "types" can still resolve a .ts file even when only
        // declarations were requested.
        let expanded_extensions = if extensions == Extensions::DECLARATION {
            Extensions::TYPE_SCRIPT | Extensions::DECLARATION
        } else {
            extensions
        };
        let save_esm_mode = self.esm_mode;
        let save_candidate = self.candidate_ending_is_from_config;
        self.candidate_ending_is_from_config = true;
        if package_info.is_some_and(|p| {
            p.exists() && p.contents().fields().header.type_.get_value().0.as_str() != "module"
        }) {
            self.esm_mode = false;
        }
        let result = self.node_load_module_by_relative_name(expanded_extensions, candidate, false);
        self.esm_mode = save_esm_mode;
        self.candidate_ending_is_from_config = save_candidate;
        result
    }

    // Go: internal/module/resolver.go:resolutionState.loadFileNameFromPackageJSONField
    pub(crate) fn load_file_name_from_package_json_field(
        &mut self,
        extensions: Extensions,
        candidate: &str,
        package_json_value: &str,
    ) -> crate::Resolved {
        if (extensions.contains(Extensions::TYPE_SCRIPT)
            && tspath::has_implementation_ts_file_extension(candidate))
            || (extensions.contains(Extensions::DECLARATION)
                && tspath::is_declaration_file_name(candidate))
        {
            let (path, ok) = self.try_file(candidate);
            if ok {
                let extension = tspath::try_extract_ts_extension(&path);
                let resolved_using_ts_extension =
                    package_json_value.ends_with('*') && !extension.is_empty();
                return Some(ResolvedInner {
                    path,
                    extension: extension.to_string(),
                    resolved_using_ts_extension,
                    ..Default::default()
                });
            }
            return continue_searching();
        }

        if self.is_config_lookup
            && extensions.contains(Extensions::JSON)
            && tspath::file_extension_is(candidate, EXTENSION_JSON)
        {
            let (path, ok) = self.try_file(candidate);
            if ok {
                return Some(ResolvedInner {
                    path,
                    extension: EXTENSION_JSON.to_string(),
                    ..Default::default()
                });
            }
        }

        self.load_module_from_file_no_implicit_extensions(extensions, candidate)
    }

    // Go: internal/module/resolver.go:resolutionState.getPackageFile
    pub(crate) fn get_package_file(
        &mut self,
        extensions: Extensions,
        package_info: &PackageJsonInfo,
    ) -> Option<String> {
        if !package_info.exists() {
            return None;
        }
        let dir = package_info.package_directory().to_string();
        let fields = package_info.contents().fields();
        if self.is_config_lookup {
            return self.get_package_json_path_field("tsconfig", &fields.path.tsconfig, &dir);
        }
        if extensions.contains(Extensions::DECLARATION) {
            if let Some(file) =
                self.get_package_json_path_field("typings", &fields.path.typings, &dir)
            {
                return Some(file);
            }
            if let Some(file) = self.get_package_json_path_field("types", &fields.path.types, &dir)
            {
                return Some(file);
            }
        }
        if extensions.intersects(Extensions::IMPLEMENTATION_FILES | Extensions::DECLARATION) {
            return self.get_package_json_path_field("main", &fields.path.main, &dir);
        }
        None
    }
}

#[cfg(test)]
#[path = "file_load_test.rs"]
mod tests;
