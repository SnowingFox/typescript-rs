//! `TypeValidatedField`: the common interface over a `package.json` field that
//! tracks its presence, validity, and expected vs actual JSON type.
//!
//! 1:1 port of Go `internal/packagejson/validated.go`.

use crate::{Expected, ExpectedJsonType};

/// A `package.json` field whose value type can be validated against the type
/// TypeScript expects.
///
/// Implemented by [`Expected<T>`](crate::Expected); it lets resolvers report a
/// uniform "expected `X`, got `Y`" diagnostic without knowing the field's
/// concrete value type.
///
/// # Examples
/// ```
/// use tsgo_packagejson::{expected_of, TypeValidatedField};
/// let field = expected_of("name".to_string());
/// fn describe(f: &dyn TypeValidatedField) -> (bool, &'static str) {
///     (f.is_valid(), f.expected_json_type())
/// }
/// assert_eq!(describe(&field), (true, "string"));
/// ```
// Go: internal/packagejson/validated.go:TypeValidatedField
pub trait TypeValidatedField {
    /// Reports whether the field was present in the JSON document.
    ///
    /// Side effects: none (pure).
    fn is_present(&self) -> bool;

    /// Reports whether the JSON value matched the expected type.
    ///
    /// Side effects: none (pure).
    fn is_valid(&self) -> bool;

    /// Returns the JSON type TypeScript expects for this field.
    ///
    /// Side effects: none (pure).
    fn expected_json_type(&self) -> &'static str;

    /// Returns the JSON type that actually appeared (empty when absent).
    ///
    /// Side effects: none (pure).
    fn actual_json_type(&self) -> &str;
}

impl<T: ExpectedJsonType> TypeValidatedField for Expected<T> {
    fn is_present(&self) -> bool {
        Expected::is_present(self)
    }

    fn is_valid(&self) -> bool {
        Expected::is_valid(self)
    }

    fn expected_json_type(&self) -> &'static str {
        Expected::expected_json_type(self)
    }

    fn actual_json_type(&self) -> &str {
        Expected::actual_json_type(self)
    }
}

#[cfg(test)]
#[path = "validated_test.rs"]
mod tests;
