use super::*;
use crate::test_support::{MockHost, MockSourceFile};
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_module::{Ending, ResolvedEntrypoint};

fn entrypoint(module_specifier: &str, ending: Ending) -> ResolvedEntrypoint {
    ResolvedEntrypoint {
        original_file_name: String::new(),
        resolved_file_name: String::new(),
        module_specifier: module_specifier.to_string(),
        ending,
        include_conditions: None,
        exclude_conditions: None,
    }
}

// Go: internal/modulespecifiers/util.go:ProcessEntrypointEnding
#[test]
fn process_entrypoint_ending_fixed_is_verbatim() {
    let host = MockHost::project();
    let opts = CompilerOptions::default();
    let sf = MockSourceFile::default();
    let ep = entrypoint("pkg/sub", Ending::Fixed);
    assert_eq!(
        process_entrypoint_ending(
            &ep,
            &UserPreferences::default(),
            &host,
            &opts,
            &sf,
            &[ModuleSpecifierEnding::Minimal]
        ),
        "pkg/sub"
    );
}

#[test]
fn process_entrypoint_ending_changeable_minimal_drops_index_js() {
    let host = MockHost::project();
    let opts = CompilerOptions::default();
    let sf = MockSourceFile::default();
    let ep = entrypoint("lodash/index.js", Ending::Changeable);
    assert_eq!(
        process_entrypoint_ending(
            &ep,
            &UserPreferences::default(),
            &host,
            &opts,
            &sf,
            &[ModuleSpecifierEnding::Minimal]
        ),
        "lodash"
    );
}

#[test]
fn process_entrypoint_ending_dts_to_js() {
    let host = MockHost::project();
    let opts = CompilerOptions::default();
    let sf = MockSourceFile::default();
    let ep = entrypoint("pkg/index.d.ts", Ending::Changeable);
    assert_eq!(
        process_entrypoint_ending(
            &ep,
            &UserPreferences::default(),
            &host,
            &opts,
            &sf,
            &[ModuleSpecifierEnding::JsExtension]
        ),
        "pkg/index.js"
    );
}

// Go: internal/modulespecifiers/tests.md supplementary (util.go:IsExcludedByRegex)
#[test]
fn is_excluded_by_regex_match() {
    assert!(is_excluded_by_regex("lodash", &["^lodash$".to_string()]));
}

#[test]
fn is_excluded_by_regex_no_match() {
    assert!(!is_excluded_by_regex("react", &["^lodash$".to_string()]));
}

// Go: internal/modulespecifiers/specifiers_test.go:TestTryGetRealFileNameForNonJSDeclarationFileName
#[test]
fn real_name_json_decl() {
    assert_eq!(
        try_get_real_file_name_for_non_js_declaration_file_name("/project/foo.d.json.ts"),
        "/project/foo.json"
    );
}

#[test]
fn real_name_multidot_decl() {
    assert_eq!(
        try_get_real_file_name_for_non_js_declaration_file_name("/project/foo.module.d.css.ts"),
        "/project/foo.module.css"
    );
}

#[test]
fn real_name_plain_dts_empty() {
    assert_eq!(
        try_get_real_file_name_for_non_js_declaration_file_name("/project/foo.d.ts"),
        ""
    );
}

// Go: internal/modulespecifiers/util.go:PathIsBareSpecifier (behavior coverage per PORTING §8.6)
#[test]
fn path_is_bare_specifier_classifies() {
    assert!(path_is_bare_specifier("lodash"));
    assert!(path_is_bare_specifier("@scope/pkg"));
    assert!(!path_is_bare_specifier("./a"));
    assert!(!path_is_bare_specifier("../a"));
    assert!(!path_is_bare_specifier("/a/b"));
}

// Go: internal/modulespecifiers/util.go:ensurePathIsNonModuleName
#[test]
fn ensure_path_is_non_module_name_dot_prefixes_bare() {
    assert_eq!(ensure_path_is_non_module_name("a/b"), "./a/b");
    assert_eq!(ensure_path_is_non_module_name("./a/b"), "./a/b");
    assert_eq!(ensure_path_is_non_module_name("/a/b"), "/a/b");
}

// Go: internal/modulespecifiers/util.go:GetJSExtensionForDeclarationFileExtension
#[test]
fn js_extension_for_declaration_file_extension_maps() {
    assert_eq!(
        get_js_extension_for_declaration_file_extension(".d.ts"),
        ".js"
    );
    assert_eq!(
        get_js_extension_for_declaration_file_extension(".d.mts"),
        ".mjs"
    );
    assert_eq!(
        get_js_extension_for_declaration_file_extension(".d.cts"),
        ".cjs"
    );
    assert_eq!(
        get_js_extension_for_declaration_file_extension(".d.json.ts"),
        ".json"
    );
}

// Go: internal/modulespecifiers/util.go:prefersTsExtension
#[test]
fn prefers_ts_extension_order() {
    use ModuleSpecifierEnding::*;
    assert!(prefers_ts_extension(&[TsExtension, JsExtension]));
    assert!(!prefers_ts_extension(&[JsExtension, TsExtension]));
    // ts present but js absent -> false (Go: tsPriority < -1)
    assert!(!prefers_ts_extension(&[Minimal, TsExtension]));
    // ts absent -> false
    assert!(!prefers_ts_extension(&[Minimal, JsExtension]));
}

// Go: internal/modulespecifiers/util.go:replaceFirstStar
#[test]
fn replace_first_star_replaces_only_first() {
    assert_eq!(replace_first_star("./a/*", "thing1"), "./a/thing1");
    assert_eq!(replace_first_star("a/*/b/*", "X"), "a/X/b/*");
    assert_eq!(replace_first_star("no-star", "X"), "no-star");
}

// Go: internal/modulespecifiers/util.go:isPathRelativeToParent
#[test]
fn is_path_relative_to_parent_detects_dotdot() {
    assert!(is_path_relative_to_parent("../a"));
    assert!(!is_path_relative_to_parent("./a"));
    assert!(!is_path_relative_to_parent("a"));
}

// Go: internal/modulespecifiers/util.go:packageJsonPathsAreEqual
#[test]
fn package_json_paths_are_equal_compares() {
    let opts = ComparePathsOptions {
        use_case_sensitive_file_names: true,
        current_directory: "/".to_string(),
    };
    assert!(package_json_paths_are_equal("/a/p", "/a/p", &opts));
    assert!(!package_json_paths_are_equal("", "/a/p", &opts));
    assert!(!package_json_paths_are_equal("/a/p", "", &opts));
    assert!(package_json_paths_are_equal("/a/b/../p", "/a/p", &opts));
    assert!(!package_json_paths_are_equal("/a/p", "/a/q", &opts));
}

// Go: internal/modulespecifiers/util.go:comparePathsByRedirect
#[test]
fn compare_paths_by_redirect_orders_non_redirect_first() {
    let redirect = ModulePath {
        file_name: "/a.ts".to_string(),
        is_in_node_modules: false,
        is_redirect: true,
    };
    let non_redirect = ModulePath {
        file_name: "/z.ts".to_string(),
        is_in_node_modules: false,
        is_redirect: false,
    };
    assert_eq!(
        compare_paths_by_redirect(&redirect, &non_redirect, true),
        std::cmp::Ordering::Greater
    );
    assert_eq!(
        compare_paths_by_redirect(&non_redirect, &redirect, true),
        std::cmp::Ordering::Less
    );
    // Same redirect flag -> compare by file name.
    assert_eq!(
        compare_paths_by_redirect(&non_redirect, &non_redirect, true),
        std::cmp::Ordering::Equal
    );
}

// Go: internal/modulespecifiers/util.go:getRelativePathIfInSameVolume
#[test]
fn relative_path_if_in_same_volume_returns_relative() {
    assert_eq!(
        get_relative_path_if_in_same_volume("/proj/lib/x.ts", "/proj/src", true),
        "../lib/x.ts"
    );
}

// Go: internal/modulespecifiers/util.go:getPathsRelativeToRootDirs
#[test]
fn paths_relative_to_root_dirs_filters_parent() {
    // "/proj/lib/x.ts" is under "/proj" -> "lib/x.ts" (kept); under "/other" it
    // would escape with ".." and is dropped.
    let result = get_paths_relative_to_root_dirs("/proj/lib/x.ts", &["/proj".to_string()], true);
    assert_eq!(result, vec!["lib/x.ts".to_string()]);
}

// Go: internal/modulespecifiers/util.go:GetPackageNameFromDirectory
#[test]
fn package_name_from_directory_handles_scopes() {
    assert_eq!(
        get_package_name_from_directory("/p/node_modules/lodash/index.js"),
        "lodash"
    );
    assert_eq!(
        get_package_name_from_directory("/p/node_modules/@a/b/file.js"),
        "@a/b"
    );
    assert_eq!(get_package_name_from_directory("/p/src/utils.ts"), "");
    assert_eq!(
        get_package_name_from_directory("/p/node_modules/.bin/x"),
        ""
    );
}

// Go: internal/modulespecifiers/util.go:GetNodeModulePathParts
#[test]
fn node_module_path_parts_indices() {
    let parts = get_node_module_path_parts("/a/node_modules/pkg/file.ts").unwrap();
    assert_eq!(parts.top_level_node_modules_index, 2);
    assert_eq!(parts.top_level_package_name_index, 15);
    assert_eq!(parts.package_root_index, 19);
    assert_eq!(parts.file_name_index, 19);
    assert!(get_node_module_path_parts("/a/src/file.ts").is_none());
}

// Go: internal/modulespecifiers/util.go:getJSExtensionForFile
#[test]
fn js_extension_for_file_maps_ts_to_js() {
    let opts = CompilerOptions::default();
    assert_eq!(get_js_extension_for_file("/a/foo.ts", &opts), ".js");
    assert_eq!(get_js_extension_for_file("/a/foo.mts", &opts), ".mjs");
}

#[test]
#[should_panic(expected = "unknown extension")]
fn js_extension_for_file_panics_on_unknown() {
    // For an unknown extension, `extension_from_path` (evaluated to build the
    // panic message) panics first, matching Go's eager argument evaluation.
    let opts = CompilerOptions::default();
    let _ = get_js_extension_for_file("/a/foo.txt", &opts);
}

// Go: internal/modulespecifiers/util.go:extensionFromPath
#[test]
fn extension_from_path_returns_known_ext() {
    assert_eq!(extension_from_path("/a/foo.ts"), ".ts");
    assert_eq!(extension_from_path("/a/foo.json"), ".json");
}

#[test]
#[should_panic(expected = "unknown extension")]
fn extension_from_path_panics_when_missing() {
    let _ = extension_from_path("/a/foo");
}

// Go: internal/modulespecifiers/util.go:tryGetAnyFileFromPath
#[test]
fn try_get_any_file_from_path_respects_file_exists() {
    let present = MockHost {
        current_dir: "/project".to_string(),
        use_case_sensitive_file_names: true,
        file_exists: true,
        ..Default::default()
    };
    assert!(try_get_any_file_from_path(&present, "/project/lib/foo"));

    let absent = MockHost {
        current_dir: "/project".to_string(),
        use_case_sensitive_file_names: true,
        file_exists: false,
        ..Default::default()
    };
    assert!(!try_get_any_file_from_path(&absent, "/project/lib/foo"));
}

// Go: internal/modulespecifiers/util.go:allKeysStartWithDot
#[test]
fn all_keys_start_with_dot_classifies() {
    let subpaths: ExportsOrImports =
        tsgo_json::unmarshal(br#"{"./a":"./a.js","./b":"./b.js"}"#).unwrap();
    assert!(all_keys_start_with_dot(subpaths.as_object()));

    let conditions: ExportsOrImports =
        tsgo_json::unmarshal(br#"{"import":"./a.js","require":"./a.cjs"}"#).unwrap();
    assert!(!all_keys_start_with_dot(conditions.as_object()));
}
