use tsgo_collections::OrderedMap;
use tsgo_core::compileroptions::{CompilerOptions, ModuleKind, ModuleResolutionKind};

use crate::test_support::resolver;

// Go: internal/module/resolver.go:tryLoadModuleUsingPaths
#[test]
fn paths_wildcard_mapping() {
    let mut paths: OrderedMap<String, Vec<String>> = OrderedMap::default();
    paths.set("@app/*".to_string(), vec!["./src/*".to_string()]);
    let opts = CompilerOptions {
        module_resolution: ModuleResolutionKind::Bundler,
        paths: Some(paths),
        ..Default::default()
    };
    let files = [("/repo/src/thing.ts", ""), ("/repo/main.ts", "")];
    let r = resolver(&files, "/repo", opts);
    let (res, _) = r.resolve_module_name("@app/thing", "/repo/main.ts", ModuleKind::EsNext, None);
    assert!(res.is_resolved());
    assert_eq!(res.resolved_file_name, "/repo/src/thing.ts");
}

// Go: internal/module/resolver.go:tryLoadModuleUsingPaths (exact key)
#[test]
fn paths_exact_mapping() {
    let mut paths: OrderedMap<String, Vec<String>> = OrderedMap::default();
    paths.set("config".to_string(), vec!["./config/index.ts".to_string()]);
    let opts = CompilerOptions {
        module_resolution: ModuleResolutionKind::Bundler,
        paths: Some(paths),
        ..Default::default()
    };
    let files = [("/repo/config/index.ts", ""), ("/repo/main.ts", "")];
    let r = resolver(&files, "/repo", opts);
    let (res, _) = r.resolve_module_name("config", "/repo/main.ts", ModuleKind::EsNext, None);
    assert!(res.is_resolved());
    assert_eq!(res.resolved_file_name, "/repo/config/index.ts");
}

// Go: internal/module/resolver.go:tryLoadModuleUsingRootDirs
#[test]
fn root_dirs_cross_directory() {
    let opts = CompilerOptions {
        module_resolution: ModuleResolutionKind::Bundler,
        root_dirs: vec!["/repo/generated".to_string(), "/repo/src".to_string()],
        ..Default::default()
    };
    let files = [("/repo/src/a.ts", ""), ("/repo/generated/b.ts", "")];
    let r = resolver(&files, "/repo", opts);
    let (res, _) = r.resolve_module_name("./b", "/repo/src/a.ts", ModuleKind::EsNext, None);
    assert!(res.is_resolved());
    assert_eq!(res.resolved_file_name, "/repo/generated/b.ts");
}

// Go: internal/module/resolver.go:tryLoadModuleUsingPathsIfEligible (relative names skip paths)
#[test]
fn paths_skipped_for_relative_names() {
    let mut paths: OrderedMap<String, Vec<String>> = OrderedMap::default();
    paths.set("*".to_string(), vec!["./generated/*".to_string()]);
    let opts = CompilerOptions {
        module_resolution: ModuleResolutionKind::Bundler,
        paths: Some(paths),
        ..Default::default()
    };
    let files = [
        ("/repo/main.ts", ""),
        ("/repo/dep.ts", ""),
        ("/repo/generated/dep.ts", ""),
    ];
    let r = resolver(&files, "/repo", opts);
    // A relative specifier must not be rewritten through `paths`.
    let (res, _) = r.resolve_module_name("./dep", "/repo/main.ts", ModuleKind::EsNext, None);
    assert_eq!(res.resolved_file_name, "/repo/dep.ts");
}
