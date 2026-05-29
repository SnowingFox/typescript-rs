//! Three-state boolean (`Tristate`) with JSON and `Display`.
//!
//! 1:1 port of Go `internal/core/tristate.go` and
//! `tristate_stringer_generated.go`.

use serde::de::{Deserialize, Deserializer, Visitor};
use serde::ser::{Serialize, Serializer};

/// A three-state boolean: unknown (default), false, or true.
///
/// Serializes as `null` (unknown), `false`, or `true`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Tristate {
    /// Unset / default state.
    #[default]
    Unknown = 0,
    /// Explicit false.
    False = 1,
    /// Explicit true.
    True = 2,
}

impl Tristate {
    /// Reports whether this is [`Tristate::True`].
    ///
    /// Side effects: none (pure).
    // Go: internal/core/tristate.go:IsTrue
    pub fn is_true(self) -> bool {
        self == Tristate::True
    }

    /// Reports whether this is true or unknown.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/tristate.go:IsTrueOrUnknown
    pub fn is_true_or_unknown(self) -> bool {
        self == Tristate::True || self == Tristate::Unknown
    }

    /// Reports whether this is [`Tristate::False`].
    ///
    /// Side effects: none (pure).
    // Go: internal/core/tristate.go:IsFalse
    pub fn is_false(self) -> bool {
        self == Tristate::False
    }

    /// Reports whether this is false or unknown.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/tristate.go:IsFalseOrUnknown
    pub fn is_false_or_unknown(self) -> bool {
        self == Tristate::False || self == Tristate::Unknown
    }

    /// Reports whether this is [`Tristate::Unknown`].
    ///
    /// Side effects: none (pure).
    // Go: internal/core/tristate.go:IsUnknown
    pub fn is_unknown(self) -> bool {
        self == Tristate::Unknown
    }

    /// Returns `value` if this is unknown; otherwise returns `self`.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/tristate.go:DefaultIfUnknown
    pub fn default_if_unknown(self, value: Tristate) -> Tristate {
        if self == Tristate::Unknown {
            value
        } else {
            self
        }
    }
}

/// Converts a bool to a [`Tristate`] (`true` -> True, `false` -> False).
///
/// Side effects: none (pure).
// Go: internal/core/tristate.go:BoolToTristate
pub fn bool_to_tristate(b: bool) -> Tristate {
    if b {
        Tristate::True
    } else {
        Tristate::False
    }
}

// Go: internal/core/tristate_stringer_generated.go:String
impl std::fmt::Display for Tristate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Tristate::Unknown => "TSUnknown",
            Tristate::False => "TSFalse",
            Tristate::True => "TSTrue",
        };
        f.write_str(s)
    }
}

// Go: internal/core/tristate.go:MarshalJSON
impl Serialize for Tristate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Tristate::True => serializer.serialize_bool(true),
            Tristate::False => serializer.serialize_bool(false),
            Tristate::Unknown => serializer.serialize_none(),
        }
    }
}

struct TristateVisitor;

impl<'de> Visitor<'de> for TristateVisitor {
    type Value = Tristate;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a boolean or null")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Tristate, E> {
        Ok(if v { Tristate::True } else { Tristate::False })
    }

    fn visit_unit<E>(self) -> Result<Tristate, E> {
        Ok(Tristate::Unknown)
    }

    fn visit_none<E>(self) -> Result<Tristate, E> {
        Ok(Tristate::Unknown)
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Tristate, D::Error> {
        d.deserialize_any(TristateVisitor)
    }

    // Any other JSON value maps to Unknown (mirrors Go's default branch).
    fn visit_str<E>(self, _v: &str) -> Result<Tristate, E> {
        Ok(Tristate::Unknown)
    }

    fn visit_i64<E>(self, _v: i64) -> Result<Tristate, E> {
        Ok(Tristate::Unknown)
    }

    fn visit_u64<E>(self, _v: u64) -> Result<Tristate, E> {
        Ok(Tristate::Unknown)
    }

    fn visit_f64<E>(self, _v: f64) -> Result<Tristate, E> {
        Ok(Tristate::Unknown)
    }
}

// Go: internal/core/tristate.go:UnmarshalJSON
impl<'de> Deserialize<'de> for Tristate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(TristateVisitor)
    }
}

#[cfg(test)]
#[path = "tristate_test.rs"]
mod tests;
