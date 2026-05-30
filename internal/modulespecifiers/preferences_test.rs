use super::*;
use crate::test_support::{MockHost, MockSourceFile};
use tsgo_core::compileroptions::{CompilerOptions, ModuleResolutionKind};
use tsgo_core::tristate::Tristate;

// Go: internal/modulespecifiers/preferences.go:shouldAllowImportingTsExtension
#[test]
fn should_allow_importing_ts_extension_cases() {
    let mut opts = CompilerOptions::default();
    assert!(!should_allow_importing_ts_extension(&opts, "/a/b.ts"));
    // Declaration files always allow it.
    assert!(should_allow_importing_ts_extension(&opts, "/a/b.d.ts"));
    // The option forces it on.
    opts.allow_importing_ts_extensions = Tristate::True;
    assert!(should_allow_importing_ts_extension(&opts, "/a/b.ts"));
}

// Go: internal/modulespecifiers/preferences.go:usesExtensionsOnImports
#[test]
fn uses_extensions_on_imports_detects_relative_with_extension() {
    let with_ext = MockSourceFile {
        imports: vec!["./a.js".to_string()],
        ..Default::default()
    };
    assert!(uses_extensions_on_imports(&with_ext));

    let without_ext = MockSourceFile {
        imports: vec!["./a".to_string(), "lodash".to_string()],
        ..Default::default()
    };
    assert!(!uses_extensions_on_imports(&without_ext));
}

// Go: internal/modulespecifiers/preferences.go:inferPreference
#[test]
fn infer_preference_prefers_ts_then_js_then_minimal() {
    use tsgo_core::compileroptions::RESOLUTION_MODE_NONE;

    let ts = MockSourceFile {
        imports: vec!["./a.ts".to_string()],
        ..Default::default()
    };
    assert_eq!(
        infer_preference(RESOLUTION_MODE_NONE, &ts, false),
        ModuleSpecifierEnding::TsExtension
    );

    let js = MockSourceFile {
        imports: vec!["./a.js".to_string()],
        ..Default::default()
    };
    assert_eq!(
        infer_preference(RESOLUTION_MODE_NONE, &js, false),
        ModuleSpecifierEnding::JsExtension
    );

    let none = MockSourceFile {
        imports: vec!["./a".to_string()],
        ..Default::default()
    };
    assert_eq!(
        infer_preference(RESOLUTION_MODE_NONE, &none, false),
        ModuleSpecifierEnding::Minimal
    );
}

// Go: internal/modulespecifiers/preferences.go:getModuleSpecifierEndingPreference
#[test]
fn ending_preference_minimal_and_index_honored() {
    let opts = CompilerOptions::default();
    let sf = MockSourceFile::default();
    use tsgo_core::compileroptions::RESOLUTION_MODE_NONE;
    assert_eq!(
        get_module_specifier_ending_preference(
            ImportModuleSpecifierEndingPreference::Minimal,
            RESOLUTION_MODE_NONE,
            &opts,
            &sf
        ),
        ModuleSpecifierEnding::Minimal
    );
    assert_eq!(
        get_module_specifier_ending_preference(
            ImportModuleSpecifierEndingPreference::Index,
            RESOLUTION_MODE_NONE,
            &opts,
            &sf
        ),
        ModuleSpecifierEnding::Index
    );
}

// Go: internal/modulespecifiers/tests.md supplementary (GetAllowedEndingsInPreferredOrder)
#[test]
fn allowed_endings_minimal_default() {
    let prefs = UserPreferences::default();
    let opts = CompilerOptions {
        module_resolution: ModuleResolutionKind::Bundler,
        ..Default::default()
    };
    let host = MockHost::default();
    let sf = MockSourceFile::default();
    let result = get_allowed_endings_in_preferred_order(
        &prefs,
        &host,
        &opts,
        &sf,
        "",
        tsgo_core::compileroptions::RESOLUTION_MODE_NONE,
    );
    assert_eq!(
        result,
        vec![
            ModuleSpecifierEnding::Minimal,
            ModuleSpecifierEnding::Index,
            ModuleSpecifierEnding::JsExtension,
        ]
    );
}

// Go: internal/modulespecifiers/tests.md supplementary (GetAllowedEndingsInPreferredOrder)
#[test]
fn allowed_endings_nodenext_esm_js() {
    let prefs = UserPreferences::default();
    let opts = CompilerOptions {
        module_resolution: ModuleResolutionKind::NodeNext,
        ..Default::default()
    };
    let host = MockHost::default();
    let sf = MockSourceFile::default();
    let result = get_allowed_endings_in_preferred_order(
        &prefs,
        &host,
        &opts,
        &sf,
        "",
        tsgo_core::compileroptions::RESOLUTION_MODE_ESM,
    );
    assert_eq!(result, vec![ModuleSpecifierEnding::JsExtension]);
}

// Go: internal/modulespecifiers/preferences.go:getModuleSpecifierPreferences
#[test]
fn module_specifier_preferences_relative_preference() {
    let host = MockHost::default();
    let opts = CompilerOptions::default();
    let sf = MockSourceFile::default();

    let default_prefs = UserPreferences::default();
    let p = get_module_specifier_preferences(&default_prefs, &host, &opts, &sf, "");
    assert_eq!(p.relative_preference, RelativePreferenceKind::Shortest);

    let relative_prefs = UserPreferences {
        import_module_specifier_preference: ImportModuleSpecifierPreference::Relative,
        ..Default::default()
    };
    let p = get_module_specifier_preferences(&relative_prefs, &host, &opts, &sf, "");
    assert_eq!(p.relative_preference, RelativePreferenceKind::Relative);

    // An old relative specifier forces Relative regardless of preference.
    let p = get_module_specifier_preferences(&default_prefs, &host, &opts, &sf, "../x");
    assert_eq!(p.relative_preference, RelativePreferenceKind::Relative);

    // An old bare specifier forces NonRelative.
    let p = get_module_specifier_preferences(&default_prefs, &host, &opts, &sf, "lodash");
    assert_eq!(p.relative_preference, RelativePreferenceKind::NonRelative);
}
