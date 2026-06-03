// Go: internal/project/parsecache.go
use super::*;
use tsgo_core::scriptkind::ScriptKind;

#[test]
fn parse_cache_key_new_roundtrip() {
    // Go: internal/project/parsecache.go:NewParseCacheKey
    let opts = SourceFileParseOptions {
        file_name: "main.ts".to_string(),
        path: tsgo_tspath::Path("main.ts".to_string()),
        jsx: false,
        force_external_module_indicator: false,
    };
    let hash = ContentHash { hi: 0xAA, lo: 0xBB };
    let key = ParseCacheKey::new(opts.clone(), hash, ScriptKind::Ts);
    assert_eq!(key.options, opts);
    assert_eq!(key.hash, hash);
    assert_eq!(key.script_kind, ScriptKind::Ts);
}

#[test]
fn parse_cache_key_equality() {
    let opts1 = SourceFileParseOptions {
        file_name: "a.ts".to_string(),
        path: Default::default(),
        jsx: false,
        force_external_module_indicator: false,
    };
    let opts2 = opts1.clone();
    let hash = ContentHash { hi: 1, lo: 2 };
    let k1 = ParseCacheKey::new(opts1, hash, ScriptKind::Ts);
    let k2 = ParseCacheKey::new(opts2, hash, ScriptKind::Ts);
    assert_eq!(k1, k2);
}

#[test]
fn parse_cache_key_differs_by_script_kind() {
    let opts = SourceFileParseOptions {
        file_name: "a.ts".to_string(),
        path: Default::default(),
        jsx: false,
        force_external_module_indicator: false,
    };
    let hash = ContentHash { hi: 1, lo: 2 };
    let k1 = ParseCacheKey::new(opts.clone(), hash, ScriptKind::Ts);
    let k2 = ParseCacheKey::new(opts, hash, ScriptKind::Tsx);
    assert_ne!(k1, k2);
}

#[test]
fn parse_cache_key_differs_by_hash() {
    let opts = SourceFileParseOptions {
        file_name: "a.ts".to_string(),
        path: Default::default(),
        jsx: false,
        force_external_module_indicator: false,
    };
    let k1 = ParseCacheKey::new(opts.clone(), ContentHash { hi: 1, lo: 2 }, ScriptKind::Ts);
    let k2 = ParseCacheKey::new(opts, ContentHash { hi: 3, lo: 4 }, ScriptKind::Ts);
    assert_ne!(k1, k2);
}

#[test]
fn parse_cache_key_differs_by_file_name() {
    let hash = ContentHash { hi: 1, lo: 2 };
    let k1 = ParseCacheKey::new(
        SourceFileParseOptions {
            file_name: "a.ts".to_string(),
            path: Default::default(),
            jsx: false,
            force_external_module_indicator: false,
        },
        hash,
        ScriptKind::Ts,
    );
    let k2 = ParseCacheKey::new(
        SourceFileParseOptions {
            file_name: "b.ts".to_string(),
            path: Default::default(),
            jsx: false,
            force_external_module_indicator: false,
        },
        hash,
        ScriptKind::Ts,
    );
    assert_ne!(k1, k2);
}

#[test]
fn content_hash_default_is_zero() {
    let h = ContentHash::default();
    assert_eq!(h.hi, 0);
    assert_eq!(h.lo, 0);
}

#[test]
fn parse_cache_key_is_hashable() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    let opts = SourceFileParseOptions {
        file_name: "a.ts".to_string(),
        path: Default::default(),
        jsx: false,
        force_external_module_indicator: false,
    };
    let hash = ContentHash { hi: 1, lo: 2 };
    let key = ParseCacheKey::new(opts, hash, ScriptKind::Ts);
    set.insert(key.clone());
    assert!(set.contains(&key));
}
