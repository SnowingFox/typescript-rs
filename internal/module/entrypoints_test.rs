use tsgo_core::compileroptions::{CompilerOptions, ModuleResolutionKind};

use super::*;
use crate::test_support::resolver;

fn bundler(files: &[(&str, &str)]) -> crate::Resolver {
    resolver(
        files,
        "/repo",
        CompilerOptions {
            module_resolution: ModuleResolutionKind::Bundler,
            ..Default::default()
        },
    )
}

// Go: internal/module/resolver.go:Resolver.GetEntrypointsFromPackageJsonInfo (main/types)
#[test]
fn entrypoints_from_types_field() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r#"{"name":"pkg","types":"index.d.ts"}"#,
        ),
        ("/repo/node_modules/pkg/index.d.ts", ""),
    ];
    let r = bundler(&files);
    let scope = r
        .get_package_scope_for_path("/repo/node_modules/pkg")
        .expect("scope");
    let entrypoints = r.get_entrypoints_from_package_json_info(&scope, "pkg", false);
    assert_eq!(entrypoints.len(), 1);
    assert!(entrypoints[0].resolved_file_name.ends_with("/index.d.ts"));
    assert_eq!(entrypoints[0].module_specifier, "pkg");
    assert_eq!(entrypoints[0].ending, Ending::Fixed);
    assert_eq!(
        entrypoints[0].symlink_or_realpath(),
        entrypoints[0].resolved_file_name
    );
}

// Go: internal/module/resolver.go:resolutionState.loadEntrypointsFromExportMap (subpaths)
#[test]
fn entrypoints_from_exports_map() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r#"{"name":"pkg","exports":{".":"./index.d.ts","./sub":"./sub.d.ts"}}"#,
        ),
        ("/repo/node_modules/pkg/index.d.ts", ""),
        ("/repo/node_modules/pkg/sub.d.ts", ""),
    ];
    let r = bundler(&files);
    let scope = r
        .get_package_scope_for_path("/repo/node_modules/pkg")
        .expect("scope");
    let entrypoints = r.get_entrypoints_from_package_json_info(&scope, "pkg", false);
    assert_eq!(entrypoints.len(), 2);
    let specifiers: Vec<&str> = entrypoints
        .iter()
        .map(|e| e.module_specifier.as_str())
        .collect();
    assert!(specifiers.contains(&"pkg"));
    assert!(specifiers.contains(&"pkg/sub"));
}

// Go: internal/module/resolver.go:Resolver.GetEntrypointsFromPackageJsonInfo (directory scan)
#[test]
fn entrypoints_directory_search() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r#"{"name":"pkg","types":"index.d.ts"}"#,
        ),
        ("/repo/node_modules/pkg/index.d.ts", ""),
        ("/repo/node_modules/pkg/extra.d.ts", ""),
    ];
    let r = bundler(&files);
    let scope = r
        .get_package_scope_for_path("/repo/node_modules/pkg")
        .expect("scope");
    let entrypoints = r.get_entrypoints_from_package_json_info(&scope, "pkg", true);
    // The main entry plus the additional discovered file.
    assert!(entrypoints.len() >= 2);
}
