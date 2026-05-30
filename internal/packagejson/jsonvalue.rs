//! Dynamic JSON value that preserves the original JSON shape, used for
//! `package.json` fields whose type is not statically known
//! (`typesVersions`, and the building block for `exports`/`imports`).
//!
//! 1:1 port of Go `internal/packagejson/jsonvalue.go`.
//!
//! # Divergence from Go
//! Go models the value as `struct JSONValue { Type JSONValueType; Value any }`.
//! Here it is a discriminated [`JsonValue`] enum (per PORTING.md §3), which
//! removes the `any` downcasts; the Go `JSONValueType` survives as the
//! [`JsonValueType`] tag returned by [`JsonValue::value_type`].

use std::fmt;
use std::marker::PhantomData;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use tsgo_collections::OrderedMap;

/// The JSON shape a [`JsonValue`] currently holds.
///
/// Mirrors Go's `JSONValueType` `iota` constants, including the numeric
/// ordering used by [`fmt::Display`] for the `unknown(n)` fallback.
///
/// # Examples
/// ```
/// use tsgo_packagejson::JsonValueType;
/// assert_eq!(JsonValueType::Number.to_string(), "number");
/// ```
#[repr(i8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonValueType {
    /// The field was absent from the JSON document.
    NotPresent = 0,
    /// A JSON `null`.
    Null = 1,
    /// A JSON string.
    String = 2,
    /// A JSON number (always stored as `f64`).
    Number = 3,
    /// A JSON boolean.
    Boolean = 4,
    /// A JSON array.
    Array = 5,
    /// A JSON object.
    Object = 6,
}

impl fmt::Display for JsonValueType {
    // Go: internal/packagejson/jsonvalue.go:JSONValueType.String
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonValueType::Null => f.write_str("null"),
            JsonValueType::String => f.write_str("string"),
            JsonValueType::Number => f.write_str("number"),
            JsonValueType::Boolean => f.write_str("boolean"),
            JsonValueType::Array => f.write_str("array"),
            JsonValueType::Object => f.write_str("object"),
            other => write!(f, "unknown({})", *other as i8),
        }
    }
}

/// A dynamically-typed JSON value that retains its original JSON shape.
///
/// Objects use an [`OrderedMap`] so insertion order (which drives diagnostic
/// and resolution order) is preserved. Numbers are always stored as `f64`,
/// matching Go's decode of JSON numbers into `float64`.
///
/// # Examples
/// ```
/// use tsgo_packagejson::{JsonValue, JsonValueType};
/// let v: JsonValue = tsgo_json::unmarshal(b"[1, \"x\", null]").unwrap();
/// assert_eq!(v.value_type(), JsonValueType::Array);
/// assert_eq!(v.as_array().len(), 3);
/// ```
#[derive(Debug, Clone, Default)]
pub enum JsonValue {
    /// The field was absent from the JSON document (Go zero value).
    #[default]
    NotPresent,
    /// A JSON `null`.
    Null,
    /// A JSON string.
    Str(String),
    /// A JSON number (stored as `f64`).
    Num(f64),
    /// A JSON boolean.
    Bool(bool),
    /// A JSON array of [`JsonValue`] elements.
    Array(Vec<JsonValue>),
    /// A JSON object keyed by insertion order.
    Object(OrderedMap<String, JsonValue>),
}

impl JsonValue {
    /// Returns the discriminant describing this value's current shape.
    ///
    /// Side effects: none (pure).
    pub fn value_type(&self) -> JsonValueType {
        match self {
            JsonValue::NotPresent => JsonValueType::NotPresent,
            JsonValue::Null => JsonValueType::Null,
            JsonValue::Str(_) => JsonValueType::String,
            JsonValue::Num(_) => JsonValueType::Number,
            JsonValue::Bool(_) => JsonValueType::Boolean,
            JsonValue::Array(_) => JsonValueType::Array,
            JsonValue::Object(_) => JsonValueType::Object,
        }
    }

    /// Reports whether the field was present in the JSON document.
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/jsonvalue.go:IsPresent
    pub fn is_present(&self) -> bool {
        !matches!(self, JsonValue::NotPresent)
    }

    /// Reports whether the value is falsy in the JavaScript sense (absent,
    /// `null`, empty string, `0`, or `false`); arrays and objects are truthy.
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/jsonvalue.go:IsFalsy
    pub fn is_falsy(&self) -> bool {
        match self {
            JsonValue::NotPresent | JsonValue::Null => true,
            JsonValue::Str(s) => s.is_empty(),
            JsonValue::Num(n) => *n == 0.0,
            JsonValue::Bool(b) => !*b,
            JsonValue::Array(_) | JsonValue::Object(_) => false,
        }
    }

    /// Returns the underlying object map.
    ///
    /// # Panics
    /// Panics if this value is not an object (matching Go's `AsObject`).
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/jsonvalue.go:AsObject
    pub fn as_object(&self) -> &OrderedMap<String, JsonValue> {
        match self {
            JsonValue::Object(o) => o,
            other => panic!("expected object, got {}", other.value_type()),
        }
    }

    /// Returns the underlying array slice.
    ///
    /// # Panics
    /// Panics if this value is not an array (matching Go's `AsArray`).
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/jsonvalue.go:AsArray
    pub fn as_array(&self) -> &[JsonValue] {
        match self {
            JsonValue::Array(a) => a,
            other => panic!("expected array, got {}", other.value_type()),
        }
    }

    /// Returns the underlying string.
    ///
    /// # Panics
    /// Panics if this value is not a string (matching Go's `AsString`).
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/jsonvalue.go:AsString
    pub fn as_str(&self) -> &str {
        match self {
            JsonValue::Str(s) => s,
            other => panic!("expected string, got {}", other.value_type()),
        }
    }
}

/// Constructs a dynamic JSON value (either [`JsonValue`] or
/// `ExportsOrImports`) from each decoded JSON token.
///
/// This mirrors Go's single generic `unmarshalJSONValueV2[T]`: the element type
/// of arrays/objects is the implementing type itself, so nested values keep the
/// same wrapper (and `ExportsOrImports` keeps its lazy `objectKind`).
pub(crate) trait JsonBuilder: Sized {
    /// Builds the value for a JSON `null`.
    fn null() -> Self;
    /// Builds the value for a JSON boolean.
    fn boolean(value: bool) -> Self;
    /// Builds the value for a JSON number (always stored as `f64`).
    fn number(value: f64) -> Self;
    /// Builds the value for a JSON string.
    fn string(value: String) -> Self;
    /// Builds the value for a JSON array.
    fn array(items: Vec<Self>) -> Self;
    /// Builds the value for a JSON object (insertion-ordered).
    fn object(map: OrderedMap<String, Self>) -> Self;
}

pub(crate) struct JsonValueVisitor<T>(PhantomData<fn() -> T>);

impl<T> JsonValueVisitor<T> {
    pub(crate) fn new() -> Self {
        JsonValueVisitor(PhantomData)
    }
}

impl<'de, T> Visitor<'de> for JsonValueVisitor<T>
where
    T: JsonBuilder + Deserialize<'de>,
{
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<T, E> {
        Ok(T::boolean(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<T, E> {
        Ok(T::number(v as f64))
    }

    fn visit_u64<E>(self, v: u64) -> Result<T, E> {
        Ok(T::number(v as f64))
    }

    fn visit_f64<E>(self, v: f64) -> Result<T, E> {
        Ok(T::number(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<T, E> {
        Ok(T::string(v.to_string()))
    }

    fn visit_string<E>(self, v: String) -> Result<T, E> {
        Ok(T::string(v))
    }

    fn visit_unit<E>(self) -> Result<T, E> {
        Ok(T::null())
    }

    fn visit_none<E>(self) -> Result<T, E> {
        Ok(T::null())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<T, A::Error> {
        let mut elements = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(element) = seq.next_element::<T>()? {
            elements.push(element);
        }
        Ok(T::array(elements))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<T, A::Error> {
        let mut object = OrderedMap::with_size_hint(map.size_hint().unwrap_or(0));
        while let Some((key, value)) = map.next_entry::<String, T>()? {
            object.set(key, value);
        }
        Ok(T::object(object))
    }
}

impl JsonBuilder for JsonValue {
    fn null() -> Self {
        JsonValue::Null
    }
    fn boolean(value: bool) -> Self {
        JsonValue::Bool(value)
    }
    fn number(value: f64) -> Self {
        JsonValue::Num(value)
    }
    fn string(value: String) -> Self {
        JsonValue::Str(value)
    }
    fn array(items: Vec<Self>) -> Self {
        JsonValue::Array(items)
    }
    fn object(map: OrderedMap<String, Self>) -> Self {
        JsonValue::Object(map)
    }
}

// Go: internal/packagejson/jsonvalue.go:unmarshalJSONValueV2
impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(JsonValueVisitor::<JsonValue>::new())
    }
}

#[cfg(test)]
#[path = "jsonvalue_test.rs"]
mod tests;
