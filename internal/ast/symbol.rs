//! `Symbol`, `SymbolTable`, and internal symbol-name constants.

use crate::ids::{NodeId, SymbolId};
use crate::{CheckFlags, SymbolFlags};
use rustc_hash::FxHashMap;

/// A binder-time symbol prototype.
///
/// References to other symbols (`parent`, `export_symbol`) use [`SymbolId`] and
/// references to declarations use [`NodeId`], mirroring the arena ownership
/// model used for the node graph.
///
/// Side effects: none (pure value type).
// Go: internal/ast/symbol.go:Symbol
#[derive(Clone, Debug, Default)]
pub struct Symbol {
    /// What this symbol is (variable/class/type/...).
    pub flags: SymbolFlags,
    /// Checker-time flags (non-zero only on transient symbols).
    pub check_flags: CheckFlags,
    /// The symbol name (possibly an internal name; see the `INTERNAL_*` consts).
    pub name: String,
    /// Declaration nodes contributing to this symbol.
    pub declarations: Vec<NodeId>,
    /// The primary value declaration, if any.
    pub value_declaration: Option<NodeId>,
    /// Member symbol table.
    pub members: SymbolTable,
    /// Export symbol table.
    pub exports: SymbolTable,
    /// Parent symbol, if any.
    pub parent: Option<SymbolId>,
    /// The export symbol this local is associated with, if any.
    pub export_symbol: Option<SymbolId>,
}

impl Default for CheckFlags {
    fn default() -> Self {
        CheckFlags::empty()
    }
}

impl Default for SymbolFlags {
    fn default() -> Self {
        SymbolFlags::empty()
    }
}

impl Symbol {
    /// Reports whether this symbol denotes an external module (a module symbol
    /// whose name is a quoted string).
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::{Symbol, SymbolFlags};
    /// let mut s = Symbol::default();
    /// s.flags = SymbolFlags::VALUE_MODULE;
    /// s.name = "\"mod\"".to_string();
    /// assert!(s.is_external_module());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/symbol.go:Symbol.IsExternalModule
    pub fn is_external_module(&self) -> bool {
        self.flags.intersects(SymbolFlags::MODULE) && self.name.starts_with('"')
    }

    /// Returns this symbol's flags combined with its export symbol's flags.
    ///
    /// Unlike Go (which dereferences `s.ExportSymbol` directly), the export
    /// symbol's flags are passed in by the caller after arena resolution.
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/symbol.go:Symbol.CombinedLocalAndExportSymbolFlags
    pub fn combined_local_and_export_symbol_flags(
        &self,
        export_symbol_flags: Option<SymbolFlags>,
    ) -> SymbolFlags {
        match export_symbol_flags {
            Some(export) => self.flags | export,
            None => self.flags,
        }
    }
}

/// A symbol table: a map from name to [`SymbolId`].
// Go: internal/ast/symbol.go:SymbolTable
pub type SymbolTable = FxHashMap<String, SymbolId>;

/// Prefix marking an internal symbol name (never a valid identifier name).
///
/// DIVERGENCE(port): Go uses the raw byte `0xFE` (invalid UTF-8) as the prefix.
/// Rust strings must be valid UTF-8, so we use the code point `U+00FE`. This
/// only affects internal symbol-table keys and never appears in emitted output.
// Go: internal/ast/symbol.go:InternalSymbolNamePrefix
pub const INTERNAL_SYMBOL_NAME_PREFIX: &str = "\u{FE}";

/// Internal name for call signatures.
// Go: internal/ast/symbol.go:InternalSymbolNameCall
pub const INTERNAL_SYMBOL_NAME_CALL: &str = "\u{FE}call";
/// Internal name for constructor implementations.
// Go: internal/ast/symbol.go:InternalSymbolNameConstructor
pub const INTERNAL_SYMBOL_NAME_CONSTRUCTOR: &str = "\u{FE}constructor";
/// Internal name for constructor signatures.
// Go: internal/ast/symbol.go:InternalSymbolNameNew
pub const INTERNAL_SYMBOL_NAME_NEW: &str = "\u{FE}new";
/// Internal name for index signatures.
// Go: internal/ast/symbol.go:InternalSymbolNameIndex
pub const INTERNAL_SYMBOL_NAME_INDEX: &str = "\u{FE}index";
/// Internal name for module `export *` declarations.
// Go: internal/ast/symbol.go:InternalSymbolNameExportStar
pub const INTERNAL_SYMBOL_NAME_EXPORT_STAR: &str = "\u{FE}export";
/// Internal name for the global self-reference.
// Go: internal/ast/symbol.go:InternalSymbolNameGlobal
pub const INTERNAL_SYMBOL_NAME_GLOBAL: &str = "\u{FE}global";
/// Internal name indicating a missing symbol.
// Go: internal/ast/symbol.go:InternalSymbolNameMissing
pub const INTERNAL_SYMBOL_NAME_MISSING: &str = "\u{FE}missing";
/// Internal name for an anonymous type literal symbol.
// Go: internal/ast/symbol.go:InternalSymbolNameType
pub const INTERNAL_SYMBOL_NAME_TYPE: &str = "\u{FE}type";
/// Internal name for an anonymous object literal declaration.
// Go: internal/ast/symbol.go:InternalSymbolNameObject
pub const INTERNAL_SYMBOL_NAME_OBJECT: &str = "\u{FE}object";
/// Internal name for anonymous JSX attributes.
// Go: internal/ast/symbol.go:InternalSymbolNameJSXAttributes
pub const INTERNAL_SYMBOL_NAME_JSX_ATTRIBUTES: &str = "\u{FE}jsxAttributes";
/// Internal name for an unnamed class expression.
// Go: internal/ast/symbol.go:InternalSymbolNameClass
pub const INTERNAL_SYMBOL_NAME_CLASS: &str = "\u{FE}class";
/// Internal name for an unnamed function expression.
// Go: internal/ast/symbol.go:InternalSymbolNameFunction
pub const INTERNAL_SYMBOL_NAME_FUNCTION: &str = "\u{FE}function";
/// Internal name for a computed property with a dynamic name.
// Go: internal/ast/symbol.go:InternalSymbolNameComputed
pub const INTERNAL_SYMBOL_NAME_COMPUTED: &str = "\u{FE}computed";
/// Internal name for assignment declarations.
// Go: internal/ast/symbol.go:InternalSymbolNameAssignmentDeclaration
pub const INTERNAL_SYMBOL_NAME_ASSIGNMENT_DECLARATION: &str = "\u{FE}assignment";
/// Internal name for instantiation expressions.
// Go: internal/ast/symbol.go:InternalSymbolNameInstantiationExpression
pub const INTERNAL_SYMBOL_NAME_INSTANTIATION_EXPRESSION: &str = "\u{FE}instantiationExpression";
/// Internal name for import attributes.
// Go: internal/ast/symbol.go:InternalSymbolNameImportAttributes
pub const INTERNAL_SYMBOL_NAME_IMPORT_ATTRIBUTES: &str = "\u{FE}importAttributes";
/// Name for the `export =` assignment symbol (not prefixed).
// Go: internal/ast/symbol.go:InternalSymbolNameExportEquals
pub const INTERNAL_SYMBOL_NAME_EXPORT_EQUALS: &str = "export=";
/// Name for the default export symbol (not prefixed).
// Go: internal/ast/symbol.go:InternalSymbolNameDefault
pub const INTERNAL_SYMBOL_NAME_DEFAULT: &str = "default";
/// Name for the `this` symbol.
// Go: internal/ast/symbol.go:InternalSymbolNameThis
pub const INTERNAL_SYMBOL_NAME_THIS: &str = "this";
/// Name for the CommonJS `module.exports` symbol.
// Go: internal/ast/symbol.go:InternalSymbolNameModuleExports
pub const INTERNAL_SYMBOL_NAME_MODULE_EXPORTS: &str = "module.exports";

/// Replaces internal symbol-name prefixes with `__`.
///
/// # Examples
/// ```
/// use tsgo_ast::symbol::{escape_all_internal_symbol_names, INTERNAL_SYMBOL_NAME_CALL};
/// assert_eq!(escape_all_internal_symbol_names(INTERNAL_SYMBOL_NAME_CALL), "__call");
/// ```
///
/// Side effects: none (pure).
// Go: internal/ast/symbol.go:EscapeAllInternalSymbolNames
pub fn escape_all_internal_symbol_names(name: &str) -> String {
    name.replace(INTERNAL_SYMBOL_NAME_PREFIX, "__")
}

#[cfg(test)]
#[path = "symbol_test.rs"]
mod tests;
