//! `Expected<T>`: a `package.json` field wrapper that records both the value
//! and its *actual* JSON type, so resolvers can diagnose type mismatches even
//! when the JSON is the wrong shape (e.g. `"version": 2`).
//!
//! 1:1 port of Go `internal/packagejson/expected.go`.
//!
//! # Divergence from Go
//! Go derives the *expected* JSON type at runtime via `reflect.TypeFor[T]()`.
//! Rust has no reflection, so the expected type is provided statically by the
//! [`ExpectedJsonType`] trait (one impl per concrete `T`). The *actual* JSON
//! type is detected during deserialization, matching Go's first-byte switch.

use std::collections::HashMap;

use serde::de::{Deserialize, DeserializeOwned, Deserializer};

/// Maps a concrete value type to the JSON type string TypeScript expects for
/// it (e.g. `String -> "string"`, `HashMap -> "object"`).
///
/// This replaces Go's reflection-based `ExpectedJSONType`; each supported field
/// value type provides one impl.
///
/// # Examples
/// ```
/// use tsgo_packagejson::ExpectedJsonType;
/// assert_eq!(String::expected_json_type(), "string");
/// ```
pub trait ExpectedJsonType {
    /// Returns the JSON type name TypeScript expects for this value type.
    ///
    /// Side effects: none (pure).
    fn expected_json_type() -> &'static str;
}

impl ExpectedJsonType for String {
    fn expected_json_type() -> &'static str {
        "string"
    }
}

impl ExpectedJsonType for bool {
    fn expected_json_type() -> &'static str {
        "boolean"
    }
}

impl ExpectedJsonType for HashMap<String, String> {
    fn expected_json_type() -> &'static str {
        "object"
    }
}

/// A `package.json` field value plus the JSON type that actually appeared.
///
/// Even when the JSON shape is wrong for `T` (so [`Expected::is_valid`] is
/// `false` and the value falls back to `T::default()`), the original JSON type
/// is retained via [`Expected::actual_json_type`] for diagnostics.
///
/// # Examples
/// ```
/// use tsgo_packagejson::Expected;
/// // `"version": 2` decoded as a string: type mismatch, value defaults.
/// let e: Expected<String> = tsgo_json::unmarshal(b"2").unwrap();
/// assert!(!e.is_valid());
/// assert_eq!(e.actual_json_type(), "number");
/// assert_eq!(e.get_value().0, "");
/// ```
#[derive(Debug, Clone, Default)]
pub struct Expected<T> {
    actual_json_type: String,
    null: bool,
    valid: bool,
    value: T,
}

impl<T> Expected<T> {
    /// Reports whether the field was present in the JSON document.
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/expected.go:IsPresent
    pub fn is_present(&self) -> bool {
        !self.actual_json_type.is_empty()
    }

    /// Returns the value together with whether it was valid for `T`.
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/expected.go:GetValue
    pub fn get_value(&self) -> (&T, bool) {
        (&self.value, self.valid)
    }

    /// Reports whether the JSON value matched the expected type for `T`.
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/expected.go:IsValid
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// Reports whether the field was present and explicitly `null`.
    ///
    /// Side effects: none (pure).
    pub fn is_null(&self) -> bool {
        self.null
    }

    /// Returns the JSON type that actually appeared (empty when absent).
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/expected.go:ActualJSONType
    pub fn actual_json_type(&self) -> &str {
        &self.actual_json_type
    }
}

impl<T: ExpectedJsonType> Expected<T> {
    /// Returns the JSON type TypeScript expects for this field (independent of
    /// what actually appeared).
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/expected.go:ExpectedJSONType
    pub fn expected_json_type(&self) -> &'static str {
        T::expected_json_type()
    }
}

/// Builds a valid [`Expected`] from a known-good value, tagging it with `T`'s
/// expected JSON type.
///
/// # Examples
/// ```
/// use tsgo_packagejson::expected_of;
/// let e = expected_of("1.0.0".to_string());
/// assert!(e.is_valid());
/// assert_eq!(e.get_value().0, "1.0.0");
/// ```
///
/// Side effects: none (pure).
// Go: internal/packagejson/expected.go:ExpectedOf
pub fn expected_of<T: ExpectedJsonType>(value: T) -> Expected<T> {
    Expected {
        actual_json_type: T::expected_json_type().to_string(),
        null: false,
        valid: true,
        value,
    }
}

// Go: internal/packagejson/expected.go:UnmarshalJSON
//
// Detects the actual JSON type, then attempts to decode the value as `T`;
// success sets `valid`, failure keeps the default value (mirroring Go's
// `json.Unmarshal(data, &e.Value) == nil` check).
impl<'de, T: DeserializeOwned + Default> Deserialize<'de> for Expected<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.is_null() {
            return Ok(Expected {
                actual_json_type: "null".to_string(),
                null: true,
                valid: false,
                value: T::default(),
            });
        }
        let actual_json_type = match &value {
            serde_json::Value::String(_) => "string",
            serde_json::Value::Bool(_) => "boolean",
            serde_json::Value::Array(_) => "array",
            serde_json::Value::Object(_) => "object",
            // Numbers (and the already-handled null) fall through to "number".
            _ => "number",
        }
        .to_string();
        let (valid, value) = match serde_json::from_value::<T>(value) {
            Ok(v) => (true, v),
            Err(_) => (false, T::default()),
        };
        Ok(Expected {
            actual_json_type,
            null: false,
            valid,
            value,
        })
    }
}

#[cfg(test)]
#[path = "expected_test.rs"]
mod tests;
