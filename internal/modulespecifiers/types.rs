//! Traits, enums, and plain-data structs shared across the crate.
//!
//! 1:1 port of Go `internal/modulespecifiers/types.go`.
//!
//! # Divergence from Go
//! * Go's string-valued constant "enums" (`ImportModuleSpecifierPreference`
//!   etc.) become real discriminated enums with `as_str`/`from_str` round-trips.
//! * `ast.HasFileName` is not yet ported in `tsgo_ast`, so a minimal
//!   [`HasFileName`] trait is defined locally with the same two accessors.
//! * `SourceFileForSpecifierGeneration.Imports()` returns `[]*ast.StringLiteralLike`
//!   in Go; the AST node graph is not ported, so [`imports`](SourceFileForSpecifierGeneration::imports)
//!   exposes only the specifier texts, which is all the ported consumers read.
//! * `tsoptions.SourceOutputAndProjectReference` is not yet ported (P6), so a
//!   minimal [`SourceOutputAndProjectReference`] is defined locally.
//! * The two host methods that take `*ast.StringLiteralLike`
//!   (`GetResolvedModuleFromModuleSpecifier`/`GetModeForUsageLocation`) are
//!   omitted until `computeModuleSpecifiers` is ported in a later phase.

use tsgo_core::compileroptions::ResolutionMode;
use tsgo_packagejson::InfoCacheEntry;
use tsgo_symlinks::KnownSymlinks;
use tsgo_tspath::Path;

/// Minimal stand-in for Go's `ast.HasFileName`: anything that knows its own
/// file name and canonical path.
///
/// Defined locally because `tsgo_ast` has not yet ported `ast.HasFileName`.
// Go: internal/ast/ast.go:HasFileName
pub trait HasFileName {
    /// The file's name (its `parseOptions.FileName`).
    fn file_name(&self) -> String;
    /// The file's canonical [`Path`].
    fn path(&self) -> Path;
}

/// The minimal source-file shape needed to generate module specifiers for it.
///
/// Mirrors Go's `SourceFileForSpecifierGeneration`. Because the AST node graph
/// is not yet ported, [`imports`](Self::imports) yields the import-specifier
/// texts rather than the `*ast.StringLiteralLike` nodes.
// Go: internal/modulespecifiers/types.go:SourceFileForSpecifierGeneration
pub trait SourceFileForSpecifierGeneration: HasFileName {
    /// The module-specifier texts of this file's imports.
    fn imports(&self) -> Vec<String>;
    /// Whether the file is a JavaScript file.
    fn is_js(&self) -> bool;
}

// The `CheckerShape` trait (a checker that resolves a node's symbol and follows
// aliases) is used only by ambient-module specifier generation.
// DEFER(phase-checker): blocked-by: tsgo_ast Node / GetSymbolAtLocation graph
// not ported. Go: internal/modulespecifiers/types.go:CheckerShape

/// Which generation strategy produced a specifier (in priority order).
///
/// # Examples
/// ```
/// use tsgo_modulespecifiers::ResultKind;
/// assert_eq!(ResultKind::None as u8, 0);
/// assert_eq!(ResultKind::Ambient as u8, 5);
/// ```
///
/// Side effects: none (plain data).
// Go: internal/modulespecifiers/types.go:ResultKind
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultKind {
    /// No specifier / pre-existing specifier reused.
    None = 0,
    /// A bare `node_modules` package specifier.
    NodeModules = 1,
    /// A `paths`/`baseUrl` mapping.
    Paths = 2,
    /// A project-reference redirect.
    Redirect = 3,
    /// A relative path.
    Relative = 4,
    /// An ambient module declaration name.
    Ambient = 5,
}

/// One equivalent on-disk path to the imported file (possibly via symlink or
/// project-reference redirect).
///
/// # Examples
/// ```
/// use tsgo_modulespecifiers::ModulePath;
/// let p = ModulePath {
///     file_name: "/a/b.ts".to_string(),
///     is_in_node_modules: false,
///     is_redirect: false,
/// };
/// assert_eq!(p.file_name, "/a/b.ts");
/// ```
///
/// Side effects: none (plain data).
// Go: internal/modulespecifiers/types.go:ModulePath
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModulePath {
    /// The absolute file name of this path.
    pub file_name: String,
    /// Whether the path passes through a `node_modules` directory.
    pub is_in_node_modules: bool,
    /// Whether the path is a project-reference redirect target.
    pub is_redirect: bool,
}

/// The minimal project-reference redirect data read during specifier
/// generation.
///
/// Defined locally because `tsoptions.SourceOutputAndProjectReference` is not
/// yet ported (it belongs to P6).
// Go: internal/tsoptions/parsedcommandline.go:SourceOutputAndProjectReference
// DEFER(phase-6): blocked-by: tsgo_tsoptions::SourceOutputAndProjectReference not ported.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceOutputAndProjectReference {
    /// The original source file name.
    pub source: String,
    /// The emitted declaration file name (`.d.ts`) for the source, if any.
    pub output_dts: String,
}

/// The user's preference for relative vs non-relative import specifiers.
///
/// # Examples
/// ```
/// use tsgo_modulespecifiers::ImportModuleSpecifierPreference;
/// assert_eq!(ImportModuleSpecifierPreference::Shortest.as_str(), "shortest");
/// assert_eq!(
///     ImportModuleSpecifierPreference::from_str("non-relative"),
///     ImportModuleSpecifierPreference::NonRelative
/// );
/// ```
///
/// Side effects: none (plain data).
// Go: internal/modulespecifiers/types.go:ImportModuleSpecifierPreference
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImportModuleSpecifierPreference {
    /// Unset (the empty string in Go).
    #[default]
    None,
    /// `"shortest"`.
    Shortest,
    /// `"project-relative"`.
    ProjectRelative,
    /// `"relative"`.
    Relative,
    /// `"non-relative"`.
    NonRelative,
}

impl ImportModuleSpecifierPreference {
    /// Returns the wire string for this preference.
    ///
    /// Side effects: none (pure).
    pub fn as_str(self) -> &'static str {
        match self {
            ImportModuleSpecifierPreference::None => "",
            ImportModuleSpecifierPreference::Shortest => "shortest",
            ImportModuleSpecifierPreference::ProjectRelative => "project-relative",
            ImportModuleSpecifierPreference::Relative => "relative",
            ImportModuleSpecifierPreference::NonRelative => "non-relative",
        }
    }

    /// Parses a preference from its wire string (unknown values map to `None`).
    ///
    /// Named `from_str` to mirror Go; this is an infallible mapping (unknown ->
    /// `None`), not the fallible [`std::str::FromStr`].
    ///
    /// Side effects: none (pure).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "shortest" => ImportModuleSpecifierPreference::Shortest,
            "project-relative" => ImportModuleSpecifierPreference::ProjectRelative,
            "relative" => ImportModuleSpecifierPreference::Relative,
            "non-relative" => ImportModuleSpecifierPreference::NonRelative,
            _ => ImportModuleSpecifierPreference::None,
        }
    }
}

/// The user's preference for how a specifier should end (extension/index).
///
/// # Examples
/// ```
/// use tsgo_modulespecifiers::ImportModuleSpecifierEndingPreference;
/// assert_eq!(ImportModuleSpecifierEndingPreference::Js.as_str(), "js");
/// ```
///
/// Side effects: none (plain data).
// Go: internal/modulespecifiers/types.go:ImportModuleSpecifierEndingPreference
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImportModuleSpecifierEndingPreference {
    /// Unset (the empty string in Go).
    #[default]
    None,
    /// `"auto"`.
    Auto,
    /// `"minimal"`.
    Minimal,
    /// `"index"`.
    Index,
    /// `"js"`.
    Js,
}

impl ImportModuleSpecifierEndingPreference {
    /// Returns the wire string for this preference.
    ///
    /// Side effects: none (pure).
    pub fn as_str(self) -> &'static str {
        match self {
            ImportModuleSpecifierEndingPreference::None => "",
            ImportModuleSpecifierEndingPreference::Auto => "auto",
            ImportModuleSpecifierEndingPreference::Minimal => "minimal",
            ImportModuleSpecifierEndingPreference::Index => "index",
            ImportModuleSpecifierEndingPreference::Js => "js",
        }
    }

    /// Parses an ending preference from its wire string (unknown -> `None`).
    ///
    /// Named `from_str` to mirror Go; infallible, not [`std::str::FromStr`].
    ///
    /// Side effects: none (pure).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "auto" => ImportModuleSpecifierEndingPreference::Auto,
            "minimal" => ImportModuleSpecifierEndingPreference::Minimal,
            "index" => ImportModuleSpecifierEndingPreference::Index,
            "js" => ImportModuleSpecifierEndingPreference::Js,
            _ => ImportModuleSpecifierEndingPreference::None,
        }
    }
}

/// The user-configurable knobs that influence specifier generation.
///
/// Side effects: none (plain data).
// Go: internal/modulespecifiers/types.go:UserPreferences
#[derive(Clone, Debug, Default)]
pub struct UserPreferences {
    /// Relative vs non-relative preference.
    pub import_module_specifier_preference: ImportModuleSpecifierPreference,
    /// Ending (extension/index) preference.
    pub import_module_specifier_ending: ImportModuleSpecifierEndingPreference,
    /// Regexes whose match excludes a candidate from auto-import suggestions.
    pub auto_import_specifier_exclude_regexes: Vec<String>,
}

/// Per-call overrides for specifier generation.
///
/// Side effects: none (plain data).
// Go: internal/modulespecifiers/types.go:ModuleSpecifierOptions
#[derive(Clone, Copy, Debug, Default)]
pub struct ModuleSpecifierOptions {
    /// Force a specific import mode instead of the file's default.
    pub override_import_mode: ResolutionMode,
}

/// The resolved relative-preference policy (after folding old-specifier hints).
///
/// Side effects: none (plain data).
// Go: internal/modulespecifiers/types.go:RelativePreferenceKind
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelativePreferenceKind {
    /// Always prefer a relative path.
    Relative = 0,
    /// Always prefer a non-relative specifier.
    NonRelative = 1,
    /// Prefer whichever is shortest.
    Shortest = 2,
    /// Prefer non-relative only when crossing a project/package boundary.
    ExternalNonRelative = 3,
}

/// How a generated specifier should end.
///
/// # Examples
/// ```
/// use tsgo_modulespecifiers::ModuleSpecifierEnding;
/// assert_eq!(ModuleSpecifierEnding::Minimal as u8, 0);
/// ```
///
/// Side effects: none (plain data).
// Go: internal/modulespecifiers/types.go:ModuleSpecifierEnding
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleSpecifierEnding {
    /// No extension and no trailing `/index`.
    Minimal = 0,
    /// No extension, but keep `/index`.
    Index = 1,
    /// A `.js`-family extension.
    JsExtension = 2,
    /// A `.ts`-family extension.
    TsExtension = 3,
}

/// How an `exports`/`imports` key is matched against a target path.
///
/// Side effects: none (plain data).
// Go: internal/modulespecifiers/types.go:MatchingMode
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchingMode {
    /// The key matches a path exactly.
    Exact = 0,
    /// The key matches a directory prefix (ends with `/`).
    Directory = 1,
    /// The key contains a `*` wildcard.
    Pattern = 2,
}

/// The host that supplies file-system, resolution-cache, and project-reference
/// state needed to generate module specifiers.
///
/// Mirrors Go's `ModuleSpecifierGenerationHost`. The two methods that take
/// `*ast.StringLiteralLike` are omitted until `computeModuleSpecifiers` is
/// ported (they require the not-yet-ported AST node graph).
// Go: internal/modulespecifiers/types.go:ModuleSpecifierGenerationHost
pub trait ModuleSpecifierGenerationHost {
    /// The known symlink cache, if any.
    fn get_symlink_cache(&self) -> Option<&KnownSymlinks>;
    /// The longest common directory of the program's input files.
    fn common_source_directory(&self) -> String;
    /// The global typings cache location, or empty.
    fn get_global_typings_cache_location(&self) -> String;
    /// Whether file names are compared case-sensitively.
    fn use_case_sensitive_file_names(&self) -> bool;
    /// The current working directory.
    fn get_current_directory(&self) -> String;
    /// The project-reference redirect for `path`, if the source maps to one.
    fn get_project_reference_from_source(
        &self,
        path: &Path,
    ) -> Option<SourceOutputAndProjectReference>;
    /// Other files that redirect to the same module as `path`.
    fn get_redirect_targets(&self, path: &Path) -> Vec<String>;
    /// The original source file name when `file` is a project-reference output.
    fn get_source_of_project_reference_if_output_included(&self, file: &dyn HasFileName) -> String;
    /// Whether `path` exists on disk.
    fn file_exists(&self, path: &str) -> bool;
    /// The nearest ancestor of `dirname` that contains a `package.json`.
    fn get_nearest_ancestor_directory_with_package_json(&self, dirname: &str) -> String;
    /// The cached `package.json` info at `pkg_json_path`, if any.
    fn get_package_json_info(&self, pkg_json_path: &str) -> Option<&InfoCacheEntry>;
    /// The default import resolution mode for `file`.
    fn get_default_resolution_mode_for_file(&self, file: &dyn HasFileName) -> ResolutionMode;
}

#[cfg(test)]
#[path = "types_test.rs"]
mod tests;
