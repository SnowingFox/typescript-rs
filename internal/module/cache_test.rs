use std::sync::Arc;

use tsgo_core::compileroptions::{CompilerOptions, ModuleKind};

use super::*;
use crate::types::{ModeAwareCacheKey, PackageId, ResolvedModule, ResolvedTypeReferenceDirective};

// Go: internal/module/cache.go:newCaches
#[test]
fn new_caches_creates_info_cache() {
    let opts = CompilerOptions::default();
    let caches = new_caches("/repo", true, &opts);
    assert!(caches.parsed_patterns_for_paths.get().is_none());
    // The package.json info cache starts empty.
    assert!(caches
        .package_json_info_cache
        .get("/repo/package.json")
        .is_none());
}

// Go: internal/module/cache.go:moduleResolutionCache.Get/Set
#[test]
fn module_resolution_cache_roundtrip() {
    let cache = ModuleResolutionCache::default();
    let key = ModuleResolutionCacheKey {
        containing_directory: "/repo/src".into(),
        module_name: "pkg".into(),
        resolution_mode: ModuleKind::EsNext,
        redirect_config_name: String::new(),
    };
    assert!(cache.get(&key).is_none());

    let value = Arc::new(ResolvedModule {
        resolved_file_name: "/repo/node_modules/pkg/main.d.ts".into(),
        ..Default::default()
    });
    cache.set(key.clone(), value.clone());
    let got = cache.get(&key).expect("entry should be cached");
    assert_eq!(got.resolved_file_name, "/repo/node_modules/pkg/main.d.ts");
}

// Go: internal/module/cache.go:typeRefDirectiveResolutionCache.Get/Set
#[test]
fn type_ref_directive_resolution_cache_roundtrip() {
    let cache = TypeRefDirectiveResolutionCache::default();
    let key = TypeRefDirectiveResolutionCacheKey {
        containing_directory: "/repo/src".into(),
        type_reference_name: "node".into(),
        resolution_mode: ModuleKind::CommonJs,
        redirect_config_name: String::new(),
        from_inferred_types_containing_file: false,
    };
    assert!(cache.get(&key).is_none());

    let value = Arc::new(ResolvedTypeReferenceDirective {
        resolved_file_name: "/repo/node_modules/@types/node/index.d.ts".into(),
        primary: true,
        ..Default::default()
    });
    cache.set(key.clone(), value);
    let got = cache.get(&key).expect("entry should be cached");
    assert!(got.primary);
}

// Go: internal/module/cache.go:ModeAwareCache
#[test]
fn mode_aware_cache_basic() {
    let mut cache: ModeAwareCache<String> = ModeAwareCache::default();
    let key = ModeAwareCacheKey {
        name: "pkg".into(),
        mode: ModuleKind::EsNext,
    };
    cache.insert(key.clone(), "/repo/pkg.d.ts".into());
    assert_eq!(cache.get(&key).map(String::as_str), Some("/repo/pkg.d.ts"));
}

struct StubRef {
    config_name: String,
}

impl ResolvedProjectReference for StubRef {
    fn config_name(&self) -> &str {
        &self.config_name
    }
    fn compiler_options(&self) -> Option<Arc<CompilerOptions>> {
        None
    }
}

// Go: internal/module/cache.go:getRedirectConfigName
#[test]
fn get_redirect_config_name_behaviors() {
    assert_eq!(get_redirect_config_name(None), "");
    let stub = StubRef {
        config_name: "/repo/tsconfig.json".into(),
    };
    assert_eq!(get_redirect_config_name(Some(&stub)), "/repo/tsconfig.json");
}

// Keep PackageId referenced so the test module mirrors the cached value shape.
#[test]
fn cached_resolved_module_carries_package_id() {
    let value = ResolvedModule {
        package_id: PackageId {
            name: "pkg".into(),
            version: "1.0.0".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(value.package_id.name, "pkg");
}
