//! Name resolution along the scope chain.
//!
//! Ports Go `internal/binder/nameresolver.go`. The full `Resolve` routine
//! depends on checker-injected hooks (`GetSymbolOfDeclaration`, `Error`,
//! `Lookup`, ...), so its body is deferred to the checker phase; the resolver
//! shell and the hook-free `get_local_symbol_for_export_default` helper land
//! here now.

use rustc_hash::FxHashMap;
use tsgo_ast::{ModifierFlags, NodeArena, NodeId, Symbol, SymbolId};

use crate::astquery as q;

/// Resolves names along the scope chain to symbols.
///
/// The checker injects the resolution hooks; until then this is a structural
/// placeholder. The precise hook shape is settled in the checker phase.
///
/// Side effects: none yet (resolution is deferred).
// Go: internal/binder/nameresolver.go:NameResolver
#[derive(Default)]
pub struct NameResolver {
    /// Globals table consulted as the outermost scope (checker-provided).
    pub globals: FxHashMap<String, SymbolId>,
}

impl NameResolver {
    /// Creates an empty name resolver.
    ///
    /// # Examples
    /// ```
    /// use tsgo_binder::NameResolver;
    /// let r = NameResolver::default();
    /// assert!(r.globals.is_empty());
    /// ```
    ///
    /// Side effects: none.
    pub fn new() -> NameResolver {
        NameResolver::default()
    }
}

/// Returns the local symbol associated with an `export default` declaration's
/// symbol, if any.
///
/// Mirrors Go `GetLocalSymbolForExportDefault`: the symbol must be an
/// export-default symbol (its first declaration carries the `default`
/// modifier), and the first declaration with a local symbol wins.
///
/// # Examples
/// ```
/// use tsgo_binder::nameresolver::get_local_symbol_for_export_default;
/// use tsgo_ast::{NodeArena, Symbol, SymbolId};
/// use rustc_hash::FxHashMap;
/// let arena = NodeArena::new();
/// let symbols: Vec<Symbol> = vec![Symbol::default()];
/// let locals = FxHashMap::default();
/// assert_eq!(
///     get_local_symbol_for_export_default(&symbols, &locals, &arena, SymbolId(0)),
///     None
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/binder/nameresolver.go:GetLocalSymbolForExportDefault
pub fn get_local_symbol_for_export_default(
    symbols: &[Symbol],
    node_local_symbol: &FxHashMap<NodeId, SymbolId>,
    arena: &NodeArena,
    symbol: SymbolId,
) -> Option<SymbolId> {
    let declarations = &symbols[symbol.index()].declarations;
    if !is_export_default_symbol(symbols, arena, symbol) || declarations.is_empty() {
        return None;
    }
    for decl in declarations {
        if let Some(&local) = node_local_symbol.get(decl) {
            return Some(local);
        }
    }
    None
}

// Go: internal/binder/nameresolver.go:isExportDefaultSymbol
fn is_export_default_symbol(symbols: &[Symbol], arena: &NodeArena, symbol: SymbolId) -> bool {
    let declarations = &symbols[symbol.index()].declarations;
    !declarations.is_empty()
        && q::has_syntactic_modifier(arena, declarations[0], ModifierFlags::DEFAULT)
}

#[cfg(test)]
#[path = "nameresolver_test.rs"]
mod tests;
