//! `tsgo_nodebuilder` — 1:1 Rust port of Go `internal/nodebuilder`.
//!
//! Defines the shared *node builder* contract: the [`SymbolTracker`] callback
//! trait plus the [`Flags`] / [`InternalFlags`] bit sets. The concrete node
//! builder is implemented on top of the checker, but these types are also
//! consumed by the emit resolver in the printer, so they live in this small
//! leaf crate to break the checker <-> printer dependency cycle.

use tsgo_ast::{NodeId, SymbolFlags, SymbolId};

/// Callbacks the node builder invokes while serializing a `Type`/`Symbol`,
/// letting the host (declaration emit, hover) record diagnostics and tracked
/// symbols.
///
/// Mirrors Go `nodebuilder.SymbolTracker`. Go passes `*ast.Symbol` / `*ast.Node`
/// / `*ast.SourceFile` pointers; the Rust port uses arena indices ([`SymbolId`],
/// [`NodeId`]) per the project ownership model. `enclosing_declaration` is the
/// canonically optional pointer, so it maps to `Option<NodeId>`. The Go source
/// is itself the only `*ast.SourceFile`-typed parameter (`containing_file`), and
/// since a source file is a node it is represented here as a [`NodeId`].
///
/// The trait is intentionally object-safe so the printer can hold a
/// `&mut dyn SymbolTracker` without depending on the checker.
///
/// # Examples
/// ```
/// use tsgo_nodebuilder::SymbolTracker;
/// use tsgo_ast::{NodeId, SymbolId, SymbolFlags};
///
/// struct Noop;
/// impl SymbolTracker for Noop {
///     fn track_symbol(&mut self, _s: SymbolId, _d: Option<NodeId>, _m: SymbolFlags) -> bool {
///         false
///     }
///     fn report_inaccessible_this_error(&mut self) {}
///     fn report_private_in_base_of_class_expression(&mut self, _p: &str) {}
///     fn report_inaccessible_unique_symbol_error(&mut self) {}
///     fn report_cyclic_structure_error(&mut self) {}
///     fn report_likely_unsafe_import_required_error(&mut self, _s: &str, _n: &str) {}
///     fn report_truncation_error(&mut self) {}
///     fn report_nonlocal_augmentation(&mut self, _f: NodeId, _p: SymbolId, _a: SymbolId) {}
///     fn report_non_serializable_property(&mut self, _p: &str) {}
///     fn report_inference_fallback(&mut self, _n: NodeId) {}
///     fn push_error_fallback_node(&mut self, _n: NodeId) {}
///     fn pop_error_fallback_node(&mut self) {}
/// }
/// let mut t: Box<dyn SymbolTracker> = Box::new(Noop);
/// assert!(!t.track_symbol(SymbolId(0), None, SymbolFlags::NONE));
/// ```
///
/// Side effects: defined by the implementor (typically diagnostic reporting and
/// recording of tracked symbols); the trait itself prescribes none.
// Go: internal/nodebuilder/types.go:SymbolTracker
pub trait SymbolTracker {
    /// Records that `symbol` was referenced; returns `true` if the host
    /// reported an accessibility diagnostic for it.
    fn track_symbol(
        &mut self,
        symbol: SymbolId,
        enclosing_declaration: Option<NodeId>,
        meaning: SymbolFlags,
    ) -> bool;
    /// Reports that `this` is not accessible in the current context.
    fn report_inaccessible_this_error(&mut self);
    /// Reports a private member of a base class expression named `property_name`.
    fn report_private_in_base_of_class_expression(&mut self, property_name: &str);
    /// Reports an inaccessible `unique symbol` type.
    fn report_inaccessible_unique_symbol_error(&mut self);
    /// Reports that the type/symbol graph is cyclic.
    fn report_cyclic_structure_error(&mut self);
    /// Reports that emitting the type likely requires an unsafe import of
    /// `symbol_name` from `specifier`.
    fn report_likely_unsafe_import_required_error(&mut self, specifier: &str, symbol_name: &str);
    /// Reports that the printed type was truncated.
    fn report_truncation_error(&mut self);
    /// Reports a non-local augmentation of `augmenting_symbol` onto
    /// `parent_symbol` originating from `containing_file`.
    fn report_nonlocal_augmentation(
        &mut self,
        containing_file: NodeId,
        parent_symbol: SymbolId,
        augmenting_symbol: SymbolId,
    );
    /// Reports a property named `property_name` that cannot be serialized.
    fn report_non_serializable_property(&mut self, property_name: &str);
    /// Reports that inference fell back at `node`.
    fn report_inference_fallback(&mut self, node: NodeId);
    /// Pushes `node` as the current error-fallback node.
    fn push_error_fallback_node(&mut self, node: NodeId);
    /// Pops the most recently pushed error-fallback node.
    fn pop_error_fallback_node(&mut self);
}

bitflags::bitflags! {
    /// Controls how the node builder serializes a `Type`/`Symbol` back into AST
    /// type nodes (for `.d.ts` emit and hover display).
    ///
    /// Mirrors Go `nodebuilder.Flags` (a `uint32` flag set). The bit values are
    /// copied 1:1 from the Go literals, including the deliberately non-contiguous
    /// high bits (25, 27..=30) declared out of order in the Go source.
    ///
    /// NOTE: If modifying this enum, must modify `TypeFormatFlags` too!
    ///
    /// # Examples
    /// ```
    /// use tsgo_nodebuilder::Flags;
    /// assert_eq!(Flags::NO_TRUNCATION.bits(), 1 << 0);
    /// assert_eq!(Flags::USE_INSTANTIATION_EXPRESSIONS.bits(), 1 << 30);
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/nodebuilder/types.go:Flags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct Flags: u32 {
        /// No flags set.
        const NONE = 0;
        /// Do not truncate long type strings.
        const NO_TRUNCATION = 1 << 0;
        /// Write `Array<T>` instead of `T[]`.
        const WRITE_ARRAY_AS_GENERIC_TYPE = 1 << 1;
        /// Generate fresh names for shadowed type parameters.
        const GENERATE_NAMES_FOR_SHADOWED_TYPE_PARAMS = 1 << 2;
        /// Fall back to structural printing when nominal printing fails.
        const USE_STRUCTURAL_FALLBACK = 1 << 3;
        /// Forbid `obj[Symbol]`-style references in output.
        const FORBID_INDEXED_ACCESS_SYMBOL_REFERENCES = 1 << 4;
        /// Write type arguments of a signature.
        const WRITE_TYPE_ARGUMENTS_OF_SIGNATURE = 1 << 5;
        /// Use the fully qualified name of a type.
        const USE_FULLY_QUALIFIED_TYPE = 1 << 6;
        /// Only use external module aliasing.
        const USE_ONLY_EXTERNAL_ALIASING = 1 << 7;
        /// Suppress an inferred `any` return type.
        const SUPPRESS_ANY_RETURN_TYPE = 1 << 8;
        /// Write type parameters within a qualified name.
        const WRITE_TYPE_PARAMETERS_IN_QUALIFIED_NAME = 1 << 9;
        /// Emit object literals across multiple lines.
        const MULTILINE_OBJECT_LITERALS = 1 << 10;
        /// Write a class expression as a type literal.
        const WRITE_CLASS_EXPRESSION_AS_TYPE_LITERAL = 1 << 11;
        /// Use `typeof fn` form for functions.
        const USE_TYPE_OF_FUNCTION = 1 << 12;
        /// Omit parameter modifiers (`public`/`private`/...).
        const OMIT_PARAMETER_MODIFIERS = 1 << 13;
        /// Use an alias defined outside the current scope.
        const USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE = 1 << 14;
        /// Drop a synthesized `this` parameter.
        const OMIT_THIS_PARAMETER = 1 << 25;
        /// Write call signatures in call style (`(x): T`).
        const WRITE_CALL_STYLE_SIGNATURE = 1 << 27;
        /// Use single quotes for string-literal types.
        const USE_SINGLE_QUOTES_FOR_STRING_LITERAL_TYPE = 1 << 28;
        /// Skip type reduction (union/intersection normalization).
        const NO_TYPE_REDUCTION = 1 << 29;
        /// Allow instantiation expressions (`f<T>`) in output.
        const USE_INSTANTIATION_EXPRESSIONS = 1 << 30;
        /// Allow `node_modules`-relative import paths in output.
        const ALLOW_NODE_MODULES_RELATIVE_PATHS = 1 << 26;
        /// State: currently building inside an object type literal.
        const IN_OBJECT_TYPE_LITERAL = 1 << 22;
        /// State: currently building inside a type alias.
        const IN_TYPE_ALIAS = 1 << 23;
        /// State: currently building the initial entity name of a qualified name.
        const IN_INITIAL_ENTITY_NAME = 1 << 24;
        /// Error handling: tolerate `this` in an object literal.
        const ALLOW_THIS_IN_OBJECT_LITERAL = 1 << 15;
        /// Error handling: tolerate a qualified name where an identifier is expected.
        const ALLOW_QUALIFIED_NAME_IN_PLACE_OF_IDENTIFIER = 1 << 16;
        /// Error handling: tolerate an anonymous identifier.
        const ALLOW_ANONYMOUS_IDENTIFIER = 1 << 17;
        /// Error handling: tolerate an empty union or intersection.
        const ALLOW_EMPTY_UNION_OR_INTERSECTION = 1 << 18;
        /// Error handling: tolerate an empty tuple.
        const ALLOW_EMPTY_TUPLE = 1 << 19;
        /// Error handling: tolerate a `unique symbol` type.
        const ALLOW_UNIQUE_ES_SYMBOL_TYPE = 1 << 20;
        /// Error handling: tolerate an empty index-info type.
        const ALLOW_EMPTY_INDEX_INFO_TYPE = 1 << 21;
        /// Suppress error reporting by tolerating the otherwise-flagged shapes.
        ///
        /// Mirrors Go `FlagsIgnoreErrors`; note `ALLOW_UNIQUE_ES_SYMBOL_TYPE`
        /// is deliberately excluded.
        const IGNORE_ERRORS = (1 << 15)
            | (1 << 16)
            | (1 << 17)
            | (1 << 18)
            | (1 << 19)
            | (1 << 21)
            | (1 << 26);
    }
}

bitflags::bitflags! {
    /// Internal-only node builder flags (`@internal` in the TypeScript source).
    ///
    /// Mirrors Go `nodebuilder.InternalFlags`, whose underlying type is the
    /// signed `int32`; the Rust storage type is therefore `i32` to match 1:1.
    ///
    /// # Examples
    /// ```
    /// use tsgo_nodebuilder::InternalFlags;
    /// assert_eq!(InternalFlags::WRITE_COMPUTED_PROPS.bits(), 1 << 0);
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/nodebuilder/types.go:InternalFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct InternalFlags: i32 {
        /// No flags set.
        const NONE = 0;
        /// Write computed property names verbatim.
        const WRITE_COMPUTED_PROPS = 1 << 0;
        /// Disable the syntactic (fast-path) printer.
        const NO_SYNTACTIC_PRINTER = 1 << 1;
        /// Do not include the symbol chain when resolving names.
        const DO_NOT_INCLUDE_SYMBOL_CHAIN = 1 << 2;
        /// Allow unresolved names to be emitted as-is.
        const ALLOW_UNRESOLVED_NAMES = 1 << 3;
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
