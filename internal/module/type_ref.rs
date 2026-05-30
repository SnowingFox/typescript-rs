//! Type-reference-directive resolution: primary `typeRoots` lookup, the
//! `@types` scoped-name mangling, the typeRoots fallback used for declaration
//! resolution, and the typings-location extra pass.
//!
//! Split from Go `internal/module/resolver.go` (PORTING.md §2).

use tsgo_core::compileroptions::ModuleKind;
use tsgo_diagnostics::{
    AUTO_DISCOVERY_FOR_TYPINGS_IS_ENABLED_IN_PROJECT_0_RUNNING_EXTRA_RESOLUTION_PASS_FOR_MODULE_1_USING_CACHE_LOCATION_2,
    DIRECTORY_0_DOES_NOT_EXIST_SKIPPING_ALL_LOOKUPS_IN_IT,
    LOOKING_UP_IN_NODE_MODULES_FOLDER_INITIAL_LOCATION_0,
    RESOLVING_TYPE_REFERENCE_DIRECTIVE_FOR_PROGRAM_THAT_SPECIFIES_CUSTOM_TYPEROOTS_SKIPPING_LOOKUP_IN_NODE_MODULES_FOLDER,
    RESOLVING_WITH_PRIMARY_SEARCH_PATH_0,
    ROOT_DIRECTORY_CANNOT_BE_DETERMINED_SKIPPING_PRIMARY_SEARCH_PATHS,
    SCOPED_PACKAGE_DETECTED_LOOKING_IN_0,
};
use tsgo_tspath as tspath;

use crate::state::ResolutionState;
use crate::util::{mangle_scoped_package_name, parse_node_module_from_path};
use crate::{
    continue_searching, normalize_path_for_cjs_resolution, write_trace, Extensions, ResolvedExt,
    ResolvedModule, Resolver, Tracer,
};

impl ResolutionState<'_> {
    // Go: internal/module/resolver.go:resolutionState.resolveTypeReferenceDirective
    pub(crate) fn resolve_type_reference_directive(
        &mut self,
        type_roots: &[String],
        from_config: bool,
        from_inferred_types_containing_file: bool,
    ) -> crate::ResolvedTypeReferenceDirective {
        if !type_roots.is_empty() {
            if self.tracer.is_some() {
                let joined = type_roots.join(", ");
                write_trace(
                    &mut self.tracer,
                    &RESOLVING_WITH_PRIMARY_SEARCH_PATH_0,
                    &[&joined],
                );
            }
            for type_root in type_roots {
                let candidate = self.get_candidate_from_type_root(type_root);
                if !self.resolver.host.fs().directory_exists(type_root) {
                    write_trace(
                        &mut self.tracer,
                        &DIRECTORY_0_DOES_NOT_EXIST_SKIPPING_ALL_LOOKUPS_IN_IT,
                        &[type_root],
                    );
                    continue;
                }
                if from_config {
                    let mut resolved_from_file =
                        self.load_module_from_file(Extensions::DECLARATION, &candidate);
                    if !resolved_from_file.should_continue_searching() {
                        let path = resolved_from_file.as_ref().expect("some").path.clone();
                        let package_directory = parse_node_module_from_path(&path, false);
                        if !package_directory.is_empty() {
                            let info = self.get_package_json_info(&package_directory);
                            let pkg_id = self.get_package_id(&path, info.as_ref());
                            resolved_from_file.as_mut().expect("some").package_id = pkg_id;
                        }
                        return self
                            .create_resolved_type_reference_directive(resolved_from_file, true);
                    }
                }
                let resolved_from_directory =
                    self.load_node_module_from_directory(Extensions::DECLARATION, &candidate, true);
                if !resolved_from_directory.should_continue_searching() {
                    return self
                        .create_resolved_type_reference_directive(resolved_from_directory, true);
                }
            }
        } else {
            write_trace(
                &mut self.tracer,
                &ROOT_DIRECTORY_CANNOT_BE_DETERMINED_SKIPPING_PRIMARY_SEARCH_PATHS,
                &[],
            );
        }

        let mut resolved = continue_searching();
        if !from_config || !from_inferred_types_containing_file {
            if self.tracer.is_some() {
                let dir = self.containing_directory.clone();
                write_trace(
                    &mut self.tracer,
                    &LOOKING_UP_IN_NODE_MODULES_FOLDER_INITIAL_LOCATION_0,
                    &[&dir],
                );
            }
            if !tspath::is_external_module_name_relative(&self.name) {
                resolved = self.load_module_from_nearest_node_modules_directory(false);
            } else {
                let candidate =
                    normalize_path_for_cjs_resolution(&self.containing_directory, &self.name);
                resolved = self.node_load_module_by_relative_name(
                    Extensions::DECLARATION,
                    &candidate,
                    true,
                );
            }
        } else {
            write_trace(
                &mut self.tracer,
                &RESOLVING_TYPE_REFERENCE_DIRECTIVE_FOR_PROGRAM_THAT_SPECIFIES_CUSTOM_TYPEROOTS_SKIPPING_LOOKUP_IN_NODE_MODULES_FOLDER,
                &[],
            );
        }
        self.create_resolved_type_reference_directive(resolved, false)
    }

    // Go: internal/module/resolver.go:resolutionState.getCandidateFromTypeRoot
    pub(crate) fn get_candidate_from_type_root(&mut self, type_root: &str) -> String {
        let name_for_lookup = if type_root.ends_with("/node_modules/@types")
            || type_root.ends_with("/node_modules/@types/")
        {
            let name = self.name.clone();
            self.mangle_scoped_package_name(&name)
        } else {
            self.name.clone()
        };
        tspath::combine_paths(type_root, &[&name_for_lookup])
    }

    // Go: internal/module/resolver.go:resolutionState.mangleScopedPackageName
    pub(crate) fn mangle_scoped_package_name(&mut self, name: &str) -> String {
        let mangled = mangle_scoped_package_name(name);
        if self.tracer.is_some() && mangled != name {
            let m = mangled.clone();
            write_trace(
                &mut self.tracer,
                &SCOPED_PACKAGE_DETECTED_LOOKING_IN_0,
                &[&m],
            );
        }
        mangled
    }

    // Go: internal/module/resolver.go:resolutionState.resolveFromTypeRoot
    pub(crate) fn resolve_from_type_root(&mut self) -> crate::Resolved {
        let Some(type_roots) = self.compiler_options.type_roots.clone() else {
            return continue_searching();
        };
        for type_root in &type_roots {
            let candidate = self.get_candidate_from_type_root(type_root);
            if !self.resolver.host.fs().directory_exists(type_root) {
                write_trace(
                    &mut self.tracer,
                    &DIRECTORY_0_DOES_NOT_EXIST_SKIPPING_ALL_LOOKUPS_IN_IT,
                    &[type_root],
                );
                continue;
            }
            let mut resolved_from_file =
                self.load_module_from_file(Extensions::DECLARATION, &candidate);
            if !resolved_from_file.should_continue_searching() {
                let path = resolved_from_file.as_ref().expect("some").path.clone();
                let package_directory = parse_node_module_from_path(&path, false);
                if !package_directory.is_empty() {
                    let info = self.get_package_json_info(&package_directory);
                    let pkg_id = self.get_package_id(&path, info.as_ref());
                    resolved_from_file.as_mut().expect("some").package_id = pkg_id;
                }
                return resolved_from_file;
            }
            let resolved =
                self.load_node_module_from_directory(Extensions::DECLARATION, &candidate, true);
            if !resolved.should_continue_searching() {
                return resolved;
            }
        }
        continue_searching()
    }
}

impl Resolver {
    // Go: internal/module/resolver.go:Resolver.tryResolveFromTypingsLocation
    pub(crate) fn try_resolve_from_typings_location(
        &self,
        module_name: &str,
        containing_directory: &str,
        original_result: ResolvedModule,
        trace_builder: &mut Option<Tracer>,
    ) -> ResolvedModule {
        if self.typings_location.is_empty()
            || tspath::is_external_module_name_relative(module_name)
            || (!original_result.resolved_file_name.is_empty()
                && tspath::extension_is_one_of(
                    &original_result.extension,
                    tspath::SUPPORTED_TS_EXTENSIONS_WITH_JSON_FLAT.as_slice(),
                ))
        {
            return original_result;
        }

        let tracer = trace_builder.take();
        let mut state = ResolutionState::new(
            module_name,
            containing_directory,
            false,
            ModuleKind::None,
            self.compiler_options.clone(),
            self,
            tracer,
        );
        if state.tracer.is_some() {
            let project = self.project_name.clone();
            let location = self.typings_location.clone();
            write_trace(
                &mut state.tracer,
                &AUTO_DISCOVERY_FOR_TYPINGS_IS_ENABLED_IN_PROJECT_0_RUNNING_EXTRA_RESOLUTION_PASS_FOR_MODULE_1_USING_CACHE_LOCATION_2,
                &[&project, module_name, &location],
            );
        }
        let typings_location = self.typings_location.clone();
        let global_resolved = state.load_module_from_immediate_node_modules_directory(
            Extensions::DECLARATION,
            &typings_location,
            false,
        );
        if global_resolved.is_none() {
            *trace_builder = state.into_tracer();
            return original_result;
        }
        let mut result = state.create_resolved_module(global_resolved, true);
        *trace_builder = state.into_tracer();
        let mut diagnostics = original_result.resolution_diagnostics;
        diagnostics.append(&mut result.resolution_diagnostics);
        result.resolution_diagnostics = diagnostics;
        result
    }
}

#[cfg(test)]
#[path = "type_ref_test.rs"]
mod tests;
