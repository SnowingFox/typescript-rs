//! Typed arena indices that replace Go's `*Node` / `*Symbol` pointers.

/// A typed index into a `NodeArena`, replacing Go's `*Node`.
///
/// `Copy` and cheap to pass by value. Go uses `uint64`; a `u32` is more than
/// enough for any real source file and halves the per-edge cost.
///
/// # Examples
/// ```
/// use tsgo_ast::ids::NodeId;
/// let a = NodeId(0);
/// let b = NodeId(1);
/// assert_ne!(a, b);
/// assert_eq!(a.index(), 0);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/ast/ids.go:NodeId
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct NodeId(pub u32);

impl NodeId {
    /// Returns the raw `u32` index, for use as a `Vec` subscript.
    ///
    /// Side effects: none (pure).
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A typed index into a `SymbolArena`, replacing Go's `*Symbol`.
///
/// # Examples
/// ```
/// use tsgo_ast::ids::SymbolId;
/// assert_eq!(SymbolId(2), SymbolId(2));
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/ast/ids.go:SymbolId
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct SymbolId(pub u32);

impl SymbolId {
    /// Returns the raw `u32` index, for use as a `Vec` subscript.
    ///
    /// Side effects: none (pure).
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[cfg(test)]
#[path = "ids_test.rs"]
mod tests;
