use tsgo_core::compileroptions::{CompilerOptions, ModuleKind, ModuleResolutionKind};
use tsgo_core::tristate::Tristate;

use super::*;
use crate::test_support::resolver;
use crate::NodeResolutionFeatures;

fn bundler() -> CompilerOptions {
    CompilerOptions {
        module_resolution: ModuleResolutionKind::Bundler,
        ..Default::default()
    }
}

// Go: internal/module/resolver.go:GetConditions
#[test]
fn get_conditions_bundler_defaults_to_import() {
    let opts = bundler();
    assert_eq!(
        get_conditions(&opts, ModuleKind::EsNext),
        vec!["import", "types"]
    );
    // None mode under bundler is treated as ESM.
    assert_eq!(
        get_conditions(&opts, ModuleKind::None),
        vec!["import", "types"]
    );
}

// Go: internal/module/resolver.go:GetConditions
#[test]
fn get_conditions_node16_adds_node() {
    let opts = CompilerOptions {
        module_resolution: ModuleResolutionKind::Node16,
        ..Default::default()
    };
    assert_eq!(
        get_conditions(&opts, ModuleKind::CommonJs),
        vec!["require", "types", "node"]
    );
    assert_eq!(
        get_conditions(&opts, ModuleKind::EsNext),
        vec!["import", "types", "node"]
    );
}

// Go: internal/module/resolver.go:GetConditions (custom conditions appended)
#[test]
fn get_conditions_appends_custom() {
    let opts = CompilerOptions {
        module_resolution: ModuleResolutionKind::Bundler,
        custom_conditions: vec!["custom".into()],
        ..Default::default()
    };
    assert_eq!(
        get_conditions(&opts, ModuleKind::EsNext),
        vec!["import", "types", "custom"]
    );
}

// Go: internal/module/resolver.go:getNodeResolutionFeatures
#[test]
fn get_node_resolution_features_defaults() {
    assert_eq!(
        get_node_resolution_features(&bundler()),
        NodeResolutionFeatures::BUNDLER_DEFAULT
    );
    let node16 = CompilerOptions {
        module_resolution: ModuleResolutionKind::Node16,
        ..Default::default()
    };
    assert_eq!(
        get_node_resolution_features(&node16),
        NodeResolutionFeatures::NODE16_DEFAULT
    );
}

// Go: internal/module/resolver.go:getNodeResolutionFeatures (overrides)
#[test]
fn get_node_resolution_features_overrides() {
    let mut opts = bundler();
    opts.resolve_package_json_exports = Tristate::False;
    assert!(!get_node_resolution_features(&opts).contains(NodeResolutionFeatures::EXPORTS));
    opts.resolve_package_json_imports = Tristate::False;
    assert!(!get_node_resolution_features(&opts).contains(NodeResolutionFeatures::IMPORTS));
}

// Go: internal/module/resolver.go:resolutionState.resolveNodeLike (relative path)
#[test]
fn resolve_node_like_relative_resolution() {
    let files = [("/repo/a.ts", ""), ("/repo/b.ts", "")];
    let r = resolver(&files, "/repo", bundler());
    let (resolved, _) = r.resolve_module_name("./b", "/repo/a.ts", ModuleKind::EsNext, None);
    assert!(resolved.is_resolved());
    assert_eq!(resolved.resolved_file_name, "/repo/b.ts");
}

// Go: internal/module/resolver.go:Resolver.GetPackageScopeForPath
#[test]
fn get_package_scope_for_path_finds_nearest() {
    let files = [
        ("/repo/node_modules/pkg/package.json", r#"{"name":"pkg"}"#),
        ("/repo/node_modules/pkg/sub/file.ts", ""),
    ];
    let r = resolver(&files, "/repo", bundler());
    let scope = r
        .get_package_scope_for_path("/repo/node_modules/pkg/sub")
        .expect("scope should exist");
    assert_eq!(scope.package_directory(), "/repo/node_modules/pkg");
    assert!(r.get_package_scope_for_path("/repo/src").is_none());
}
