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

// Go: internal/module/resolver.go:getPackageJsonInfo / getPackageFile (main field + packageId)
#[test]
fn package_main_field_and_package_id() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r#"{"name":"pkg","version":"1.2.3","main":"lib/main.js"}"#,
        ),
        ("/repo/node_modules/pkg/lib/main.js", ""),
        ("/repo/src/a.ts", ""),
    ];
    let r = bundler(&files);
    let (res, _) = r.resolve_module_name("pkg", "/repo/src/a.ts", ModuleKind::EsNext, None);
    assert!(res.is_resolved());
    assert_eq!(res.resolved_file_name, "/repo/node_modules/pkg/lib/main.js");
    assert_eq!(res.package_id.name, "pkg");
    assert_eq!(res.package_id.version, "1.2.3");
}

// Go: internal/module/resolver.go:getPackageFile (typings preferred over main)
#[test]
fn package_typings_field() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r#"{"name":"pkg","version":"1.0.0","typings":"types/index.d.ts","main":"index.js"}"#,
        ),
        ("/repo/node_modules/pkg/types/index.d.ts", ""),
        ("/repo/src/a.ts", ""),
    ];
    let r = bundler(&files);
    let (res, _) = r.resolve_module_name("pkg", "/repo/src/a.ts", ModuleKind::EsNext, None);
    assert!(res.is_resolved());
    assert_eq!(
        res.resolved_file_name,
        "/repo/node_modules/pkg/types/index.d.ts"
    );
}

// Go: internal/module/resolver.go:readPackageJsonPeerDependencies
#[test]
fn package_peer_dependencies_recorded() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r#"{"name":"pkg","version":"1.0.0","types":"index.d.ts","peerDependencies":{"peer":"^1.0.0"}}"#,
        ),
        ("/repo/node_modules/pkg/index.d.ts", ""),
        (
            "/repo/node_modules/peer/package.json",
            r#"{"name":"peer","version":"2.5.0"}"#,
        ),
        ("/repo/src/a.ts", ""),
    ];
    let r = bundler(&files);
    let (res, _) = r.resolve_module_name("pkg", "/repo/src/a.ts", ModuleKind::EsNext, None);
    assert!(res.is_resolved());
    assert_eq!(res.package_id.peer_dependencies, "+peer@2.5.0");
}

// Go: internal/module/resolver.go:getPackageJsonInfo (no package.json, JS index)
#[test]
fn package_missing_json_resolves_index_js() {
    let files = [
        ("/repo/node_modules/pkg/index.js", ""),
        ("/repo/src/a.ts", ""),
    ];
    let r = bundler(&files);
    // No package.json and only a `.js` index resolves via the JS pass.
    let (res, _) = r.resolve_module_name("pkg", "/repo/src/a.ts", ModuleKind::EsNext, None);
    assert!(res.is_resolved());
    assert_eq!(res.resolved_file_name, "/repo/node_modules/pkg/index.js");
}
