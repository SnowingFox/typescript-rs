//! The `Export` value types stored in the auto-import index.
//!
//! 1:1 port of the data types in Go `internal/ls/autoimport/export.go` (the
//! checker-driven extraction functions `SymbolToExport` / `extractFirstExport`
//! are deferred — see the crate worklog). An [`Export`] records one exported
//! name, where it lives ([`ModuleId`] + file/path), how it was exported
//! ([`ExportSyntax`]), and the resolved kind/flags used to rank completions.

use tsgo_ast::symbol::{
    INTERNAL_SYMBOL_NAME_DEFAULT, INTERNAL_SYMBOL_NAME_EXPORT_EQUALS,
    INTERNAL_SYMBOL_NAME_EXPORT_STAR,
};
use tsgo_ast::SymbolFlags;
use tsgo_ls_lsutil::{ScriptElementKind, ScriptElementKindModifier};
use tsgo_tspath::{is_external_module_name_relative, Path};

use crate::index::Named;

/// Uniquely identifies a module across multiple declarations.
///
/// If the export is from an ambient module declaration this is the module name;
/// from a module augmentation it is the resolved module file's `Path`; otherwise
/// it is the exporting source file's `Path`.
///
/// # Examples
/// ```
/// use tsgo_ls_autoimport::ModuleId;
/// let m = ModuleId::new("/src/a.ts");
/// assert_eq!(m.as_str(), "/src/a.ts");
/// ```
///
/// Side effects: none (plain data).
// Go: internal/ls/autoimport/export.go:ModuleID
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ModuleId(pub String);

impl ModuleId {
    /// Wraps a string as a [`ModuleId`].
    ///
    /// Side effects: none (pure).
    pub fn new(s: impl Into<String>) -> ModuleId {
        ModuleId(s.into())
    }

    /// Returns the underlying string.
    ///
    /// Side effects: none (pure).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A `(module, exported name)` pair identifying one export across the program.
///
/// Side effects: none (plain data).
// Go: internal/ls/autoimport/export.go:ExportID
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ExportId {
    /// The module the export lives in.
    pub module_id: ModuleId,
    /// The exported name within that module.
    pub export_name: String,
}

/// The syntactic form an export takes.
///
/// Mirrors Go's `iota`-numbered `ExportSyntax`; discriminants match 1:1, so
/// `ExportSyntax::None as i32 == 0`.
///
/// # Examples
/// ```
/// use tsgo_ls_autoimport::ExportSyntax;
/// assert_eq!(ExportSyntax::None as i32, 0);
/// assert_eq!(ExportSyntax::Star as i32, 7);
/// assert_eq!(ExportSyntax::Named.to_string(), "ExportSyntaxNamed");
/// ```
///
/// Side effects: none (plain data).
// Go: internal/ls/autoimport/export.go:ExportSyntax
#[repr(i32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExportSyntax {
    /// No / unknown export syntax.
    #[default]
    None = 0,
    /// `export const x = {}` (a modifier on the declaration).
    Modifier,
    /// `export { x }`.
    Named,
    /// `export default function f() {}`.
    DefaultModifier,
    /// `export default f`.
    DefaultDeclaration,
    /// `export = x`.
    Equals,
    /// `export as namespace x`.
    Umd,
    /// `export * from "module"`.
    Star,
    /// `module.exports = {}`.
    CommonJsModuleExports,
    /// `exports.x = {}`.
    CommonJsExportsProperty,
}

impl std::fmt::Display for ExportSyntax {
    // Go: internal/ls/autoimport/export_stringer_generated.go:ExportSyntax.String
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ExportSyntax::None => "ExportSyntaxNone",
            ExportSyntax::Modifier => "ExportSyntaxModifier",
            ExportSyntax::Named => "ExportSyntaxNamed",
            ExportSyntax::DefaultModifier => "ExportSyntaxDefaultModifier",
            ExportSyntax::DefaultDeclaration => "ExportSyntaxDefaultDeclaration",
            ExportSyntax::Equals => "ExportSyntaxEquals",
            ExportSyntax::Umd => "ExportSyntaxUMD",
            ExportSyntax::Star => "ExportSyntaxStar",
            ExportSyntax::CommonJsModuleExports => "ExportSyntaxCommonJSModuleExports",
            ExportSyntax::CommonJsExportsProperty => "ExportSyntaxCommonJSExportsProperty",
        };
        f.write_str(name)
    }
}

/// One exported symbol indexed for auto-import.
///
/// Embeds an [`ExportId`] (`id`) the way Go embeds `ExportID`; the promoted Go
/// fields `e.ModuleID` / `e.ExportName` are reached here as `e.id.module_id` /
/// `e.id.export_name`. The unexported Go fields `localName` / `through` become
/// `pub(crate)` (the crate is the analog of Go's package).
///
/// Side effects: none (plain data).
// Go: internal/ls/autoimport/export.go:Export
#[derive(Clone, Debug)]
pub struct Export {
    /// The `(module, name)` identity of this export.
    pub id: ExportId,
    /// The original file name of the exporting module.
    pub module_file_name: String,
    /// How the export was written.
    pub syntax: ExportSyntax,
    /// The exported symbol's combined flags.
    pub flags: SymbolFlags,
    /// A usable display name for a default-like export (`localName` in Go).
    pub(crate) local_name: String,
    /// The module-symbol export this was found on (`export=`, export-star, or
    /// empty) — `through` in Go.
    pub(crate) through: String,
    /// The resolved target of an aliased export.
    pub target: ExportId,
    /// Whether the export is type-only.
    pub is_type_only: bool,
    /// The element kind used for completion display/sorting.
    pub script_element_kind: ScriptElementKind,
    /// The element kind modifiers (accessibility, file extension, ...).
    pub script_element_kind_modifiers: ScriptElementKindModifier,
    /// The file the export was found in.
    pub path: Path,
    /// The owning package name (empty for project-local files).
    pub package_name: String,
}

impl Default for Export {
    fn default() -> Self {
        Export {
            id: ExportId::default(),
            module_file_name: String::new(),
            syntax: ExportSyntax::None,
            flags: SymbolFlags::NONE,
            local_name: String::new(),
            through: String::new(),
            target: ExportId::default(),
            is_type_only: false,
            script_element_kind: ScriptElementKind::Unknown,
            script_element_kind_modifiers: ScriptElementKindModifier::empty(),
            path: Path::default(),
            package_name: String::new(),
        }
    }
}

impl Export {
    /// The display name of this export.
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/autoimport/export.go:Export.Name
    pub fn name(&self) -> String {
        if !self.local_name.is_empty() {
            return self.local_name.clone();
        }
        if self.id.export_name == INTERNAL_SYMBOL_NAME_EXPORT_EQUALS {
            return self.target.export_name.clone();
        }
        self.id.export_name.clone()
    }

    /// Whether the export's name can be renamed at the import site (the symbol
    /// is `export=` or a `default` export, so the import binding is free).
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/autoimport/export.go:Export.IsRenameable
    pub fn is_renameable(&self) -> bool {
        self.id.export_name == INTERNAL_SYMBOL_NAME_EXPORT_EQUALS
            || self.id.export_name == INTERNAL_SYMBOL_NAME_DEFAULT
    }

    /// The ambient-module name this export belongs to, or empty if the module is
    /// a relative/file path.
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/autoimport/export.go:Export.AmbientModuleName
    pub fn ambient_module_name(&self) -> String {
        if !is_external_module_name_relative(self.id.module_id.as_str()) {
            return self.id.module_id.0.clone();
        }
        String::new()
    }

    /// Whether this export is an alias that could not be resolved to a target.
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/autoimport/export.go:Export.IsUnresolvedAlias
    pub fn is_unresolved_alias(&self) -> bool {
        self.flags == SymbolFlags::ALIAS
    }

    /// The module-symbol export this export was found on: `export=`,
    /// [`INTERNAL_SYMBOL_NAME_EXPORT_STAR`], or empty.
    ///
    /// Mirrors Go's unexported `through` field; the import-statement builder
    /// (deferred) reads it to decide how to write the import.
    ///
    /// Side effects: none (pure).
    // Go: internal/ls/autoimport/export.go:Export.through
    pub fn through(&self) -> &str {
        &self.through
    }
}

impl Named for Export {
    fn name(&self) -> String {
        Export::name(self)
    }
}

/// Whether `name` is not usable as a written import binding (an empty or
/// internal placeholder name).
///
/// # Examples
/// ```
/// use tsgo_ls_autoimport::export::is_unusable_name;
/// assert!(is_unusable_name(""));
/// assert!(is_unusable_name("default"));
/// assert!(!is_unusable_name("foo"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/ls/autoimport/extract.go:isUnusableName
pub fn is_unusable_name(name: &str) -> bool {
    name.is_empty()
        || name == "_default"
        || name == INTERNAL_SYMBOL_NAME_EXPORT_STAR
        || name == INTERNAL_SYMBOL_NAME_DEFAULT
        || name == INTERNAL_SYMBOL_NAME_EXPORT_EQUALS
}

#[cfg(test)]
#[path = "export_test.rs"]
mod tests;
