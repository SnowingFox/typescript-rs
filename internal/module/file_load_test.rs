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

// Go: internal/module/resolver.go:tryAddingExtensions (extensionless -> .ts)
#[test]
fn relative_extensionless_resolves_ts() {
    let r = bundler(&[("/repo/a.ts", ""), ("/repo/dep.ts", "")]);
    let (res, _) = r.resolve_module_name("./dep", "/repo/a.ts", ModuleKind::EsNext, None);
    assert_eq!(res.resolved_file_name, "/repo/dep.ts");
    assert_eq!(res.extension, ".ts");
}

// Go: internal/module/resolver.go:loadModuleFromFileNoImplicitExtensions (.js -> .ts)
#[test]
fn relative_js_specifier_maps_to_ts() {
    let r = bundler(&[("/repo/a.ts", ""), ("/repo/dep.ts", "")]);
    let (res, _) = r.resolve_module_name("./dep.js", "/repo/a.ts", ModuleKind::EsNext, None);
    assert_eq!(res.resolved_file_name, "/repo/dep.ts");
    assert_eq!(res.extension, ".ts");
}

// Go: internal/module/resolver.go:loadNodeModuleFromDirectoryWorker (index lookup)
#[test]
fn relative_directory_index() {
    let r = bundler(&[("/repo/a.ts", ""), ("/repo/dir/index.ts", "")]);
    let (res, _) = r.resolve_module_name("./dir", "/repo/a.ts", ModuleKind::EsNext, None);
    assert_eq!(res.resolved_file_name, "/repo/dir/index.ts");
}

// Go: internal/module/resolver.go:tryAddingExtensions (.tsx prefers .tsx over .ts)
#[test]
fn relative_tsx_specifier() {
    let r = bundler(&[("/repo/a.ts", ""), ("/repo/dep.tsx", "")]);
    let (res, _) = r.resolve_module_name("./dep.jsx", "/repo/a.ts", ModuleKind::EsNext, None);
    assert_eq!(res.resolved_file_name, "/repo/dep.tsx");
    assert_eq!(res.extension, ".tsx");
}

// Go: internal/module/resolver.go:nodeLoadModuleByRelativeName (missing parent dir)
#[test]
fn relative_missing_directory_is_unresolved() {
    let r = bundler(&[("/repo/a.ts", "")]);
    let (res, _) = r.resolve_module_name("./nope/dep", "/repo/a.ts", ModuleKind::EsNext, None);
    assert!(!res.is_resolved());
}
