use tsgo_core::compileroptions::{CompilerOptions, ModuleKind, ModuleResolutionKind};

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

// Go: internal/module/resolver.go:loadModuleFromExports (subpath mapping)
#[test]
fn exports_subpath_mapping() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r#"{"name":"pkg","exports":{"./sub":"./sub.js"}}"#,
        ),
        ("/repo/node_modules/pkg/sub.d.ts", ""),
        ("/repo/src/a.ts", ""),
    ];
    let r = bundler(&files);
    let (res, _) = r.resolve_module_name("pkg/sub", "/repo/src/a.ts", ModuleKind::EsNext, None);
    assert!(res.is_resolved());
    // The `.js` target resolves to the sibling `.d.ts` during the TS pass.
    assert_eq!(res.resolved_file_name, "/repo/node_modules/pkg/sub.d.ts");
}

// Go: internal/module/resolver.go:loadModuleFromTargetExportOrImport (conditional exports)
#[test]
fn conditional_exports_picks_types() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r#"{"name":"pkg","exports":{"types":"./index.d.ts","import":"./index.js"}}"#,
        ),
        ("/repo/node_modules/pkg/index.d.ts", ""),
        ("/repo/src/a.ts", ""),
    ];
    let r = bundler(&files);
    let (res, _) = r.resolve_module_name("pkg", "/repo/src/a.ts", ModuleKind::EsNext, None);
    assert!(res.is_resolved());
    assert_eq!(res.resolved_file_name, "/repo/node_modules/pkg/index.d.ts");
}

// Go: internal/module/resolver.go:loadModuleFromTargetExportOrImport (null target unresolved)
#[test]
fn exports_null_target_blocks_resolution() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r#"{"name":"pkg","exports":{"./blocked":null}}"#,
        ),
        ("/repo/node_modules/pkg/blocked.d.ts", ""),
        ("/repo/src/a.ts", ""),
    ];
    let r = bundler(&files);
    let (res, _) = r.resolve_module_name("pkg/blocked", "/repo/src/a.ts", ModuleKind::EsNext, None);
    assert!(!res.is_resolved());
}

// Go: internal/module/resolver.go:loadModuleFromImports (#imports map)
#[test]
fn imports_hash_specifier() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r##"{"name":"pkg","imports":{"#internal":"./internal.js"}}"##,
        ),
        ("/repo/node_modules/pkg/internal.d.ts", ""),
        ("/repo/node_modules/pkg/index.ts", ""),
    ];
    let r = bundler(&files);
    let (res, _) = r.resolve_module_name(
        "#internal",
        "/repo/node_modules/pkg/index.ts",
        ModuleKind::EsNext,
        None,
    );
    assert!(res.is_resolved());
    assert_eq!(
        res.resolved_file_name,
        "/repo/node_modules/pkg/internal.d.ts"
    );
}
