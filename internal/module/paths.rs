//! `paths` mapping, `rootDirs` resolution, and the loader dispatch shared by
//! both (`tryLoadModuleUsingPaths`).
//!
//! Split from Go `internal/module/resolver.go` (PORTING.md §2).
//!
//! # Divergence from Go
//! `getParsedPatternsForPaths` returns an owned [`ParsedPatterns`] clone (Go
//! returns a `*ParsedPatterns`); cloning avoids holding a `&self` borrow across
//! the subsequent `&mut self` loader call. `paths`/`rootDirs` are rarely set,
//! so this is negligible and full coverage is deferred to P10.

use tsgo_collections::OrderedMap;
use tsgo_diagnostics::{
    CHECKING_IF_0_IS_THE_LONGEST_MATCHING_PREFIX_FOR_1_2,
    LOADING_0_FROM_THE_ROOT_DIR_1_CANDIDATE_LOCATION_2, LONGEST_MATCHING_PREFIX_FOR_0_IS_1,
    MODULE_NAME_0_MATCHED_PATTERN_1, MODULE_RESOLUTION_USING_ROOTDIRS_HAS_FAILED,
    TRYING_OTHER_ENTRIES_IN_ROOTDIRS, TRYING_SUBSTITUTION_0_CANDIDATE_MODULE_LOCATION_COLON_1,
    X_PATHS_OPTION_IS_SPECIFIED_LOOKING_FOR_A_PATTERN_TO_MATCH_MODULE_NAME_0,
    X_ROOTDIRS_OPTION_IS_SET_USING_IT_TO_RESOLVE_RELATIVE_MODULE_NAME_0,
};
use tsgo_tspath as tspath;

use crate::state::ResolutionState;
use crate::{
    continue_searching, match_pattern_or_exact, try_parse_patterns, write_trace, Extensions,
    Loader, ParsedPatterns, Resolved, ResolvedExt, ResolvedInner, Resolver,
};

impl Resolver {
    // Go: internal/module/resolver.go:Resolver.getParsedPatternsForPaths
    pub(crate) fn get_parsed_patterns_for_paths(&self) -> &ParsedPatterns {
        self.caches
            .parsed_patterns_for_paths
            .get_or_init(|| try_parse_patterns(self.compiler_options.paths.as_ref()))
    }
}

impl ResolutionState<'_> {
    // Go: internal/module/resolver.go:resolutionState.tryLoadModuleUsingOptionalResolutionSettings
    pub(crate) fn try_load_module_using_optional_resolution_settings(&mut self) -> Resolved {
        let resolved = self.try_load_module_using_paths_if_eligible();
        if !resolved.should_continue_searching() {
            return resolved;
        }
        if !tspath::is_external_module_name_relative(&self.name) {
            continue_searching()
        } else {
            self.try_load_module_using_root_dirs()
        }
    }

    // Go: internal/module/resolver.go:resolutionState.getParsedPatternsForPaths
    pub(crate) fn get_parsed_patterns_for_paths(&mut self) -> ParsedPatterns {
        if std::sync::Arc::ptr_eq(&self.compiler_options, &self.resolver.compiler_options) {
            return self.resolver.get_parsed_patterns_for_paths().clone();
        }
        self.parsed_patterns_for_paths
            .get_or_init(|| try_parse_patterns(self.compiler_options.paths.as_ref()))
            .clone()
    }

    // Go: internal/module/resolver.go:resolutionState.tryLoadModuleUsingPathsIfEligible
    pub(crate) fn try_load_module_using_paths_if_eligible(&mut self) -> Resolved {
        let paths_size = self.compiler_options.paths.as_ref().map_or(0, |p| p.size());
        if paths_size > 0 && !tspath::path_is_relative(&self.name) {
            if self.tracer.is_some() {
                let name = self.name.clone();
                write_trace(
                    &mut self.tracer,
                    &X_PATHS_OPTION_IS_SPECIFIED_LOOKING_FOR_A_PATTERN_TO_MATCH_MODULE_NAME_0,
                    &[&name],
                );
            }
        } else {
            return continue_searching();
        }
        let base_directory = self
            .compiler_options
            .get_paths_base_path(self.resolver.host.get_current_directory());
        let path_patterns = self.get_parsed_patterns_for_paths();
        let extensions = self.extensions;
        let name = self.name.clone();
        let opts = self.compiler_options.clone();
        let loader = Loader::NodeLoadByRelativeName;
        self.try_load_module_using_paths(
            extensions,
            &name,
            &base_directory,
            opts.paths.as_ref(),
            &path_patterns,
            &loader,
        )
    }

    // Go: internal/module/resolver.go:resolutionState.tryLoadModuleUsingPaths
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn try_load_module_using_paths(
        &mut self,
        extensions: Extensions,
        module_name: &str,
        containing_directory: &str,
        paths: Option<&OrderedMap<String, Vec<String>>>,
        path_patterns: &ParsedPatterns,
        loader: &Loader,
    ) -> Resolved {
        let matched_pattern = match_pattern_or_exact(path_patterns, module_name);
        if matched_pattern.is_valid() {
            let matched_star = matched_pattern.matched_text(module_name);
            if self.tracer.is_some() {
                let text = matched_pattern.text.clone();
                write_trace(
                    &mut self.tracer,
                    &MODULE_NAME_0_MATCHED_PATTERN_1,
                    &[module_name, &text],
                );
            }
            let substs: Vec<String> = paths
                .map(|p| p.get_or_zero(&matched_pattern.text))
                .unwrap_or_default();
            for subst in &substs {
                let path = subst.replacen('*', &matched_star, 1);
                let candidate =
                    tspath::normalize_path(&tspath::combine_paths(containing_directory, &[&path]));
                write_trace(
                    &mut self.tracer,
                    &TRYING_SUBSTITUTION_0_CANDIDATE_MODULE_LOCATION_COLON_1,
                    &[subst, &path],
                );
                let extension_from_subst = tspath::try_get_extension_from_path(subst);
                if !extension_from_subst.is_empty() {
                    let (file, ok) = self.try_file(&candidate);
                    if ok {
                        return Some(ResolvedInner {
                            path: file,
                            extension: extension_from_subst.to_string(),
                            ..Default::default()
                        });
                    }
                }
                let save = self.candidate_ending_is_from_config;
                if !extension_from_subst.is_empty() {
                    self.candidate_ending_is_from_config = true;
                }
                let resolved = self.invoke_loader(loader, extensions, &candidate);
                self.candidate_ending_is_from_config = save;
                if !resolved.should_continue_searching() {
                    return resolved;
                }
            }
        }
        continue_searching()
    }

    // Go: internal/module/resolver.go:resolutionState.tryLoadModuleUsingRootDirs
    pub(crate) fn try_load_module_using_root_dirs(&mut self) -> Resolved {
        let root_dirs = self.compiler_options.root_dirs.clone();
        if root_dirs.is_empty() {
            return continue_searching();
        }
        if self.tracer.is_some() {
            let name = self.name.clone();
            write_trace(
                &mut self.tracer,
                &X_ROOTDIRS_OPTION_IS_SET_USING_IT_TO_RESOLVE_RELATIVE_MODULE_NAME_0,
                &[&name],
            );
        }
        let candidate = tspath::normalize_path(&tspath::combine_paths(
            &self.containing_directory,
            &[&self.name],
        ));

        let mut matched_root_dir = String::new();
        let mut matched_normalized_prefix = String::new();
        for root_dir in &root_dirs {
            let mut normalized_root = tspath::normalize_path(root_dir);
            if !normalized_root.ends_with('/') {
                normalized_root.push('/');
            }
            let is_longest_matching_prefix = candidate.starts_with(&normalized_root)
                && (matched_normalized_prefix.is_empty()
                    || matched_normalized_prefix.len() < normalized_root.len());
            if self.tracer.is_some() {
                let longest = is_longest_matching_prefix.to_string();
                write_trace(
                    &mut self.tracer,
                    &CHECKING_IF_0_IS_THE_LONGEST_MATCHING_PREFIX_FOR_1_2,
                    &[&normalized_root, &candidate, &longest],
                );
            }
            if is_longest_matching_prefix {
                matched_normalized_prefix = normalized_root.clone();
                matched_root_dir = root_dir.clone();
            }
        }

        if !matched_normalized_prefix.is_empty() {
            write_trace(
                &mut self.tracer,
                &LONGEST_MATCHING_PREFIX_FOR_0_IS_1,
                &[&candidate, &matched_normalized_prefix],
            );
            let suffix = candidate[matched_normalized_prefix.len()..].to_string();
            write_trace(
                &mut self.tracer,
                &LOADING_0_FROM_THE_ROOT_DIR_1_CANDIDATE_LOCATION_2,
                &[&suffix, &matched_normalized_prefix, &candidate],
            );
            let ext = self.extensions;
            let resolved = self.node_load_module_by_relative_name(ext, &candidate, true);
            if !resolved.should_continue_searching() {
                return resolved;
            }
            write_trace(&mut self.tracer, &TRYING_OTHER_ENTRIES_IN_ROOTDIRS, &[]);
            for root_dir in &root_dirs {
                if *root_dir == matched_root_dir {
                    continue;
                }
                let candidate2 =
                    tspath::combine_paths(&tspath::normalize_path(root_dir), &[&suffix]);
                write_trace(
                    &mut self.tracer,
                    &LOADING_0_FROM_THE_ROOT_DIR_1_CANDIDATE_LOCATION_2,
                    &[&suffix, root_dir, &candidate2],
                );
                let resolved = self.node_load_module_by_relative_name(ext, &candidate2, true);
                if !resolved.should_continue_searching() {
                    return resolved;
                }
            }
            write_trace(
                &mut self.tracer,
                &MODULE_RESOLUTION_USING_ROOTDIRS_HAS_FAILED,
                &[],
            );
        }
        continue_searching()
    }

    // Dispatches a `Loader` variant (mirrors Go's closures passed to
    // `tryLoadModuleUsingPaths`).
    // Go: internal/module/resolver.go:resolutionKindSpecificLoader
    fn invoke_loader(
        &mut self,
        loader: &Loader,
        extensions: Extensions,
        candidate: &str,
    ) -> Resolved {
        match loader {
            Loader::NodeLoadByRelativeName => {
                self.node_load_module_by_relative_name(extensions, candidate, true)
            }
            Loader::SpecificNodeModules { package_info, rest } => {
                self.specific_node_modules_loader(extensions, candidate, *package_info, rest)
            }
            Loader::DirectoryWorker {
                package_info,
                package_file,
            } => self.directory_worker_loader(extensions, candidate, *package_info, package_file),
        }
    }
}

#[cfg(test)]
#[path = "paths_test.rs"]
mod tests;
