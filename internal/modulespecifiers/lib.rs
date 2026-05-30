//! `tsgo_modulespecifiers` — 1:1 Rust port of Go `internal/modulespecifiers`.
//!
//! The "reverse" of module resolution: given a target file (the file declaring
//! a symbol to import) and an importing file, compute the best module specifier
//! string — a relative path, a `paths` alias, a `node_modules` package name, an
//! `exports` subpath, or an ambient module name — ranked by the user's
//! relative/ending preferences and de-duplicated. Drives auto-imports,
//! quick-fixes, and `.d.ts` emit.

mod compare;
mod preferences;
mod types;
mod util;

pub use compare::*;
pub use preferences::*;
pub use types::*;
pub use util::*;

#[cfg(test)]
mod test_support;

use tsgo_collections::OrderedMap;
use tsgo_core::compileroptions::{
    CompilerOptions, ModuleResolutionKind, ResolutionMode, RESOLUTION_MODE_COMMON_JS,
    RESOLUTION_MODE_ESM, RESOLUTION_MODE_NONE,
};
use tsgo_core::pattern::Pattern;
use tsgo_core::{every, index_after, map, some};
use tsgo_module::{
    get_conditions, get_package_name_from_types_package_name, is_applicable_versioned_types_key,
    match_pattern_or_exact, try_get_js_extension_for_file, try_parse_patterns,
};
use tsgo_outputpaths::{
    get_output_declaration_file_name_worker, get_output_js_file_name_worker, OutputPathsHost,
};
use tsgo_packagejson::{ExportsOrImports, JsonValueType};
use tsgo_stringutil::{has_prefix, has_prefix_and_suffix_without_overlap, has_suffix};
use tsgo_tspath::{
    change_extension, change_full_extension, combine_paths, compare_paths, contains_path,
    ensure_trailing_directory_separator, file_extension_is_one_of,
    for_each_ancestor_directory_stopping_at_global_cache, get_base_file_name,
    get_declaration_file_extension, get_directory_path, get_normalized_absolute_path,
    get_relative_path_from_directory, has_implementation_ts_file_extension, has_ts_file_extension,
    is_declaration_file_name, normalize_path, path_is_relative, remove_extension,
    remove_file_extension, remove_trailing_directory_separator, resolve_path,
    starts_with_directory, to_path, try_get_extension_from_path, ComparePathsOptions,
    EXTENSIONS_NOT_SUPPORTING_EXTENSIONLESS_RESOLUTION, EXTENSION_CJS, EXTENSION_CTS,
    EXTENSION_DCTS, EXTENSION_DMTS, EXTENSION_DTS, EXTENSION_JSON, EXTENSION_MJS, EXTENSION_MTS,
    EXTENSION_TS,
};

// `ModulePath`, `MatchingMode`, `ResultKind`, the host/source-file traits,
// `ModuleSpecifierPreferences`, `get_module_specifier_preferences`, etc. are all
// in scope via the `pub use {types,preferences,util,compare}::*` globs above.

/// Returns all equivalent file paths for the imported module, including
/// symlink alternatives and project-reference redirect targets.
///
/// When `prefer_symlinks` is set, symlink alternatives are listed before the
/// real (target) paths. Ignored paths (e.g. `node_modules/.pnpm`) are dropped
/// only when at least one non-ignored alternative exists, so the result is
/// never empty for a resolvable file.
///
/// # Examples
/// Requires a [`ModuleSpecifierGenerationHost`]; behavior is covered by
/// `lib_test.rs`.
///
/// Side effects: reads the host's symlink cache and project-reference data.
// Go: internal/modulespecifiers/specifiers.go:GetEachFileNameOfModule
pub fn get_each_file_name_of_module(
    importing_file_name: &str,
    imported_file_name: &str,
    host: &dyn ModuleSpecifierGenerationHost,
    prefer_symlinks: bool,
) -> Vec<ModulePath> {
    let cwd = host.get_current_directory();
    let use_case_sensitive = host.use_case_sensitive_file_names();
    let imported_path = to_path(imported_file_name, &cwd, use_case_sensitive);

    let mut reference_redirect = String::new();
    if let Some(output_and_reference) = host.get_project_reference_from_source(&imported_path) {
        if !output_and_reference.output_dts.is_empty() {
            reference_redirect = output_and_reference.output_dts;
        }
    }

    let redirects = host.get_redirect_targets(&imported_path);
    let mut imported_file_names: Vec<String> = Vec::with_capacity(2 + redirects.len());
    if !reference_redirect.is_empty() {
        imported_file_names.push(reference_redirect.clone());
    }
    imported_file_names.push(imported_file_name.to_string());
    imported_file_names.extend(redirects);
    let targets: Vec<String> = map(&imported_file_names, |f| {
        get_normalized_absolute_path(f, &cwd)
    });
    let mut should_filter_ignored_paths = !every(&targets, |p| contains_ignored_path(p));

    let mut results: Vec<ModulePath> = Vec::with_capacity(2);
    let push_target = |results: &mut Vec<ModulePath>, should_filter: bool| {
        for p in &targets {
            if !(should_filter && contains_ignored_path(p)) {
                results.push(ModulePath {
                    file_name: p.clone(),
                    is_in_node_modules: contains_node_modules(p),
                    is_redirect: reference_redirect == *p,
                });
            }
        }
    };

    if !prefer_symlinks {
        push_target(&mut results, should_filter_ignored_paths);
    }

    let full_imported_file_name = get_normalized_absolute_path(imported_file_name, &cwd);
    if let Some(symlink_cache) = host.get_symlink_cache() {
        for_each_ancestor_directory_stopping_at_global_cache::<bool>(
            &host.get_global_typings_cache_location(),
            &get_directory_path(&full_imported_file_name),
            |real_path_directory| {
                let key = to_path(real_path_directory, &cwd, use_case_sensitive)
                    .ensure_trailing_directory_separator();
                let (symlink_set, ok) = symlink_cache.directories_by_realpath().load(&key);
                if !ok {
                    return (false, false); // Continue to ancestor directory.
                }

                // Don't let a package globally import from itself.
                if starts_with_directory(
                    importing_file_name,
                    real_path_directory,
                    use_case_sensitive,
                ) {
                    return (false, true); // Stop: every ancestor also hits this.
                }

                for target in &targets {
                    if !starts_with_directory(target, real_path_directory, use_case_sensitive) {
                        continue;
                    }
                    let relative = get_relative_path_from_directory(
                        real_path_directory,
                        target,
                        &ComparePathsOptions {
                            use_case_sensitive_file_names: use_case_sensitive,
                            current_directory: cwd.clone(),
                        },
                    );
                    for symlink_directory in symlink_set.keys() {
                        let option = resolve_path(&symlink_directory, &[&relative]);
                        results.push(ModulePath {
                            is_in_node_modules: contains_node_modules(&option),
                            is_redirect: *target == reference_redirect,
                            file_name: option,
                        });
                        // Found a non-ignored symlink path, so ignored realpaths
                        // may now be rejected.
                        should_filter_ignored_paths = true;
                    }
                }

                (false, false)
            },
        );
    }

    if prefer_symlinks {
        push_target(&mut results, should_filter_ignored_paths);
    }

    results
}

/// Reports whether a path contains a `/node_modules/` segment.
///
/// # Examples
/// ```
/// use tsgo_modulespecifiers::contains_node_modules;
/// assert!(contains_node_modules("/p/node_modules/lodash/index.js"));
/// assert!(!contains_node_modules("/p/src/utils.ts"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/modulespecifiers/specifiers.go:ContainsNodeModules
pub fn contains_node_modules(s: &str) -> bool {
    s.contains("/node_modules/")
}

// Go: internal/modulespecifiers/specifiers.go:containsIgnoredPath
//
// Duplicates `tspath.ContainsIgnoredPath` for performance (matches Go).
fn contains_ignored_path(s: &str) -> bool {
    s.contains("/node_modules/.") || s.contains("/.git") || s.contains(".#")
}

/// Adapts a [`ModuleSpecifierGenerationHost`] to [`OutputPathsHost`] so the
/// `outputpaths` helpers can be invoked with the same host (Go relies on
/// structural interface satisfaction).
struct OutputPathsHostAdapter<'a>(&'a dyn ModuleSpecifierGenerationHost);

impl OutputPathsHost for OutputPathsHostAdapter<'_> {
    fn common_source_directory(&self) -> String {
        self.0.common_source_directory()
    }
    fn get_current_directory(&self) -> String {
        self.0.get_current_directory()
    }
    fn use_case_sensitive_file_names(&self) -> bool {
        self.0.use_case_sensitive_file_names()
    }
}

// Go: internal/modulespecifiers/specifiers.go:tryGetModuleNameFromExportsOrImports
#[allow(clippy::too_many_arguments)]
fn try_get_module_name_from_exports_or_imports(
    options: &CompilerOptions,
    host: &dyn ModuleSpecifierGenerationHost,
    target_file_path: &str,
    package_directory: &str,
    package_name: &str,
    exports: &ExportsOrImports,
    conditions: &[String],
    mode: MatchingMode,
    is_imports: bool,
    prefer_ts_extension: bool,
) -> String {
    match exports.value_type() {
        JsonValueType::NotPresent | JsonValueType::Null => String::new(),
        JsonValueType::String => {
            let str_value = exports.as_str();

            // NOTE(port): like Go, this always uses the host project's
            // compiler options, not those of the targeted package.json.
            let mut output_file = String::new();
            let mut declaration_file = String::new();
            if is_imports {
                let adapter = OutputPathsHostAdapter(host);
                output_file = get_output_js_file_name_worker(target_file_path, options, &adapter);
                declaration_file =
                    get_output_declaration_file_name_worker(target_file_path, options, &adapter);
            }

            let path_or_pattern =
                get_normalized_absolute_path(&combine_paths(package_directory, &[str_value]), "");
            let extension_swapped_target = if has_ts_file_extension(target_file_path) {
                format!(
                    "{}{}",
                    remove_file_extension(target_file_path),
                    try_get_js_extension_for_file(target_file_path, options)
                )
            } else {
                String::new()
            };
            let can_try_ts_extension =
                prefer_ts_extension && has_implementation_ts_file_extension(target_file_path);

            let compare_opts = ComparePathsOptions {
                use_case_sensitive_file_names: host.use_case_sensitive_file_names(),
                current_directory: host.get_current_directory(),
            };

            match mode {
                MatchingMode::Exact => {
                    if (!extension_swapped_target.is_empty()
                        && compare_paths(
                            &extension_swapped_target,
                            &path_or_pattern,
                            &compare_opts,
                        ) == std::cmp::Ordering::Equal)
                        || compare_paths(target_file_path, &path_or_pattern, &compare_opts)
                            == std::cmp::Ordering::Equal
                        || (!output_file.is_empty()
                            && compare_paths(&output_file, &path_or_pattern, &compare_opts)
                                == std::cmp::Ordering::Equal)
                        || (!declaration_file.is_empty()
                            && compare_paths(&declaration_file, &path_or_pattern, &compare_opts)
                                == std::cmp::Ordering::Equal)
                    {
                        return package_name.to_string();
                    }
                }
                MatchingMode::Directory => {
                    if can_try_ts_extension
                        && contains_path(&path_or_pattern, target_file_path, &compare_opts)
                    {
                        let fragment = get_relative_path_from_directory(
                            &path_or_pattern,
                            target_file_path,
                            &compare_opts,
                        );
                        return get_normalized_absolute_path(
                            &combine_paths(
                                &combine_paths(package_name, &[str_value]),
                                &[&fragment],
                            ),
                            "",
                        );
                    }
                    if !extension_swapped_target.is_empty()
                        && contains_path(&path_or_pattern, &extension_swapped_target, &compare_opts)
                    {
                        let fragment = get_relative_path_from_directory(
                            &path_or_pattern,
                            &extension_swapped_target,
                            &compare_opts,
                        );
                        return get_normalized_absolute_path(
                            &combine_paths(
                                &combine_paths(package_name, &[str_value]),
                                &[&fragment],
                            ),
                            "",
                        );
                    }
                    if !can_try_ts_extension
                        && contains_path(&path_or_pattern, target_file_path, &compare_opts)
                    {
                        let fragment = get_relative_path_from_directory(
                            &path_or_pattern,
                            target_file_path,
                            &compare_opts,
                        );
                        return get_normalized_absolute_path(
                            &combine_paths(
                                &combine_paths(package_name, &[str_value]),
                                &[&fragment],
                            ),
                            "",
                        );
                    }
                    if !output_file.is_empty()
                        && contains_path(&path_or_pattern, &output_file, &compare_opts)
                    {
                        let fragment = get_relative_path_from_directory(
                            &path_or_pattern,
                            &output_file,
                            &compare_opts,
                        );
                        return combine_paths(package_name, &[&fragment]);
                    }
                    if !declaration_file.is_empty()
                        && contains_path(&path_or_pattern, &declaration_file, &compare_opts)
                    {
                        let fragment = get_relative_path_from_directory(
                            &path_or_pattern,
                            &declaration_file,
                            &compare_opts,
                        );
                        let js_extension = get_js_extension_for_file(&declaration_file, options);
                        let fragment_with_js_extension = change_extension(&fragment, &js_extension);
                        return combine_paths(package_name, &[&fragment_with_js_extension]);
                    }
                }
                MatchingMode::Pattern => {
                    let (leading_slice, trailing_slice) = path_or_pattern
                        .split_once('*')
                        .unwrap_or((&path_or_pattern, ""));
                    let case_sensitive = host.use_case_sensitive_file_names();
                    if can_try_ts_extension
                        && has_prefix_and_suffix_without_overlap(
                            target_file_path,
                            leading_slice,
                            trailing_slice,
                            case_sensitive,
                        )
                    {
                        let star_replacement = &target_file_path
                            [leading_slice.len()..target_file_path.len() - trailing_slice.len()];
                        return replace_first_star(package_name, star_replacement);
                    }
                    if !extension_swapped_target.is_empty()
                        && has_prefix_and_suffix_without_overlap(
                            &extension_swapped_target,
                            leading_slice,
                            trailing_slice,
                            case_sensitive,
                        )
                    {
                        let star_replacement = &extension_swapped_target[leading_slice.len()
                            ..extension_swapped_target.len() - trailing_slice.len()];
                        return replace_first_star(package_name, star_replacement);
                    }
                    if !can_try_ts_extension
                        && has_prefix_and_suffix_without_overlap(
                            target_file_path,
                            leading_slice,
                            trailing_slice,
                            case_sensitive,
                        )
                    {
                        let star_replacement = &target_file_path
                            [leading_slice.len()..target_file_path.len() - trailing_slice.len()];
                        return replace_first_star(package_name, star_replacement);
                    }
                    if !output_file.is_empty()
                        && has_prefix_and_suffix_without_overlap(
                            &output_file,
                            leading_slice,
                            trailing_slice,
                            case_sensitive,
                        )
                    {
                        let star_replacement = &output_file
                            [leading_slice.len()..output_file.len() - trailing_slice.len()];
                        return replace_first_star(package_name, star_replacement);
                    }
                    if !declaration_file.is_empty()
                        && has_prefix_and_suffix_without_overlap(
                            &declaration_file,
                            leading_slice,
                            trailing_slice,
                            case_sensitive,
                        )
                    {
                        let star_replacement = &declaration_file
                            [leading_slice.len()..declaration_file.len() - trailing_slice.len()];
                        let substituted = replace_first_star(package_name, star_replacement);
                        let js_extension =
                            try_get_js_extension_for_file(&declaration_file, options);
                        if !js_extension.is_empty() {
                            return change_full_extension(&substituted, js_extension);
                        }
                    }
                }
            }
            String::new()
        }
        JsonValueType::Array => {
            for e in exports.as_array() {
                let result = try_get_module_name_from_exports_or_imports(
                    options,
                    host,
                    target_file_path,
                    package_directory,
                    package_name,
                    e,
                    conditions,
                    mode,
                    is_imports,
                    prefer_ts_extension,
                );
                if !result.is_empty() {
                    return result;
                }
            }
            String::new()
        }
        JsonValueType::Object => {
            // Conditional mapping.
            for (key, value) in exports.as_object().entries() {
                if key == "default"
                    || conditions.iter().any(|c| c == key)
                    || (conditions.iter().any(|c| c == "types")
                        && is_applicable_versioned_types_key(key))
                {
                    let result = try_get_module_name_from_exports_or_imports(
                        options,
                        host,
                        target_file_path,
                        package_directory,
                        package_name,
                        value,
                        conditions,
                        mode,
                        is_imports,
                        prefer_ts_extension,
                    );
                    if !result.is_empty() {
                        return result;
                    }
                }
            }
            String::new()
        }
        JsonValueType::Number | JsonValueType::Boolean => String::new(),
    }
}

/// Cached per-call facts about the importing file.
///
/// Side effects: none (plain data).
// Go: internal/modulespecifiers/specifiers.go:Info
#[derive(Clone, Debug)]
pub struct Info {
    /// Whether file names are compared case-sensitively.
    pub use_case_sensitive_file_names: bool,
    /// The importing source file's name.
    pub importing_source_file_name: String,
    /// The directory containing the importing source file.
    pub source_directory: String,
}

// Go: internal/modulespecifiers/specifiers.go:getInfo
pub(crate) fn get_info(
    importing_source_file_name: &str,
    host: &dyn ModuleSpecifierGenerationHost,
) -> Info {
    Info {
        importing_source_file_name: importing_source_file_name.to_string(),
        source_directory: get_directory_path(importing_source_file_name),
        use_case_sensitive_file_names: host.use_case_sensitive_file_names(),
    }
}

// Go: internal/modulespecifiers/specifiers.go:getAllModulePaths
//
// Currently only reached by the AST-blocked entry points
// (`GetModuleSpecifier`/`UpdateModuleSpecifier`/`GetNodeModulesPackageName`),
// which are deferred until `tsgo_ast` ports `SourceFile`.
// DEFER(phase-checker): blocked-by: tsgo_ast SourceFile-based public entry points.
#[allow(dead_code)]
pub(crate) fn get_all_module_paths(
    info: &Info,
    imported_file_name: &str,
    host: &dyn ModuleSpecifierGenerationHost,
    compiler_options: &CompilerOptions,
    options: ModuleSpecifierOptions,
) -> Vec<ModulePath> {
    // NOTE(port): the module-specifier cache is not yet modelled (matches the
    // commented-out cache in Go); this simply delegates to the worker.
    get_all_module_paths_worker(info, imported_file_name, host, compiler_options, options)
}

// Go: internal/modulespecifiers/specifiers.go:getAllModulePathsWorker
pub(crate) fn get_all_module_paths_worker(
    info: &Info,
    imported_file_name: &str,
    host: &dyn ModuleSpecifierGenerationHost,
    _compiler_options: &CompilerOptions,
    _options: ModuleSpecifierOptions,
) -> Vec<ModulePath> {
    let paths = get_each_file_name_of_module(
        &info.importing_source_file_name,
        imported_file_name,
        host,
        true,
    );
    let mut all_file_names: std::collections::HashMap<String, ModulePath> =
        std::collections::HashMap::with_capacity(paths.len());
    for p in &paths {
        all_file_names.insert(p.file_name.clone(), p.clone());
    }

    let use_case_sensitive = info.use_case_sensitive_file_names;

    // Sort by paths closest to the importing file's directory.
    let mut sorted_paths: Vec<ModulePath> = Vec::with_capacity(paths.len());
    let mut directory = info.source_directory.clone();
    while !all_file_names.is_empty() {
        let directory_start = ensure_trailing_directory_separator(&directory);
        let mut paths_in_directory: Vec<ModulePath> = Vec::new();
        let keys: Vec<String> = all_file_names.keys().cloned().collect();
        for file_name in keys {
            if file_name.starts_with(&directory_start) {
                if let Some(p) = all_file_names.remove(&file_name) {
                    paths_in_directory.push(p);
                }
            }
        }
        if !paths_in_directory.is_empty() {
            paths_in_directory.sort_by(|a, b| compare_paths_by_redirect(a, b, use_case_sensitive));
            sorted_paths.append(&mut paths_in_directory);
        }
        let new_directory = get_directory_path(&directory);
        if new_directory == directory {
            break;
        }
        directory = new_directory;
    }
    if !all_file_names.is_empty() {
        let mut remaining: Vec<ModulePath> = all_file_names.into_values().collect();
        remaining.sort_by(|a, b| compare_paths_by_redirect(a, b, use_case_sensitive));
        sorted_paths.append(&mut remaining);
    }
    sorted_paths
}

/// Returns the index of `ending` in `endings`, or `-1` if absent (mirrors Go's
/// `slices.Index` for arithmetic comparison).
fn ending_index(endings: &[ModuleSpecifierEnding], ending: ModuleSpecifierEnding) -> i32 {
    endings
        .iter()
        .position(|&e| e == ending)
        .map_or(-1, |i| i as i32)
}

// Go: internal/modulespecifiers/specifiers.go:processEnding
fn process_ending(
    file_name: &str,
    allowed_endings: &[ModuleSpecifierEnding],
    options: &CompilerOptions,
    host: &dyn ModuleSpecifierGenerationHost,
) -> String {
    if file_extension_is_one_of(file_name, &[EXTENSION_JSON, EXTENSION_MJS, EXTENSION_CJS]) {
        return file_name.to_string();
    }

    let no_extension = remove_file_extension(file_name);
    if file_name == no_extension {
        return file_name.to_string();
    }

    let js_priority = ending_index(allowed_endings, ModuleSpecifierEnding::JsExtension);
    let ts_priority = ending_index(allowed_endings, ModuleSpecifierEnding::TsExtension);
    if file_extension_is_one_of(file_name, &[EXTENSION_MTS, EXTENSION_CTS])
        && ts_priority != -1
        && ts_priority < js_priority
    {
        return file_name.to_string();
    }
    if file_extension_is_one_of(file_name, &[EXTENSION_DMTS, EXTENSION_DCTS]) {
        let input_ext = get_declaration_file_extension(file_name);
        let ext = get_js_extension_for_declaration_file_extension(&input_ext);
        return format!("{}{}", remove_extension(file_name, &input_ext), ext);
    }
    if file_extension_is_one_of(file_name, &[EXTENSION_MTS, EXTENSION_CTS]) {
        return format!(
            "{}{}",
            no_extension,
            get_js_extension_for_file(file_name, options)
        );
    }
    if !file_extension_is_one_of(file_name, &[EXTENSION_DTS])
        && file_extension_is_one_of(file_name, &[EXTENSION_TS])
        && file_name.contains(".d.")
    {
        // `foo.d.json.ts` and the like - remap back to `foo.json`.
        let result = try_get_real_file_name_for_non_js_declaration_file_name(file_name);
        if !result.is_empty() {
            return result;
        }
    }

    match allowed_endings[0] {
        ModuleSpecifierEnding::Minimal => {
            let without_index = no_extension.strip_suffix("/index").unwrap_or(no_extension);
            if without_index != no_extension && try_get_any_file_from_path(host, without_index) {
                // Can't remove `/index` if a file shares the directory's name.
                return no_extension.to_string();
            }
            without_index.to_string()
        }
        ModuleSpecifierEnding::Index => no_extension.to_string(),
        ModuleSpecifierEnding::JsExtension => {
            format!(
                "{}{}",
                no_extension,
                get_js_extension_for_file(file_name, options)
            )
        }
        ModuleSpecifierEnding::TsExtension => {
            // We don't yet know if this import is type-only, so a `.d.ts`
            // extension may be invalid; use no extension or a `.js` extension.
            if is_declaration_file_name(file_name) {
                let mut extensionless_priority = -1i32;
                for (i, e) in allowed_endings.iter().enumerate() {
                    if *e == ModuleSpecifierEnding::Minimal || *e == ModuleSpecifierEnding::Index {
                        extensionless_priority = i as i32;
                        break;
                    }
                }
                if extensionless_priority != -1 && extensionless_priority < js_priority {
                    return no_extension.to_string();
                }
                return format!(
                    "{}{}",
                    no_extension,
                    get_js_extension_for_file(file_name, options)
                );
            }
            file_name.to_string()
        }
    }
}

// Go: internal/modulespecifiers/specifiers.go:tryGetModuleNameFromRootDirs
fn try_get_module_name_from_root_dirs(
    root_dirs: &[String],
    module_file_name: &str,
    source_directory: &str,
    allowed_endings: &[ModuleSpecifierEnding],
    compiler_options: &CompilerOptions,
    host: &dyn ModuleSpecifierGenerationHost,
) -> String {
    let use_case_sensitive = host.use_case_sensitive_file_names();
    let normalized_target_paths =
        get_paths_relative_to_root_dirs(module_file_name, root_dirs, use_case_sensitive);
    if normalized_target_paths.is_empty() {
        return String::new();
    }

    let normalized_source_paths =
        get_paths_relative_to_root_dirs(source_directory, root_dirs, use_case_sensitive);
    let mut shortest = String::new();
    let mut shortest_sep_count = 0usize;
    for source_path in &normalized_source_paths {
        for target_path in &normalized_target_paths {
            let candidate = ensure_path_is_non_module_name(&get_relative_path_from_directory(
                source_path,
                target_path,
                &ComparePathsOptions {
                    use_case_sensitive_file_names: use_case_sensitive,
                    current_directory: host.get_current_directory(),
                },
            ));
            let candidate_sep_count = candidate.matches('/').count();
            if shortest.is_empty() || candidate_sep_count < shortest_sep_count {
                shortest = candidate;
                shortest_sep_count = candidate_sep_count;
            }
        }
    }

    if shortest.is_empty() {
        return String::new();
    }
    process_ending(&shortest, allowed_endings, compiler_options, host)
}

// Go: internal/modulespecifiers/specifiers.go:specPair
struct SpecPair {
    ending: ModuleSpecifierEnding,
    value: String,
}

// Go: internal/modulespecifiers/specifiers.go:tryGetModuleNameFromPaths
fn try_get_module_name_from_paths(
    relative_to_base_url: &str,
    paths: &OrderedMap<String, Vec<String>>,
    allowed_endings: &[ModuleSpecifierEnding],
    base_directory: &str,
    host: &dyn ModuleSpecifierGenerationHost,
    compiler_options: &CompilerOptions,
) -> String {
    let case_sensitive = host.use_case_sensitive_file_names();
    for (key, values) in paths.entries() {
        for pattern_text in values {
            let normalized = normalize_path(pattern_text);
            let mut pattern =
                get_relative_path_if_in_same_volume(&normalized, base_directory, case_sensitive);
            if pattern.is_empty() {
                pattern = normalized;
            }
            let (prefix, suffix, ok) = match pattern.split_once('*') {
                Some((p, s)) => (p.to_string(), s.to_string(), true),
                None => (pattern.clone(), String::new(), false),
            };

            // See the Go source for the extension/`*`-substitution rationale.
            let mut candidates: Vec<SpecPair> = Vec::new();
            for ending in allowed_endings {
                let result =
                    process_ending(relative_to_base_url, &[*ending], compiler_options, host);
                candidates.push(SpecPair {
                    ending: *ending,
                    value: result,
                });
            }
            if !try_get_extension_from_path(&pattern).is_empty() {
                candidates.push(SpecPair {
                    ending: ModuleSpecifierEnding::JsExtension,
                    value: relative_to_base_url.to_string(),
                });
            }

            if ok {
                for c in &candidates {
                    let value = &c.value;
                    if value.len() >= prefix.len() + suffix.len()
                        && has_prefix(value, &prefix, case_sensitive)
                        && has_suffix(value, &suffix, case_sensitive)
                        && validate_ending(c, relative_to_base_url, compiler_options, host)
                    {
                        let matched_star = &value[prefix.len()..value.len() - suffix.len()];
                        if !path_is_relative(matched_star) {
                            return replace_first_star(key, matched_star);
                        }
                    }
                }
            } else if some(&candidates, |c| {
                c.ending != ModuleSpecifierEnding::Minimal && pattern == c.value
            }) || some(&candidates, |c| {
                c.ending == ModuleSpecifierEnding::Minimal
                    && pattern == c.value
                    && validate_ending(c, relative_to_base_url, compiler_options, host)
            }) {
                return key.clone();
            }
        }
    }
    String::new()
}

// Go: internal/modulespecifiers/specifiers.go:validateEnding
fn validate_ending(
    c: &SpecPair,
    relative_to_base_url: &str,
    compiler_options: &CompilerOptions,
    host: &dyn ModuleSpecifierGenerationHost,
) -> bool {
    c.ending != ModuleSpecifierEnding::Minimal
        || c.value == process_ending(relative_to_base_url, &[c.ending], compiler_options, host)
}

// Go: internal/modulespecifiers/specifiers.go:tryGetModuleNameFromExports
fn try_get_module_name_from_exports(
    options: &CompilerOptions,
    host: &dyn ModuleSpecifierGenerationHost,
    target_file_path: &str,
    package_directory: &str,
    package_name: &str,
    exports: &ExportsOrImports,
    conditions: &[String],
) -> String {
    if exports.is_subpaths() {
        // sub-mappings: directory (`/`), pattern (`*`), or exact.
        for (k, subk) in exports.as_object().entries() {
            let sub_package_name =
                get_normalized_absolute_path(&combine_paths(package_name, &[k.as_str()]), "");
            let mode = if k.ends_with('/') {
                MatchingMode::Directory
            } else if k.contains('*') {
                MatchingMode::Pattern
            } else {
                MatchingMode::Exact
            };
            let result = try_get_module_name_from_exports_or_imports(
                options,
                host,
                target_file_path,
                package_directory,
                &sub_package_name,
                subk,
                conditions,
                mode,
                false,
                false,
            );
            if !result.is_empty() {
                return result;
            }
        }
    }
    try_get_module_name_from_exports_or_imports(
        options,
        host,
        target_file_path,
        package_directory,
        package_name,
        exports,
        conditions,
        MatchingMode::Exact,
        false,
        false,
    )
}

// Go: internal/modulespecifiers/specifiers.go:tryGetModuleNameFromPackageJsonImports
fn try_get_module_name_from_package_json_imports(
    module_file_name: &str,
    source_directory: &str,
    options: &CompilerOptions,
    host: &dyn ModuleSpecifierGenerationHost,
    import_mode: ResolutionMode,
    prefer_ts_extension: bool,
) -> String {
    if !options.get_resolve_package_json_imports() {
        return String::new();
    }

    let ancestor_directory_with_package_json =
        host.get_nearest_ancestor_directory_with_package_json(source_directory);
    if ancestor_directory_with_package_json.is_empty() {
        return String::new();
    }
    let package_json_path = combine_paths(&ancestor_directory_with_package_json, &["package.json"]);

    let Some(info) = host.get_package_json_info(&package_json_path) else {
        return String::new();
    };
    // Go dereferences `GetContents()` directly; an absent contents is treated
    // as no imports here (safer than panicking).
    let Some(contents) = info.get_contents() else {
        return String::new();
    };

    let imports = &contents.fields().path.imports;
    match imports.value_type() {
        JsonValueType::NotPresent | JsonValueType::Array | JsonValueType::String => String::new(),
        JsonValueType::Object => {
            let conditions = get_conditions(options, import_mode);
            for (k, value) in imports.as_object().entries() {
                if k == "#" || k == "#/" || !k.starts_with('#') {
                    continue; // invalid imports entry
                }
                if k.starts_with("#/")
                    && options.get_module_resolution_kind() != ModuleResolutionKind::NodeNext
                    && options.get_module_resolution_kind() != ModuleResolutionKind::Bundler
                {
                    continue; // "#/" keys only valid under nodenext/bundler
                }
                let mode = if k.ends_with('/') {
                    MatchingMode::Directory
                } else if k.contains('*') {
                    MatchingMode::Pattern
                } else {
                    MatchingMode::Exact
                };
                let result = try_get_module_name_from_exports_or_imports(
                    options,
                    host,
                    module_file_name,
                    &ancestor_directory_with_package_json,
                    k,
                    value,
                    &conditions,
                    mode,
                    true,
                    prefer_ts_extension,
                );
                if !result.is_empty() {
                    return result;
                }
            }
            String::new()
        }
        JsonValueType::Null | JsonValueType::Number | JsonValueType::Boolean => String::new(),
    }
}

// Go: internal/modulespecifiers/specifiers.go:pkgJsonDirAttemptResult
#[derive(Default)]
struct PkgJsonDirAttemptResult {
    module_file_to_try: String,
    package_root_path: String,
    blocked_by_exports: bool,
    verbatim_from_exports: bool,
}

// Go: internal/modulespecifiers/specifiers.go:tryDirectoryWithPackageJson
fn try_directory_with_package_json(
    parts: &NodeModulePathParts,
    path_obj: &ModulePath,
    importing_source_file: &dyn SourceFileForSpecifierGeneration,
    host: &dyn ModuleSpecifierGenerationHost,
    override_mode: ResolutionMode,
    options: &CompilerOptions,
    allowed_endings: &[ModuleSpecifierEnding],
) -> PkgJsonDirAttemptResult {
    let mut root_idx = parts.package_root_index;
    if root_idx == -1 {
        // js slice semantics differ; mirror Go's fallback to the full length.
        root_idx = path_obj.file_name.len() as i32;
    }
    let package_root_path = path_obj.file_name[..root_idx as usize].to_string();
    let package_json_path = combine_paths(&package_root_path, &["package.json"]);
    let mut module_file_to_try = path_obj.file_name.clone();
    let mut maybe_blocked_by_types_versions = false;

    let Some(package_json) = host.get_package_json_info(&package_json_path) else {
        // No package.json exists; an index.js will still resolve as the package name.
        let file_name = &path_obj.file_name[(parts.package_root_index + 1) as usize..];
        if file_name == "index.d.ts"
            || file_name == "index.js"
            || file_name == "index.ts"
            || file_name == "index.tsx"
        {
            return PkgJsonDirAttemptResult {
                module_file_to_try,
                package_root_path,
                ..Default::default()
            };
        }
        return PkgJsonDirAttemptResult {
            module_file_to_try,
            ..Default::default()
        };
    };

    let mut import_mode = override_mode;
    if import_mode == RESOLUTION_MODE_NONE {
        import_mode = host.get_default_resolution_mode_for_file(importing_source_file);
    }

    let package_json_content = package_json.get_contents();
    if options.get_resolve_package_json_exports() {
        // Use the actual node_modules directory name, not `package.json` `name`.
        let node_modules_directory_name =
            &package_root_path[(parts.top_level_package_name_index + 1) as usize..];
        let package_name = get_package_name_from_types_package_name(node_modules_directory_name);

        // Determine resolution mode for exports condition matching by the
        // target file's extension (see Go comment).
        if file_extension_is_one_of(
            &path_obj.file_name,
            &[EXTENSION_CJS, EXTENSION_CTS, EXTENSION_DCTS],
        ) {
            import_mode = RESOLUTION_MODE_COMMON_JS;
        } else if file_extension_is_one_of(
            &path_obj.file_name,
            &[EXTENSION_MJS, EXTENSION_MTS, EXTENSION_DMTS],
        ) {
            import_mode = RESOLUTION_MODE_ESM;
        }

        let conditions = get_conditions(options, import_mode);

        let mut from_exports = String::new();
        if let Some(content) = package_json_content {
            if content.fields().path.exports.value_type() != JsonValueType::NotPresent {
                from_exports = try_get_module_name_from_exports(
                    options,
                    host,
                    &path_obj.file_name,
                    &package_root_path,
                    &package_name,
                    &content.fields().path.exports,
                    &conditions,
                );
            }
        }
        if !from_exports.is_empty() {
            return PkgJsonDirAttemptResult {
                module_file_to_try: from_exports,
                verbatim_from_exports: true,
                ..Default::default()
            };
        }
        if let Some(content) = package_json_content {
            if content.fields().path.exports.value_type() != JsonValueType::NotPresent {
                return PkgJsonDirAttemptResult {
                    module_file_to_try: path_obj.file_name.clone(),
                    blocked_by_exports: true,
                    ..Default::default()
                };
            }
        }
    }

    let version_paths = package_json_content.and_then(|content| {
        if content.fields().path.types_versions.value_type() == JsonValueType::Object {
            Some(content.get_version_paths(None))
        } else {
            None
        }
    });
    let version_paths_paths = version_paths.and_then(|vp| vp.get_paths());
    if let Some(paths_map) = version_paths_paths {
        let sub_module_name = &path_obj.file_name[package_root_path.len() + 1..];
        let from_paths = try_get_module_name_from_paths(
            sub_module_name,
            paths_map,
            allowed_endings,
            &package_root_path,
            host,
            options,
        );
        if from_paths.is_empty() {
            maybe_blocked_by_types_versions = true;
        } else {
            module_file_to_try = combine_paths(&package_root_path, &[&from_paths]);
        }
    }

    // If the file is the main module, it can be imported by the package name.
    let mut main_file_relative = "index.js".to_string();
    if let Some(content) = package_json_content {
        let (typings, typings_ok) = content.fields().path.typings.get_value();
        let (types, types_ok) = content.fields().path.types.get_value();
        let (main, main_ok) = content.fields().path.main.get_value();
        if typings_ok {
            main_file_relative = typings.clone();
        } else if types_ok {
            main_file_relative = types.clone();
        } else if main_ok {
            main_file_relative = main.clone();
        }
    }

    let blocked_main = maybe_blocked_by_types_versions
        && match_pattern_or_exact(
            &try_parse_patterns(version_paths_paths),
            &main_file_relative,
        ) != Pattern::default();
    if !main_file_relative.is_empty() && !blocked_main {
        let main_export_file = to_path(
            &main_file_relative,
            &package_root_path,
            host.use_case_sensitive_file_names(),
        );
        let compare_opt = ComparePathsOptions {
            use_case_sensitive_file_names: host.use_case_sensitive_file_names(),
            current_directory: host.get_current_directory(),
        };
        if compare_paths(
            remove_file_extension(main_export_file.as_str()),
            remove_file_extension(&module_file_to_try),
            &compare_opt,
        ) == std::cmp::Ordering::Equal
        {
            // An arbitrary extension removal for this comparison (matches Go).
            return PkgJsonDirAttemptResult {
                package_root_path,
                module_file_to_try,
                ..Default::default()
            };
        }
        let matches_index_directory = match package_json_content {
            None => true,
            Some(content) => {
                let (type_value, _) = content.fields().header.type_.get_value();
                type_value != "module"
                    && !file_extension_is_one_of(
                        &module_file_to_try,
                        EXTENSIONS_NOT_SUPPORTING_EXTENSIONLESS_RESOLUTION,
                    )
                    && has_prefix(
                        &module_file_to_try,
                        main_export_file.as_str(),
                        host.use_case_sensitive_file_names(),
                    )
                    && compare_paths(
                        &get_directory_path(&module_file_to_try),
                        remove_trailing_directory_separator(main_export_file.as_str()),
                        &compare_opt,
                    ) == std::cmp::Ordering::Equal
                    && remove_file_extension(&get_base_file_name(&module_file_to_try)) == "index"
            }
        };
        if matches_index_directory {
            return PkgJsonDirAttemptResult {
                package_root_path,
                module_file_to_try,
                ..Default::default()
            };
        }
    }

    PkgJsonDirAttemptResult {
        module_file_to_try,
        ..Default::default()
    }
}

// Go: internal/modulespecifiers/specifiers.go:tryGetModuleNameAsNodeModule
#[allow(clippy::too_many_arguments)]
fn try_get_module_name_as_node_module(
    path_obj: &ModulePath,
    info: &Info,
    importing_source_file: &dyn SourceFileForSpecifierGeneration,
    host: &dyn ModuleSpecifierGenerationHost,
    options: &CompilerOptions,
    user_preferences: &UserPreferences,
    package_name_only: bool,
    override_mode: ResolutionMode,
) -> String {
    let Some(parts) = get_node_module_path_parts(&path_obj.file_name) else {
        return String::new();
    };

    // Simplify the full file path to something Node can resolve.
    let preferences = get_module_specifier_preferences(
        user_preferences,
        host,
        options,
        importing_source_file,
        "",
    );
    let allowed_endings = preferences.get_allowed_endings_in_preferred_order(RESOLUTION_MODE_NONE);

    let case_sensitive = host.use_case_sensitive_file_names();
    let mut module_specifier = path_obj.file_name.clone();
    let mut is_package_root_path = false;
    if !package_name_only {
        let mut package_root_index = parts.package_root_index;
        let mut module_file_name = String::new();
        loop {
            // If the module could be imported by a directory name, use that.
            let pkg_json_results = try_directory_with_package_json(
                &parts,
                path_obj,
                importing_source_file,
                host,
                override_mode,
                options,
                &allowed_endings,
            );
            if pkg_json_results.blocked_by_exports {
                // Under this package.json but not publicly exported.
                return String::new();
            }
            if pkg_json_results.verbatim_from_exports {
                return pkg_json_results.module_file_to_try;
            }
            if !pkg_json_results.package_root_path.is_empty() {
                module_specifier = pkg_json_results.package_root_path;
                is_package_root_path = true;
                break;
            }
            if module_file_name.is_empty() {
                module_file_name = pkg_json_results.module_file_to_try;
            }
            // Try with the next level of directory.
            package_root_index =
                index_after(&path_obj.file_name, "/", (package_root_index + 1) as usize);
            if package_root_index == -1 {
                module_specifier =
                    process_ending(&module_file_name, &allowed_endings, options, host);
                break;
            }
        }
    }

    if path_obj.is_redirect && !is_package_root_path {
        return String::new();
    }

    let global_typings_cache_location = host.get_global_typings_cache_location();
    // Get a path relative to the top-level node_modules (or the importing file).
    let path_to_top_level_node_modules =
        module_specifier[..parts.top_level_node_modules_index as usize].to_string();

    if !has_prefix(
        &info.source_directory,
        &path_to_top_level_node_modules,
        case_sensitive,
    ) || (!global_typings_cache_location.is_empty()
        && has_prefix(
            &global_typings_cache_location,
            &path_to_top_level_node_modules,
            case_sensitive,
        ))
    {
        return String::new();
    }

    // If the module was found in @types, get the actual Node package name.
    let node_modules_directory_name =
        &module_specifier[(parts.top_level_package_name_index + 1) as usize..];
    get_package_name_from_types_package_name(node_modules_directory_name)
}

// Go: internal/modulespecifiers/specifiers.go:getLocalModuleSpecifier
fn get_local_module_specifier(
    module_file_name: &str,
    info: &Info,
    compiler_options: &CompilerOptions,
    host: &dyn ModuleSpecifierGenerationHost,
    import_mode: ResolutionMode,
    preferences: &ModuleSpecifierPreferences,
    paths_only: bool,
) -> String {
    let paths = compiler_options.paths.as_ref();
    let root_dirs = &compiler_options.root_dirs;

    if paths_only && paths.is_none() {
        return String::new();
    }

    let source_directory = &info.source_directory;
    let allowed_endings = preferences.get_allowed_endings_in_preferred_order(import_mode);
    let mut relative_path = String::new();
    if !root_dirs.is_empty() {
        relative_path = try_get_module_name_from_root_dirs(
            root_dirs,
            module_file_name,
            source_directory,
            &allowed_endings,
            compiler_options,
            host,
        );
    }
    if relative_path.is_empty() {
        relative_path = process_ending(
            &ensure_path_is_non_module_name(&get_relative_path_from_directory(
                source_directory,
                module_file_name,
                &ComparePathsOptions {
                    use_case_sensitive_file_names: host.use_case_sensitive_file_names(),
                    current_directory: host.get_current_directory(),
                },
            )),
            &allowed_endings,
            compiler_options,
            host,
        );
    }

    if (paths.is_none() && !compiler_options.get_resolve_package_json_imports())
        || preferences.relative_preference == RelativePreferenceKind::Relative
    {
        if paths_only {
            return String::new();
        }
        return relative_path;
    }

    let root = compiler_options.get_paths_base_path(&host.get_current_directory());
    let base_directory = get_normalized_absolute_path(&root, &host.get_current_directory());
    let relative_to_base_url = get_relative_path_if_in_same_volume(
        module_file_name,
        &base_directory,
        host.use_case_sensitive_file_names(),
    );
    if relative_to_base_url.is_empty() {
        if paths_only {
            return String::new();
        }
        return relative_path;
    }

    let mut from_package_json_imports = String::new();
    if !paths_only {
        from_package_json_imports = try_get_module_name_from_package_json_imports(
            module_file_name,
            source_directory,
            compiler_options,
            host,
            import_mode,
            prefers_ts_extension(&allowed_endings),
        );
    }

    let mut from_paths = String::new();
    if paths_only || from_package_json_imports.is_empty() {
        if let Some(paths) = paths {
            from_paths = try_get_module_name_from_paths(
                &relative_to_base_url,
                paths,
                &allowed_endings,
                &base_directory,
                host,
                compiler_options,
            );
        }
    }

    if paths_only {
        return from_paths;
    }

    let maybe_non_relative = if !from_package_json_imports.is_empty() {
        from_package_json_imports
    } else {
        from_paths
    };
    if maybe_non_relative.is_empty() {
        return relative_path;
    }

    let relative_is_excluded = is_excluded_by_regex(&relative_path, &preferences.exclude_regexes);
    let non_relative_is_excluded =
        is_excluded_by_regex(&maybe_non_relative, &preferences.exclude_regexes);
    if !relative_is_excluded && non_relative_is_excluded {
        return relative_path;
    }
    if relative_is_excluded && !non_relative_is_excluded {
        return maybe_non_relative;
    }

    if preferences.relative_preference == RelativePreferenceKind::NonRelative
        && !path_is_relative(&maybe_non_relative)
    {
        return maybe_non_relative;
    }

    if preferences.relative_preference == RelativePreferenceKind::ExternalNonRelative
        && !path_is_relative(&maybe_non_relative)
    {
        let cwd = host.get_current_directory();
        let ucs = host.use_case_sensitive_file_names();
        let project_directory = if !compiler_options.config_file_path.is_empty() {
            to_path(
                &get_directory_path(&compiler_options.config_file_path),
                &cwd,
                ucs,
            )
        } else {
            to_path(&cwd, &cwd, ucs)
        };
        let canonical_source_directory = to_path(source_directory, &cwd, ucs);
        let module_path = to_path(module_file_name, project_directory.as_str(), ucs);

        let source_is_internal = project_directory.contains_path(&canonical_source_directory);
        let target_is_internal = project_directory.contains_path(&module_path);
        if (source_is_internal && !target_is_internal)
            || (!source_is_internal && target_is_internal)
        {
            // The import path crosses the tsconfig.json-containing directory.
            return maybe_non_relative;
        }

        let nearest_target_package_json = host.get_nearest_ancestor_directory_with_package_json(
            &get_directory_path(module_path.as_str()),
        );
        let nearest_source_package_json =
            host.get_nearest_ancestor_directory_with_package_json(source_directory);

        if !package_json_paths_are_equal(
            &nearest_target_package_json,
            &nearest_source_package_json,
            &ComparePathsOptions {
                use_case_sensitive_file_names: ucs,
                current_directory: cwd.clone(),
            },
        ) {
            // The importing and imported files are part of different packages.
            return maybe_non_relative;
        }

        return relative_path;
    }

    // Prefer a relative import over a baseUrl import with fewer components.
    if is_path_relative_to_parent(&maybe_non_relative)
        || count_path_components(&relative_path) < count_path_components(&maybe_non_relative)
    {
        return relative_path;
    }
    maybe_non_relative
}

// Go: internal/modulespecifiers/specifiers.go:computeModuleSpecifiers
fn compute_module_specifiers(
    module_paths: &[ModulePath],
    compiler_options: &CompilerOptions,
    importing_source_file: &dyn SourceFileForSpecifierGeneration,
    host: &dyn ModuleSpecifierGenerationHost,
    user_preferences: &UserPreferences,
    options: ModuleSpecifierOptions,
    for_auto_import: bool,
) -> (Vec<String>, ResultKind) {
    let info = get_info(&importing_source_file.file_name(), host);
    let preferences = get_module_specifier_preferences(
        user_preferences,
        host,
        compiler_options,
        importing_source_file,
        "",
    );

    // DEFER(phase-checker): the "reuse an existing import" loop needs the
    // importing file's `*ast.StringLiteralLike` import nodes and the host's
    // `GetResolvedModuleFromModuleSpecifier`/`GetModeForUsageLocation` (both
    // take those nodes). The AST node graph is not ported yet, so this
    // optimization is skipped; only pre-existing-import reuse is affected, not
    // the correctness of freshly generated specifiers.
    // blocked-by: tsgo_ast `StringLiteralLike` / `ModuleSpecifierGenerationHost`
    // node methods.

    let imported_file_is_in_node_modules = some(module_paths, |p| p.is_in_node_modules);

    // Specifier priority:
    //   1. Bare package specifiers (e.g. "@foo/bar") via a types entry.
    //   2. "paths" specifiers.
    //   3. Non-relative node_modules specifiers (e.g. "@foo/bar/path").
    //   4. Relative paths.
    let mut paths_specifiers: Vec<String> = Vec::new();
    let mut redirect_paths_specifiers: Vec<String> = Vec::new();
    let mut node_modules_specifiers: Vec<String> = Vec::new();
    let mut relative_specifiers: Vec<String> = Vec::new();

    for module_path in module_paths {
        let mut specifier = String::new();
        if module_path.is_in_node_modules {
            specifier = try_get_module_name_as_node_module(
                module_path,
                &info,
                importing_source_file,
                host,
                compiler_options,
                user_preferences,
                false,
                options.override_import_mode,
            );
        }
        let specifier_excluded =
            for_auto_import && is_excluded_by_regex(&specifier, &preferences.exclude_regexes);
        if !specifier.is_empty() && !specifier_excluded {
            node_modules_specifiers.push(specifier.clone());
            if module_path.is_redirect {
                // A redirect specifier is a bare package specifier; stop here.
                return (node_modules_specifiers, ResultKind::NodeModules);
            }
        }

        let mut import_mode = options.override_import_mode;
        if import_mode == RESOLUTION_MODE_NONE {
            import_mode = host.get_default_resolution_mode_for_file(importing_source_file);
        }
        let local = get_local_module_specifier(
            &module_path.file_name,
            &info,
            compiler_options,
            host,
            import_mode,
            &preferences,
            module_path.is_redirect || !specifier.is_empty(),
        );
        if local.is_empty()
            || (for_auto_import && is_excluded_by_regex(&local, &preferences.exclude_regexes))
        {
            continue;
        }
        if module_path.is_redirect {
            redirect_paths_specifiers.push(local);
        } else if path_is_bare_specifier(&local) {
            if contains_node_modules(&local) {
                // Likely an inappropriate `baseUrl` use; don't prioritize.
                relative_specifiers.push(local);
            } else {
                paths_specifiers.push(local);
            }
        } else if for_auto_import
            || !imported_file_is_in_node_modules
            || module_path.is_in_node_modules
        {
            relative_specifiers.push(local);
        }
    }

    if !paths_specifiers.is_empty() {
        return (paths_specifiers, ResultKind::Paths);
    }
    if !redirect_paths_specifiers.is_empty() {
        return (redirect_paths_specifiers, ResultKind::Redirect);
    }
    if !node_modules_specifiers.is_empty() {
        return (node_modules_specifiers, ResultKind::NodeModules);
    }
    (relative_specifiers, ResultKind::Relative)
}

/// Computes the candidate module specifiers for `module_file_name` as seen from
/// `importing_source_file`, returning them with the [`ResultKind`] that won.
///
/// # Examples
/// Requires a [`ModuleSpecifierGenerationHost`]; behavior is covered by
/// `lib_test.rs`.
///
/// Side effects: reads the host (file system, package.json, symlinks).
// Go: internal/modulespecifiers/specifiers.go:GetModuleSpecifiersForFileWithInfo
pub fn get_module_specifiers_for_file_with_info(
    importing_source_file: &dyn SourceFileForSpecifierGeneration,
    module_file_name: &str,
    compiler_options: &CompilerOptions,
    host: &dyn ModuleSpecifierGenerationHost,
    user_preferences: &UserPreferences,
    options: ModuleSpecifierOptions,
    for_auto_imports: bool,
) -> (Vec<String>, ResultKind) {
    let info = get_info(
        &host.get_source_of_project_reference_if_output_included(importing_source_file),
        host,
    );
    let module_paths =
        get_all_module_paths_worker(&info, module_file_name, host, compiler_options, options);

    compute_module_specifiers(
        &module_paths,
        compiler_options,
        importing_source_file,
        host,
        user_preferences,
        options,
        for_auto_imports,
    )
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod lib_tests;
