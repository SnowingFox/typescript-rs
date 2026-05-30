//! The per-request `ResolutionState`, condition/feature derivation, the
//! node-like resolution driver, and result construction (including symlink
//! realpath handling).
//!
//! Split from Go `internal/module/resolver.go` (PORTING.md §2).

use std::cell::OnceCell;
use std::cmp::Ordering;
use std::sync::Arc;

use tsgo_core::compileroptions::{
    CompilerOptions, ModuleKind, ModuleResolutionKind, ResolutionMode,
};
use tsgo_core::tristate::Tristate;
use tsgo_diagnostics::{
    Message, LOADING_MODULE_0_FROM_NODE_MODULES_FOLDER_TARGET_FILE_TYPES_COLON_1,
    RESOLUTION_OF_NON_RELATIVE_NAME_FAILED_TRYING_WITH_MODERN_NODE_RESOLUTION_FEATURES_DISABLED_TO_SEE_IF_NPM_LIBRARY_NEEDS_CONFIGURATION_UPDATE,
    RESOLVING_IN_0_MODE_WITH_CONDITIONS_1, RESOLVING_REAL_PATH_FOR_0_RESULT_1,
    SKIPPING_MODULE_0_THAT_LOOKS_LIKE_AN_ABSOLUTE_URI_TARGET_FILE_TYPES_COLON_1,
};
use tsgo_packagejson::{InfoCacheEntry, PackageJson, VersionPaths};
use tsgo_tspath::{self as tspath, ComparePathsOptions};

use crate::types::{NodeResolutionFeatures, ResolutionDiagnostic, ResolvedTypeReferenceDirective};
use crate::util::is_applicable_versioned_types_key;
use crate::{
    extension_is_ok, normalize_path_for_cjs_resolution, write_trace, Extensions, ParsedPatterns,
    Resolved, ResolvedExt, ResolvedModule, Resolver, Tracer,
};

/// Computes the resolution conditions (`import`/`require`, `types`, `node`,
/// plus custom conditions) for `options` and the import's `resolution_mode`.
///
/// # Examples
/// ```
/// use tsgo_core::compileroptions::{CompilerOptions, ModuleKind, ModuleResolutionKind};
/// use tsgo_module::get_conditions;
/// let opts = CompilerOptions { module_resolution: ModuleResolutionKind::Bundler, ..Default::default() };
/// assert_eq!(get_conditions(&opts, ModuleKind::EsNext), vec!["import", "types"]);
/// ```
///
/// Side effects: none (pure).
// Go: internal/module/resolver.go:GetConditions
pub fn get_conditions(options: &CompilerOptions, resolution_mode: ResolutionMode) -> Vec<String> {
    let module_resolution = options.get_module_resolution_kind();
    let mut resolution_mode = resolution_mode;
    if resolution_mode == ModuleKind::None && module_resolution == ModuleResolutionKind::Bundler {
        resolution_mode = ModuleKind::EsNext;
    }
    let mut conditions: Vec<String> = Vec::with_capacity(3 + options.custom_conditions.len());
    if resolution_mode == ModuleKind::EsNext {
        conditions.push("import".to_string());
    } else {
        conditions.push("require".to_string());
    }
    if options.no_dts_resolution != Tristate::True {
        conditions.push("types".to_string());
    }
    if module_resolution != ModuleResolutionKind::Bundler {
        conditions.push("node".to_string());
    }
    conditions.extend(options.custom_conditions.iter().cloned());
    conditions
}

/// Derives the [`NodeResolutionFeatures`] for `options`, applying the
/// `resolvePackageJsonExports`/`Imports` overrides.
///
/// Side effects: none (pure).
// Go: internal/module/resolver.go:getNodeResolutionFeatures
pub(crate) fn get_node_resolution_features(options: &CompilerOptions) -> NodeResolutionFeatures {
    let mut features = NodeResolutionFeatures::NONE;
    match options.get_module_resolution_kind() {
        ModuleResolutionKind::Node16 => features = NodeResolutionFeatures::NODE16_DEFAULT,
        ModuleResolutionKind::NodeNext => features = NodeResolutionFeatures::NODENEXT_DEFAULT,
        ModuleResolutionKind::Bundler => features = NodeResolutionFeatures::BUNDLER_DEFAULT,
        _ => {}
    }
    if options.resolve_package_json_exports == Tristate::True {
        features |= NodeResolutionFeatures::EXPORTS;
    } else if options.resolve_package_json_exports == Tristate::False {
        features &= !NodeResolutionFeatures::EXPORTS;
    }
    if options.resolve_package_json_imports == Tristate::True {
        features |= NodeResolutionFeatures::IMPORTS;
    } else if options.resolve_package_json_imports == Tristate::False {
        features &= !NodeResolutionFeatures::IMPORTS;
    }
    features
}

/// A `package.json` info handle: the cached entry plus the directory it was
/// requested for.
///
/// # Divergence from Go
/// Go returns a bare `*packagejson.InfoCacheEntry`, sometimes synthesizing a
/// new entry that shares another's `Contents` (TS PR #50740). Rust's
/// `InfoCacheEntry` owns its `Contents`, so this wrapper carries the shared
/// cached entry plus the (possibly different) `package_directory` separately.
///
/// Side effects: none (plain data).
// Go: internal/module/resolver.go (returns of getPackageJsonInfo/getPackageScopeForPath)
#[derive(Clone)]
pub struct PackageJsonInfo {
    pub(crate) entry: Arc<InfoCacheEntry>,
    pub(crate) package_directory: String,
}

impl PackageJsonInfo {
    pub(crate) fn new(entry: Arc<InfoCacheEntry>, package_directory: String) -> Self {
        PackageJsonInfo {
            entry,
            package_directory,
        }
    }

    /// Reports whether parsed `package.json` contents are present.
    ///
    /// Side effects: none (pure).
    pub fn exists(&self) -> bool {
        self.entry.contents.is_some()
    }

    /// The directory the info was requested for.
    ///
    /// Side effects: none (pure).
    pub fn package_directory(&self) -> &str {
        &self.package_directory
    }

    pub(crate) fn contents(&self) -> &PackageJson {
        self.entry
            .contents
            .as_ref()
            .expect("PackageJsonInfo::contents called when exists() is false")
    }
}

/// Per-request resolution state borrowing the [`Resolver`]. Holds the request
/// parameters (name, directory, features, conditions, extensions) and the
/// mutable accumulators (diagnostics, the `resolved a package dir` flag).
// Go: internal/module/resolver.go:resolutionState
pub(crate) struct ResolutionState<'r> {
    pub(crate) resolver: &'r Resolver,
    pub(crate) tracer: Option<Tracer>,

    // request fields
    pub(crate) name: String,
    pub(crate) containing_directory: String,
    pub(crate) is_config_lookup: bool,
    pub(crate) features: NodeResolutionFeatures,
    pub(crate) esm_mode: bool,
    pub(crate) conditions: Vec<String>,
    pub(crate) extensions: Extensions,
    pub(crate) compiler_options: Arc<CompilerOptions>,
    pub(crate) resolve_package_directory_only: bool,

    // state fields
    pub(crate) candidate_ending_is_from_config: bool,
    pub(crate) resolved_package_directory: bool,
    pub(crate) diagnostics: Vec<ResolutionDiagnostic>,

    pub(crate) parsed_patterns_for_paths: OnceCell<ParsedPatterns>,
}

impl<'r> ResolutionState<'r> {
    /// Builds a resolution state. `compiler_options` is the already
    /// redirect-resolved options (see `get_compiler_options_with_redirect`).
    // Go: internal/module/resolver.go:newResolutionState
    pub(crate) fn new(
        name: &str,
        containing_directory: &str,
        is_type_reference_directive: bool,
        resolution_mode: ResolutionMode,
        compiler_options: Arc<CompilerOptions>,
        resolver: &'r Resolver,
        trace_builder: Option<Tracer>,
    ) -> Self {
        let mut state = ResolutionState {
            resolver,
            tracer: trace_builder,
            name: name.to_string(),
            containing_directory: containing_directory.to_string(),
            is_config_lookup: false,
            features: NodeResolutionFeatures::NONE,
            esm_mode: false,
            conditions: Vec::new(),
            extensions: Extensions::empty(),
            compiler_options,
            resolve_package_directory_only: false,
            candidate_ending_is_from_config: false,
            resolved_package_directory: false,
            diagnostics: Vec::new(),
            parsed_patterns_for_paths: OnceCell::new(),
        };

        if is_type_reference_directive {
            state.extensions = Extensions::DECLARATION;
        } else if state.compiler_options.no_dts_resolution == Tristate::True {
            state.extensions = Extensions::IMPLEMENTATION_FILES;
        } else {
            state.extensions =
                Extensions::TYPE_SCRIPT | Extensions::JAVA_SCRIPT | Extensions::DECLARATION;
        }

        if !is_type_reference_directive && state.compiler_options.get_resolve_json_module() {
            state.extensions |= Extensions::JSON;
        }

        match state.compiler_options.get_module_resolution_kind() {
            ModuleResolutionKind::Node16 => {
                state.features = NodeResolutionFeatures::NODE16_DEFAULT;
                state.esm_mode = resolution_mode == ModuleKind::EsNext;
                state.conditions = get_conditions(&state.compiler_options, resolution_mode);
            }
            ModuleResolutionKind::NodeNext => {
                state.features = NodeResolutionFeatures::NODENEXT_DEFAULT;
                state.esm_mode = resolution_mode == ModuleKind::EsNext;
                state.conditions = get_conditions(&state.compiler_options, resolution_mode);
            }
            ModuleResolutionKind::Bundler => {
                state.features = get_node_resolution_features(&state.compiler_options);
                state.conditions = get_conditions(&state.compiler_options, resolution_mode);
            }
            _ => {}
        }
        state
    }

    /// A minimal state used only for `get_package_scope_for_path`.
    // Go: internal/module/resolver.go:Resolver.GetPackageScopeForPath (inline state)
    pub(crate) fn for_scope_lookup(resolver: &'r Resolver) -> Self {
        ResolutionState {
            resolver,
            tracer: None,
            name: String::new(),
            containing_directory: String::new(),
            is_config_lookup: false,
            features: NodeResolutionFeatures::NONE,
            esm_mode: false,
            conditions: Vec::new(),
            extensions: Extensions::empty(),
            compiler_options: resolver.compiler_options.clone(),
            resolve_package_directory_only: false,
            candidate_ending_is_from_config: false,
            resolved_package_directory: false,
            diagnostics: Vec::new(),
            parsed_patterns_for_paths: OnceCell::new(),
        }
    }

    /// A minimal state used by `get_entrypoints_from_package_json_info`
    /// (declaration + TypeScript extensions, all features).
    // Go: internal/module/resolver.go:Resolver.GetEntrypointsFromPackageJsonInfo (inline state)
    pub(crate) fn for_entrypoints(resolver: &'r Resolver) -> Self {
        let mut state = Self::for_scope_lookup(resolver);
        state.extensions = Extensions::TYPE_SCRIPT | Extensions::DECLARATION;
        state.features = NodeResolutionFeatures::ALL;
        state
    }

    /// Consumes the state, returning its tracer back to the caller.
    pub(crate) fn into_tracer(self) -> Option<Tracer> {
        self.tracer
    }

    /// Resolves the version-paths entry for `contents`, forwarding traces.
    // Go: internal/module/resolver.go:resolutionState.getTraceFunc (used via GetVersionPaths)
    pub(crate) fn version_paths_of(&mut self, contents: &PackageJson) -> VersionPaths {
        if let Some(tracer) = self.tracer.as_mut() {
            let mut cb = |m: &'static Message, args: &[&str]| tracer.write(m, args);
            contents.get_version_paths(Some(&mut cb)).clone()
        } else {
            contents.get_version_paths(None).clone()
        }
    }

    /// Reports whether `condition` is active in this resolution, including
    /// versioned `types@<range>` conditions when the `types` condition applies.
    // Go: internal/module/resolver.go:resolutionState.conditionMatches
    pub(crate) fn condition_matches(&self, condition: &str) -> bool {
        if condition == "default" || self.conditions.iter().any(|c| c == condition) {
            return true;
        }
        if !self.conditions.iter().any(|c| c == "types") {
            return false;
        }
        is_applicable_versioned_types_key(condition)
    }

    /// Returns the nearest containing `package.json` scope for `directory`.
    // Go: internal/module/resolver.go:resolutionState.getPackageScopeForPath
    pub(crate) fn get_package_scope_for_path(
        &mut self,
        directory: &str,
    ) -> Option<PackageJsonInfo> {
        let typings_location = self.resolver.typings_location.clone();
        tspath::for_each_ancestor_directory_stopping_at_global_cache::<Option<PackageJsonInfo>>(
            &typings_location,
            directory,
            |dir| match self.get_package_json_info(dir) {
                Some(result) => (Some(result), true),
                None => (None, false),
            },
        )
    }

    /// The node-like resolution entry point, with the modern-features-disabled
    /// retry that records an `alternate_result`.
    // Go: internal/module/resolver.go:resolutionState.resolveNodeLike
    pub(crate) fn resolve_node_like(&mut self) -> ResolvedModule {
        if self.tracer.is_some() {
            let conditions = self
                .conditions
                .iter()
                .map(|c| format!("'{c}'"))
                .collect::<Vec<_>>()
                .join(", ");
            let mode = if self.esm_mode { "ESM" } else { "CJS" };
            write_trace(
                &mut self.tracer,
                &RESOLVING_IN_0_MODE_WITH_CONDITIONS_1,
                &[mode, &conditions],
            );
        }

        let mut result = self.resolve_node_like_worker();
        if self.resolved_package_directory
            && !self.is_config_lookup
            && self.features.contains(NodeResolutionFeatures::EXPORTS)
            && self
                .extensions
                .intersects(Extensions::TYPE_SCRIPT | Extensions::DECLARATION)
            && !tspath::is_external_module_name_relative(&self.name)
            && result.is_resolved()
            && result.is_external_library_import
            && !extension_is_ok(
                Extensions::TYPE_SCRIPT | Extensions::DECLARATION,
                &result.extension,
            )
            && self.conditions.iter().any(|c| c == "import")
        {
            write_trace(
                &mut self.tracer,
                &RESOLUTION_OF_NON_RELATIVE_NAME_FAILED_TRYING_WITH_MODERN_NODE_RESOLUTION_FEATURES_DISABLED_TO_SEE_IF_NPM_LIBRARY_NEEDS_CONFIGURATION_UPDATE,
                &[],
            );
            self.features &= !NodeResolutionFeatures::EXPORTS;
            self.extensions &= Extensions::TYPE_SCRIPT | Extensions::DECLARATION;
            let diagnostics_count = self.diagnostics.len();
            let diagnostic_result = self.resolve_node_like_worker();
            if diagnostic_result.is_resolved() && diagnostic_result.is_external_library_import {
                result.alternate_result = diagnostic_result.resolved_file_name;
            }
            self.diagnostics.truncate(diagnostics_count);
        }
        result
    }

    // Go: internal/module/resolver.go:resolutionState.resolveNodeLikeWorker
    pub(crate) fn resolve_node_like_worker(&mut self) -> ResolvedModule {
        let resolved = self.try_load_module_using_optional_resolution_settings();
        if !resolved.should_continue_searching() {
            return self.create_resolved_module_handling_symlink(resolved);
        }

        if !tspath::is_external_module_name_relative(&self.name) {
            if self.features.contains(NodeResolutionFeatures::IMPORTS) && self.name.starts_with('#')
            {
                let resolved = self.load_module_from_imports();
                if !resolved.should_continue_searching() {
                    return self.create_resolved_module_handling_symlink(resolved);
                }
            }
            if self.features.contains(NodeResolutionFeatures::SELF_NAME) {
                let resolved = self.load_module_from_self_name_reference();
                if !resolved.should_continue_searching() {
                    return self.create_resolved_module_handling_symlink(resolved);
                }
            }
            if self.name.contains(':') {
                if self.tracer.is_some() {
                    let ext = self.extensions.to_string();
                    write_trace(
                        &mut self.tracer,
                        &SKIPPING_MODULE_0_THAT_LOOKS_LIKE_AN_ABSOLUTE_URI_TARGET_FILE_TYPES_COLON_1,
                        &[&self.name, &ext],
                    );
                }
                return self.create_resolved_module(None, false);
            }
            if self.tracer.is_some() {
                let ext = self.extensions.to_string();
                write_trace(
                    &mut self.tracer,
                    &LOADING_MODULE_0_FROM_NODE_MODULES_FOLDER_TARGET_FILE_TYPES_COLON_1,
                    &[&self.name, &ext],
                );
            }
            let resolved = self.load_module_from_nearest_node_modules_directory(false);
            if !resolved.should_continue_searching() {
                return self.create_resolved_module_handling_symlink(resolved);
            }
            if self.extensions.contains(Extensions::DECLARATION) {
                let resolved = self.resolve_from_type_root();
                if !resolved.should_continue_searching() {
                    return self.create_resolved_module_handling_symlink(resolved);
                }
            }
        } else {
            let candidate =
                normalize_path_for_cjs_resolution(&self.containing_directory, &self.name);
            let resolved =
                self.node_load_module_by_relative_name(self.extensions, &candidate, true);
            let is_node_modules = resolved
                .as_ref()
                .is_some_and(|r| r.path.contains("/node_modules/"));
            return self.create_resolved_module(resolved, is_node_modules);
        }
        self.create_resolved_module(None, false)
    }

    // Go: internal/module/resolver.go:resolutionState.createResolvedModuleHandlingSymlink
    pub(crate) fn create_resolved_module_handling_symlink(
        &mut self,
        mut resolved: Resolved,
    ) -> ResolvedModule {
        let is_external_library_import = resolved
            .as_ref()
            .is_some_and(|r| r.path.contains("/node_modules/"));
        if self.compiler_options.preserve_symlinks != Tristate::True
            && is_external_library_import
            && resolved
                .as_ref()
                .map(|r| r.original_path.is_empty())
                .unwrap_or(false)
            && !tspath::is_external_module_name_relative(&self.name)
        {
            let path = resolved.as_ref().expect("checked above").path.clone();
            let (original_path, resolved_file_name) =
                self.get_original_and_resolved_file_name(&path);
            if !original_path.is_empty() {
                let inner = resolved.as_mut().expect("checked above");
                inner.path = resolved_file_name;
                inner.original_path = original_path;
            }
        }
        self.create_resolved_module(resolved, is_external_library_import)
    }

    // Go: internal/module/resolver.go:resolutionState.createResolvedModule
    pub(crate) fn create_resolved_module(
        &mut self,
        resolved: Resolved,
        is_external_library_import: bool,
    ) -> ResolvedModule {
        let mut resolved_module = ResolvedModule {
            resolution_diagnostics: std::mem::take(&mut self.diagnostics),
            ..Default::default()
        };
        if let Some(r) = resolved {
            resolved_module.resolved_file_name = r.path;
            resolved_module.original_path = r.original_path;
            resolved_module.is_external_library_import = is_external_library_import;
            resolved_module.resolved_using_ts_extension = r.resolved_using_ts_extension;
            resolved_module.extension = r.extension;
            resolved_module.package_id = r.package_id;
        }
        resolved_module
    }

    // Go: internal/module/resolver.go:resolutionState.createResolvedTypeReferenceDirective
    pub(crate) fn create_resolved_type_reference_directive(
        &mut self,
        resolved: Resolved,
        primary: bool,
    ) -> ResolvedTypeReferenceDirective {
        let mut result = ResolvedTypeReferenceDirective {
            resolution_diagnostics: std::mem::take(&mut self.diagnostics),
            ..Default::default()
        };

        if resolved.is_resolved() {
            let inner = resolved.as_ref().expect("is_resolved implies Some");
            if !tspath::extension_is_ts(&inner.extension) {
                panic!("expected a TypeScript file extension");
            }
            result.resolved_file_name = inner.path.clone();
            result.primary = primary;
            result.package_id = inner.package_id.clone();
            result.is_external_library_import = inner.path.contains("/node_modules/");

            if self.compiler_options.preserve_symlinks != Tristate::True {
                let path = inner.path.clone();
                let (original_path, resolved_file_name) =
                    self.get_original_and_resolved_file_name(&path);
                if !original_path.is_empty() {
                    result.resolved_file_name = resolved_file_name;
                    result.original_path = original_path;
                }
            }
        }
        result
    }

    // Go: internal/module/resolver.go:resolutionState.getOriginalAndResolvedFileName
    pub(crate) fn get_original_and_resolved_file_name(
        &mut self,
        file_name: &str,
    ) -> (String, String) {
        let resolved_file_name = self.real_path(file_name);
        let options = ComparePathsOptions {
            use_case_sensitive_file_names: self.resolver.host.fs().use_case_sensitive_file_names(),
            current_directory: self.resolver.host.get_current_directory().to_string(),
        };
        if tspath::compare_paths(file_name, &resolved_file_name, &options) == Ordering::Equal {
            // If the fileName and realpath differ only in casing, prefer
            // fileName so casing diagnostics remain correct.
            return (String::new(), file_name.to_string());
        }
        (file_name.to_string(), resolved_file_name)
    }

    // Go: internal/module/resolver.go:resolutionState.realPath
    pub(crate) fn real_path(&mut self, path: &str) -> String {
        let rp = tspath::normalize_path(&self.resolver.host.fs().realpath(path));
        write_trace(
            &mut self.tracer,
            &RESOLVING_REAL_PATH_FOR_0_RESULT_1,
            &[path, &rp],
        );
        rp
    }
}

#[cfg(test)]
#[path = "state_test.rs"]
mod tests;
