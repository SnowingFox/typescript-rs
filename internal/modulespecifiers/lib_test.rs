use super::*;
use crate::test_support::{MockHost, MockSourceFile};
use tsgo_collections::OrderedMap;
use tsgo_core::compileroptions::{CompilerOptions, ModuleResolutionKind};
use tsgo_packagejson::ExportsOrImports;
use tsgo_symlinks::{new_known_symlink, KnownDirectoryLink};
use tsgo_tspath::to_path;

fn bundler_opts() -> CompilerOptions {
    CompilerOptions {
        module_resolution: ModuleResolutionKind::Bundler,
        ..Default::default()
    }
}

// Go: internal/modulespecifiers/specifiers.go:getInfo
#[test]
fn get_info_computes_source_directory() {
    let host = MockHost::project();
    let info = get_info("/project/src/main.ts", &host);
    assert_eq!(info.importing_source_file_name, "/project/src/main.ts");
    assert_eq!(info.source_directory, "/project/src");
    assert!(info.use_case_sensitive_file_names);
}

// Go: internal/modulespecifiers/specifiers.go:processEnding
#[test]
fn process_ending_cases() {
    use ModuleSpecifierEnding::*;
    let host = MockHost::project();
    let opts = CompilerOptions::default();
    // Verbatim extensions are kept.
    assert_eq!(
        process_ending("/a/b.json", &[Minimal], &opts, &host),
        "/a/b.json"
    );
    // Minimal drops the extension.
    assert_eq!(process_ending("/a/b.ts", &[Minimal], &opts, &host), "/a/b");
    // JsExtension swaps to a .js extension.
    assert_eq!(
        process_ending("/a/b.ts", &[JsExtension], &opts, &host),
        "/a/b.js"
    );
    // .d.ts maps to .js under JsExtension.
    assert_eq!(
        process_ending("/a/b.d.ts", &[JsExtension], &opts, &host),
        "/a/b.js"
    );
}

#[test]
fn process_ending_minimal_index_uses_host() {
    use ModuleSpecifierEnding::*;
    let opts = CompilerOptions::default();
    // When a sibling file exists, `/index` cannot be dropped.
    let present = MockHost {
        current_dir: "/project".to_string(),
        use_case_sensitive_file_names: true,
        file_exists: true,
        ..Default::default()
    };
    assert_eq!(
        process_ending("/a/b/index.ts", &[Minimal], &opts, &present),
        "/a/b/index"
    );
    // When no sibling exists, `/index` is dropped.
    let absent = MockHost {
        current_dir: "/project".to_string(),
        use_case_sensitive_file_names: true,
        file_exists: false,
        ..Default::default()
    };
    assert_eq!(
        process_ending("/a/b/index.ts", &[Minimal], &opts, &absent),
        "/a/b"
    );
}

// Go: internal/modulespecifiers/specifiers.go:tryGetModuleNameFromRootDirs
#[test]
fn root_dirs_relative_specifier() {
    use ModuleSpecifierEnding::*;
    let host = MockHost::project();
    let opts = CompilerOptions::default();
    let result = try_get_module_name_from_root_dirs(
        &["/proj".to_string()],
        "/proj/lib/x.ts",
        "/proj/src",
        &[Minimal],
        &opts,
        &host,
    );
    assert_eq!(result, "../lib/x");
}

// Go: internal/modulespecifiers/specifiers.go:tryGetModuleNameFromPaths
#[test]
fn paths_wildcard_match() {
    use ModuleSpecifierEnding::*;
    let host = MockHost::project();
    let opts = CompilerOptions::default();
    let mut paths: OrderedMap<String, Vec<String>> = OrderedMap::default();
    paths.set("@app/*".to_string(), vec!["./src/*".to_string()]);
    let result =
        try_get_module_name_from_paths("src/foo", &paths, &[Minimal], "/proj", &host, &opts);
    assert_eq!(result, "@app/foo");
}

// Go: internal/modulespecifiers/specifiers.go:tryGetModuleNameFromExports
#[test]
fn exports_exact_string_match() {
    let host = MockHost::project();
    let opts = CompilerOptions::default();
    let exports: ExportsOrImports = tsgo_json::unmarshal(br#""./index.js""#).unwrap();
    let result = try_get_module_name_from_exports(
        &opts,
        &host,
        "/nm/mypkg/index.js",
        "/nm/mypkg",
        "mypkg",
        &exports,
        &[],
    );
    assert_eq!(result, "mypkg");
}

// Go: internal/modulespecifiers/specifiers.go:tryGetModuleNameFromPackageJsonImports
#[test]
fn package_json_imports_disabled_returns_empty() {
    let host = MockHost::project();
    let opts = CompilerOptions::default();
    // `resolvePackageJsonImports` is off by default -> "".
    let result = try_get_module_name_from_package_json_imports(
        "/proj/src/x.ts",
        "/proj/src",
        &opts,
        &host,
        tsgo_core::compileroptions::RESOLUTION_MODE_NONE,
        false,
    );
    assert_eq!(result, "");
}

// Go: internal/modulespecifiers/specifiers.go:getAllModulePathsWorker
#[test]
fn all_module_paths_worker_single_path() {
    let host = MockHost::project();
    let opts = CompilerOptions::default();
    let info = get_info("/project/src/main.ts", &host);
    let result = get_all_module_paths_worker(
        &info,
        "/project/lib/utils.ts",
        &host,
        &opts,
        ModuleSpecifierOptions::default(),
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].file_name, "/project/lib/utils.ts");
    assert!(!result[0].is_in_node_modules);
}

// Go: internal/modulespecifiers/specifiers.go:tryGetModuleNameAsNodeModule
#[test]
fn node_module_specifier_for_index() {
    let host = MockHost::project();
    let opts = CompilerOptions::default();
    let sf = MockSourceFile::default();
    let info = get_info("/project/src/main.ts", &host);
    let path = ModulePath {
        file_name: "/project/node_modules/lodash/index.js".to_string(),
        is_in_node_modules: true,
        is_redirect: false,
    };
    let result = try_get_module_name_as_node_module(
        &path,
        &info,
        &sf,
        &host,
        &opts,
        &UserPreferences::default(),
        false,
        tsgo_core::compileroptions::RESOLUTION_MODE_NONE,
    );
    assert_eq!(result, "lodash");
}

// Go: internal/modulespecifiers/specifiers.go:getLocalModuleSpecifier
#[test]
fn local_module_specifier_relative() {
    let host = MockHost::project();
    let opts = bundler_opts();
    let sf = MockSourceFile::default();
    let info = get_info("/project/src/main.ts", &host);
    let prefs = UserPreferences::default();
    let preferences =
        crate::preferences::get_module_specifier_preferences(&prefs, &host, &opts, &sf, "");
    let result = get_local_module_specifier(
        "/project/lib/utils.ts",
        &info,
        &opts,
        &host,
        tsgo_core::compileroptions::RESOLUTION_MODE_NONE,
        &preferences,
        false,
    );
    assert_eq!(result, "../lib/utils");
}

// Go: internal/modulespecifiers/specifiers.go:GetModuleSpecifiersForFileWithInfo (relative result)
#[test]
fn module_specifiers_for_file_relative() {
    let host = MockHost::project();
    let opts = bundler_opts();
    let sf = MockSourceFile::default();
    let (specifiers, kind) = get_module_specifiers_for_file_with_info(
        &sf,
        "/project/lib/utils.ts",
        &opts,
        &host,
        &UserPreferences::default(),
        ModuleSpecifierOptions::default(),
        false,
    );
    assert_eq!(specifiers, vec!["../lib/utils".to_string()]);
    assert_eq!(kind, ResultKind::Relative);
}

// Go: internal/modulespecifiers/specifiers_test.go:TestTryGetModuleNameFromExportsOrImports/with exports pattern
fn exports_pattern_target() -> ExportsOrImports {
    tsgo_json::unmarshal(br#""./src/things/*/index.js""#).unwrap()
}

#[test]
fn exports_pattern_match() {
    let host = MockHost::default();
    let result = try_get_module_name_from_exports_or_imports(
        &CompilerOptions::default(),
        &host,
        "/pkg/src/things/thing1/index.ts",
        "/pkg",
        "./src/things/*",
        &exports_pattern_target(),
        &[],
        MatchingMode::Pattern,
        false,
        false,
    );
    assert_eq!(result, "./src/things/thing1");
}

#[test]
fn exports_pattern_mismatch() {
    let host = MockHost::default();
    let result = try_get_module_name_from_exports_or_imports(
        &CompilerOptions::default(),
        &host,
        "/pkg/src/things/index.ts",
        "/pkg",
        "./src/things/*",
        &exports_pattern_target(),
        &[],
        MatchingMode::Pattern,
        false,
        false,
    );
    assert_eq!(result, "");
}

// Go: internal/modulespecifiers/specifiers_test.go:TestGetEachFileNameOfModule/basic file path
#[test]
fn each_file_basic_path() {
    let host = MockHost::project();
    let result = get_each_file_name_of_module(
        "/project/src/main.ts",
        "/project/lib/utils.ts",
        &host,
        false,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].file_name, "/project/lib/utils.ts");
    assert!(result.iter().all(|p| !p.file_name.is_empty()));
}

// Go: .../symlink preference false
#[test]
fn each_file_symlink_pref_false() {
    let host = MockHost::project();
    let result = get_each_file_name_of_module(
        "/project/src/main.ts",
        "/project/lib/utils.ts",
        &host,
        false,
    );
    assert_eq!(result.len(), 1);
    assert!(result.iter().all(|p| !p.file_name.is_empty()));
}

// Go: .../symlink preference true
#[test]
fn each_file_symlink_pref_true() {
    let host = MockHost::project();
    let result =
        get_each_file_name_of_module("/project/src/main.ts", "/project/lib/utils.ts", &host, true);
    assert_eq!(result.len(), 1);
    assert!(result.iter().all(|p| !p.file_name.is_empty()));
}

// Go: .../ignored path with no alternatives
#[test]
fn each_file_ignored_no_alternatives() {
    let host = MockHost::project();
    let result = get_each_file_name_of_module(
        "/project/src/main.ts",
        "/project/node_modules/.pnpm/file.ts",
        &host,
        false,
    );
    // Returns 1 because there is no better option (all paths are ignored).
    assert_eq!(result.len(), 1);
    assert!(result.iter().all(|p| !p.file_name.is_empty()));
}

// Go: internal/modulespecifiers/specifiers_test.go:TestGetEachFileNameOfModuleWithSymlinks
#[test]
fn each_file_with_symlink_dir() {
    let host = MockHost {
        current_dir: "/project".to_string(),
        use_case_sensitive_file_names: true,
        symlink_cache: Some(new_known_symlink("/project", true)),
        ..Default::default()
    };
    let symlink_path =
        to_path("/project/symlink", "/project", true).ensure_trailing_directory_separator();
    let real_directory = KnownDirectoryLink {
        real: "/real/path/".to_string(),
        real_path: to_path("/real/path", "/project", true).ensure_trailing_directory_separator(),
    };
    host.symlink_cache.as_ref().unwrap().set_directory(
        "/project/symlink",
        symlink_path,
        Some(real_directory),
    );

    let result =
        get_each_file_name_of_module("/project/src/main.ts", "/real/path/file.ts", &host, true);

    assert!(
        result
            .iter()
            .any(|p| p.file_name == "/project/symlink/file.ts"),
        "expected to find symlink path /project/symlink/file.ts, got {result:?}"
    );
}

// Go: internal/modulespecifiers/specifiers_test.go:TestContainsNodeModules
#[test]
fn contains_nm_true() {
    assert!(contains_node_modules(
        "/project/node_modules/lodash/index.js"
    ));
}

#[test]
fn contains_nm_false() {
    assert!(!contains_node_modules("/project/src/utils.ts"));
}

#[test]
fn contains_nm_middle() {
    assert!(contains_node_modules(
        "/project/packages/node_modules/pkg/file.js"
    ));
}

#[test]
fn contains_nm_empty() {
    assert!(!contains_node_modules(""));
}

// Go: internal/modulespecifiers/specifiers_test.go:TestContainsIgnoredPath
#[test]
fn ignored_pnpm() {
    assert!(contains_ignored_path("/project/node_modules/.pnpm/file.ts"));
}

#[test]
fn ignored_normal_false() {
    assert!(!contains_ignored_path("/project/src/file.ts"));
}
