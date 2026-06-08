//! Symbol-resolution scaffolding layered on `tsgo_ast`'s symbol model and the
//! binder's output.
//!
//! The binder (`tsgo_binder`) produces `tsgo_ast::Symbol`s addressed by
//! [`SymbolId`] and grouped into [`SymbolTable`]s. The checker attaches its own
//! lazily-computed per-symbol "links" (Go's ~30 `core.LinkStore[*ast.Symbol,
//! XxxLinks]` stores). Round 4a establishes that link-store foundation plus the
//! first concrete link record; `resolve_name` / `get_symbol_at_location` and the
//! remaining link kinds land in sub-phase 4b.

use rustc_hash::FxHashMap;
use tsgo_ast::NodeId;
pub use tsgo_ast::{Symbol, SymbolFlags, SymbolId, SymbolTable};
use tsgo_core::linkstore::LinkStore;

use super::types::TypeId;

/// A lazily-populated, per-symbol side table keyed by [`SymbolId`].
///
/// This is the checker's equivalent of Go's `core.LinkStore[*ast.Symbol, V]`:
/// `get` materializes a `V::default()` on first access, while `try_get`/`has`
/// observe without creating. The checker holds one such store per link kind.
///
/// # Examples
/// ```
/// use tsgo_checker::{SymbolLinks, SymbolReferenceLinks};
/// use tsgo_ast::{SymbolFlags, SymbolId};
///
/// let mut links: SymbolLinks<SymbolReferenceLinks> = SymbolLinks::default();
/// let id = SymbolId(3);
/// assert!(!links.has(&id));
/// links.get(id).reference_kinds = SymbolFlags::VALUE;
/// assert_eq!(links.try_get(&id).unwrap().reference_kinds, SymbolFlags::VALUE);
/// ```
///
/// Side effects: none (pure type alias).
// Go: internal/core/linkstore.go:LinkStore (as used throughout internal/checker/checker.go)
pub type SymbolLinks<V> = LinkStore<SymbolId, V>;

/// Per-symbol links recording the meanings under which a symbol was referenced.
///
/// # Examples
/// ```
/// use tsgo_checker::SymbolReferenceLinks;
/// use tsgo_ast::SymbolFlags;
/// let links = SymbolReferenceLinks::default();
/// assert_eq!(links.reference_kinds, SymbolFlags::empty());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:SymbolReferenceLinks
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SymbolReferenceLinks {
    /// Flags for the meanings of the symbol that were referenced.
    pub reference_kinds: SymbolFlags,
}

/// Per-symbol links for a value symbol's resolved type and related state.
///
/// 4b ports the id/symbol-valued fields; Go's `mapper *TypeMapper` field is
/// omitted until the type mapper is ported.
///
/// DEFER(phase-4-checker-4d): the `mapper` field (synthetic-property
/// instantiation) is added with `TypeMapper`.
/// blocked-by: `TypeMapper` (`mapper.go`) lands in sub-phase 4d.
///
/// # Examples
/// ```
/// use tsgo_checker::ValueSymbolLinks;
/// let links = ValueSymbolLinks::default();
/// assert!(links.resolved_type.is_none());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:ValueSymbolLinks
#[derive(Clone, Debug, Default)]
pub struct ValueSymbolLinks {
    /// Type of the value symbol.
    pub resolved_type: Option<TypeId>,
    /// The write type (for accessors / write-only positions).
    pub write_type: Option<TypeId>,
    /// The aliased/instantiation target symbol, if any.
    pub target: Option<SymbolId>,
    /// Substitution mapper for an instantiated symbol.
    pub mapper: Option<super::mapper::TypeMapper>,
    /// The literal "name type" for a computed/synthetic property.
    pub name_type: Option<TypeId>,
    /// The containing union/intersection/mapped type for a synthetic property.
    pub containing_type: Option<TypeId>,
    /// Whether a function/constructor symbol's signatures were checked.
    pub function_or_constructor_checked: bool,
}

/// Per-symbol links for a mapped-type property.
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:MappedSymbolLinks
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MappedSymbolLinks {
    pub key_type: Option<super::types::TypeId>,
}

/// Cached derived types of a mapped-type object.
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go (MappedType payload fields)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MappedTypeLinks {
    pub type_parameter: Option<super::types::TypeId>,
    pub constraint_type: Option<super::types::TypeId>,
    pub template_type: Option<super::types::TypeId>,
    pub modifiers_type: Option<super::types::TypeId>,
}

/// Per-symbol links for an alias symbol (`import x = ...`, re-exports).
///
/// # Examples
/// ```
/// use tsgo_checker::AliasSymbolLinks;
/// let links = AliasSymbolLinks::default();
/// assert!(links.alias_target.is_none());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:AliasSymbolLinks
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AliasSymbolLinks {
    /// Immediate alias target (may itself be an alias).
    pub immediate_target: Option<SymbolId>,
    /// Resolved (non-alias) alias target.
    pub alias_target: Option<SymbolId>,
    /// Whether the alias was referenced as an emittable value.
    pub referenced: bool,
    /// First declaration that makes the symbol type-only, if any.
    pub type_only_declaration: Option<NodeId>,
}

/// Per-symbol links for an export-type symbol produced by namespace-import ES
/// module interop wrapping (Go's `ExportTypeLinks`).
// Go: internal/checker/types.go:ExportTypeLinks
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportTypeLinks {
    /// The underlying export symbol the namespace import re-exports.
    pub target: Option<SymbolId>,
    /// The import declaration that produced the wrapped symbol.
    pub originating_import: Option<NodeId>,
}

/// Per-symbol links for a module symbol's resolved exports.
///
/// # Examples
/// ```
/// use tsgo_checker::ModuleSymbolLinks;
/// let links = ModuleSymbolLinks::default();
/// assert!(links.resolved_exports.is_empty());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:ModuleSymbolLinks
#[derive(Clone, Debug, Default)]
pub struct ModuleSymbolLinks {
    /// Resolved exports of the module (combined static members for a class).
    pub resolved_exports: SymbolTable,
    /// Exports resolved through `export type * from "mod"`, by name.
    pub type_only_export_star_map: FxHashMap<String, NodeId>,
    /// Whether the module's exports have been checked.
    pub exports_checked: bool,
}

/// Per-symbol links for a declared type (interface/class/enum/type parameter).
///
/// # Examples
/// ```
/// use tsgo_checker::DeclaredTypeLinks;
/// let links = DeclaredTypeLinks::default();
/// assert!(links.declared_type.is_none());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:DeclaredTypeLinks
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeclaredTypeLinks {
    /// The declared type of the symbol, once built.
    pub declared_type: Option<TypeId>,
    /// Whether the interface's members have been resolved.
    pub interface_checked: bool,
    /// Whether index signatures have been resolved.
    pub index_signatures_checked: bool,
    /// Whether type parameters have been resolved.
    pub type_parameters_checked: bool,
    /// Whether enum members have been resolved.
    pub enum_checked: bool,
}

#[derive(Clone, Debug, Default)]
pub struct LateBoundLinks {
    pub late_symbol: Option<SymbolId>,
}

#[derive(Clone, Debug, Default)]
pub struct SymbolNodeLinks {
    pub resolved_symbol: Option<SymbolId>,
}

#[derive(Clone, Debug, Default)]
pub struct MembersAndExportsLinks {
    pub resolved_exports: Option<SymbolTable>,
    pub resolved_members: Option<SymbolTable>,
}

/// Per-symbol links for a type alias (`type X = ...`).
///
/// DEFER(phase-4-checker-4d): the generic-alias instantiation cache (Go's
/// `instantiations map`) is added with type-argument instantiation.
/// blocked-by: instantiation (`TypeMapper`) lands in sub-phase 4d.
///
/// # Examples
/// ```
/// use tsgo_checker::TypeAliasLinks;
/// let links = TypeAliasLinks::default();
/// assert!(links.declared_type.is_none());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:TypeAliasLinks
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypeAliasLinks {
    /// The declared (aliased) type, once resolved.
    pub declared_type: Option<TypeId>,
    /// Type parameters of a generic type alias.
    pub type_parameters: Vec<TypeId>,
    /// Whether the alias is a constructor-declared property.
    pub is_constructor_declared_property: bool,
}

/// The checker's symbol-merge table (Go's `Checker.mergedSymbols`).
///
/// When the binder merges declarations of the same entity across files, the
/// checker records `source -> target` so lookups can canonicalize a symbol to
/// its merged form via [`MergedSymbols::get`].
///
/// # Examples
/// ```
/// use tsgo_checker::MergedSymbols;
/// use tsgo_ast::SymbolId;
/// let mut merged = MergedSymbols::default();
/// // An unrecorded symbol maps to itself.
/// assert_eq!(merged.get(SymbolId(1)), SymbolId(1));
/// merged.record(SymbolId(9), SymbolId(1));
/// assert_eq!(merged.get(SymbolId(1)), SymbolId(9));
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/checker.go:Checker.mergedSymbols
#[derive(Clone, Debug, Default)]
pub struct MergedSymbols {
    entries: FxHashMap<SymbolId, SymbolId>,
}

impl MergedSymbols {
    /// Returns the merged symbol for `symbol`, or `symbol` itself if none.
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/checker.go:Checker.getMergedSymbol
    pub fn get(&self, symbol: SymbolId) -> SymbolId {
        self.entries.get(&symbol).copied().unwrap_or(symbol)
    }

    /// Records that `source` was merged into `target`.
    ///
    /// Side effects: mutates the merge table.
    // Go: internal/checker/checker.go:Checker.recordMergedSymbol
    pub fn record(&mut self, target: SymbolId, source: SymbolId) {
        self.entries.insert(source, target);
    }
}

/// Resolves an alias symbol to its target, or returns `symbol` unchanged when
/// it is not an alias.
///
/// Mirrors the intent of Go's `resolveSymbol`/`skipAlias`: a non-alias symbol
/// is returned as-is; an alias is followed to its resolved `alias_target`.
///
/// DEFER(phase-4-checker-4b+): when a symbol carries the `ALIAS` flag but its
/// target has not been computed (`alias_target == None`), the full
/// module-import/export resolution (`resolveAlias`) is not yet ported, so the
/// symbol is returned unchanged.
/// blocked-by: module import/export resolution (`exports.go`,
/// `resolveExternalModuleSymbol`) lands in a later checker round.
///
/// # Examples
/// ```
/// use tsgo_checker::{skip_alias, AliasSymbolLinks};
/// use tsgo_ast::{SymbolFlags, SymbolId};
/// // A plain value symbol is returned unchanged.
/// assert_eq!(skip_alias(SymbolId(1), SymbolFlags::VALUE, None), SymbolId(1));
/// // An alias with a known target is followed.
/// let links = AliasSymbolLinks { alias_target: Some(SymbolId(7)), ..Default::default() };
/// assert_eq!(skip_alias(SymbolId(1), SymbolFlags::ALIAS, Some(&links)), SymbolId(7));
/// ```
///
/// Side effects: none (pure).
// Go: internal/checker/checker.go:Checker.resolveSymbol / resolveAlias
pub fn skip_alias(
    symbol: SymbolId,
    symbol_flags: SymbolFlags,
    alias_links: Option<&AliasSymbolLinks>,
) -> SymbolId {
    if !symbol_flags.contains(SymbolFlags::ALIAS) {
        return symbol;
    }
    alias_links.and_then(|l| l.alias_target).unwrap_or(symbol)
}

/// Resolves `name` to a symbol along the scope chain rooted at `location`,
/// keeping only symbols whose flags intersect `meaning`.
///
/// Walks each ancestor's `locals` table from `location` outward, then (unless
/// `exclude_globals`) consults `globals`. This is the structural core of Go's
/// `resolveName`.
///
/// DEFER(phase-4-checker-4c+): the many special rules of the full resolver are
/// not yet ported: block-scope temporal-dead-zone checks, function-parameter
/// and `arguments`/`this` scoping, property/member scopes, special names,
/// alias type-only validation, merged-symbol canonicalization at lookup, and
/// use-before-declaration error reporting.
/// blocked-by: the binder's `NameResolver.Resolve` hook surface plus
/// declared-type/error machinery land across sub-phases 4c+.
///
/// # Examples
/// ```
/// use tsgo_checker::{resolve_name, BoundProgram};
/// use tsgo_ast::{NodeId, SymbolFlags, SymbolId};
/// // Generic over any bound program (no instantiation needed to type-check).
/// fn find_value<P: BoundProgram>(p: &P, at: NodeId) -> Option<SymbolId> {
///     resolve_name(p, at, "foo", SymbolFlags::VALUE, false, None)
/// }
/// ```
///
/// Side effects: none (pure read over the bound program).
// Go: internal/checker/checker.go:Checker.resolveName / internal/binder/nameresolver.go:Resolve
pub fn resolve_name(
    program: &dyn super::program::BoundProgram,
    location: NodeId,
    name: &str,
    meaning: SymbolFlags,
    exclude_globals: bool,
    globals: Option<&SymbolTable>,
) -> Option<SymbolId> {
    let arena = program.arena();
    let mut current = Some(location);
    while let Some(node) = current {
        if let Some(table) = program.locals(node) {
            if let Some(&symbol) = table.get(name) {
                if program.symbol(symbol).flags.intersects(meaning) {
                    return Some(symbol);
                }
            }
        }
        current = arena.parent(node);
    }
    if !exclude_globals {
        if let Some(&symbol) = globals.and_then(|g| g.get(name)) {
            if program.symbol(symbol).flags.intersects(meaning) {
                return Some(symbol);
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "symbols_test.rs"]
mod tests;
