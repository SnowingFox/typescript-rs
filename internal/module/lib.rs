//! `tsgo_module` — 1:1 Rust port of Go `internal/module`.
//!
//! Implements TypeScript's module-name resolver: given a module specifier and
//! `CompilerOptions`, it resolves to a concrete file on disk under a
//! [`Vfs`](tsgo_vfs::Fs), honoring the Node16/NodeNext/Bundler feature sets,
//! `package.json` `exports`/`imports`/`typesVersions`/`main`/`types`, `paths`,
//! `rootDirs`, `typeRoots`, `@types` fallback, and symlink realpaths.
//!
//! # Mega-file split (PORTING.md §2)
//! Go's `resolver.go` is ~2300 lines. It is split here into cohesive modules
//! (`state`, `node_modules`, `node_resolution`, `file_load`, `package_info`,
//! `paths`, `type_ref`, `entrypoints`) that each implement a slice of the
//! `ResolutionState`/`Resolver` algorithm. Every ported function keeps a
//! `// Go: resolver.go:<Func>` anchor to its original location.
//!
//! # Divergence from Go
//! - `*resolved` (three states: nil / empty / resolved) becomes
//!   `Option<ResolvedInner>` with the `ResolvedExt` helper trait.
//! - `*ast.Diagnostic` results are kept as [`ResolutionDiagnostic`]
//!   (message + args) until `tsgo_ast::Diagnostic` is ported.
//! - `SyncMap`/`sync.Once` become `SyncMap`/`OnceLock`; concurrency semantics
//!   (the `package.json` info-cache `LoadOrStore`) are preserved.

#[cfg(test)]
mod test_support;

mod cache;
mod entrypoints;
mod file_load;
mod node_modules;
mod node_resolution;
mod package_info;
mod paths;
mod state;
mod type_ref;
mod types;
mod util;

pub use cache::{get_redirect_config_name, ModeAwareCache};
pub use entrypoints::{Ending, ResolvedEntrypoint};
pub use state::{get_conditions, PackageJsonInfo};
pub use types::*;
pub use util::*;

use std::sync::Arc;

use tsgo_collections::{OrderedMap, Set};
use tsgo_core::compileroptions::{
    CompilerOptions, ModuleKind, ModuleResolutionKind, ResolutionMode,
};
use tsgo_core::pattern::{find_best_pattern_match, try_parse_pattern, Pattern};
use tsgo_diagnostics::Message;
use tsgo_packagejson::InfoCache;
use tsgo_tspath as tspath;

use crate::cache::{
    new_caches, Caches, ModuleResolutionCacheKey, TypeRefDirectiveResolutionCacheKey,
};
use crate::state::ResolutionState;

/// The intermediate result of a single resolution step.
///
/// Mirrors Go's `*resolved`: `path` empty means "stopped, unresolved"; a
/// non-empty `path` means "resolved". The "keep searching" state is
/// represented by `Resolved` being `None` (see [`continue_searching`]).
///
/// Side effects: none (plain data).
// Go: internal/module/resolver.go:resolved
#[derive(Debug, Clone, Default)]
pub(crate) struct ResolvedInner {
    pub(crate) path: String,
    pub(crate) extension: String,
    pub(crate) package_id: PackageId,
    pub(crate) original_path: String,
    pub(crate) resolved_using_ts_extension: bool,
}

/// The three-state resolution outcome: `None` keeps searching, `Some(inner)`
/// stops (resolved when `inner.path` is non-empty).
// Go: internal/module/resolver.go:resolved (as *resolved)
pub(crate) type Resolved = Option<ResolvedInner>;

/// Resolution-result helpers mirroring Go's `shouldContinueSearching`/
/// `isResolved` on `*resolved`.
pub(crate) trait ResolvedExt {
    fn should_continue_searching(&self) -> bool;
    fn is_resolved(&self) -> bool;
}

impl ResolvedExt for Resolved {
    // Go: internal/module/resolver.go:resolved.shouldContinueSearching
    fn should_continue_searching(&self) -> bool {
        self.is_none()
    }

    // Go: internal/module/resolver.go:resolved.isResolved
    fn is_resolved(&self) -> bool {
        self.as_ref().is_some_and(|r| !r.path.is_empty())
    }
}

/// The "keep searching" result (Go `continueSearching` / nil).
// Go: internal/module/resolver.go:continueSearching
pub(crate) fn continue_searching() -> Resolved {
    None
}

/// The "stopped, unresolved" result (Go `unresolved` / `&resolved{}`).
// Go: internal/module/resolver.go:unresolved
pub(crate) fn unresolved() -> Resolved {
    Some(ResolvedInner::default())
}

/// A trace diagnostic plus its stringified arguments, accumulated when
/// `traceResolution` is enabled.
///
/// # Divergence from Go
/// Go's `Args []any` is stored here as `Vec<String>` (diagnostics arguments are
/// rendered to strings); trace output itself is consumed in a later phase.
///
/// Side effects: none (plain data).
// Go: internal/module/resolver.go:DiagAndArgs
#[derive(Debug, Clone)]
pub struct DiagAndArgs {
    /// The trace message.
    pub message: &'static Message,
    /// The stringified message arguments.
    pub args: Vec<String>,
}

/// Accumulates resolution trace diagnostics when `traceResolution` is on.
// Go: internal/module/resolver.go:tracer
#[derive(Debug, Default)]
pub(crate) struct Tracer {
    pub(crate) traces: Vec<DiagAndArgs>,
}

impl Tracer {
    // Go: internal/module/resolver.go:tracer.write
    pub(crate) fn write(&mut self, message: &'static Message, args: &[&str]) {
        self.traces.push(DiagAndArgs {
            message,
            args: args.iter().map(|s| (*s).to_string()).collect(),
        });
    }
}

/// Returns the traces accumulated by `tracer`, or an empty vec when tracing is
/// disabled (Go's nil-receiver `getTraces`).
///
/// Side effects: none (pure).
// Go: internal/module/resolver.go:tracer.getTraces
fn get_traces(tracer: Option<Tracer>) -> Vec<DiagAndArgs> {
    match tracer {
        Some(t) => t.traces,
        None => Vec::new(),
    }
}

/// Writes a trace through an optional tracer, mirroring Go's nil-tolerant
/// `tracer.write`. Taking `&mut Option<Tracer>` lets callers borrow the tracer
/// field disjointly from the rest of a `ResolutionState`.
///
/// Side effects: appends to `tracer` when present.
// Go: internal/module/resolver.go:tracer.write
pub(crate) fn write_trace(tracer: &mut Option<Tracer>, message: &'static Message, args: &[&str]) {
    if let Some(t) = tracer.as_mut() {
        t.write(message, args);
    }
}

/// The loader strategy passed to path-mapping resolution. Mirrors Go's
/// `resolutionKindSpecificLoader` closures (which differ by call site) as an
/// explicit enum so it can be dispatched without capturing `&mut self`.
// Go: internal/module/resolver.go:resolutionKindSpecificLoader
pub(crate) enum Loader<'a> {
    /// `nodeLoadModuleByRelativeName(_, _, considerPackageJson=true)`.
    NodeLoadByRelativeName,
    /// The loader inside `loadModuleFromSpecificNodeModulesDirectory`.
    SpecificNodeModules {
        package_info: Option<&'a PackageJsonInfo>,
        rest: &'a str,
    },
    /// The loader inside `loadNodeModuleFromDirectoryWorker`.
    DirectoryWorker {
        package_info: Option<&'a PackageJsonInfo>,
        package_file: &'a str,
    },
}

/// Options for constructing a [`Resolver`].
// Go: internal/module/resolver.go:ResolverOptions
#[derive(Default)]
pub struct ResolverOptions {
    /// A shared `package.json` info cache to reuse across resolvers.
    pub package_json_cache: Option<Arc<InfoCache>>,
}

/// The module-name resolver: holds the host, compiler options, caches, and the
/// optional typings location, and resolves module/type-reference specifiers.
// Go: internal/module/resolver.go:Resolver
pub struct Resolver {
    pub(crate) caches: Caches,
    pub(crate) host: Arc<dyn ResolutionHost>,
    pub(crate) compiler_options: Arc<CompilerOptions>,
    pub(crate) typings_location: String,
    pub(crate) project_name: String,
}

impl Resolver {
    /// Creates a resolver with fresh caches.
    ///
    /// Side effects: allocates the resolution caches.
    // Go: internal/module/resolver.go:NewResolver
    pub fn new(
        host: Arc<dyn ResolutionHost>,
        options: Arc<CompilerOptions>,
        typings_location: impl Into<String>,
        project_name: impl Into<String>,
    ) -> Resolver {
        let caches = new_caches(
            host.get_current_directory(),
            host.fs().use_case_sensitive_file_names(),
            &options,
        );
        Resolver {
            caches,
            host,
            compiler_options: options,
            typings_location: typings_location.into(),
            project_name: project_name.into(),
        }
    }

    /// Creates a resolver, optionally sharing an existing `package.json` info
    /// cache.
    ///
    /// Side effects: allocates the resolution caches (reusing the supplied
    /// `package.json` cache when present).
    // Go: internal/module/resolver.go:NewResolverWithOptions
    pub fn with_options(
        host: Arc<dyn ResolutionHost>,
        compiler_options: Arc<CompilerOptions>,
        typings_location: impl Into<String>,
        project_name: impl Into<String>,
        opts: ResolverOptions,
    ) -> Resolver {
        let mut caches = new_caches(
            host.get_current_directory(),
            host.fs().use_case_sensitive_file_names(),
            &compiler_options,
        );
        if let Some(cache) = opts.package_json_cache {
            caches.package_json_info_cache = cache;
        }
        Resolver {
            caches,
            host,
            compiler_options,
            typings_location: typings_location.into(),
            project_name: project_name.into(),
        }
    }

    // Go: internal/module/resolver.go:Resolver.newTraceBuilder
    pub(crate) fn new_trace_builder(&self) -> Option<Tracer> {
        if self.compiler_options.trace_resolution.is_true() {
            Some(Tracer::default())
        } else {
            None
        }
    }

    /// Returns the `package.json` scope (nearest containing `package.json`) for
    /// `directory`.
    ///
    /// Side effects: reads the file system (cached).
    // Go: internal/module/resolver.go:Resolver.GetPackageScopeForPath
    pub fn get_package_scope_for_path(&self, directory: &str) -> Option<PackageJsonInfo> {
        let mut state = ResolutionState::for_scope_lookup(self);
        state.get_package_scope_for_path(directory)
    }

    /// Resolves a module specifier `module_name` imported from `containing_file`.
    ///
    /// Side effects: reads the file system; populates the resolution caches.
    // Go: internal/module/resolver.go:Resolver.ResolveModuleName
    pub fn resolve_module_name(
        &self,
        module_name: &str,
        containing_file: &str,
        resolution_mode: ResolutionMode,
        redirected_reference: Option<&dyn ResolvedProjectReference>,
    ) -> (Arc<ResolvedModule>, Vec<DiagAndArgs>) {
        let containing_directory = tspath::get_directory_path(containing_file);
        let mut trace_builder = self.new_trace_builder();

        let cache_key = ModuleResolutionCacheKey {
            containing_directory: containing_directory.clone(),
            module_name: module_name.to_string(),
            resolution_mode,
            redirect_config_name: get_redirect_config_name(redirected_reference),
        };

        if trace_builder.is_none() {
            if let Some(cached) = self.caches.module_resolution_cache.get(&cache_key) {
                return (cached, Vec::new());
            }
        }

        let compiler_options =
            get_compiler_options_with_redirect(self.compiler_options.clone(), redirected_reference);
        if let Some(t) = trace_builder.as_mut() {
            t.write(
                &tsgo_diagnostics::RESOLVING_MODULE_0_FROM_1,
                &[module_name, containing_file],
            );
            trace_resolution_using_project_reference(t, redirected_reference);
        }

        let module_resolution = compiler_options.get_module_resolution_kind();
        if let Some(t) = trace_builder.as_mut() {
            if compiler_options.module_resolution != module_resolution {
                t.write(
                    &tsgo_diagnostics::MODULE_RESOLUTION_KIND_IS_NOT_SPECIFIED_USING_0,
                    &[&module_resolution.to_string()],
                );
            } else {
                t.write(
                    &tsgo_diagnostics::EXPLICITLY_SPECIFIED_MODULE_RESOLUTION_KIND_COLON_0,
                    &[&module_resolution.to_string()],
                );
            }
        }

        let result: ResolvedModule = match module_resolution {
            ModuleResolutionKind::Node16
            | ModuleResolutionKind::NodeNext
            | ModuleResolutionKind::Bundler => {
                let mut state = ResolutionState::new(
                    module_name,
                    &containing_directory,
                    false,
                    resolution_mode,
                    compiler_options,
                    self,
                    trace_builder.take(),
                );
                let r = state.resolve_node_like();
                trace_builder = state.into_tracer();
                r
            }
            other => panic!("Unexpected moduleResolution: {}", other as i32),
        };

        if let Some(t) = trace_builder.as_mut() {
            if result.is_resolved() {
                if !result.package_id.name.is_empty() {
                    t.write(
                        &tsgo_diagnostics::MODULE_NAME_0_WAS_SUCCESSFULLY_RESOLVED_TO_1_WITH_PACKAGE_ID_2,
                        &[module_name, &result.resolved_file_name, &result.package_id.to_string()],
                    );
                } else {
                    t.write(
                        &tsgo_diagnostics::MODULE_NAME_0_WAS_SUCCESSFULLY_RESOLVED_TO_1,
                        &[module_name, &result.resolved_file_name],
                    );
                }
            } else {
                t.write(
                    &tsgo_diagnostics::MODULE_NAME_0_WAS_NOT_RESOLVED,
                    &[module_name],
                );
            }
        }

        let final_result = self.try_resolve_from_typings_location(
            module_name,
            &containing_directory,
            result,
            &mut trace_builder,
        );
        let final_result = Arc::new(final_result);
        self.caches
            .module_resolution_cache
            .set(cache_key, final_result.clone());

        (final_result, get_traces(trace_builder))
    }

    /// Resolves a type reference directive `type_reference_directive_name`
    /// referenced from `containing_file`.
    ///
    /// Side effects: reads the file system; populates the resolution caches.
    // Go: internal/module/resolver.go:Resolver.ResolveTypeReferenceDirective
    pub fn resolve_type_reference_directive(
        &self,
        type_reference_directive_name: &str,
        containing_file: &str,
        resolution_mode: ResolutionMode,
        redirected_reference: Option<&dyn ResolvedProjectReference>,
    ) -> (Arc<ResolvedTypeReferenceDirective>, Vec<DiagAndArgs>) {
        let containing_directory = tspath::get_directory_path(containing_file);
        let mut trace_builder = self.new_trace_builder();

        let from_inferred_types_containing_file =
            containing_file.ends_with(INFERRED_TYPES_CONTAINING_FILE);

        let cache_key = TypeRefDirectiveResolutionCacheKey {
            containing_directory: containing_directory.clone(),
            type_reference_name: type_reference_directive_name.to_string(),
            resolution_mode,
            redirect_config_name: get_redirect_config_name(redirected_reference),
            from_inferred_types_containing_file,
        };

        if trace_builder.is_none() {
            if let Some(cached) = self
                .caches
                .type_ref_directive_resolution_cache
                .get(&cache_key)
            {
                return (cached, Vec::new());
            }
        }

        let compiler_options =
            get_compiler_options_with_redirect(self.compiler_options.clone(), redirected_reference);

        let (type_roots, from_config) =
            compiler_options.get_effective_type_roots(self.host.get_current_directory());
        if let Some(t) = trace_builder.as_mut() {
            t.write(
                &tsgo_diagnostics::RESOLVING_TYPE_REFERENCE_DIRECTIVE_0_CONTAINING_FILE_1_ROOT_DIRECTORY_2,
                &[type_reference_directive_name, containing_file, &type_roots.join(",")],
            );
            trace_resolution_using_project_reference(t, redirected_reference);
        }

        let mut state = ResolutionState::new(
            type_reference_directive_name,
            &containing_directory,
            true,
            resolution_mode,
            compiler_options,
            self,
            trace_builder.take(),
        );
        let result = state.resolve_type_reference_directive(
            &type_roots,
            from_config,
            from_inferred_types_containing_file,
        );
        trace_builder = state.into_tracer();

        if let Some(t) = trace_builder.as_mut() {
            trace_type_reference_directive_result(t, type_reference_directive_name, &result);
        }

        let result = Arc::new(result);
        self.caches
            .type_ref_directive_resolution_cache
            .set(cache_key, result.clone());

        (result, get_traces(trace_builder))
    }

    /// Resolves only the package directory for `module_name` (no file probing).
    ///
    /// Side effects: reads the file system.
    // Go: internal/module/resolver.go:Resolver.ResolvePackageDirectory
    pub fn resolve_package_directory(
        &self,
        module_name: &str,
        containing_file: &str,
        resolution_mode: ResolutionMode,
        redirected_reference: Option<&dyn ResolvedProjectReference>,
    ) -> Option<ResolvedModule> {
        let compiler_options =
            get_compiler_options_with_redirect(self.compiler_options.clone(), redirected_reference);
        let containing_directory = tspath::get_directory_path(containing_file);
        let mut state = ResolutionState::new(
            module_name,
            &containing_directory,
            false,
            resolution_mode,
            compiler_options,
            self,
            None,
        );
        state.resolve_package_directory_only = true;
        let result = state.load_module_from_nearest_node_modules_directory(false);
        if result.as_ref().is_some_and(|r| !r.path.is_empty()) {
            return Some(state.create_resolved_module_handling_symlink(result));
        }
        None
    }

    // Go: internal/module/resolver.go:Resolver.resolveConfig
    fn resolve_config(&self, module_name: &str, containing_file: &str) -> ResolvedModule {
        let containing_directory = tspath::get_directory_path(containing_file);
        let mut state = ResolutionState::new(
            module_name,
            &containing_directory,
            false,
            ModuleKind::CommonJs,
            self.compiler_options.clone(),
            self,
            None,
        );
        state.is_config_lookup = true;
        state.extensions = Extensions::JSON;
        state.resolve_node_like()
    }
}

/// Returns the compiler options to use, preferring a redirect's options when
/// present.
///
/// # Examples
/// ```
/// use std::sync::Arc;
/// use tsgo_core::compileroptions::CompilerOptions;
/// use tsgo_module::get_compiler_options_with_redirect;
/// let opts = Arc::new(CompilerOptions::default());
/// let same = get_compiler_options_with_redirect(opts.clone(), None);
/// assert!(Arc::ptr_eq(&opts, &same));
/// ```
///
/// Side effects: none (pure).
// Go: internal/module/resolver.go:GetCompilerOptionsWithRedirect
pub fn get_compiler_options_with_redirect(
    compiler_options: Arc<CompilerOptions>,
    redirected_reference: Option<&dyn ResolvedProjectReference>,
) -> Arc<CompilerOptions> {
    match redirected_reference {
        None => compiler_options,
        Some(redirect) => match redirect.compiler_options() {
            Some(options) => options,
            None => compiler_options,
        },
    }
}

// Go: internal/module/resolver.go:tracer.traceResolutionUsingProjectReference
fn trace_resolution_using_project_reference(
    tracer: &mut Tracer,
    redirected_reference: Option<&dyn ResolvedProjectReference>,
) {
    if let Some(r) = redirected_reference {
        if r.compiler_options().is_some() {
            tracer.write(
                &tsgo_diagnostics::USING_COMPILER_OPTIONS_OF_PROJECT_REFERENCE_REDIRECT_0,
                &[r.config_name()],
            );
        }
    }
}

// Go: internal/module/resolver.go:tracer.traceTypeReferenceDirectiveResult
fn trace_type_reference_directive_result(
    tracer: &mut Tracer,
    type_reference_directive_name: &str,
    result: &ResolvedTypeReferenceDirective,
) {
    if !result.is_resolved() {
        tracer.write(
            &tsgo_diagnostics::TYPE_REFERENCE_DIRECTIVE_0_WAS_NOT_RESOLVED,
            &[type_reference_directive_name],
        );
    } else if !result.package_id.name.is_empty() {
        tracer.write(
            &tsgo_diagnostics::TYPE_REFERENCE_DIRECTIVE_0_WAS_SUCCESSFULLY_RESOLVED_TO_1_WITH_PACKAGE_ID_2_PRIMARY_COLON_3,
            &[
                type_reference_directive_name,
                &result.resolved_file_name,
                &result.package_id.to_string(),
                &result.primary.to_string(),
            ],
        );
    } else {
        tracer.write(
            &tsgo_diagnostics::TYPE_REFERENCE_DIRECTIVE_0_WAS_SUCCESSFULLY_RESOLVED_TO_1_PRIMARY_COLON_2,
            &[
                type_reference_directive_name,
                &result.resolved_file_name,
                &result.primary.to_string(),
            ],
        );
    }
}

/// A parsed representation of `CompilerOptions.paths`: exact-match keys plus
/// wildcard patterns.
///
/// Side effects: none (plain data).
// Go: internal/module/resolver.go:ParsedPatterns
#[derive(Debug, Clone, Default)]
pub struct ParsedPatterns {
    matchable_string_set: Set<String>,
    patterns: Vec<Pattern>,
}

/// Parses a `paths`-style mapping into a [`ParsedPatterns`].
///
/// Returns an empty result when `path_mappings` is `None`.
///
/// Side effects: none (pure).
// Go: internal/module/resolver.go:TryParsePatterns
pub fn try_parse_patterns(
    path_mappings: Option<&OrderedMap<String, Vec<String>>>,
) -> ParsedPatterns {
    let Some(path_mappings) = path_mappings else {
        return ParsedPatterns::default();
    };

    let mut num_patterns = 0usize;
    for path in path_mappings.keys() {
        let pattern = try_parse_pattern(path);
        if pattern.is_valid() && pattern.star_index == -1 {
            num_patterns += 1;
        }
    }
    let num_matchables = path_mappings.size() - num_patterns;

    // The capacity hints mirror Go's `TryParsePatterns` exactly (which swaps
    // them); this is harmless preallocation and does not affect correctness.
    let mut patterns: Vec<Pattern> = Vec::new();
    let mut matchable_string_set: Set<String> = Set::default();
    if num_patterns != 0 {
        patterns = Vec::with_capacity(num_patterns);
    }
    if num_matchables != 0 {
        matchable_string_set = Set::with_size_hint(num_matchables);
    }

    for path in path_mappings.keys() {
        let pattern = try_parse_pattern(path);
        if pattern.is_valid() {
            if pattern.star_index == -1 {
                matchable_string_set.add(path.clone());
            } else {
                patterns.push(pattern);
            }
        }
    }

    ParsedPatterns {
        matchable_string_set,
        patterns,
    }
}

/// Matches `candidate` against `patterns`, preferring an exact match and then
/// the best wildcard pattern.
///
/// Returns an invalid (default) [`Pattern`] when nothing matches.
///
/// Side effects: none (pure).
// Go: internal/module/resolver.go:MatchPatternOrExact
pub fn match_pattern_or_exact(patterns: &ParsedPatterns, candidate: &str) -> Pattern {
    if patterns.matchable_string_set.has(&candidate.to_string()) {
        return Pattern {
            text: candidate.to_string(),
            star_index: -1,
        };
    }
    if patterns.patterns.is_empty() {
        return Pattern::default();
    }
    match find_best_pattern_match(&patterns.patterns, |p| p.clone(), candidate) {
        Some(i) => patterns.patterns[i].clone(),
        None => Pattern::default(),
    }
}

/// If you import from `.` inside a containing directory `/foo`, the normalized
/// result `/foo` loses the intent to look *inside* `foo`. Node treats module
/// paths ending in `.`/`..` as ending in `./`/`../`, so this restores the
/// trailing separator.
///
/// Side effects: none (pure).
// Go: internal/module/resolver.go:normalizePathForCJSResolution
pub(crate) fn normalize_path_for_cjs_resolution(
    containing_directory: &str,
    module_name: &str,
) -> String {
    let combined = tspath::combine_paths(containing_directory, &[module_name]);
    let parts = tspath::get_path_components(&combined, "");
    let last_part = parts.last().map(String::as_str).unwrap_or("");
    if last_part == "." || last_part == ".." {
        tspath::ensure_trailing_directory_separator(&tspath::normalize_path(&combined))
    } else {
        tspath::normalize_path(&combined)
    }
}

/// Reports whether `name` matches the wildcard `target` where the `*` is
/// surrounded by a fixed prefix and suffix (a "pattern trailer").
///
/// Side effects: none (pure).
// Go: internal/module/resolver.go:matchesPatternWithTrailer
pub(crate) fn matches_pattern_with_trailer(target: &str, name: &str) -> bool {
    if target.ends_with('*') {
        return false;
    }
    match target.split_once('*') {
        None => false,
        Some((before, after)) => name.starts_with(before) && name.ends_with(after),
    }
}

/// Reports whether `extension` is permitted by `extensions`.
///
/// Side effects: none (pure).
// Go: internal/module/resolver.go:extensionIsOk
pub(crate) fn extension_is_ok(extensions: Extensions, extension: &str) -> bool {
    (extensions.contains(Extensions::JAVA_SCRIPT)
        && (extension == tspath::EXTENSION_JS
            || extension == tspath::EXTENSION_JSX
            || extension == tspath::EXTENSION_MJS
            || extension == tspath::EXTENSION_CJS))
        || (extensions.contains(Extensions::TYPE_SCRIPT)
            && (extension == tspath::EXTENSION_TS
                || extension == tspath::EXTENSION_TSX
                || extension == tspath::EXTENSION_MTS
                || extension == tspath::EXTENSION_CTS))
        || (extensions.contains(Extensions::DECLARATION)
            && (extension == tspath::EXTENSION_DTS
                || extension == tspath::EXTENSION_DMTS
                || extension == tspath::EXTENSION_DCTS))
        || (extensions.contains(Extensions::JSON) && extension == tspath::EXTENSION_JSON)
}

/// Resolves a tsconfig-extends-style module specifier under NodeNext rules.
///
/// Side effects: reads the file system.
// Go: internal/module/resolver.go:ResolveConfig
pub fn resolve_config(
    module_name: &str,
    containing_file: &str,
    host: Arc<dyn ResolutionHost>,
) -> ResolvedModule {
    let resolver = Resolver::new(
        host,
        Arc::new(CompilerOptions {
            module_resolution: ModuleResolutionKind::NodeNext,
            ..Default::default()
        }),
        "",
        "",
    );
    resolver.resolve_config(module_name, containing_file)
}

/// Computes the automatic `types` directive names from the configured type
/// roots, expanding the `"*"` wildcard against the directories present on disk.
///
/// Side effects: reads the file system.
// Go: internal/module/resolver.go:GetAutomaticTypeDirectiveNames
pub fn get_automatic_type_directive_names(
    options: &CompilerOptions,
    host: &dyn ResolutionHost,
) -> Vec<String> {
    if !options.uses_wildcard_types() {
        return options.types.clone();
    }

    let mut wildcard_matches: Vec<String> = Vec::new();
    let (type_roots, _) = options.get_effective_type_roots(host.get_current_directory());
    for root in &type_roots {
        if host.fs().directory_exists(root) {
            for type_directive_path in host.fs().get_accessible_entries(root).directories {
                let normalized = tspath::normalize_path(&type_directive_path);
                let package_json_path = tspath::combine_paths(root, &[&normalized, "package.json"]);
                let mut is_not_needed_package = false;
                if host.fs().file_exists(&package_json_path) {
                    let contents = host.fs().read_file(&package_json_path).unwrap_or_default();
                    if let Ok(fields) = tsgo_packagejson::parse(contents.as_bytes()) {
                        // `types-publisher` sometimes emits `"typings": null` for
                        // packages that do not provide their own types.
                        is_not_needed_package = fields.path.typings.is_null();
                    }
                }
                if !is_not_needed_package {
                    let base_file_name = tspath::get_base_file_name(&normalized);
                    if !base_file_name.starts_with('.') {
                        wildcard_matches.push(base_file_name);
                    }
                }
            }
        }
    }

    let mut result: Vec<String> = Vec::new();
    for t in &options.types {
        if t == "*" {
            result.extend(wildcard_matches.iter().cloned());
        } else {
            result.push(t.clone());
        }
    }
    tsgo_core::deduplicate(&result)
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
