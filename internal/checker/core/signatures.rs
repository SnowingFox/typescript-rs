//! `Signature` and `IndexInfo` representations plus their arenas.
//!
//! Like the `Type` graph, signatures and index signatures live in arenas and
//! are referred to by [`SignatureId`]/[`IndexInfoId`] handles rather than Go's
//! `*Signature`/`*IndexInfo` pointers (PORTING, section 5).
//!
//! Round 4c lands the data shapes and arenas so object/interface types can hold
//! call/construct signatures and index infos. The construction paths that fill
//! them (signature resolution from declarations, index-signature collection)
//! arrive with member resolution in later sub-phases.

use tsgo_ast::{NodeId, SymbolId};

use super::flow::TypePredicateInfo;

use super::types::TypeId;

bitflags::bitflags! {
    /// Flags describing a call/construct [`Signature`].
    ///
    /// Mirrors Go `SignatureFlags`. 4c ports the single-bit flags plus the
    /// `PropagatingFlags`/`CallChainFlags` masks.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::SignatureFlags;
    /// assert_eq!(SignatureFlags::CONSTRUCT.bits(), 1 << 2);
    /// assert!(SignatureFlags::PROPAGATING_FLAGS.contains(SignatureFlags::HAS_REST_PARAMETER));
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/checker/types.go:SignatureFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct SignatureFlags: u32 {
        /// No flags.
        const NONE = 0;
        /// The last parameter is a rest parameter.
        const HAS_REST_PARAMETER = 1 << 0;
        /// The signature is specialized (has literal types).
        const HAS_LITERAL_TYPES = 1 << 1;
        /// The signature is a construct signature.
        const CONSTRUCT = 1 << 2;
        /// The signature comes from an abstract class/constructor.
        const ABSTRACT = 1 << 3;
        /// From a call chain nested in an outer optional chain.
        const IS_INNER_CALL_CHAIN = 1 << 4;
        /// From a call chain that is the outermost optional chain.
        const IS_OUTER_CALL_CHAIN = 1 << 5;
        /// From a JS file with no types.
        const IS_UNTYPED_SIGNATURE_IN_JS_FILE = 1 << 6;
        /// From a non-inferrable type.
        const IS_NON_INFERRABLE = 1 << 7;
        /// A candidate retained for overload-failure reporting.
        const IS_SIGNATURE_CANDIDATE_FOR_OVERLOAD_FAILURE = 1 << 8;

        /// Flags propagated to instantiated signatures.
        const PROPAGATING_FLAGS = Self::HAS_REST_PARAMETER.bits()
            | Self::HAS_LITERAL_TYPES.bits()
            | Self::CONSTRUCT.bits()
            | Self::ABSTRACT.bits()
            | Self::IS_UNTYPED_SIGNATURE_IN_JS_FILE.bits()
            | Self::IS_SIGNATURE_CANDIDATE_FOR_OVERLOAD_FAILURE.bits();
        /// The two call-chain flags.
        const CALL_CHAIN_FLAGS = Self::IS_INNER_CALL_CHAIN.bits() | Self::IS_OUTER_CALL_CHAIN.bits();
    }
}

/// A call or construct signature.
///
/// 4c ports the structural fields; Go's instantiation/predicate/composite
/// fields are added when signature resolution and instantiation land.
///
/// DEFER(phase-4-checker-4d): `resolved_type_predicate`, `target`, `mapper`,
/// `isolated_signature_type`, and `composite` (signature instantiation and
/// type-guard machinery).
/// blocked-by: `TypeMapper`/instantiation (4d) and type predicates (4g).
///
/// # Examples
/// ```
/// use tsgo_checker::{Signature, SignatureFlags};
/// let s = Signature::new(SignatureFlags::NONE);
/// assert_eq!(s.min_argument_count, 0);
/// assert!(s.parameters.is_empty());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:Signature
#[derive(Clone, Debug, Default)]
pub struct Signature {
    /// Signature flags.
    pub flags: SignatureFlags,
    /// The minimum number of arguments.
    pub min_argument_count: i32,
    /// The declaration this signature came from, if any.
    pub declaration: Option<NodeId>,
    /// Type parameters (for a generic signature).
    pub type_parameters: Vec<TypeId>,
    /// Parameter symbols, in order.
    pub parameters: Vec<SymbolId>,
    /// The `this` parameter symbol, if declared.
    pub this_parameter: Option<SymbolId>,
    /// The resolved return type, once computed.
    pub resolved_return_type: Option<TypeId>,
    /// The resolved type predicate for a `parameter is Type` return annotation.
    pub resolved_type_predicate: Option<TypePredicateInfo>,
    /// The signature this was instantiated from, if any.
    pub target: Option<SignatureId>,
    /// For an instantiated signature, the type mapper applied to its parameter
    /// and return types (Go's `Signature.mapper`). `None` for an un-instantiated
    /// signature. Parameter symbols are kept as the un-instantiated base
    /// symbols; their types are mapped through this on read (Go re-instantiates
    /// the parameter symbols, which is observationally equivalent).
    pub mapper: Option<super::mapper::TypeMapper>,
}

impl Default for SignatureFlags {
    fn default() -> Self {
        SignatureFlags::NONE
    }
}

impl Signature {
    /// Creates an empty signature with the given flags.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Signature, SignatureFlags};
    /// let s = Signature::new(SignatureFlags::CONSTRUCT);
    /// assert!(s.flags.contains(SignatureFlags::CONSTRUCT));
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/checker.go:Checker.newSignature
    pub fn new(flags: SignatureFlags) -> Signature {
        Signature {
            flags,
            ..Default::default()
        }
    }
}

/// An index signature (`[k: K]: V`).
///
/// # Examples
/// ```
/// use tsgo_checker::{IndexInfo, TypeId};
/// let info = IndexInfo::new(TypeId(7), TypeId(5), true);
/// assert!(info.is_readonly);
/// assert_eq!(info.value_type, TypeId(5));
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/types.go:IndexInfo
#[derive(Clone, Debug)]
pub struct IndexInfo {
    /// The key type (`string`/`number`/`symbol`/template literal).
    pub key_type: TypeId,
    /// The value type.
    pub value_type: TypeId,
    /// Whether the index signature is `readonly`.
    pub is_readonly: bool,
    /// The `IndexSignatureDeclaration` node, if any.
    pub declaration: Option<NodeId>,
    /// The synthetic property symbol for this index signature, if any.
    pub index_symbol: Option<SymbolId>,
}

impl IndexInfo {
    /// Creates an index info with no declaration/symbol attached.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{IndexInfo, TypeId};
    /// let info = IndexInfo::new(TypeId(1), TypeId(2), false);
    /// assert_eq!(info.key_type, TypeId(1));
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/checker.go:Checker.newIndexInfo
    pub fn new(key_type: TypeId, value_type: TypeId, is_readonly: bool) -> IndexInfo {
        IndexInfo {
            key_type,
            value_type,
            is_readonly,
            declaration: None,
            index_symbol: None,
        }
    }
}

/// A handle into a [`SignatureArena`], replacing Go's `*Signature`.
///
/// # Examples
/// ```
/// use tsgo_checker::SignatureId;
/// assert_eq!(SignatureId(1).arena_index(), 0);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/checker.go:Checker.signatureArena handle
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct SignatureId(pub u32);

impl SignatureId {
    /// Returns the zero-based `Vec` subscript for this id (`id - 1`).
    ///
    /// Side effects: none (pure).
    pub fn arena_index(self) -> usize {
        (self.0 - 1) as usize
    }
}

/// A handle into an [`IndexInfoArena`], replacing Go's `*IndexInfo`.
///
/// # Examples
/// ```
/// use tsgo_checker::IndexInfoId;
/// assert_eq!(IndexInfoId(2).arena_index(), 1);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/checker.go:Checker.indexInfoArena handle
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct IndexInfoId(pub u32);

impl IndexInfoId {
    /// Returns the zero-based `Vec` subscript for this id (`id - 1`).
    ///
    /// Side effects: none (pure).
    pub fn arena_index(self) -> usize {
        (self.0 - 1) as usize
    }
}

/// An arena owning every [`Signature`], addressed by [`SignatureId`].
///
/// # Examples
/// ```
/// use tsgo_checker::{Signature, SignatureArena, SignatureFlags, SignatureId};
/// let mut arena = SignatureArena::new();
/// let id = arena.alloc(Signature::new(SignatureFlags::NONE));
/// assert_eq!(id, SignatureId(1));
/// assert_eq!(arena.len(), 1);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/checker.go:Checker.signatureArena
#[derive(Clone, Debug, Default)]
pub struct SignatureArena {
    signatures: Vec<Signature>,
}

impl SignatureArena {
    /// Creates an empty signature arena.
    ///
    /// Side effects: none (pure).
    pub fn new() -> Self {
        SignatureArena::default()
    }

    /// Returns the number of allocated signatures.
    ///
    /// Side effects: none (pure).
    pub fn len(&self) -> usize {
        self.signatures.len()
    }

    /// Reports whether the arena is empty.
    ///
    /// Side effects: none (pure).
    pub fn is_empty(&self) -> bool {
        self.signatures.is_empty()
    }

    /// Allocates `signature`, returning its handle.
    ///
    /// Side effects: mutates `self`.
    pub fn alloc(&mut self, signature: Signature) -> SignatureId {
        self.signatures.push(signature);
        SignatureId(self.signatures.len() as u32)
    }

    /// Returns the signature for `id`.
    ///
    /// # Panics
    /// Panics if `id` was not produced by this arena.
    ///
    /// Side effects: none (pure).
    pub fn get(&self, id: SignatureId) -> &Signature {
        &self.signatures[id.arena_index()]
    }

    /// Returns a mutable reference to the signature for `id`.
    ///
    /// # Panics
    /// Panics if `id` was not produced by this arena.
    ///
    /// Side effects: mutates the referenced signature.
    pub fn get_mut(&mut self, id: SignatureId) -> &mut Signature {
        &mut self.signatures[id.arena_index()]
    }
}

/// An arena owning every [`IndexInfo`], addressed by [`IndexInfoId`].
///
/// # Examples
/// ```
/// use tsgo_checker::{IndexInfo, IndexInfoArena, IndexInfoId, TypeId};
/// let mut arena = IndexInfoArena::new();
/// let id = arena.alloc(IndexInfo::new(TypeId(1), TypeId(2), false));
/// assert_eq!(id, IndexInfoId(1));
/// assert_eq!(arena.get(id).value_type, TypeId(2));
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/checker.go:Checker.indexInfoArena
#[derive(Clone, Debug, Default)]
pub struct IndexInfoArena {
    infos: Vec<IndexInfo>,
}

impl IndexInfoArena {
    /// Creates an empty index-info arena.
    ///
    /// Side effects: none (pure).
    pub fn new() -> Self {
        IndexInfoArena::default()
    }

    /// Returns the number of allocated index infos.
    ///
    /// Side effects: none (pure).
    pub fn len(&self) -> usize {
        self.infos.len()
    }

    /// Reports whether the arena is empty.
    ///
    /// Side effects: none (pure).
    pub fn is_empty(&self) -> bool {
        self.infos.is_empty()
    }

    /// Allocates `info`, returning its handle.
    ///
    /// Side effects: mutates `self`.
    pub fn alloc(&mut self, info: IndexInfo) -> IndexInfoId {
        self.infos.push(info);
        IndexInfoId(self.infos.len() as u32)
    }

    /// Returns the index info for `id`.
    ///
    /// # Panics
    /// Panics if `id` was not produced by this arena.
    ///
    /// Side effects: none (pure).
    pub fn get(&self, id: IndexInfoId) -> &IndexInfo {
        &self.infos[id.arena_index()]
    }
}

#[cfg(test)]
#[path = "signatures_test.rs"]
mod tests;
