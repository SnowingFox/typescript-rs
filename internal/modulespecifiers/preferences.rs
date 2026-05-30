//! Translating `UserPreferences` + `CompilerOptions` into a concrete relative
//! preference and an ordered list of acceptable specifier endings.
//!
//! 1:1 port of Go `internal/modulespecifiers/preferences.go`.
//!
//! # Divergence from Go
//! Go's `ModuleSpecifierPreferences` stores a closure
//! (`getAllowedEndingsInPreferredOrder`) that captures the host, options, and
//! importing file. The Rust struct keeps the same boxed closure but carries a
//! lifetime tying it to those borrows.

use tsgo_core::compileroptions::{
    CompilerOptions, ModuleResolutionKind, ResolutionMode, RESOLUTION_MODE_ESM,
    RESOLUTION_MODE_NONE,
};
use tsgo_tspath::{
    file_extension_is_one_of, has_js_file_extension, has_ts_file_extension,
    is_declaration_file_name, is_external_module_name_relative, path_is_relative,
    EXTENSIONS_NOT_SUPPORTING_EXTENSIONLESS_RESOLUTION,
};

use crate::types::{
    ImportModuleSpecifierEndingPreference, ImportModuleSpecifierPreference, ModuleSpecifierEnding,
    ModuleSpecifierGenerationHost, RelativePreferenceKind, SourceFileForSpecifierGeneration,
    UserPreferences,
};

// Go: internal/modulespecifiers/preferences.go:shouldAllowImportingTsExtension
//
// Program errors validate that `noEmit`/`emitDeclarationOnly` is also set, so
// this does not re-check them (to avoid propagating errors).
pub(crate) fn should_allow_importing_ts_extension(
    compiler_options: &CompilerOptions,
    from_file_name: &str,
) -> bool {
    compiler_options.get_allow_importing_ts_extensions()
        || (!from_file_name.is_empty() && is_declaration_file_name(from_file_name))
}

// Go: internal/modulespecifiers/preferences.go:usesExtensionsOnImports
pub(crate) fn uses_extensions_on_imports(file: &dyn SourceFileForSpecifierGeneration) -> bool {
    for text in file.imports() {
        if path_is_relative(&text)
            && !file_extension_is_one_of(&text, EXTENSIONS_NOT_SUPPORTING_EXTENSIONLESS_RESOLUTION)
        {
            return has_ts_file_extension(&text) || has_js_file_extension(&text);
        }
    }
    false
}

// Go: internal/modulespecifiers/preferences.go:inferPreference
pub(crate) fn infer_preference(
    resolution_mode: ResolutionMode,
    source_file: &dyn SourceFileForSpecifierGeneration,
    module_resolution_is_nodenext: bool,
) -> ModuleSpecifierEnding {
    let mut uses_js_extensions = false;
    // JS `require(...)` detection at the top of a file is not yet ported
    // (matches Go's TODO); only `Imports()` contributes specifiers.
    let specifiers = source_file.imports();

    for path in specifiers {
        if path_is_relative(&path) {
            if module_resolution_is_nodenext
                && resolution_mode == tsgo_core::compileroptions::RESOLUTION_MODE_COMMON_JS
            {
                // Deciding a CommonJS specifier while looking at an ESM import.
                continue;
            }
            if file_extension_is_one_of(&path, EXTENSIONS_NOT_SUPPORTING_EXTENSIONLESS_RESOLUTION) {
                // These extensions are not optional, so do not indicate a preference.
                continue;
            }
            if has_ts_file_extension(&path) {
                return ModuleSpecifierEnding::TsExtension;
            }
            if has_js_file_extension(&path) {
                uses_js_extensions = true;
            }
        }
    }

    if uses_js_extensions {
        ModuleSpecifierEnding::JsExtension
    } else {
        ModuleSpecifierEnding::Minimal
    }
}

// Go: internal/modulespecifiers/preferences.go:getModuleSpecifierEndingPreference
pub(crate) fn get_module_specifier_ending_preference(
    pref: ImportModuleSpecifierEndingPreference,
    resolution_mode: ResolutionMode,
    compiler_options: &CompilerOptions,
    source_file: &dyn SourceFileForSpecifierGeneration,
) -> ModuleSpecifierEnding {
    let module_resolution = compiler_options.get_module_resolution_kind();
    let is_nodenext = module_resolution_is_nodenext(module_resolution);

    if pref == ImportModuleSpecifierEndingPreference::Js
        || (resolution_mode == RESOLUTION_MODE_ESM && is_nodenext)
    {
        // Extensions are explicitly requested or required; choose .js vs .ts.
        if !should_allow_importing_ts_extension(compiler_options, "") {
            return ModuleSpecifierEnding::JsExtension;
        }
        // `allowImportingTsExtensions` is a strong signal: prefer .ts unless the
        // file already uses .js extensions and no .ts extensions.
        if infer_preference(resolution_mode, source_file, is_nodenext)
            != ModuleSpecifierEnding::JsExtension
        {
            return ModuleSpecifierEnding::TsExtension;
        }
        return ModuleSpecifierEnding::JsExtension;
    }

    if pref == ImportModuleSpecifierEndingPreference::Minimal {
        return ModuleSpecifierEnding::Minimal;
    }

    if pref == ImportModuleSpecifierEndingPreference::Index {
        return ModuleSpecifierEnding::Index;
    }

    // No preference: guess from imports/requires whether .js/.ts/extensionless
    // is preferred. (`Index` detection is intentionally unsupported.)
    if !should_allow_importing_ts_extension(compiler_options, "") {
        if uses_extensions_on_imports(source_file) {
            return ModuleSpecifierEnding::JsExtension;
        }
        return ModuleSpecifierEnding::Minimal;
    }

    infer_preference(resolution_mode, source_file, is_nodenext)
}

// Go: internal/modulespecifiers/preferences.go:getPreferredEnding
pub(crate) fn get_preferred_ending(
    prefs: &UserPreferences,
    host: &dyn ModuleSpecifierGenerationHost,
    compiler_options: &CompilerOptions,
    importing_source_file: &dyn SourceFileForSpecifierGeneration,
    old_import_specifier: &str,
    resolution_mode: ResolutionMode,
) -> ModuleSpecifierEnding {
    if !old_import_specifier.is_empty() {
        if has_js_file_extension(old_import_specifier) {
            return ModuleSpecifierEnding::JsExtension;
        }
        if old_import_specifier.ends_with("/index") {
            return ModuleSpecifierEnding::Index;
        }
    }
    let mut resolution_mode = resolution_mode;
    if resolution_mode == RESOLUTION_MODE_NONE {
        resolution_mode = host.get_default_resolution_mode_for_file(importing_source_file);
    }
    get_module_specifier_ending_preference(
        prefs.import_module_specifier_ending,
        resolution_mode,
        compiler_options,
        importing_source_file,
    )
}

/// Returns the acceptable specifier endings in descending preference order.
///
/// The result depends on the user's ending preference, whether `.ts` imports
/// are allowed, and whether the target is an ESM module under
/// `node16`/`nodenext` resolution.
///
/// # Examples
/// Behavior is exercised by the crate's `preferences_test.rs`; the function
/// requires a [`ModuleSpecifierGenerationHost`] and a
/// [`SourceFileForSpecifierGeneration`], so it is not shown as a doctest.
///
/// Side effects: none beyond reading the supplied host.
// Go: internal/modulespecifiers/preferences.go:GetAllowedEndingsInPreferredOrder
pub fn get_allowed_endings_in_preferred_order(
    prefs: &UserPreferences,
    host: &dyn ModuleSpecifierGenerationHost,
    compiler_options: &CompilerOptions,
    importing_source_file: &dyn SourceFileForSpecifierGeneration,
    old_import_specifier: &str,
    syntax_implied_node_format: ResolutionMode,
) -> Vec<ModuleSpecifierEnding> {
    use ModuleSpecifierEnding::*;

    let mut preferred_ending = get_preferred_ending(
        prefs,
        host,
        compiler_options,
        importing_source_file,
        old_import_specifier,
        RESOLUTION_MODE_NONE,
    );
    let resolution_mode = host.get_default_resolution_mode_for_file(importing_source_file);
    if resolution_mode != syntax_implied_node_format {
        preferred_ending = get_preferred_ending(
            prefs,
            host,
            compiler_options,
            importing_source_file,
            old_import_specifier,
            syntax_implied_node_format,
        );
    }
    let module_resolution = compiler_options.get_module_resolution_kind();
    let is_nodenext = module_resolution_is_nodenext(module_resolution);
    let allow_importing_ts_extension =
        should_allow_importing_ts_extension(compiler_options, &importing_source_file.file_name());

    if syntax_implied_node_format == RESOLUTION_MODE_ESM && is_nodenext {
        if allow_importing_ts_extension {
            return vec![TsExtension, JsExtension];
        }
        return vec![JsExtension];
    }

    match preferred_ending {
        JsExtension => {
            if allow_importing_ts_extension {
                vec![JsExtension, TsExtension, Minimal, Index]
            } else {
                vec![JsExtension, Minimal, Index]
            }
        }
        TsExtension => vec![TsExtension, Minimal, JsExtension, Index],
        Index => {
            if allow_importing_ts_extension {
                vec![Index, Minimal, TsExtension, JsExtension]
            } else {
                vec![Index, Minimal, JsExtension]
            }
        }
        Minimal => {
            if allow_importing_ts_extension {
                vec![Minimal, Index, TsExtension, JsExtension]
            } else {
                vec![Minimal, Index, JsExtension]
            }
        }
    }
}

/// The resolved relative preference plus a recompute-able allowed-endings
/// callback and the auto-import exclusion regexes.
///
/// Side effects: none (plain data + a captured closure).
// Go: internal/modulespecifiers/preferences.go:ModuleSpecifierPreferences
pub struct ModuleSpecifierPreferences<'a> {
    pub(crate) relative_preference: RelativePreferenceKind,
    pub(crate) exclude_regexes: Vec<String>,
    #[allow(clippy::type_complexity)]
    get_allowed_endings: Box<dyn Fn(ResolutionMode) -> Vec<ModuleSpecifierEnding> + 'a>,
}

impl ModuleSpecifierPreferences<'_> {
    /// Returns the allowed endings for the given syntax-implied node format.
    ///
    /// Side effects: none beyond the captured host read.
    pub(crate) fn get_allowed_endings_in_preferred_order(
        &self,
        syntax_implied_node_format: ResolutionMode,
    ) -> Vec<ModuleSpecifierEnding> {
        (self.get_allowed_endings)(syntax_implied_node_format)
    }
}

// Go: internal/modulespecifiers/preferences.go:getModuleSpecifierPreferences
pub(crate) fn get_module_specifier_preferences<'a>(
    prefs: &'a UserPreferences,
    host: &'a dyn ModuleSpecifierGenerationHost,
    compiler_options: &'a CompilerOptions,
    importing_source_file: &'a dyn SourceFileForSpecifierGeneration,
    old_import_specifier: &str,
) -> ModuleSpecifierPreferences<'a> {
    let exclude_regexes = prefs.auto_import_specifier_exclude_regexes.clone();
    let mut relative_preference = RelativePreferenceKind::Shortest;
    if !old_import_specifier.is_empty() {
        relative_preference = if is_external_module_name_relative(old_import_specifier) {
            RelativePreferenceKind::Relative
        } else {
            RelativePreferenceKind::NonRelative
        };
    } else {
        match prefs.import_module_specifier_preference {
            ImportModuleSpecifierPreference::Relative => {
                relative_preference = RelativePreferenceKind::Relative;
            }
            ImportModuleSpecifierPreference::NonRelative => {
                relative_preference = RelativePreferenceKind::NonRelative;
            }
            ImportModuleSpecifierPreference::ProjectRelative => {
                relative_preference = RelativePreferenceKind::ExternalNonRelative;
            }
            // None / Shortest -> Shortest.
            _ => {}
        }
    }

    let old_import_specifier = old_import_specifier.to_string();
    let get_allowed_endings = Box::new(move |syntax_implied_node_format: ResolutionMode| {
        get_allowed_endings_in_preferred_order(
            prefs,
            host,
            compiler_options,
            importing_source_file,
            &old_import_specifier,
            syntax_implied_node_format,
        )
    });

    ModuleSpecifierPreferences {
        relative_preference,
        exclude_regexes,
        get_allowed_endings,
    }
}

/// Reports whether `module_resolution` is in the `node16`..=`nodenext` range
/// (mirrors Go's `Node16 <= mr <= NodeNext`).
fn module_resolution_is_nodenext(module_resolution: ModuleResolutionKind) -> bool {
    let mr = module_resolution as i32;
    (ModuleResolutionKind::Node16 as i32) <= mr && mr <= (ModuleResolutionKind::NodeNext as i32)
}

#[cfg(test)]
#[path = "preferences_test.rs"]
mod tests;
