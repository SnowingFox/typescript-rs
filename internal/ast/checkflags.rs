//! `CheckFlags` bit set for transient symbols created during type checking.

bitflags::bitflags! {
    /// Checker-time flags on transient symbols (synthetic properties, mapped
    /// type members, late-bound names, ...).
    ///
    /// Mirrors Go `CheckFlags` (a `uint32` `iota` enum).
    ///
    /// # Examples
    /// ```
    /// use tsgo_ast::checkflags::CheckFlags;
    /// assert_eq!(
    ///     CheckFlags::SYNTHETIC,
    ///     CheckFlags::SYNTHETIC_PROPERTY | CheckFlags::SYNTHETIC_METHOD
    /// );
    /// ```
    ///
    /// Side effects: none (pure value type).
    // Go: internal/ast/checkflags.go:CheckFlags
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct CheckFlags: u32 {
        /// No flags.
        const NONE = 0;
        /// Instantiated symbol.
        const INSTANTIATED = 1 << 0;
        /// Property in a union or intersection type.
        const SYNTHETIC_PROPERTY = 1 << 1;
        /// Method in a union or intersection type.
        const SYNTHETIC_METHOD = 1 << 2;
        /// Readonly transient symbol.
        const READONLY = 1 << 3;
        /// Synthetic property present in some but not all constituents.
        const READ_PARTIAL = 1 << 4;
        /// Synthetic property satisfied only by an index signature in others.
        const WRITE_PARTIAL = 1 << 5;
        /// Synthetic property with non-uniform type across constituents.
        const HAS_NON_UNIFORM_TYPE = 1 << 6;
        /// Synthetic property with at least one literal type.
        const HAS_LITERAL_TYPE = 1 << 7;
        /// Synthetic property with public constituent(s).
        const CONTAINS_PUBLIC = 1 << 8;
        /// Synthetic property with protected constituent(s).
        const CONTAINS_PROTECTED = 1 << 9;
        /// Synthetic property with private constituent(s).
        const CONTAINS_PRIVATE = 1 << 10;
        /// Synthetic property with static constituent(s).
        const CONTAINS_STATIC = 1 << 11;
        /// Late-bound symbol for a computed property with a dynamic name.
        const LATE = 1 << 12;
        /// Property of a reverse-inferred homomorphic mapped type.
        const REVERSE_MAPPED = 1 << 13;
        /// Optional parameter.
        const OPTIONAL_PARAMETER = 1 << 14;
        /// Rest parameter.
        const REST_PARAMETER = 1 << 15;
        /// Type calculation is deferred.
        const DEFERRED_TYPE = 1 << 16;
        /// Synthetic property with at least one `never` type.
        const HAS_NEVER_TYPE = 1 << 17;
        /// Property of a mapped type.
        const MAPPED = 1 << 18;
        /// Strip optionality in a mapped property.
        const STRIP_OPTIONAL = 1 << 19;
        /// Unresolved type alias symbol.
        const UNRESOLVED = 1 << 20;
        /// `IS_DISCRIMINANT` has been computed.
        const IS_DISCRIMINANT_COMPUTED = 1 << 21;
        /// Discriminant property.
        const IS_DISCRIMINANT = 1 << 22;
        /// Synthetic property created from an index signature.
        const INDEX_SYMBOL = 1 << 23;

        /// Any synthetic union/intersection member.
        const SYNTHETIC = Self::SYNTHETIC_PROPERTY.bits() | Self::SYNTHETIC_METHOD.bits();
        /// Non-uniform-or-literal mask.
        const NON_UNIFORM_AND_LITERAL = Self::HAS_NON_UNIFORM_TYPE.bits() | Self::HAS_LITERAL_TYPE.bits();
        /// Either partial flavor.
        const PARTIAL = Self::READ_PARTIAL.bits() | Self::WRITE_PARTIAL.bits();
    }
}

#[cfg(test)]
#[path = "checkflags_test.rs"]
mod tests;
