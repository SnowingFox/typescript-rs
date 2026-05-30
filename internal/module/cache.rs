//! Resolution caches: the mode-aware cache alias, the module- and
//! type-reference-resolution caches, and the per-resolver [`Caches`] bundle.
//!
//! 1:1 port of Go `internal/module/cache.go`.
//!
//! # Divergence from Go
//! - Go's `SyncMap[key, *ResolvedModule]` stores nilable pointers; here the
//!   value type is `Arc<ResolvedModule>` (results are always non-nil).
//! - `sync.Once` for the parsed `paths` patterns becomes [`OnceLock`] so the
//!   resolver stays `Sync`.

use std::sync::{Arc, OnceLock};

use rustc_hash::FxHashMap;
use tsgo_collections::SyncMap;
use tsgo_core::compileroptions::{CompilerOptions, ResolutionMode};
use tsgo_packagejson::InfoCache;

use crate::types::{
    ModeAwareCacheKey, ResolvedModule, ResolvedProjectReference, ResolvedTypeReferenceDirective,
};
use crate::ParsedPatterns;

/// A cache keyed by `(name, mode)`.
// Go: internal/module/cache.go:ModeAwareCache
pub type ModeAwareCache<T> = FxHashMap<ModeAwareCacheKey, T>;

/// The cache key for a module-name resolution.
// Go: internal/module/cache.go:moduleResolutionCacheKey
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ModuleResolutionCacheKey {
    pub(crate) containing_directory: String,
    pub(crate) module_name: String,
    pub(crate) resolution_mode: ResolutionMode,
    pub(crate) redirect_config_name: String,
}

/// A concurrent cache of resolved modules.
// Go: internal/module/cache.go:moduleResolutionCache
#[derive(Default)]
pub(crate) struct ModuleResolutionCache {
    cache: SyncMap<ModuleResolutionCacheKey, Arc<ResolvedModule>>,
}

impl ModuleResolutionCache {
    /// Returns the cached resolution for `key`, if present.
    ///
    /// Side effects: reads the shared map.
    // Go: internal/module/cache.go:moduleResolutionCache.Get
    pub(crate) fn get(&self, key: &ModuleResolutionCacheKey) -> Option<Arc<ResolvedModule>> {
        let (value, ok) = self.cache.load(key);
        if ok {
            Some(value)
        } else {
            None
        }
    }

    /// Stores `value` for `key`.
    ///
    /// Side effects: mutates the shared map.
    // Go: internal/module/cache.go:moduleResolutionCache.Set
    pub(crate) fn set(&self, key: ModuleResolutionCacheKey, value: Arc<ResolvedModule>) {
        self.cache.store(key, value);
    }
}

/// The cache key for a type-reference-directive resolution.
// Go: internal/module/cache.go:typeRefDirectiveResolutionCacheKey
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TypeRefDirectiveResolutionCacheKey {
    pub(crate) containing_directory: String,
    pub(crate) type_reference_name: String,
    pub(crate) resolution_mode: ResolutionMode,
    pub(crate) redirect_config_name: String,
    pub(crate) from_inferred_types_containing_file: bool,
}

/// A concurrent cache of resolved type-reference directives.
// Go: internal/module/cache.go:typeRefDirectiveResolutionCache
#[derive(Default)]
pub(crate) struct TypeRefDirectiveResolutionCache {
    cache: SyncMap<TypeRefDirectiveResolutionCacheKey, Arc<ResolvedTypeReferenceDirective>>,
}

impl TypeRefDirectiveResolutionCache {
    /// Returns the cached resolution for `key`, if present.
    ///
    /// Side effects: reads the shared map.
    // Go: internal/module/cache.go:typeRefDirectiveResolutionCache.Get
    pub(crate) fn get(
        &self,
        key: &TypeRefDirectiveResolutionCacheKey,
    ) -> Option<Arc<ResolvedTypeReferenceDirective>> {
        let (value, ok) = self.cache.load(key);
        if ok {
            Some(value)
        } else {
            None
        }
    }

    /// Stores `value` for `key`.
    ///
    /// Side effects: mutates the shared map.
    // Go: internal/module/cache.go:typeRefDirectiveResolutionCache.Set
    pub(crate) fn set(
        &self,
        key: TypeRefDirectiveResolutionCacheKey,
        value: Arc<ResolvedTypeReferenceDirective>,
    ) {
        self.cache.store(key, value);
    }
}

/// The bundle of caches shared by a resolver across resolution requests.
// Go: internal/module/cache.go:caches
pub(crate) struct Caches {
    pub(crate) package_json_info_cache: Arc<InfoCache>,
    pub(crate) module_resolution_cache: ModuleResolutionCache,
    pub(crate) type_ref_directive_resolution_cache: TypeRefDirectiveResolutionCache,
    /// Cached representation of `CompilerOptions.paths`.
    pub(crate) parsed_patterns_for_paths: OnceLock<ParsedPatterns>,
}

/// Builds an empty [`Caches`] rooted at `current_directory`.
///
/// `_options` is accepted to mirror Go's signature; it is unused (Go's
/// `newCaches` ignores it as well).
///
/// Side effects: allocates the `package.json` info cache.
// Go: internal/module/cache.go:newCaches
pub(crate) fn new_caches(
    current_directory: &str,
    use_case_sensitive_file_names: bool,
    _options: &CompilerOptions,
) -> Caches {
    Caches {
        package_json_info_cache: Arc::new(InfoCache::new(
            current_directory.to_string(),
            use_case_sensitive_file_names,
        )),
        module_resolution_cache: ModuleResolutionCache::default(),
        type_ref_directive_resolution_cache: TypeRefDirectiveResolutionCache::default(),
        parsed_patterns_for_paths: OnceLock::new(),
    }
}

/// Returns the config name of a project-reference redirect, or empty when there
/// is none.
///
/// # Examples
/// ```
/// use tsgo_module::get_redirect_config_name;
/// assert_eq!(get_redirect_config_name(None), "");
/// ```
///
/// Side effects: none (pure).
// Go: internal/module/cache.go:getRedirectConfigName
pub fn get_redirect_config_name(redirect: Option<&dyn ResolvedProjectReference>) -> String {
    match redirect {
        None => String::new(),
        Some(r) => r.config_name().to_string(),
    }
}

#[cfg(test)]
#[path = "cache_test.rs"]
mod tests;
