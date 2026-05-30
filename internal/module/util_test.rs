use std::cmp::Ordering;

use tsgo_core::compileroptions::{CompilerOptions, JsxEmit};
use tsgo_core::tristate::Tristate;

use super::*;

// Go: internal/module/util.go:IsApplicableVersionedTypesKey
#[test]
fn is_applicable_versioned_types_key_behaviors() {
    // The current compiler version is always >= 1.0.
    assert!(is_applicable_versioned_types_key("types@>=1.0"));
    // Missing `types@` prefix.
    assert!(!is_applicable_versioned_types_key("foo"));
    // Not a valid semver range.
    assert!(!is_applicable_versioned_types_key("types@not a range"));
    // A version that the current compiler cannot satisfy.
    assert!(!is_applicable_versioned_types_key("types@>=9999"));
}

// Go: internal/module/util.go:ParseNodeModuleFromPath
#[test]
fn parse_node_module_from_path_plain() {
    assert_eq!(
        parse_node_module_from_path("/x/node_modules/pkg/a.js", false),
        "/x/node_modules/pkg"
    );
}

// Go: internal/module/util.go:ParseNodeModuleFromPath
#[test]
fn parse_node_module_from_path_scoped() {
    assert_eq!(
        parse_node_module_from_path("/x/node_modules/@s/pkg/a.js", false),
        "/x/node_modules/@s/pkg"
    );
}

// Go: internal/module/util.go:ParseNodeModuleFromPath
#[test]
fn parse_node_module_from_path_not_in_node_modules() {
    assert_eq!(parse_node_module_from_path("/x/y/a.js", false), "");
}

// Go: internal/module/util.go:ParsePackageName
#[test]
fn parse_package_name_plain() {
    assert_eq!(
        parse_package_name("foo/bar/baz"),
        ("foo".to_string(), "bar/baz".to_string())
    );
}

// Go: internal/module/util.go:ParsePackageName
#[test]
fn parse_package_name_scoped() {
    assert_eq!(
        parse_package_name("@a/b/c"),
        ("@a/b".to_string(), "c".to_string())
    );
}

// Go: internal/module/util.go:ParsePackageName
#[test]
fn parse_package_name_no_slash() {
    assert_eq!(
        parse_package_name("foo"),
        ("foo".to_string(), String::new())
    );
    // Scoped name without a trailing subpath.
    assert_eq!(parse_package_name("@a"), ("@a".to_string(), String::new()));
}

// Go: internal/module/util.go:MangleScopedPackageName
#[test]
fn mangle_scoped() {
    assert_eq!(mangle_scoped_package_name("@a/b"), "a__b");
}

// Go: internal/module/util.go:MangleScopedPackageName
#[test]
fn mangle_non_scoped() {
    assert_eq!(mangle_scoped_package_name("foo"), "foo");
    // Scoped but without a slash is returned unchanged.
    assert_eq!(mangle_scoped_package_name("@a"), "@a");
}

// Go: internal/module/util.go:UnmangleScopedPackageName
#[test]
fn unmangle_scoped() {
    assert_eq!(unmangle_scoped_package_name("a__b"), "@a/b");
    assert_eq!(unmangle_scoped_package_name("foo"), "foo");
}

// Go: internal/module/util.go:GetTypesPackageName
#[test]
fn types_package_name() {
    assert_eq!(get_types_package_name("@a/b"), "@types/a__b");
    assert_eq!(get_types_package_name("foo"), "@types/foo");
}

// Go: internal/module/util.go:GetPackageNameFromTypesPackageName
#[test]
fn pkg_name_from_types() {
    assert_eq!(
        get_package_name_from_types_package_name("@types/a__b"),
        "@a/b"
    );
    assert_eq!(get_package_name_from_types_package_name("foo"), "foo");
}

// Go: internal/module/util.go:ComparePatternKeys
#[test]
fn compare_pattern_keys_base_len() {
    // Longer fixed prefix sorts first (Go returns -1 = Less).
    assert_eq!(compare_pattern_keys("ab/*", "a/*"), Ordering::Less);
    assert_eq!(compare_pattern_keys("a/*", "ab/*"), Ordering::Greater);
}

// Go: internal/module/util.go:ComparePatternKeys
#[test]
fn compare_pattern_keys_no_star() {
    // Equal base length: a pattern outranks an exact key.
    assert_eq!(compare_pattern_keys("ab", "a*"), Ordering::Greater);
    assert_eq!(compare_pattern_keys("a*", "ab"), Ordering::Less);
}

// Go: internal/module/util.go:ComparePatternKeys
#[test]
fn compare_pattern_keys_len_tiebreak() {
    // Equal base, both patterns: the longer key sorts first.
    assert_eq!(compare_pattern_keys("a*bc", "a*b"), Ordering::Less);
    assert_eq!(compare_pattern_keys("a*b", "a*bc"), Ordering::Greater);
    assert_eq!(compare_pattern_keys("a*", "a*"), Ordering::Equal);
}

// Go: internal/module/util.go:TryGetJSExtensionForFile
#[test]
fn try_get_js_extension_for_file_behaviors() {
    let opts = CompilerOptions::default();
    assert_eq!(try_get_js_extension_for_file("a.ts", &opts), ".js");
    assert_eq!(try_get_js_extension_for_file("a.d.ts", &opts), ".js");
    assert_eq!(try_get_js_extension_for_file("a.mts", &opts), ".mjs");
    assert_eq!(try_get_js_extension_for_file("a.cts", &opts), ".cjs");
    assert_eq!(try_get_js_extension_for_file("a.js", &opts), ".js");
    assert_eq!(try_get_js_extension_for_file("a.json", &opts), ".json");
    assert_eq!(try_get_js_extension_for_file("a.css", &opts), "");
    // .tsx maps to .js unless jsx=preserve.
    assert_eq!(try_get_js_extension_for_file("a.tsx", &opts), ".js");
    let preserve = CompilerOptions {
        jsx: JsxEmit::Preserve,
        ..Default::default()
    };
    assert_eq!(try_get_js_extension_for_file("a.tsx", &preserve), ".jsx");
}

// Go: internal/module/util.go:GetResolutionDiagnostic
#[test]
fn get_resolution_diagnostic_ts_extension_is_allowed() {
    let opts = CompilerOptions::default();
    let m = ResolvedModule {
        extension: ".ts".into(),
        ..Default::default()
    };
    assert!(get_resolution_diagnostic(&opts, &m, false).is_none());
}

// Go: internal/module/util.go:GetResolutionDiagnostic
#[test]
fn get_resolution_diagnostic_json_requires_resolve_json_module() {
    let off = CompilerOptions {
        resolve_json_module: Tristate::False,
        ..Default::default()
    };
    let m = ResolvedModule {
        extension: ".json".into(),
        ..Default::default()
    };
    let diag = get_resolution_diagnostic(&off, &m, false).expect("expected a diagnostic");
    assert_eq!(diag.code(), 7042);

    let on = CompilerOptions {
        resolve_json_module: Tristate::True,
        ..Default::default()
    };
    assert!(get_resolution_diagnostic(&on, &m, false).is_none());
}

// Go: internal/module/util.go:GetResolutionDiagnostic
#[test]
fn get_resolution_diagnostic_jsx_requires_jsx_option() {
    let opts = CompilerOptions::default();
    let m = ResolvedModule {
        extension: ".tsx".into(),
        ..Default::default()
    };
    let diag = get_resolution_diagnostic(&opts, &m, false).expect("expected a diagnostic");
    assert_eq!(diag.code(), 6142);

    let with_jsx = CompilerOptions {
        jsx: JsxEmit::Preserve,
        ..Default::default()
    };
    assert!(get_resolution_diagnostic(&with_jsx, &m, false).is_none());
}

// Go: internal/module/util.go:GetResolutionDiagnostic
#[test]
fn get_resolution_diagnostic_js_needs_allow_js_under_strict() {
    let strict = CompilerOptions {
        strict: Tristate::True,
        ..Default::default()
    };
    let m = ResolvedModule {
        extension: ".js".into(),
        ..Default::default()
    };
    let diag = get_resolution_diagnostic(&strict, &m, false).expect("expected a diagnostic");
    assert_eq!(diag.code(), 7016);

    // allowJs suppresses the diagnostic.
    let strict_allow_js = CompilerOptions {
        strict: Tristate::True,
        allow_js: Tristate::True,
        ..Default::default()
    };
    assert!(get_resolution_diagnostic(&strict_allow_js, &m, false).is_none());
}

// Go: internal/module/util.go:GetResolutionDiagnostic
#[test]
fn get_resolution_diagnostic_arbitrary_extension() {
    let opts = CompilerOptions::default();
    let m = ResolvedModule {
        extension: ".css".into(),
        ..Default::default()
    };
    // Non-declaration file without allowArbitraryExtensions -> diagnostic.
    let diag = get_resolution_diagnostic(&opts, &m, false).expect("expected a diagnostic");
    assert_eq!(diag.code(), 6263);
    // Declaration files are exempt.
    assert!(get_resolution_diagnostic(&opts, &m, true).is_none());
}
