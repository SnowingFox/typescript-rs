//! `GeneratedIdentifierFlags`: the kind (low 3 bits) and flags of an
//! auto-generated identifier.

use std::ops::{BitAnd, BitOr};

/// Describes how an auto-generated identifier was produced.
///
/// The low three bits hold the *kind* (`AUTO`/`LOOP`/`UNIQUE`/`NODE`), while the
/// higher bits are independent flags. This mixed layout mirrors Go's
/// `GeneratedIdentifierFlags` exactly, so it is modeled as a small integer
/// newtype rather than `bitflags`.
///
/// # Examples
/// ```
/// use tsgo_printer::generatedidentifierflags::GeneratedIdentifierFlags;
/// let f = GeneratedIdentifierFlags::AUTO | GeneratedIdentifierFlags::RESERVED_IN_NESTED_SCOPES;
/// assert!(f.is_auto());
/// assert!(f.is_reserved_in_nested_scopes());
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct GeneratedIdentifierFlags(i32);

impl GeneratedIdentifierFlags {
    /// Not automatically generated.
    pub const NONE: GeneratedIdentifierFlags = GeneratedIdentifierFlags(0);
    /// Automatically generated identifier.
    pub const AUTO: GeneratedIdentifierFlags = GeneratedIdentifierFlags(1);
    /// Automatically generated identifier with a preference for `_i`.
    pub const LOOP: GeneratedIdentifierFlags = GeneratedIdentifierFlags(2);
    /// Unique name based on the `text` property.
    pub const UNIQUE: GeneratedIdentifierFlags = GeneratedIdentifierFlags(3);
    /// Unique name based on the node in the `Node` property.
    pub const NODE: GeneratedIdentifierFlags = GeneratedIdentifierFlags(4);
    /// Mask to extract the kind of identifier from its flags.
    pub const KIND_MASK: GeneratedIdentifierFlags = GeneratedIdentifierFlags(7);

    /// Reserve the generated name in nested scopes.
    pub const RESERVED_IN_NESTED_SCOPES: GeneratedIdentifierFlags =
        GeneratedIdentifierFlags(1 << 3);
    /// First instance won't use `_#` if there is no conflict.
    pub const OPTIMISTIC: GeneratedIdentifierFlags = GeneratedIdentifierFlags(1 << 4);
    /// Use only the file identifiers list and not generated names to search for conflicts.
    pub const FILE_LEVEL: GeneratedIdentifierFlags = GeneratedIdentifierFlags(1 << 5);
    /// Marks generated nodes which can have substitutions performed upon them.
    pub const ALLOW_NAME_SUBSTITUTION: GeneratedIdentifierFlags = GeneratedIdentifierFlags(1 << 6);

    /// Returns the raw integer bits.
    ///
    /// Side effects: none (pure).
    pub fn bits(self) -> i32 {
        self.0
    }

    /// Builds a value from raw integer bits.
    ///
    /// Side effects: none (pure).
    pub fn from_bits(bits: i32) -> GeneratedIdentifierFlags {
        GeneratedIdentifierFlags(bits)
    }

    /// Returns just the kind portion (the low three bits).
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.Kind
    pub fn kind(self) -> GeneratedIdentifierFlags {
        self & GeneratedIdentifierFlags::KIND_MASK
    }

    /// Reports whether the kind is `AUTO`.
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.IsAuto
    pub fn is_auto(self) -> bool {
        self.kind() == GeneratedIdentifierFlags::AUTO
    }

    /// Reports whether the kind is `LOOP`.
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.IsLoop
    pub fn is_loop(self) -> bool {
        self.kind() == GeneratedIdentifierFlags::LOOP
    }

    /// Reports whether the kind is `UNIQUE`.
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.IsUnique
    pub fn is_unique(self) -> bool {
        self.kind() == GeneratedIdentifierFlags::UNIQUE
    }

    /// Reports whether the kind is `NODE`.
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.IsNode
    pub fn is_node(self) -> bool {
        self.kind() == GeneratedIdentifierFlags::NODE
    }

    /// Reports whether the `RESERVED_IN_NESTED_SCOPES` flag is set.
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.IsReservedInNestedScopes
    pub fn is_reserved_in_nested_scopes(self) -> bool {
        (self & GeneratedIdentifierFlags::RESERVED_IN_NESTED_SCOPES)
            != GeneratedIdentifierFlags::NONE
    }

    /// Reports whether the `OPTIMISTIC` flag is set.
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.IsOptimistic
    pub fn is_optimistic(self) -> bool {
        (self & GeneratedIdentifierFlags::OPTIMISTIC) != GeneratedIdentifierFlags::NONE
    }

    /// Reports whether the `FILE_LEVEL` flag is set.
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.IsFileLevel
    pub fn is_file_level(self) -> bool {
        (self & GeneratedIdentifierFlags::FILE_LEVEL) != GeneratedIdentifierFlags::NONE
    }

    /// Reports whether the `ALLOW_NAME_SUBSTITUTION` flag is set.
    ///
    /// Side effects: none (pure).
    // Go: internal/printer/generatedidentifierflags.go:GeneratedIdentifierFlags.HasAllowNameSubstitution
    pub fn has_allow_name_substitution(self) -> bool {
        (self & GeneratedIdentifierFlags::ALLOW_NAME_SUBSTITUTION) != GeneratedIdentifierFlags::NONE
    }
}

impl BitOr for GeneratedIdentifierFlags {
    type Output = GeneratedIdentifierFlags;
    fn bitor(self, rhs: GeneratedIdentifierFlags) -> GeneratedIdentifierFlags {
        GeneratedIdentifierFlags(self.0 | rhs.0)
    }
}

impl BitAnd for GeneratedIdentifierFlags {
    type Output = GeneratedIdentifierFlags;
    fn bitand(self, rhs: GeneratedIdentifierFlags) -> GeneratedIdentifierFlags {
        GeneratedIdentifierFlags(self.0 & rhs.0)
    }
}

#[cfg(test)]
#[path = "generatedidentifierflags_test.rs"]
mod tests;
