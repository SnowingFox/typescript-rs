//! TypeScript/JavaScript file extension constants and helpers.
//!
//! 1:1 port of Go `internal/tspath/extension.go`.

use std::sync::LazyLock;

use crate::path::{file_extension_is, get_any_extension_from_path, get_base_file_name};

/// `.ts` extension.
pub const EXTENSION_TS: &str = ".ts";
/// `.tsx` extension.
pub const EXTENSION_TSX: &str = ".tsx";
/// `.d.ts` declaration extension.
pub const EXTENSION_DTS: &str = ".d.ts";
/// `.js` extension.
pub const EXTENSION_JS: &str = ".js";
/// `.jsx` extension.
pub const EXTENSION_JSX: &str = ".jsx";
/// `.json` extension.
pub const EXTENSION_JSON: &str = ".json";
/// `.tsbuildinfo` extension.
pub const EXTENSION_TS_BUILD_INFO: &str = ".tsbuildinfo";
/// `.mjs` extension.
pub const EXTENSION_MJS: &str = ".mjs";
/// `.mts` extension.
pub const EXTENSION_MTS: &str = ".mts";
/// `.d.mts` declaration extension.
pub const EXTENSION_DMTS: &str = ".d.mts";
/// `.cjs` extension.
pub const EXTENSION_CJS: &str = ".cjs";
/// `.cts` extension.
pub const EXTENSION_CTS: &str = ".cts";
/// `.d.cts` declaration extension.
pub const EXTENSION_DCTS: &str = ".d.cts";

/// Supported declaration extensions.
pub const SUPPORTED_DECLARATION_EXTENSIONS: &[&str] =
    &[EXTENSION_DTS, EXTENSION_DCTS, EXTENSION_DMTS];

/// Supported TS implementation extensions.
pub const SUPPORTED_TS_IMPLEMENTATION_EXTENSIONS: &[&str] =
    &[EXTENSION_TS, EXTENSION_TSX, EXTENSION_MTS, EXTENSION_CTS];

const SUPPORTED_TS_EXTENSIONS_FOR_EXTRACT_EXTENSION: &[&str] = &[
    EXTENSION_DTS,
    EXTENSION_DCTS,
    EXTENSION_DMTS,
    EXTENSION_TS,
    EXTENSION_TSX,
    EXTENSION_MTS,
    EXTENSION_CTS,
];

/// All supported extensions, grouped by module format.
pub const ALL_SUPPORTED_EXTENSIONS: &[&[&str]] = &[
    &[
        EXTENSION_TS,
        EXTENSION_TSX,
        EXTENSION_DTS,
        EXTENSION_JS,
        EXTENSION_JSX,
    ],
    &[EXTENSION_CTS, EXTENSION_DCTS, EXTENSION_CJS],
    &[EXTENSION_MTS, EXTENSION_DMTS, EXTENSION_MJS],
];

/// Supported TS extensions, grouped by module format.
pub const SUPPORTED_TS_EXTENSIONS: &[&[&str]] = &[
    &[EXTENSION_TS, EXTENSION_TSX, EXTENSION_DTS],
    &[EXTENSION_CTS, EXTENSION_DCTS],
    &[EXTENSION_MTS, EXTENSION_DMTS],
];

/// Flat list of supported TS extensions.
pub const SUPPORTED_TS_EXTENSIONS_FLAT: &[&str] = &[
    EXTENSION_TS,
    EXTENSION_TSX,
    EXTENSION_DTS,
    EXTENSION_CTS,
    EXTENSION_DCTS,
    EXTENSION_MTS,
    EXTENSION_DMTS,
];

/// Supported JS extensions, grouped by module format.
pub const SUPPORTED_JS_EXTENSIONS: &[&[&str]] = &[
    &[EXTENSION_JS, EXTENSION_JSX],
    &[EXTENSION_MJS],
    &[EXTENSION_CJS],
];

/// Flat list of supported JS extensions.
pub const SUPPORTED_JS_EXTENSIONS_FLAT: &[&str] =
    &[EXTENSION_JS, EXTENSION_JSX, EXTENSION_MJS, EXTENSION_CJS];

/// All supported extensions with `.json` appended as its own group.
pub static ALL_SUPPORTED_EXTENSIONS_WITH_JSON: LazyLock<Vec<Vec<&'static str>>> =
    LazyLock::new(|| {
        let mut v: Vec<Vec<&'static str>> = ALL_SUPPORTED_EXTENSIONS
            .iter()
            .map(|g| g.to_vec())
            .collect();
        v.push(vec![EXTENSION_JSON]);
        v
    });

/// Supported TS extensions with `.json` appended as its own group.
pub static SUPPORTED_TS_EXTENSIONS_WITH_JSON: LazyLock<Vec<Vec<&'static str>>> =
    LazyLock::new(|| {
        let mut v: Vec<Vec<&'static str>> =
            SUPPORTED_TS_EXTENSIONS.iter().map(|g| g.to_vec()).collect();
        v.push(vec![EXTENSION_JSON]);
        v
    });

/// Flat list of supported TS extensions with `.json` appended.
pub static SUPPORTED_TS_EXTENSIONS_WITH_JSON_FLAT: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| {
        let mut v: Vec<&'static str> = SUPPORTED_TS_EXTENSIONS_FLAT.to_vec();
        v.push(EXTENSION_JSON);
        v
    });

/// Extensions that do not support extensionless module resolution.
pub const EXTENSIONS_NOT_SUPPORTING_EXTENSIONLESS_RESOLUTION: &[&str] = &[
    EXTENSION_MTS,
    EXTENSION_DMTS,
    EXTENSION_MJS,
    EXTENSION_CTS,
    EXTENSION_DCTS,
    EXTENSION_CJS,
];

const EXTENSIONS_TO_REMOVE: &[&str] = &[
    EXTENSION_DTS,
    EXTENSION_DMTS,
    EXTENSION_DCTS,
    EXTENSION_MJS,
    EXTENSION_MTS,
    EXTENSION_CJS,
    EXTENSION_CTS,
    EXTENSION_TS,
    EXTENSION_JS,
    EXTENSION_TSX,
    EXTENSION_JSX,
    EXTENSION_JSON,
];

/// Reports whether an extension is a TypeScript extension (including custom
/// `.d.*.ts`).
///
/// # Examples
/// ```
/// use tsgo_tspath::{extension_is_ts, EXTENSION_TS};
/// assert!(extension_is_ts(EXTENSION_TS));
/// assert!(extension_is_ts(".d.json.ts"));
/// assert!(!extension_is_ts(".js"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:ExtensionIsTs
pub fn extension_is_ts(ext: &str) -> bool {
    ext == EXTENSION_TS
        || ext == EXTENSION_TSX
        || ext == EXTENSION_DTS
        || ext == EXTENSION_MTS
        || ext == EXTENSION_DMTS
        || ext == EXTENSION_CTS
        || ext == EXTENSION_DCTS
        || (ext.len() >= 7 && &ext[..3] == ".d." && &ext[ext.len() - 3..] == ".ts")
}

/// Removes a known file extension (even multi-dot) from `path`.
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:RemoveFileExtension
pub fn remove_file_extension(path: &str) -> &str {
    for ext in EXTENSIONS_TO_REMOVE {
        if let Some(stripped) = path.strip_suffix(ext) {
            return stripped;
        }
    }
    path
}

/// Returns the known extension of `p`, or `""` if none.
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:TryGetExtensionFromPath
pub fn try_get_extension_from_path(p: &str) -> &'static str {
    for &ext in EXTENSIONS_TO_REMOVE {
        if file_extension_is(p, ext) {
            return ext;
        }
    }
    ""
}

/// Removes a known `extension` suffix from `path`.
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:RemoveExtension
pub fn remove_extension<'a>(path: &'a str, extension: &str) -> &'a str {
    &path[..path.len() - extension.len()]
}

/// Reports whether `path` ends with one of `extensions`.
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:FileExtensionIsOneOf
pub fn file_extension_is_one_of(path: &str, extensions: &[&str]) -> bool {
    extensions.iter().any(|ext| file_extension_is(path, ext))
}

/// Returns the TS extension of `file_name`, or `""` if none.
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:TryExtractTSExtension
pub fn try_extract_ts_extension(file_name: &str) -> &'static str {
    for &ext in SUPPORTED_TS_EXTENSIONS_FOR_EXTRACT_EXTENSION {
        if file_extension_is(file_name, ext) {
            return ext;
        }
    }
    ""
}

/// Reports whether `path` has a supported TS file extension.
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:HasTSFileExtension
pub fn has_ts_file_extension(path: &str) -> bool {
    file_extension_is_one_of(path, SUPPORTED_TS_EXTENSIONS_FLAT)
}

/// Reports whether `path` is a TS implementation file (not a declaration).
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:HasImplementationTSFileExtension
pub fn has_implementation_ts_file_extension(path: &str) -> bool {
    file_extension_is_one_of(path, SUPPORTED_TS_IMPLEMENTATION_EXTENSIONS)
        && !is_declaration_file_name(path)
}

/// Reports whether `path` has a supported JS file extension.
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:HasJSFileExtension
pub fn has_js_file_extension(path: &str) -> bool {
    file_extension_is_one_of(path, SUPPORTED_JS_EXTENSIONS_FLAT)
}

/// Reports whether `path` ends with `.json`.
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:HasJSONFileExtension
pub fn has_json_file_extension(path: &str) -> bool {
    file_extension_is(path, EXTENSION_JSON)
}

/// Reports whether `file_name` is a declaration file.
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:IsDeclarationFileName
pub fn is_declaration_file_name(file_name: &str) -> bool {
    !get_declaration_file_extension(file_name).is_empty()
}

/// Reports whether `ext` is one of `extensions`.
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:ExtensionIsOneOf
pub fn extension_is_one_of(ext: &str, extensions: &[&str]) -> bool {
    extensions.contains(&ext)
}

/// Returns the declaration file extension of `file_name`, or `""` if none.
///
/// # Examples
/// ```
/// use tsgo_tspath::get_declaration_file_extension;
/// assert_eq!(get_declaration_file_extension("foo.d.ts"), ".d.ts");
/// assert_eq!(get_declaration_file_extension("foo.d.json.ts"), ".d.json.ts");
/// assert_eq!(get_declaration_file_extension("foo.ts"), "");
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:GetDeclarationFileExtension
pub fn get_declaration_file_extension(file_name: &str) -> String {
    let base = get_base_file_name(file_name);
    for ext in SUPPORTED_DECLARATION_EXTENSIONS {
        if base.ends_with(ext) {
            return (*ext).to_string();
        }
    }
    if base.ends_with(EXTENSION_TS) {
        if let Some(index) = base.find(".d.") {
            return base[index..].to_string();
        }
    }
    String::new()
}

/// Returns the declaration emit extension for `path`.
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:GetDeclarationEmitExtensionForPath
pub fn get_declaration_emit_extension_for_path(path: &str) -> String {
    if file_extension_is_one_of(path, &[EXTENSION_MJS, EXTENSION_MTS]) {
        EXTENSION_DMTS.to_string()
    } else if file_extension_is_one_of(path, &[EXTENSION_CJS, EXTENSION_CTS]) {
        EXTENSION_DCTS.to_string()
    } else if file_extension_is_one_of(
        path,
        &[EXTENSION_TS, EXTENSION_TSX, EXTENSION_JS, EXTENSION_JSX],
    ) {
        EXTENSION_DTS.to_string()
    } else {
        let ext = get_any_extension_from_path(path, &[], false);
        if !ext.is_empty() {
            format!(".d{ext}.ts")
        } else {
            EXTENSION_DTS.to_string()
        }
    }
}

/// Changes the extension of `path` to `ext` if it currently matches one of
/// `extensions`.
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:ChangeAnyExtension
pub fn change_any_extension(
    path: &str,
    ext: &str,
    extensions: &[&str],
    ignore_case: bool,
) -> String {
    let pathext = get_any_extension_from_path(path, extensions, ignore_case);
    if !pathext.is_empty() {
        let result = &path[..path.len() - pathext.len()];
        if ext.is_empty() {
            return result.to_string();
        }
        if ext.starts_with('.') {
            return format!("{result}{ext}");
        }
        return format!("{result}.{ext}");
    }
    path.to_string()
}

/// Changes the extension of `path` (recognizing any known extension).
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:ChangeExtension
pub fn change_extension(path: &str, new_extension: &str) -> String {
    change_any_extension(path, new_extension, EXTENSIONS_TO_REMOVE, false)
}

/// Like [`change_extension`], but declaration extensions are replaced from the
/// `.d`.
///
/// # Examples
/// ```
/// use tsgo_tspath::change_full_extension;
/// assert_eq!(change_full_extension("file.d.ts", ".js"), "file.js");
/// ```
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:ChangeFullExtension
pub fn change_full_extension(path: &str, new_extension: &str) -> String {
    let declaration_extension = get_declaration_file_extension(path);
    if !declaration_extension.is_empty() {
        let mut ext = new_extension.to_string();
        if !ext.starts_with('.') {
            ext = format!(".{ext}");
        }
        return format!(
            "{}{}",
            &path[..path.len() - declaration_extension.len()],
            ext
        );
    }
    change_extension(path, new_extension)
}

/// Returns the possible original input extensions for an emitted `path`.
///
/// Side effects: none (pure).
// Go: internal/tspath/extension.go:GetPossibleOriginalInputExtensionForExtension
pub fn get_possible_original_input_extension_for_extension(path: &str) -> Vec<String> {
    if file_extension_is_one_of(path, &[EXTENSION_DMTS, EXTENSION_MJS, EXTENSION_MTS]) {
        return vec![EXTENSION_MTS.to_string(), EXTENSION_MJS.to_string()];
    }
    if file_extension_is_one_of(path, &[EXTENSION_DCTS, EXTENSION_CJS, EXTENSION_CTS]) {
        return vec![EXTENSION_CTS.to_string(), EXTENSION_CJS.to_string()];
    }
    // Handle any custom .d.x.ts extension (e.g. .d.json.ts -> .json).
    let ext = get_declaration_file_extension(path);
    if !ext.is_empty() && ext != EXTENSION_DTS {
        let inner = &ext[".d.".len()..ext.len() - ".ts".len()];
        return vec![format!(".{inner}")];
    }
    vec![
        EXTENSION_TSX.to_string(),
        EXTENSION_TS.to_string(),
        EXTENSION_JSX.to_string(),
        EXTENSION_JS.to_string(),
    ]
}

#[cfg(test)]
#[path = "extension_test.rs"]
mod tests;
