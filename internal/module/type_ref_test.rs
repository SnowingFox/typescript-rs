use std::sync::Arc;

use tsgo_core::compileroptions::{CompilerOptions, ModuleKind, ModuleResolutionKind};
use tsgo_vfs::vfstest::MapFs;

use crate::resolve_config;
use crate::test_support::{resolver, StubHost};

// Go: internal/module/resolver.go:resolutionState.resolveTypeReferenceDirective
#[test]
fn type_reference_directive_at_types() {
    let files = [
        ("/repo/node_modules/@types/node/index.d.ts", ""),
        (
            "/repo/node_modules/@types/node/package.json",
            r#"{"name":"@types/node","version":"1.0.0","types":"index.d.ts"}"#,
        ),
        ("/repo/src/a.ts", ""),
    ];
    let opts = CompilerOptions {
        module_resolution: ModuleResolutionKind::Node16,
        ..Default::default()
    };
    let r = resolver(&files, "/repo", opts);
    let (res, _) =
        r.resolve_type_reference_directive("node", "/repo/src/a.ts", ModuleKind::CommonJs, None);
    assert!(res.is_resolved());
    assert!(res.resolved_file_name.ends_with("/index.d.ts"));
    assert!(res.primary);
}

// Go: internal/module/resolver.go:resolutionState.resolveTypeReferenceDirective (unresolved)
#[test]
fn type_reference_directive_unresolved() {
    let files = [("/repo/src/a.ts", "")];
    let opts = CompilerOptions {
        module_resolution: ModuleResolutionKind::Node16,
        ..Default::default()
    };
    let r = resolver(&files, "/repo", opts);
    let (res, _) =
        r.resolve_type_reference_directive("missing", "/repo/src/a.ts", ModuleKind::CommonJs, None);
    assert!(!res.is_resolved());
}

// Go: internal/module/resolver.go:ResolveConfig
#[test]
fn resolve_config_extends_package() {
    let files = [
        ("/repo/node_modules/@tsconfig/base/tsconfig.json", "{}"),
        (
            "/repo/node_modules/@tsconfig/base/package.json",
            r#"{"name":"@tsconfig/base","version":"1.0.0"}"#,
        ),
        ("/repo/tsconfig.json", "{}"),
    ];
    let fs = MapFs::from_map(files.iter().copied(), true);
    let host = Arc::new(StubHost {
        fs,
        cwd: "/repo".to_string(),
    });
    let res = resolve_config("@tsconfig/base/tsconfig.json", "/repo/tsconfig.json", host);
    assert!(res.is_resolved());
    assert_eq!(res.extension, ".json");
}
