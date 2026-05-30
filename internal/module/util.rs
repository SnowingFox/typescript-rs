//! Pure helpers for module-name parsing, `@types` mangling, `exports` pattern
//! key ordering, and extension/diagnostic classification.
//!
//! 1:1 port of Go `internal/module/util.go`.
//!
//! # Divergence from Go
//! - `GetResolutionDiagnostic` takes a `*ast.SourceFile` in Go but only reads
//!   `IsDeclarationFile`; `tsgo_ast` does not yet expose a usable `SourceFile`
//!   handle, so [`get_resolution_diagnostic`] takes `is_declaration_file: bool`
//!   directly. blocked-by: tsgo_ast::SourceFile.
//! - `ComparePatternKeys` returns Go `int` (-1/0/1); [`compare_pattern_keys`]
//!   returns [`Ordering`] so it composes with `slice::sort_by` (same ordering).

use std::cmp::Ordering;
use std::sync::LazyLock;

use tsgo_core::compileroptions::{CompilerOptions, JsxEmit};
use tsgo_core::version::version;
use tsgo_diagnostics::{
    Message, COULD_NOT_FIND_A_DECLARATION_FILE_FOR_MODULE_0_1_IMPLICITLY_HAS_AN_ANY_TYPE,
    MODULE_0_WAS_RESOLVED_TO_1_BUT_ALLOWARBITRARYEXTENSIONS_IS_NOT_SET,
    MODULE_0_WAS_RESOLVED_TO_1_BUT_JSX_IS_NOT_SET,
    MODULE_0_WAS_RESOLVED_TO_1_BUT_RESOLVEJSONMODULE_IS_NOT_USED,
};
use tsgo_semver::{must_parse, try_parse_version_range, Version};
use tsgo_tspath::{
    normalize_path, try_get_extension_from_path, EXTENSION_CJS, EXTENSION_CTS, EXTENSION_DCTS,
    EXTENSION_DMTS, EXTENSION_DTS, EXTENSION_JS, EXTENSION_JSON, EXTENSION_JSX, EXTENSION_MJS,
    EXTENSION_MTS, EXTENSION_TS, EXTENSION_TSX,
};

use crate::types::ResolvedModule;

// Go: internal/module/util.go:typeScriptVersion
static TYPESCRIPT_VERSION: LazyLock<Version> = LazyLock::new(|| must_parse(version()));

/// The synthetic containing-file name used for inferred type directives.
// Go: internal/module/util.go:InferredTypesContainingFile
pub const INFERRED_TYPES_CONTAINING_FILE: &str = "__inferred type names__.ts";

/// Reports whether `key` is a `types@<range>` condition matching the current
/// TypeScript compiler version.
///
/// # Examples
/// ```
/// use tsgo_module::is_applicable_versioned_types_key;
/// assert!(is_applicable_versioned_types_key("types@>=1.0"));
/// assert!(!is_applicable_versioned_types_key("foo"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/module/util.go:IsApplicableVersionedTypesKey
pub fn is_applicable_versioned_types_key(key: &str) -> bool {
    let Some(rest) = key.strip_prefix("types@") else {
        return false;
    };
    let (range, ok) = try_parse_version_range(rest);
    if !ok {
        return false;
    }
    range.test(&TYPESCRIPT_VERSION)
}

/// Returns the `node_modules` package directory containing `resolved`, or empty
/// when `resolved` is not under a `node_modules` directory.
///
/// `is_folder` indicates whether `resolved` is itself a directory, which
/// affects where the package boundary falls when there is no trailing path.
///
/// # Examples
/// ```
/// use tsgo_module::parse_node_module_from_path;
/// assert_eq!(
///     parse_node_module_from_path("/x/node_modules/pkg/a.js", false),
///     "/x/node_modules/pkg"
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/module/util.go:ParseNodeModuleFromPath
pub fn parse_node_module_from_path(resolved: &str, is_folder: bool) -> String {
    let path = normalize_path(resolved);
    let Some(idx) = path.rfind("/node_modules/") else {
        return String::new();
    };

    let index_after_node_modules = idx + "/node_modules/".len();
    let mut index_after_package_name =
        move_to_next_directory_separator_if_available(&path, index_after_node_modules, is_folder);
    if path.as_bytes()[index_after_node_modules] == b'@' {
        index_after_package_name = move_to_next_directory_separator_if_available(
            &path,
            index_after_package_name,
            is_folder,
        );
    }
    path[..index_after_package_name].to_string()
}

// Go: internal/module/resolver.go:moveToNextDirectorySeparatorIfAvailable
//
// Kept here next to its sole caller `parse_node_module_from_path`.
pub(crate) fn move_to_next_directory_separator_if_available(
    path: &str,
    prev_separator_index: usize,
    is_folder: bool,
) -> usize {
    let offset = prev_separator_index + 1;
    match path[offset..].find('/') {
        None => {
            if is_folder {
                path.len()
            } else {
                prev_separator_index
            }
        }
        Some(next_separator_index) => next_separator_index + offset,
    }
}

/// Splits a module name into its package name and the remaining subpath,
/// accounting for `@scope/` packages.
///
/// # Examples
/// ```
/// use tsgo_module::parse_package_name;
/// assert_eq!(parse_package_name("foo/bar/baz"), ("foo".to_string(), "bar/baz".to_string()));
/// assert_eq!(parse_package_name("@a/b/c"), ("@a/b".to_string(), "c".to_string()));
/// assert_eq!(parse_package_name("foo"), ("foo".to_string(), String::new()));
/// ```
///
/// Side effects: none (pure).
// Go: internal/module/util.go:ParsePackageName
pub fn parse_package_name(module_name: &str) -> (String, String) {
    let mut idx: i64 = module_name.find('/').map_or(-1, |i| i as i64);
    if !module_name.is_empty() && module_name.as_bytes()[0] == b'@' {
        let offset = (idx + 1) as usize;
        idx = module_name[offset..].find('/').map_or(-1, |i| i as i64);
        if idx != -1 {
            idx += offset as i64;
        }
    }
    if idx == -1 {
        return (module_name.to_string(), String::new());
    }
    let idx = idx as usize;
    (
        module_name[..idx].to_string(),
        module_name[idx + 1..].to_string(),
    )
}

/// Mangles a scoped package name (`@a/b`) into its `@types`-style flat form
/// (`a__b`); non-scoped names are returned unchanged.
///
/// # Examples
/// ```
/// use tsgo_module::mangle_scoped_package_name;
/// assert_eq!(mangle_scoped_package_name("@a/b"), "a__b");
/// assert_eq!(mangle_scoped_package_name("foo"), "foo");
/// ```
///
/// Side effects: none (pure).
// Go: internal/module/util.go:MangleScopedPackageName
pub fn mangle_scoped_package_name(package_name: &str) -> String {
    if !package_name.is_empty() && package_name.as_bytes()[0] == b'@' {
        match package_name.find('/') {
            None => package_name.to_string(),
            Some(idx) => format!("{}__{}", &package_name[1..idx], &package_name[idx + 1..]),
        }
    } else {
        package_name.to_string()
    }
}

/// Reverses [`mangle_scoped_package_name`]: `a__b` becomes `@a/b`; names with
/// no `__` are returned unchanged.
///
/// # Examples
/// ```
/// use tsgo_module::unmangle_scoped_package_name;
/// assert_eq!(unmangle_scoped_package_name("a__b"), "@a/b");
/// assert_eq!(unmangle_scoped_package_name("foo"), "foo");
/// ```
///
/// Side effects: none (pure).
// Go: internal/module/util.go:UnmangleScopedPackageName
pub fn unmangle_scoped_package_name(package_name: &str) -> String {
    match package_name.split_once("__") {
        Some((before, after)) => format!("@{before}/{after}"),
        None => package_name.to_string(),
    }
}

/// Returns the `@types` package name for `package_name`.
///
/// # Examples
/// ```
/// use tsgo_module::get_types_package_name;
/// assert_eq!(get_types_package_name("@a/b"), "@types/a__b");
/// ```
///
/// Side effects: none (pure).
// Go: internal/module/util.go:GetTypesPackageName
pub fn get_types_package_name(package_name: &str) -> String {
    format!("@types/{}", mangle_scoped_package_name(package_name))
}

/// Recovers the original package name from an `@types` package name.
///
/// # Examples
/// ```
/// use tsgo_module::get_package_name_from_types_package_name;
/// assert_eq!(get_package_name_from_types_package_name("@types/a__b"), "@a/b");
/// assert_eq!(get_package_name_from_types_package_name("foo"), "foo");
/// ```
///
/// Side effects: none (pure).
// Go: internal/module/util.go:GetPackageNameFromTypesPackageName
pub fn get_package_name_from_types_package_name(mangled_name: &str) -> String {
    match mangled_name.strip_prefix("@types/") {
        Some(rest) => unmangle_scoped_package_name(rest),
        None => mangled_name.to_string(),
    }
}

/// Orders two `exports`/`imports` pattern keys: longer fixed prefixes sort
/// first, exact keys outrank patterns, and longer keys win ties.
///
/// Returns [`Ordering::Less`] when `a` should sort before `b` (Go's `-1`).
///
/// # Examples
/// ```
/// use std::cmp::Ordering;
/// use tsgo_module::compare_pattern_keys;
/// assert_eq!(compare_pattern_keys("ab/*", "a/*"), Ordering::Less);
/// assert_eq!(compare_pattern_keys("a/*", "ab/*"), Ordering::Greater);
/// ```
///
/// Side effects: none (pure).
// Go: internal/module/util.go:ComparePatternKeys
pub fn compare_pattern_keys(a: &str, b: &str) -> Ordering {
    let a_pattern_index = a.find('*').map(|i| i as i64).unwrap_or(-1);
    let b_pattern_index = b.find('*').map(|i| i as i64).unwrap_or(-1);
    let base_len_a = if a_pattern_index != -1 {
        a_pattern_index + 1
    } else {
        a.len() as i64
    };
    let base_len_b = if b_pattern_index != -1 {
        b_pattern_index + 1
    } else {
        b.len() as i64
    };

    if base_len_a > base_len_b {
        return Ordering::Less;
    }
    if base_len_b > base_len_a {
        return Ordering::Greater;
    }
    if a_pattern_index == -1 {
        return Ordering::Greater;
    }
    if b_pattern_index == -1 {
        return Ordering::Less;
    }
    if a.len() > b.len() {
        return Ordering::Less;
    }
    if b.len() > a.len() {
        return Ordering::Greater;
    }
    Ordering::Equal
}

/// Returns a diagnostic when a resolved module would be excluded due to its
/// extension under the current compiler options.
///
/// `is_declaration_file` corresponds to `file.IsDeclarationFile` in Go.
///
/// Side effects: none (pure).
// Go: internal/module/util.go:GetResolutionDiagnostic
pub fn get_resolution_diagnostic(
    options: &CompilerOptions,
    resolved_module: &ResolvedModule,
    is_declaration_file: bool,
) -> Option<&'static Message> {
    let need_jsx = || -> Option<&'static Message> {
        if options.jsx != JsxEmit::None {
            None
        } else {
            Some(&MODULE_0_WAS_RESOLVED_TO_1_BUT_JSX_IS_NOT_SET)
        }
    };

    let need_allow_js = || -> Option<&'static Message> {
        if options.get_allow_js()
            || !options
                .no_implicit_any
                .default_if_unknown(options.strict)
                .is_true()
        {
            None
        } else {
            Some(&COULD_NOT_FIND_A_DECLARATION_FILE_FOR_MODULE_0_1_IMPLICITLY_HAS_AN_ANY_TYPE)
        }
    };

    let need_resolve_json_module = || -> Option<&'static Message> {
        if options.get_resolve_json_module() {
            None
        } else {
            Some(&MODULE_0_WAS_RESOLVED_TO_1_BUT_RESOLVEJSONMODULE_IS_NOT_USED)
        }
    };

    let need_allow_arbitrary_extensions = || -> Option<&'static Message> {
        if is_declaration_file || options.allow_arbitrary_extensions.is_true() {
            None
        } else {
            Some(&MODULE_0_WAS_RESOLVED_TO_1_BUT_ALLOWARBITRARYEXTENSIONS_IS_NOT_SET)
        }
    };

    match resolved_module.extension.as_str() {
        EXTENSION_TS | EXTENSION_DTS | EXTENSION_MTS | EXTENSION_DMTS | EXTENSION_CTS
        | EXTENSION_DCTS => None,
        EXTENSION_TSX => need_jsx(),
        EXTENSION_JSX => need_jsx().or_else(need_allow_js),
        EXTENSION_JS | EXTENSION_MJS | EXTENSION_CJS => need_allow_js(),
        EXTENSION_JSON => need_resolve_json_module(),
        _ => need_allow_arbitrary_extensions(),
    }
}

/// Maps a TS/JS/declaration file name to the JavaScript-side extension it emits
/// to, or empty when the extension is unsupported.
///
/// # Examples
/// ```
/// use tsgo_core::compileroptions::CompilerOptions;
/// use tsgo_module::try_get_js_extension_for_file;
/// let opts = CompilerOptions::default();
/// assert_eq!(try_get_js_extension_for_file("a.ts", &opts), ".js");
/// assert_eq!(try_get_js_extension_for_file("a.mts", &opts), ".mjs");
/// ```
///
/// Side effects: none (pure).
// Go: internal/module/util.go:TryGetJSExtensionForFile
pub fn try_get_js_extension_for_file(file_name: &str, options: &CompilerOptions) -> &'static str {
    let ext = try_get_extension_from_path(file_name);
    match ext {
        EXTENSION_TS | EXTENSION_DTS => EXTENSION_JS,
        EXTENSION_TSX => {
            if options.jsx == JsxEmit::Preserve {
                EXTENSION_JSX
            } else {
                EXTENSION_JS
            }
        }
        EXTENSION_JS | EXTENSION_JSX | EXTENSION_JSON => ext,
        EXTENSION_DMTS | EXTENSION_MTS | EXTENSION_MJS => EXTENSION_MJS,
        EXTENSION_DCTS | EXTENSION_CTS | EXTENSION_CJS => EXTENSION_CJS,
        _ => "",
    }
}

#[cfg(test)]
#[path = "util_test.rs"]
mod tests;
