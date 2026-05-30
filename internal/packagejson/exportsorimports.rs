//! `ExportsOrImports`: a [`JsonValue`](crate::JsonValue)-shaped value for the
//! `exports`/`imports` fields, plus lazy classification of an object as
//! *subpaths* (`.`/`./...` keys), *imports* (`#...` keys), or *conditions*.
//!
//! 1:1 port of Go `internal/packagejson/exportsorimports.go`.
//!
//! # Divergence from Go
//! Go's `IsSubpaths`/`IsImports`/`IsConditions` use a **value** receiver and
//! call `initObjectKind` (a pointer receiver) on that copy, so the cached
//! `objectKind` is written to a throwaway copy and recomputed on every call.
//! Here `object_kind` is a thread-safe [`OnceLock`] (the `Sync` analog of the
//! `Cell` suggested in `impl.md`), so the classification is computed once and
//! genuinely cached — behavior-equivalent (same result) but without the
//! redundant work, while keeping `ExportsOrImports` `Send + Sync` so it can sit
//! inside the concurrent [`InfoCache`](crate::InfoCache).

use std::sync::OnceLock;

use serde::de::{Deserialize, Deserializer};
use tsgo_collections::OrderedMap;

use crate::jsonvalue::{JsonBuilder, JsonValueVisitor};
use crate::JsonValueType;

// Go: internal/packagejson/exportsorimports.go:objectKind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ObjectKind {
    #[default]
    Unknown,
    Subpaths,
    Conditions,
    Imports,
    Invalid,
}

#[derive(Debug, Clone, Default)]
enum ExportsValue {
    #[default]
    NotPresent,
    Null,
    Str(String),
    Num(f64),
    Bool(bool),
    Array(Vec<ExportsOrImports>),
    Object(OrderedMap<String, ExportsOrImports>),
}

/// An `exports`/`imports` value: the same dynamic shape as
/// [`JsonValue`](crate::JsonValue), but objects can be classified as subpaths,
/// imports, or conditions.
///
/// Arrays and objects hold nested `ExportsOrImports`, so the classification is
/// available at every level (e.g. the value of a subpath key may itself be a
/// conditions object).
///
/// # Examples
/// ```
/// use tsgo_packagejson::ExportsOrImports;
/// let e: ExportsOrImports =
///     tsgo_json::unmarshal(br##"{ "#foo": { "import": "./foo.ts" } }"##).unwrap();
/// assert!(e.is_imports());
/// assert!(e.as_object().get(&"#foo".to_string()).unwrap().is_conditions());
/// ```
#[derive(Debug, Clone, Default)]
pub struct ExportsOrImports {
    value: ExportsValue,
    object_kind: OnceLock<ObjectKind>,
}

impl ExportsOrImports {
    fn wrap(value: ExportsValue) -> Self {
        ExportsOrImports {
            value,
            object_kind: OnceLock::new(),
        }
    }

    /// Returns the discriminant describing this value's current JSON shape.
    ///
    /// Side effects: none (pure).
    pub fn value_type(&self) -> JsonValueType {
        match &self.value {
            ExportsValue::NotPresent => JsonValueType::NotPresent,
            ExportsValue::Null => JsonValueType::Null,
            ExportsValue::Str(_) => JsonValueType::String,
            ExportsValue::Num(_) => JsonValueType::Number,
            ExportsValue::Bool(_) => JsonValueType::Boolean,
            ExportsValue::Array(_) => JsonValueType::Array,
            ExportsValue::Object(_) => JsonValueType::Object,
        }
    }

    /// Reports whether the field was present in the JSON document.
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/jsonvalue.go:IsPresent (promoted via embedding)
    pub fn is_present(&self) -> bool {
        !matches!(self.value, ExportsValue::NotPresent)
    }

    /// Reports whether the value is falsy in the JavaScript sense (absent,
    /// `null`, empty string, `0`, or `false`); arrays and objects are truthy.
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/jsonvalue.go:IsFalsy (promoted via embedding)
    pub fn is_falsy(&self) -> bool {
        match &self.value {
            ExportsValue::NotPresent | ExportsValue::Null => true,
            ExportsValue::Str(s) => s.is_empty(),
            ExportsValue::Num(n) => *n == 0.0,
            ExportsValue::Bool(b) => !*b,
            ExportsValue::Array(_) | ExportsValue::Object(_) => false,
        }
    }

    /// Returns the underlying object map.
    ///
    /// # Panics
    /// Panics if this value is not an object (matching Go's `AsObject`).
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/exportsorimports.go:AsObject
    pub fn as_object(&self) -> &OrderedMap<String, ExportsOrImports> {
        match &self.value {
            ExportsValue::Object(o) => o,
            _ => panic!("expected object, got {}", self.value_type()),
        }
    }

    /// Returns the underlying array slice.
    ///
    /// # Panics
    /// Panics if this value is not an array (matching Go's `AsArray`).
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/exportsorimports.go:AsArray
    pub fn as_array(&self) -> &[ExportsOrImports] {
        match &self.value {
            ExportsValue::Array(a) => a,
            _ => panic!("expected array, got {}", self.value_type()),
        }
    }

    /// Returns the underlying string.
    ///
    /// # Panics
    /// Panics if this value is not a string (matching Go's `AsString`).
    ///
    /// Side effects: none (pure).
    // Go: internal/packagejson/jsonvalue.go:AsString (promoted via embedding)
    pub fn as_str(&self) -> &str {
        match &self.value {
            ExportsValue::Str(s) => s,
            _ => panic!("expected string, got {}", self.value_type()),
        }
    }

    /// Reports whether this is a subpaths object (keys beginning with `.`).
    ///
    /// Side effects: caches the classification on first call (interior
    /// mutability via [`OnceLock`]); the observable result is unchanged.
    // Go: internal/packagejson/exportsorimports.go:IsSubpaths
    pub fn is_subpaths(&self) -> bool {
        self.resolved_object_kind() == ObjectKind::Subpaths
    }

    /// Reports whether this is an imports object (keys beginning with `#`).
    ///
    /// Side effects: caches the classification on first call (see
    /// [`ExportsOrImports::is_subpaths`]).
    // Go: internal/packagejson/exportsorimports.go:IsImports
    pub fn is_imports(&self) -> bool {
        self.resolved_object_kind() == ObjectKind::Imports
    }

    /// Reports whether this is a conditions object (keys are condition names).
    ///
    /// Side effects: caches the classification on first call (see
    /// [`ExportsOrImports::is_subpaths`]).
    // Go: internal/packagejson/exportsorimports.go:IsConditions
    pub fn is_conditions(&self) -> bool {
        self.resolved_object_kind() == ObjectKind::Conditions
    }

    fn resolved_object_kind(&self) -> ObjectKind {
        *self.object_kind.get_or_init(|| self.compute_object_kind())
    }

    // Go: internal/packagejson/exportsorimports.go:initObjectKind
    //
    // A non-object stays `Unknown` (Go leaves `objectKind` untouched); an empty
    // object and an all-condition object are both `Conditions`.
    fn compute_object_kind(&self) -> ObjectKind {
        let ExportsValue::Object(obj) = &self.value else {
            return ObjectKind::Unknown;
        };
        if obj.size() > 0 {
            let (mut seen_dot, mut seen_hash, mut seen_other) = (false, false, false);
            for key in obj.keys() {
                if let Some(&first) = key.as_bytes().first() {
                    seen_dot = seen_dot || first == b'.';
                    seen_hash = seen_hash || first == b'#';
                    seen_other = seen_other || (first != b'.' && first != b'#');
                    if seen_other && (seen_dot || seen_hash) {
                        return ObjectKind::Invalid;
                    }
                }
            }
            if seen_dot {
                return ObjectKind::Subpaths;
            }
            if seen_hash {
                return ObjectKind::Imports;
            }
        }
        ObjectKind::Conditions
    }
}

impl JsonBuilder for ExportsOrImports {
    fn null() -> Self {
        Self::wrap(ExportsValue::Null)
    }
    fn boolean(value: bool) -> Self {
        Self::wrap(ExportsValue::Bool(value))
    }
    fn number(value: f64) -> Self {
        Self::wrap(ExportsValue::Num(value))
    }
    fn string(value: String) -> Self {
        Self::wrap(ExportsValue::Str(value))
    }
    fn array(items: Vec<Self>) -> Self {
        Self::wrap(ExportsValue::Array(items))
    }
    fn object(map: OrderedMap<String, Self>) -> Self {
        Self::wrap(ExportsValue::Object(map))
    }
}

// Go: internal/packagejson/exportsorimports.go:UnmarshalJSONFrom
impl<'de> Deserialize<'de> for ExportsOrImports {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(JsonValueVisitor::<ExportsOrImports>::new())
    }
}

#[cfg(test)]
#[path = "exportsorimports_test.rs"]
mod tests;
