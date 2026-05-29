//! Type acquisition options (`TypeAcquisition`).
//!
//! 1:1 port of Go `internal/core/typeacquisition.go`.

use crate::tristate::Tristate;

/// Options controlling automatic type acquisition.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypeAcquisition {
    /// Whether type acquisition is enabled.
    pub enable: Tristate,
    /// Packages to include.
    pub include: Vec<String>,
    /// Packages to exclude.
    pub exclude: Vec<String>,
    /// Whether filename-based type acquisition is disabled.
    pub disable_filename_based_type_acquisition: Tristate,
}

impl TypeAcquisition {
    /// Reports whether this equals `other` field-by-field.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/typeacquisition.go:Equals
    pub fn equals(&self, other: &TypeAcquisition) -> bool {
        self == other
    }
}

#[cfg(test)]
#[path = "typeacquisition_test.rs"]
mod tests;
