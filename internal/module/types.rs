//! Module-resolution data types: the resolution host trait, cache key, feature
//! and extension bitflags, package identity, and the resolved-module/result
//! structs.
//!
//! 1:1 port of Go `internal/module/types.go`.
//!
//! # Divergence from Go
//! - Go's `ResolvedModule.ResolutionDiagnostics []*ast.Diagnostic` becomes
//!   `Vec<ResolutionDiagnostic>`: `tsgo_ast::Diagnostic` is not yet ported, so
//!   [`ResolutionDiagnostic`] keeps the message and stringified args produced
//!   by the resolver. blocked-by: tsgo_ast::Diagnostic.
//! - `NodeResolutionFeatures`/`extensions` are `int32` iota bitsets in Go; here
//!   they use the `bitflags` crate (PORTING.md §3).

use std::fmt;
use std::sync::Arc;

use bitflags::bitflags;
use tsgo_core::compileroptions::{CompilerOptions, ResolutionMode};
use tsgo_diagnostics::Message;
use tsgo_tspath::{
    EXTENSION_JSON, SUPPORTED_DECLARATION_EXTENSIONS, SUPPORTED_JS_EXTENSIONS_FLAT,
    SUPPORTED_TS_IMPLEMENTATION_EXTENSIONS,
};
use tsgo_vfs::Fs;

/// The host a [`Resolver`](crate::Resolver) consults for file-system access and
/// the current working directory.
///
/// # Examples
/// ```
/// use std::sync::Arc;
/// use tsgo_module::ResolutionHost;
/// use tsgo_vfs::Fs;
/// use tsgo_vfs::vfstest::MapFs;
///
/// struct Host { fs: MapFs, cwd: String }
/// impl ResolutionHost for Host {
///     fn fs(&self) -> &dyn Fs { &self.fs }
///     fn get_current_directory(&self) -> &str { &self.cwd }
/// }
/// let host = Host { fs: MapFs::from_map([("/x.ts", "")], true), cwd: "/".into() };
/// assert_eq!(host.get_current_directory(), "/");
/// ```
///
/// Side effects: implementations expose the underlying file system.
// Go: internal/module/types.go:ResolutionHost
pub trait ResolutionHost: Send + Sync {
    /// Returns the file system used for all resolution probing.
    ///
    /// Side effects: none (returns a borrow).
    fn fs(&self) -> &dyn Fs;

    /// Returns the current working directory used to root relative inputs.
    ///
    /// Side effects: none (returns a borrow).
    fn get_current_directory(&self) -> &str;
}

/// A module-resolution cache key combining a name with its resolution mode.
///
/// # Examples
/// ```
/// use tsgo_core::compileroptions::ModuleKind;
/// use tsgo_module::ModeAwareCacheKey;
/// let k = ModeAwareCacheKey { name: "pkg".into(), mode: ModuleKind::EsNext };
/// assert_eq!(k.name, "pkg");
/// ```
///
/// Side effects: none (plain data).
// Go: internal/module/types.go:ModeAwareCacheKey
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModeAwareCacheKey {
    /// The module/type-reference name being resolved.
    pub name: String,
    /// The resolution mode the lookup is performed in.
    pub mode: ResolutionMode,
}

/// A resolved project reference, used to redirect compiler options during
/// resolution.
///
/// Side effects: implementations expose the redirect's config name and options.
// Go: internal/module/types.go:ResolvedProjectReference
pub trait ResolvedProjectReference {
    /// The config file name of the redirected reference.
    ///
    /// Side effects: none (returns a borrow).
    fn config_name(&self) -> &str;

    /// The compiler options of the redirected reference, if any.
    ///
    /// Side effects: none (pure).
    fn compiler_options(&self) -> Option<Arc<CompilerOptions>>;
}

bitflags! {
    /// Feature toggles that gate modern Node resolution behavior (`imports`,
    /// self-name, `exports`, pattern trailers, `#/` import roots).
    ///
    /// # Examples
    /// ```
    /// use tsgo_module::NodeResolutionFeatures;
    /// assert_eq!(NodeResolutionFeatures::ALL.bits(), 31);
    /// assert_eq!(NodeResolutionFeatures::NODE16_DEFAULT.bits(), 15);
    /// ```
    ///
    /// Side effects: none (plain data).
    // Go: internal/module/types.go:NodeResolutionFeatures
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct NodeResolutionFeatures: i32 {
        /// Resolve `package.json` `imports`.
        const IMPORTS = 1 << 0;
        /// Allow self-name references to the containing package.
        const SELF_NAME = 1 << 1;
        /// Resolve `package.json` `exports`.
        const EXPORTS = 1 << 2;
        /// Allow `exports` pattern trailers (`./*.js`).
        const EXPORTS_PATTERN_TRAILERS = 1 << 3;
        /// Allow `#/` root imports in `package.json` `imports`.
        const IMPORTS_PATTERN_ROOT = 1 << 4;
    }
}

impl NodeResolutionFeatures {
    /// No features enabled.
    // Go: internal/module/types.go:NodeResolutionFeaturesNone
    pub const NONE: Self = Self::empty();
    /// All features enabled.
    // Go: internal/module/types.go:NodeResolutionFeaturesAll
    pub const ALL: Self = Self::IMPORTS
        .union(Self::SELF_NAME)
        .union(Self::EXPORTS)
        .union(Self::EXPORTS_PATTERN_TRAILERS)
        .union(Self::IMPORTS_PATTERN_ROOT);
    /// Defaults for `node16` resolution (everything except `#/` import roots).
    // Go: internal/module/types.go:NodeResolutionFeaturesNode16Default
    pub const NODE16_DEFAULT: Self = Self::IMPORTS
        .union(Self::SELF_NAME)
        .union(Self::EXPORTS)
        .union(Self::EXPORTS_PATTERN_TRAILERS);
    /// Defaults for `nodenext` resolution (all features).
    // Go: internal/module/types.go:NodeResolutionFeaturesNodeNextDefault
    pub const NODENEXT_DEFAULT: Self = Self::ALL;
    /// Defaults for `bundler` resolution (all features).
    // Go: internal/module/types.go:NodeResolutionFeaturesBundlerDefault
    pub const BUNDLER_DEFAULT: Self = Self::IMPORTS
        .union(Self::SELF_NAME)
        .union(Self::EXPORTS)
        .union(Self::EXPORTS_PATTERN_TRAILERS)
        .union(Self::IMPORTS_PATTERN_ROOT);
}

/// A package's identity: name, optional submodule, version, and a peer-deps
/// suffix.
///
/// # Examples
/// ```
/// use tsgo_module::PackageId;
/// let id = PackageId { name: "pkg".into(), sub_module_name: "sub".into(), version: "1.0.0".into(), peer_dependencies: String::new() };
/// assert_eq!(id.package_name(), "pkg/sub");
/// assert_eq!(id.to_string(), "pkg/sub@1.0.0");
/// ```
///
/// Side effects: none (plain data).
// Go: internal/module/types.go:PackageId
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageId {
    /// The package name (e.g. `@scope/pkg`).
    pub name: String,
    /// The submodule path within the package, if any.
    pub sub_module_name: String,
    /// The package version.
    pub version: String,
    /// A `+name@version` peer-dependency suffix, or empty.
    pub peer_dependencies: String,
}

impl PackageId {
    /// Returns `name/sub_module_name` when a submodule is present, else `name`.
    ///
    /// Side effects: none (pure).
    // Go: internal/module/types.go:PackageId.PackageName
    pub fn package_name(&self) -> String {
        if !self.sub_module_name.is_empty() {
            format!("{}/{}", self.name, self.sub_module_name)
        } else {
            self.name.clone()
        }
    }
}

impl fmt::Display for PackageId {
    // Go: internal/module/types.go:PackageId.String
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}@{}{}",
            self.package_name(),
            self.version,
            self.peer_dependencies
        )
    }
}

/// A resolution diagnostic recorded on a resolved module.
///
/// # Divergence from Go
/// Go stores `*ast.Diagnostic` (file + range + message + args). `tsgo_ast`
/// does not yet expose a ported `Diagnostic`, so this keeps the message and its
/// stringified arguments. blocked-by: tsgo_ast::Diagnostic.
///
/// Side effects: none (plain data).
// Go: internal/module/types.go (ResolvedModule.ResolutionDiagnostics element)
#[derive(Debug, Clone)]
pub struct ResolutionDiagnostic {
    /// The diagnostic message.
    pub message: &'static Message,
    /// The stringified diagnostic arguments.
    pub args: Vec<String>,
}

/// The result of resolving a module specifier.
///
/// # Examples
/// ```
/// use tsgo_module::ResolvedModule;
/// let mut r = ResolvedModule::default();
/// assert!(!r.is_resolved());
/// r.resolved_file_name = "/a.ts".into();
/// assert!(r.is_resolved());
/// ```
///
/// Side effects: none (plain data).
// Go: internal/module/types.go:ResolvedModule
#[derive(Debug, Clone, Default)]
pub struct ResolvedModule {
    /// Diagnostics produced while resolving (e.g. ambiguous project root).
    pub resolution_diagnostics: Vec<ResolutionDiagnostic>,
    /// The resolved on-disk file, or empty if unresolved.
    pub resolved_file_name: String,
    /// The symlink path the file was discovered through, if any.
    pub original_path: String,
    /// The resolved file's extension.
    pub extension: String,
    /// Whether the specifier resolved using a TS extension written in source.
    pub resolved_using_ts_extension: bool,
    /// The identity of the package the file belongs to.
    pub package_id: PackageId,
    /// Whether the file came from `node_modules`.
    pub is_external_library_import: bool,
    /// An alternate resolution found with modern features disabled, if any.
    pub alternate_result: String,
}

impl ResolvedModule {
    /// Reports whether a file was resolved.
    ///
    /// Side effects: none (pure).
    // Go: internal/module/types.go:ResolvedModule.IsResolved
    pub fn is_resolved(&self) -> bool {
        !self.resolved_file_name.is_empty()
    }
}

/// The result of resolving a type reference directive.
///
/// # Examples
/// ```
/// use tsgo_module::ResolvedTypeReferenceDirective;
/// let r = ResolvedTypeReferenceDirective::default();
/// assert!(!r.is_resolved());
/// ```
///
/// Side effects: none (plain data).
// Go: internal/module/types.go:ResolvedTypeReferenceDirective
#[derive(Debug, Clone, Default)]
pub struct ResolvedTypeReferenceDirective {
    /// Diagnostics produced while resolving.
    pub resolution_diagnostics: Vec<ResolutionDiagnostic>,
    /// Whether the directive was found in a primary (typeRoots) location.
    pub primary: bool,
    /// The resolved declaration file, or empty if unresolved.
    pub resolved_file_name: String,
    /// The symlink path the file was discovered through, if any.
    pub original_path: String,
    /// The identity of the package the file belongs to.
    pub package_id: PackageId,
    /// Whether the file came from `node_modules`.
    pub is_external_library_import: bool,
}

impl ResolvedTypeReferenceDirective {
    /// Reports whether a declaration file was resolved.
    ///
    /// Side effects: none (pure).
    // Go: internal/module/types.go:ResolvedTypeReferenceDirective.IsResolved
    pub fn is_resolved(&self) -> bool {
        !self.resolved_file_name.is_empty()
    }
}

bitflags! {
    /// The kinds of file extensions a resolution pass will accept.
    ///
    /// # Examples
    /// ```
    /// use tsgo_module::Extensions;
    /// assert_eq!(Extensions::IMPLEMENTATION_FILES.bits(), 3);
    /// assert_eq!(Extensions::DECLARATION.to_string(), "Declaration");
    /// ```
    ///
    /// Side effects: none (plain data).
    // Go: internal/module/types.go:extensions
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Extensions: i32 {
        /// TypeScript implementation extensions (`.ts`/`.tsx`/`.mts`/`.cts`).
        const TYPE_SCRIPT = 1 << 0;
        /// JavaScript extensions (`.js`/`.jsx`/`.mjs`/`.cjs`).
        const JAVA_SCRIPT = 1 << 1;
        /// Declaration extensions (`.d.ts`/`.d.mts`/`.d.cts`).
        const DECLARATION = 1 << 2;
        /// The `.json` extension.
        const JSON = 1 << 3;
    }
}

impl Extensions {
    /// TypeScript and JavaScript implementation files (no declarations/JSON).
    // Go: internal/module/types.go:extensionsImplementationFiles
    pub const IMPLEMENTATION_FILES: Self = Self::TYPE_SCRIPT.union(Self::JAVA_SCRIPT);

    /// Returns the concrete file extensions implied by this set, in TS, JS,
    /// declaration, JSON order.
    ///
    /// # Examples
    /// ```
    /// use tsgo_module::Extensions;
    /// assert_eq!(Extensions::JSON.to_array(), vec![".json".to_string()]);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/module/types.go:extensions.Array
    pub fn to_array(&self) -> Vec<String> {
        let mut result: Vec<String> = Vec::new();
        if self.contains(Extensions::TYPE_SCRIPT) {
            result.extend(
                SUPPORTED_TS_IMPLEMENTATION_EXTENSIONS
                    .iter()
                    .map(|s| s.to_string()),
            );
        }
        if self.contains(Extensions::JAVA_SCRIPT) {
            result.extend(SUPPORTED_JS_EXTENSIONS_FLAT.iter().map(|s| s.to_string()));
        }
        if self.contains(Extensions::DECLARATION) {
            result.extend(
                SUPPORTED_DECLARATION_EXTENSIONS
                    .iter()
                    .map(|s| s.to_string()),
            );
        }
        if self.contains(Extensions::JSON) {
            result.push(EXTENSION_JSON.to_string());
        }
        result
    }
}

impl fmt::Display for Extensions {
    // Go: internal/module/types.go:extensions.String
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<&str> = Vec::new();
        if self.contains(Extensions::TYPE_SCRIPT) {
            parts.push("TypeScript");
        }
        if self.contains(Extensions::JAVA_SCRIPT) {
            parts.push("JavaScript");
        }
        if self.contains(Extensions::DECLARATION) {
            parts.push("Declaration");
        }
        if self.contains(Extensions::JSON) {
            parts.push("JSON");
        }
        f.write_str(&parts.join(", "))
    }
}

#[cfg(test)]
#[path = "types_test.rs"]
mod tests;
