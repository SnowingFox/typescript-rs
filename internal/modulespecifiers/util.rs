//! Leaf helpers for specifier generation: regex exclusion, path/extension
//! manipulation, and `node_modules` path parsing.
//!
//! 1:1 port of Go `internal/modulespecifiers/util.go`.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

use regex::Regex;
use tsgo_collections::OrderedMap;
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::index_after;
use tsgo_module::{try_get_js_extension_for_file, Ending, ResolvedEntrypoint};
use tsgo_packagejson::ExportsOrImports;
use tsgo_tspath::{
    change_any_extension, compare_paths, file_extension_is_one_of, get_base_file_name,
    get_declaration_file_extension, get_normalized_absolute_path,
    get_relative_path_to_directory_or_url, is_rooted_disk_path, path_is_absolute, path_is_relative,
    remove_extension, remove_file_extension, try_get_extension_from_path, ComparePathsOptions,
    ALL_SUPPORTED_EXTENSIONS, EXTENSION_CJS, EXTENSION_CTS, EXTENSION_DCTS, EXTENSION_DMTS,
    EXTENSION_DTS, EXTENSION_JS, EXTENSION_JSX, EXTENSION_MJS, EXTENSION_MTS, EXTENSION_TS,
    EXTENSION_TSX,
};

use crate::preferences::get_allowed_endings_in_preferred_order;
use crate::types::{
    ModulePath, ModuleSpecifierEnding, ModuleSpecifierGenerationHost,
    SourceFileForSpecifierGeneration, UserPreferences,
};

/// Remaps a non-JS declaration file name such as `foo.d.json.ts` or
/// `foo.module.d.css.ts` back to its real name (`foo.json`, `foo.module.css`).
///
/// Returns `""` for inputs that are not `.ts`, do not contain `.d.`, or are
/// plain `.d.ts` files.
///
/// # Examples
/// ```
/// use tsgo_modulespecifiers::try_get_real_file_name_for_non_js_declaration_file_name;
/// assert_eq!(
///     try_get_real_file_name_for_non_js_declaration_file_name("/p/foo.d.json.ts"),
///     "/p/foo.json"
/// );
/// assert_eq!(
///     try_get_real_file_name_for_non_js_declaration_file_name("/p/foo.d.ts"),
///     ""
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/modulespecifiers/util.go:TryGetRealFileNameForNonJSDeclarationFileName
pub fn try_get_real_file_name_for_non_js_declaration_file_name(file_name: &str) -> String {
    let base_name = get_base_file_name(file_name);
    // Ends with .ts, contains ".d.", and is NOT a standard .d.ts file
    if !file_name.ends_with(EXTENSION_TS)
        || !base_name.contains(".d.")
        || base_name.ends_with(EXTENSION_DTS)
    {
        return String::new();
    }
    let no_extension = remove_extension(file_name, EXTENSION_TS);
    let last_dot_index = no_extension.rfind('.').unwrap();
    let ext = &no_extension[last_dot_index..];
    let before = no_extension
        .split_once(".d.")
        .map(|(b, _)| b)
        .unwrap_or(no_extension);
    format!("{before}{ext}")
}

/// Reports whether `module_specifier` matches any of the `excludes` regexes.
///
/// Each pattern is compiled (and cached) via `string_to_regex`; patterns that
/// fail to compile are skipped.
///
/// # Examples
/// ```
/// use tsgo_modulespecifiers::is_excluded_by_regex;
/// assert!(is_excluded_by_regex("lodash", &["^lodash$".to_string()]));
/// assert!(!is_excluded_by_regex("react", &["^lodash$".to_string()]));
/// ```
///
/// Side effects: populates the process-wide regex cache.
// Go: internal/modulespecifiers/util.go:IsExcludedByRegex
pub fn is_excluded_by_regex(module_specifier: &str, excludes: &[String]) -> bool {
    for pattern in excludes {
        let Some(re) = string_to_regex(pattern) else {
            continue;
        };
        if re.is_match(module_specifier) {
            return true;
        }
    }
    false
}

/// Compiles a possibly-`/pattern/flags`-delimited string into a [`Regex`],
/// caching the result (including compile failures) by `(pattern, case)`.
///
/// Mirrors the JS `AutoImportSpecifierExcludeRegexes` syntax: a leading and
/// trailing `/` with no unescaped interior `/` is stripped and its trailing
/// flags parsed (only `i` is honored); otherwise the whole string is the
/// pattern.
///
/// Side effects: reads/writes the process-wide regex cache (cleared when it
/// grows past 1000 entries, mirroring Go).
// Go: internal/modulespecifiers/util.go:stringToRegex
fn string_to_regex(pattern: &str) -> Option<Regex> {
    let mut pattern = pattern.to_string();
    let mut case_insensitive = false;

    let bytes = pattern.as_bytes();
    if bytes.len() > 2 && bytes[0] == b'/' {
        if let Some(last_slash) = pattern.rfind('/') {
            if last_slash > 0 {
                let mut has_unescaped_middle_slash = false;
                let pb = pattern.as_bytes();
                let mut i = 1;
                while i < last_slash {
                    if pb[i] == b'/' && pb[i - 1] != b'\\' {
                        has_unescaped_middle_slash = true;
                        break;
                    }
                    i += 1;
                }

                if !has_unescaped_middle_slash {
                    let flags = pattern[last_slash + 1..].to_string();
                    pattern = pattern[1..last_slash].to_string();
                    for flag in flags.chars() {
                        if flag == 'i' {
                            case_insensitive = true;
                        }
                    }
                }
            }
        }
    }

    let key = RegexPatternCacheKey {
        pattern: pattern.clone(),
        case_insensitive,
    };

    if let Ok(cache) = REGEX_PATTERN_CACHE.read() {
        if let Some(re) = cache.get(&key) {
            return re.clone();
        }
    }

    let mut cache = REGEX_PATTERN_CACHE.write().unwrap();
    if let Some(re) = cache.get(&key) {
        return re.clone();
    }

    if cache.len() > 1000 {
        cache.clear();
    }

    let compile_pattern = if case_insensitive {
        format!("(?i:{pattern})")
    } else {
        pattern.clone()
    };

    let compiled = Regex::new(&compile_pattern).ok();
    cache.insert(key, compiled.clone());
    compiled
}

static REGEX_PATTERN_CACHE: LazyLock<RwLock<HashMap<RegexPatternCacheKey, Option<Regex>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

#[derive(Clone, PartialEq, Eq, Hash)]
struct RegexPatternCacheKey {
    pattern: String,
    case_insensitive: bool,
}

// Go: internal/modulespecifiers/util.go:comparePathsByRedirect
pub(crate) fn compare_paths_by_redirect(
    a: &ModulePath,
    b: &ModulePath,
    use_case_sensitive_file_names: bool,
) -> Ordering {
    if a.is_redirect == b.is_redirect {
        return compare_paths(
            &a.file_name,
            &b.file_name,
            &ComparePathsOptions {
                use_case_sensitive_file_names,
                current_directory: String::new(),
            },
        );
    }
    if a.is_redirect {
        Ordering::Greater
    } else {
        Ordering::Less
    }
}

/// Reports whether `path` is a bare module specifier (neither absolute nor
/// dot-relative), e.g. `lodash` or `@scope/pkg`.
///
/// # Examples
/// ```
/// use tsgo_modulespecifiers::path_is_bare_specifier;
/// assert!(path_is_bare_specifier("lodash"));
/// assert!(!path_is_bare_specifier("./a"));
/// assert!(!path_is_bare_specifier("/a"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/modulespecifiers/util.go:PathIsBareSpecifier
pub fn path_is_bare_specifier(path: &str) -> bool {
    !path_is_absolute(path) && !path_is_relative(path)
}

// Go: internal/modulespecifiers/util.go:ensurePathIsNonModuleName
//
// Ensures a path is either absolute (prefixed with `/` or `c:`) or dot-relative
// (prefixed with `./` or `../`) so it is not confused with a bare module name.
pub(crate) fn ensure_path_is_non_module_name(path: &str) -> String {
    if path_is_bare_specifier(path) {
        format!("./{path}")
    } else {
        path.to_string()
    }
}

/// Maps a declaration-file extension to the JS extension it emits to
/// (`.d.ts`->`.js`, `.d.mts`->`.mjs`, `.d.cts`->`.cjs`, custom `.d.x.ts`->`.x`).
///
/// # Examples
/// ```
/// use tsgo_modulespecifiers::get_js_extension_for_declaration_file_extension;
/// assert_eq!(get_js_extension_for_declaration_file_extension(".d.ts"), ".js");
/// assert_eq!(get_js_extension_for_declaration_file_extension(".d.json.ts"), ".json");
/// ```
///
/// Side effects: none (pure).
// Go: internal/modulespecifiers/util.go:GetJSExtensionForDeclarationFileExtension
pub fn get_js_extension_for_declaration_file_extension(ext: &str) -> String {
    match ext {
        EXTENSION_DTS => EXTENSION_JS.to_string(),
        EXTENSION_DMTS => EXTENSION_MJS.to_string(),
        EXTENSION_DCTS => EXTENSION_CJS.to_string(),
        // .d.json.ts and the like: keep the inner source extension.
        _ => ext[".d".len()..ext.len() - EXTENSION_TS.len()].to_string(),
    }
}

// Go: internal/modulespecifiers/util.go:prefersTsExtension
pub(crate) fn prefers_ts_extension(allowed_endings: &[ModuleSpecifierEnding]) -> bool {
    let js_priority = index_of_ending(allowed_endings, ModuleSpecifierEnding::JsExtension);
    let ts_priority = index_of_ending(allowed_endings, ModuleSpecifierEnding::TsExtension);
    if ts_priority > -1 {
        ts_priority < js_priority
    } else {
        false
    }
}

/// Returns the index of `ending` in `endings`, or `-1` if absent (mirrors Go's
/// `slices.Index`, where `-1` participates in arithmetic comparisons).
fn index_of_ending(endings: &[ModuleSpecifierEnding], ending: ModuleSpecifierEnding) -> i32 {
    endings
        .iter()
        .position(|&e| e == ending)
        .map_or(-1, |i| i as i32)
}

// Go: internal/modulespecifiers/util.go:replaceFirstStar
pub(crate) fn replace_first_star(s: &str, replacement: &str) -> String {
    s.replacen('*', replacement, 1)
}

// Go: internal/modulespecifiers/util.go:isPathRelativeToParent
pub(crate) fn is_path_relative_to_parent(path: &str) -> bool {
    path.starts_with("..")
}

// Go: internal/modulespecifiers/util.go:packageJsonPathsAreEqual
pub(crate) fn package_json_paths_are_equal(
    a: &str,
    b: &str,
    options: &ComparePathsOptions,
) -> bool {
    if a == b {
        return true;
    }
    if a.is_empty() || b.is_empty() {
        return false;
    }
    compare_paths(a, b, options) == Ordering::Equal
}

// Go: internal/modulespecifiers/util.go:getRelativePathIfInSameVolume
pub(crate) fn get_relative_path_if_in_same_volume(
    path: &str,
    directory_path: &str,
    use_case_sensitive_file_names: bool,
) -> String {
    let relative_path = get_relative_path_to_directory_or_url(
        directory_path,
        path,
        false,
        &ComparePathsOptions {
            use_case_sensitive_file_names,
            current_directory: directory_path.to_string(),
        },
    );
    if is_rooted_disk_path(&relative_path) {
        String::new()
    } else {
        relative_path
    }
}

// Go: internal/modulespecifiers/util.go:getPathsRelativeToRootDirs
pub(crate) fn get_paths_relative_to_root_dirs(
    path: &str,
    root_dirs: &[String],
    use_case_sensitive_file_names: bool,
) -> Vec<String> {
    let mut results = Vec::new();
    for root_dir in root_dirs {
        let relative_path =
            get_relative_path_if_in_same_volume(path, root_dir, use_case_sensitive_file_names);
        if !is_path_relative_to_parent(&relative_path) {
            results.push(relative_path);
        }
    }
    results
}

/// Extracts the package name from a path that passes through `node_modules`
/// (handling `@scope/name`), or `""` if there is no `node_modules` segment.
///
/// # Examples
/// ```
/// use tsgo_modulespecifiers::get_package_name_from_directory;
/// assert_eq!(get_package_name_from_directory("/p/node_modules/lodash/x.js"), "lodash");
/// assert_eq!(get_package_name_from_directory("/p/node_modules/@a/b/x.js"), "@a/b");
/// assert_eq!(get_package_name_from_directory("/p/src/x.ts"), "");
/// ```
///
/// Side effects: none (pure).
// Go: internal/modulespecifiers/util.go:GetPackageNameFromDirectory
pub fn get_package_name_from_directory(file_or_directory_path: &str) -> String {
    let Some(idx) = file_or_directory_path.rfind("/node_modules/") else {
        return String::new();
    };

    let basename = &file_or_directory_path[idx + "/node_modules/".len()..];
    let bytes = basename.as_bytes();
    if bytes[0] == b'.' {
        return String::new();
    }

    let Some(next_slash) = basename.find('/') else {
        return basename.to_string();
    };

    if bytes[0] != b'@' || next_slash == basename.len() - 1 {
        return basename[..next_slash].to_string();
    }

    let Some(second_slash) = basename[next_slash + 1..].find('/') else {
        return basename.to_string();
    };

    basename[..next_slash + 1 + second_slash].to_string()
}

/// The index breakdown of a `node_modules` path, used to derive package names.
///
/// All indices point at the relevant `/` (or the segment start) within the
/// original path, mirroring the Go field semantics (any may be `-1`).
///
/// Side effects: none (plain data).
// Go: internal/modulespecifiers/util.go:NodeModulePathParts
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeModulePathParts {
    /// Index of the top-level `/node_modules/` segment's leading `/`.
    pub top_level_node_modules_index: i32,
    /// Index of the `/` immediately before the top-level package name.
    pub top_level_package_name_index: i32,
    /// Index of the `/` immediately after the package root directory.
    pub package_root_index: i32,
    /// Index of the `/` immediately before the file name.
    pub file_name_index: i32,
}

/// Parses the `node_modules` structure of `full_path`, returning the index
/// breakdown, or `None` if the path is not a module file under `node_modules`.
///
/// # Examples
/// ```
/// use tsgo_modulespecifiers::get_node_module_path_parts;
/// let parts = get_node_module_path_parts("/a/node_modules/pkg/file.ts").unwrap();
/// assert_eq!(parts.top_level_node_modules_index, 2);
/// assert_eq!(parts.package_root_index, 19);
/// assert!(get_node_module_path_parts("/a/src/file.ts").is_none());
/// ```
///
/// Side effects: none (pure).
// Go: internal/modulespecifiers/util.go:GetNodeModulePathParts
pub fn get_node_module_path_parts(full_path: &str) -> Option<NodeModulePathParts> {
    // Expected pattern: /base/path/node_modules/[@scope/otherpackage/...node_modules/]package/[subdir/]file.js
    let mut top_level_node_modules_index: i32 = 0;
    let mut top_level_package_name_index: i32 = 0;
    let mut package_root_index: i32 = 0;

    let mut part_start: i32 = 0;
    let mut part_end: i32 = 0;
    let mut state = NodeModulesPathParseState::BeforeNodeModules;
    let bytes = full_path.as_bytes();

    while part_end >= 0 {
        part_start = part_end;
        part_end = index_after(full_path, "/", (part_start + 1) as usize);
        match state {
            NodeModulesPathParseState::BeforeNodeModules => {
                if full_path[part_start as usize..].starts_with("/node_modules/") {
                    top_level_node_modules_index = part_start;
                    top_level_package_name_index = part_end;
                    state = NodeModulesPathParseState::NodeModules;
                }
            }
            NodeModulesPathParseState::NodeModules | NodeModulesPathParseState::Scope => {
                if state == NodeModulesPathParseState::NodeModules
                    && bytes[(part_start + 1) as usize] == b'@'
                {
                    state = NodeModulesPathParseState::Scope;
                } else {
                    package_root_index = part_end;
                    state = NodeModulesPathParseState::PackageContent;
                }
            }
            NodeModulesPathParseState::PackageContent => {
                if full_path[part_start as usize..].starts_with("/node_modules/") {
                    state = NodeModulesPathParseState::NodeModules;
                } else {
                    state = NodeModulesPathParseState::PackageContent;
                }
            }
        }
    }

    let file_name_index = part_start;

    if (state as u8) > (NodeModulesPathParseState::NodeModules as u8) {
        Some(NodeModulePathParts {
            top_level_node_modules_index,
            top_level_package_name_index,
            package_root_index,
            file_name_index,
        })
    } else {
        None
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum NodeModulesPathParseState {
    BeforeNodeModules = 0,
    NodeModules = 1,
    Scope = 2,
    PackageContent = 3,
}

// Go: internal/modulespecifiers/util.go:getJSExtensionForFile
pub(crate) fn get_js_extension_for_file(file_name: &str, options: &CompilerOptions) -> String {
    let result = try_get_js_extension_for_file(file_name, options);
    if result.is_empty() {
        panic!(
            "Extension {} is unsupported:: FileName:: {}",
            extension_from_path(file_name),
            file_name
        );
    }
    result.to_string()
}

// Go: internal/modulespecifiers/util.go:extensionFromPath
//
// Gets the extension from a path; the path must have a valid extension.
fn extension_from_path(path: &str) -> &'static str {
    let ext = try_get_extension_from_path(path);
    if ext.is_empty() {
        panic!("File {path} has unknown extension.");
    }
    ext
}

// Go: internal/modulespecifiers/util.go:tryGetAnyFileFromPath
//
// `tsoptions.GetSupportedExtensions(&CompilerOptions{AllowJs: true}, [node, json])`
// reduces to `ALL_SUPPORTED_EXTENSIONS`: the two extra extensions are not added
// because their script kinds (External/JSON) do not satisfy the inclusion
// condition. `tsoptions.GetSupportedExtensions` is not yet ported, so the
// reduced result is used directly.
pub(crate) fn try_get_any_file_from_path(
    host: &dyn ModuleSpecifierGenerationHost,
    path: &str,
) -> bool {
    for exts in ALL_SUPPORTED_EXTENSIONS {
        for e in *exts {
            let full_path = format!("{path}{e}");
            if host.file_exists(&get_normalized_absolute_path(
                &full_path,
                &host.get_current_directory(),
            )) {
                return true;
            }
        }
    }
    false
}

// Go: internal/modulespecifiers/util.go:allKeysStartWithDot
//
// Currently unused (matches the unused Go helper); kept for completeness.
#[allow(dead_code)]
fn all_keys_start_with_dot(obj: &OrderedMap<String, ExportsOrImports>) -> bool {
    obj.keys().all(|k| k.starts_with('.'))
}

/// Rewrites a pre-computed `exports` entrypoint specifier to honor the
/// entrypoint's [`Ending`] and the user's preferred endings.
///
/// A [`Ending::Fixed`] entrypoint is returned verbatim; otherwise the extension
/// (or trailing `/index`) is adjusted toward the preferred ending, respecting
/// declaration-file remapping (`.d.ts`->`.js`, etc.).
///
/// # Examples
/// Requires a [`ModuleSpecifierGenerationHost`]; behavior is covered by
/// `util_test.rs`.
///
/// Side effects: may read the host when `allowed_endings` is empty.
// Go: internal/modulespecifiers/util.go:ProcessEntrypointEnding
pub fn process_entrypoint_ending(
    entrypoint: &ResolvedEntrypoint,
    prefs: &UserPreferences,
    host: &dyn ModuleSpecifierGenerationHost,
    options: &CompilerOptions,
    importing_source_file: &dyn SourceFileForSpecifierGeneration,
    allowed_endings: &[ModuleSpecifierEnding],
) -> String {
    let mut specifier = entrypoint.module_specifier.clone();
    if entrypoint.ending == Ending::Fixed {
        return specifier;
    }

    let computed_endings;
    let allowed_endings: &[ModuleSpecifierEnding] = if allowed_endings.is_empty() {
        computed_endings = get_allowed_endings_in_preferred_order(
            prefs,
            host,
            options,
            importing_source_file,
            "",
            host.get_default_resolution_mode_for_file(importing_source_file),
        );
        &computed_endings
    } else {
        allowed_endings
    };

    let preferred_ending = allowed_endings[0];

    let trim_index = |s: String, ending: ModuleSpecifierEnding| -> String {
        if ending == ModuleSpecifierEnding::Minimal {
            if let Some(trimmed) = s.strip_suffix("/index").map(str::to_string) {
                return trimmed;
            }
        }
        s
    };

    // Declaration file extensions.
    let dts_extension = get_declaration_file_extension(&specifier);
    if !dts_extension.is_empty() {
        return match preferred_ending {
            ModuleSpecifierEnding::TsExtension | ModuleSpecifierEnding::JsExtension => {
                let js_extension = get_js_extension_for_declaration_file_extension(&dts_extension);
                change_any_extension(&specifier, &js_extension, &[&dts_extension], false)
            }
            ModuleSpecifierEnding::Minimal | ModuleSpecifierEnding::Index => {
                if entrypoint.ending == Ending::Changeable {
                    // `.d.mts`/`.d.cts` must keep an extension; rewrite instead of dropping.
                    if dts_extension == EXTENSION_DTS {
                        specifier = remove_extension(&specifier, &dts_extension).to_string();
                        return trim_index(specifier, preferred_ending);
                    }
                    let js_extension =
                        get_js_extension_for_declaration_file_extension(&dts_extension);
                    return change_any_extension(
                        &specifier,
                        &js_extension,
                        &[&dts_extension],
                        false,
                    );
                }
                // ExtensionChangeable - can only change the extension, not remove it.
                let js_extension = get_js_extension_for_declaration_file_extension(&dts_extension);
                change_any_extension(&specifier, &js_extension, &[&dts_extension], false)
            }
        };
    }

    // .ts/.tsx/.mts/.cts extensions.
    if file_extension_is_one_of(
        &specifier,
        &[EXTENSION_TS, EXTENSION_TSX, EXTENSION_MTS, EXTENSION_CTS],
    ) {
        return match preferred_ending {
            ModuleSpecifierEnding::TsExtension => specifier,
            ModuleSpecifierEnding::JsExtension => {
                let js_extension = try_get_js_extension_for_file(&specifier, options);
                if !js_extension.is_empty() {
                    format!("{}{}", remove_file_extension(&specifier), js_extension)
                } else {
                    specifier
                }
            }
            ModuleSpecifierEnding::Minimal | ModuleSpecifierEnding::Index => {
                if entrypoint.ending == Ending::Changeable {
                    specifier = remove_file_extension(&specifier).to_string();
                    return trim_index(specifier, preferred_ending);
                }
                // ExtensionChangeable - can only change the extension, not remove it.
                let js_extension = try_get_js_extension_for_file(&specifier, options);
                if !js_extension.is_empty() {
                    format!("{}{}", remove_file_extension(&specifier), js_extension)
                } else {
                    specifier
                }
            }
        };
    }

    // .js/.jsx/.mjs/.cjs extensions.
    if file_extension_is_one_of(
        &specifier,
        &[EXTENSION_JS, EXTENSION_JSX, EXTENSION_MJS, EXTENSION_CJS],
    ) {
        return match preferred_ending {
            ModuleSpecifierEnding::TsExtension | ModuleSpecifierEnding::JsExtension => specifier,
            ModuleSpecifierEnding::Minimal | ModuleSpecifierEnding::Index => {
                if entrypoint.ending == Ending::Changeable {
                    specifier = remove_file_extension(&specifier).to_string();
                    return trim_index(specifier, preferred_ending);
                }
                // ExtensionChangeable - keep the extension.
                specifier
            }
        };
    }

    // For other extensions (like .json), return as-is.
    specifier
}

#[cfg(test)]
#[path = "util_test.rs"]
mod tests;
