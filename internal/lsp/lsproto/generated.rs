//! Port of Go `internal/lsp/lsproto/lsp_generated.go` (the LSP meta-model
//! generated types).

use std::borrow::Cow;
use std::fmt;

use serde::de::{self, DeserializeOwned, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

// `ClientCapabilities` is the pointer-based request tree defined in
// `capabilities.rs`; `InitializeParams` embeds it. `CodeActionKind` and
// `BooleanOrEmptyObject` live in `resolved.rs` and are reused by the
// `ServerCapabilities` code-action and semantic-tokens options.
use crate::{
    BooleanOrEmptyObject, ClientCapabilities, CodeActionKind, CompletionItemTag, InsertTextMode,
    MarkupKind, SymbolTag,
};

/// Builds the Go `errMissing` error: `missing required properties: a, b`.
// Go: internal/lsp/lsproto/lsp.go:errMissing
fn err_missing<E: de::Error>(props: &[&str]) -> E {
    de::Error::custom(format!("missing required properties: {}", props.join(", ")))
}

/// Reads the current map value, rejecting JSON `null` for a non-nullable field
/// with Go's `errNull` text, then decodes it as `T`.
///
/// Mirrors the generated `if dec.PeekKind() == 'n' { return errNull(field) }`
/// guard emitted for every pointer/slice field.
// Go: internal/lsp/lsproto/lsp.go:errNull
fn next_reject_null<'de, A, T>(map: &mut A, field: &str) -> Result<T, A::Error>
where
    A: MapAccess<'de>,
    T: DeserializeOwned,
{
    let v = map.next_value::<serde_json::Value>()?;
    if v.is_null() {
        return Err(de::Error::custom(format!(
            "null value is not allowed for field \"{field}\""
        )));
    }
    serde_json::from_value(v).map_err(de::Error::custom)
}

/// Generates an LSP object type with hand-written `serde` impls that reproduce
/// Go's generated `UnmarshalJSONFrom`/`MarshalJSONTo` behavior:
/// reject `null` for non-nullable fields, report `missing required properties`
/// in declaration order, ignore unknown fields, and omit absent optionals.
///
/// Field kinds:
/// - `req`   — required, value type, no null guard (e.g. a nested struct/union).
/// - `reqnn` — required, pointer/slice type that rejects an explicit `null`.
/// - `opt`   — optional pointer/slice that rejects an explicit `null`.
/// - `optn`  — optional and nullable: `null` is accepted (Go emits no guard).
macro_rules! lsp_object {
    (
        $(#[$smeta:meta])*
        $name:ident {
            $( [$doc:literal] $kind:ident $rust:ident : $ty:ty => $json:literal , )*
        }
    ) => {
        $(#[$smeta])*
        #[derive(Debug, Clone, Default, PartialEq)]
        pub struct $name {
            $( #[doc = $doc] pub $rust : lsp_object!(@fty $kind $ty), )*
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut __m = serializer.serialize_map(None)?;
                $( lsp_object!(@ser self __m $kind $rust $json); )*
                __m.end()
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct __V;
                impl<'de> Visitor<'de> for __V {
                    type Value = $name;
                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str(concat!("a ", stringify!($name), " object"))
                    }
                    fn visit_map<A: MapAccess<'de>>(
                        self,
                        mut map: A,
                    ) -> Result<Self::Value, A::Error> {
                        $( let mut $rust : Option<$ty> = None; )*
                        while let Some(__k) = map.next_key::<String>()? {
                            match __k.as_str() {
                                $( $json => { lsp_object!(@de map $kind $rust $json); } )*
                                _ => {
                                    map.next_value::<de::IgnoredAny>()?;
                                }
                            }
                        }
                        let mut __missing: Vec<&str> = Vec::new();
                        $( lsp_object!(@miss __missing $kind $rust $json); )*
                        if !__missing.is_empty() {
                            return Err(err_missing(&__missing));
                        }
                        Ok($name { $( $rust: lsp_object!(@cons $kind $rust), )* })
                    }
                }
                deserializer.deserialize_map(__V)
            }
        }
    };

    (@fty req $ty:ty) => { $ty };
    (@fty reqnn $ty:ty) => { $ty };
    (@fty opt $ty:ty) => { ::core::option::Option<$ty> };
    (@fty optn $ty:ty) => { ::core::option::Option<$ty> };

    (@de $map:ident req $rust:ident $json:literal) => {
        $rust = Some($map.next_value()?);
    };
    (@de $map:ident reqnn $rust:ident $json:literal) => {
        $rust = Some(next_reject_null(&mut $map, $json)?);
    };
    (@de $map:ident opt $rust:ident $json:literal) => {
        $rust = Some(next_reject_null(&mut $map, $json)?);
    };
    (@de $map:ident optn $rust:ident $json:literal) => {
        $rust = Some($map.next_value()?);
    };

    (@miss $m:ident req $rust:ident $json:literal) => {
        if $rust.is_none() { $m.push($json); }
    };
    (@miss $m:ident reqnn $rust:ident $json:literal) => {
        if $rust.is_none() { $m.push($json); }
    };
    (@miss $m:ident opt $rust:ident $json:literal) => {};
    (@miss $m:ident optn $rust:ident $json:literal) => {};

    (@cons req $rust:ident) => { $rust.unwrap() };
    (@cons reqnn $rust:ident) => { $rust.unwrap() };
    (@cons opt $rust:ident) => { $rust };
    (@cons optn $rust:ident) => { $rust };

    (@ser $self:ident $m:ident req $rust:ident $json:literal) => {
        $m.serialize_entry($json, &$self.$rust)?;
    };
    (@ser $self:ident $m:ident reqnn $rust:ident $json:literal) => {
        $m.serialize_entry($json, &$self.$rust)?;
    };
    (@ser $self:ident $m:ident opt $rust:ident $json:literal) => {
        if let Some(ref __v) = $self.$rust { $m.serialize_entry($json, __v)?; }
    };
    (@ser $self:ident $m:ident optn $rust:ident $json:literal) => {
        if let Some(ref __v) = $self.$rust { $m.serialize_entry($json, __v)?; }
    };
}

/// Generates an "open object" type: any JSON object is accepted (all fields
/// ignored) and it serializes back to `{}`. Used for capability/options bags
/// whose full field set is deferred to the generator pass.
macro_rules! lsp_open_object {
    ($(#[$smeta:meta])* $name:ident) => {
        $(#[$smeta])*
        #[derive(Debug, Clone, Default, PartialEq)]
        pub struct $name;

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_map(Some(0))?.end()
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct __V;
                impl<'de> Visitor<'de> for __V {
                    type Value = $name;
                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str(concat!("a ", stringify!($name), " object"))
                    }
                    fn visit_map<A: MapAccess<'de>>(
                        self,
                        mut map: A,
                    ) -> Result<Self::Value, A::Error> {
                        while map.next_key::<de::IgnoredAny>()?.is_some() {
                            map.next_value::<de::IgnoredAny>()?;
                        }
                        Ok($name)
                    }
                }
                deserializer.deserialize_map(__V)
            }
        }
    };
}

/// Generates a `boolean | <Options>` union type, the shape Go emits for the
/// many `BooleanOr<Options>` provider unions in `ServerCapabilities`.
///
/// Serialization writes the single set variant (a bare JSON boolean or the
/// nested options object). Deserialization dispatches a JSON boolean to the
/// `boolean` field and a JSON object to the options field, mirroring the Go
/// `UnmarshalJSONFrom` `PeekKind` switch (`'t'`/`'f'` vs `'{'`).
macro_rules! boolean_or_options {
    (
        $(#[$smeta:meta])*
        $name:ident, $field:ident : $ty:ty, $expecting:literal
    ) => {
        $(#[$smeta])*
        #[derive(Debug, Clone, Default, PartialEq)]
        pub struct $name {
            /// The boolean variant.
            pub boolean: Option<bool>,
            /// The options-object variant.
            pub $field: Option<$ty>,
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                match (self.boolean, &self.$field) {
                    (Some(b), None) => serializer.serialize_bool(b),
                    (None, Some(o)) => o.serialize(serializer),
                    _ => Err(serde::ser::Error::custom(concat!(
                        "exactly one element of ",
                        stringify!($name),
                        " should be set"
                    ))),
                }
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct __V;
                impl<'de> Visitor<'de> for __V {
                    type Value = $name;
                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str($expecting)
                    }
                    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                        Ok($name { boolean: Some(v), $field: None })
                    }
                    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                        let o = <$ty>::deserialize(de::value::MapAccessDeserializer::new(map))?;
                        Ok($name { boolean: None, $field: Some(o) })
                    }
                }
                deserializer.deserialize_any(__V)
            }
        }
    };
}

/// Generates a `boolean | <Options> | <RegistrationOptions>` triple-union, the
/// shape Go emits for the many `BooleanOr<Options>Or<RegistrationOptions>`
/// provider unions in `ServerCapabilities` (declaration / typeDefinition /
/// implementation / color / foldingRange / selectionRange / callHierarchy /
/// linkedEditing / moniker / typeHierarchy / inlineValue / inlayHint).
///
/// Serialization writes the single set variant (a bare JSON boolean, the typed
/// options object, or the typed registration-options object).
/// Deserialization mirrors the Go `PeekKind`/`jsonObjectHasKey` dispatch: a
/// JSON boolean fills `boolean`; a JSON object carrying a `documentSelector`
/// key is decoded into the typed `<RegistrationOptions>`; any other object is
/// decoded as the typed `<Options>`.
macro_rules! boolean_or_options_or_registration {
    (
        $(#[$smeta:meta])*
        $name:ident, $field:ident : $ty:ty, registration: $reg:ty, $expecting:literal
    ) => {
        $(#[$smeta])*
        #[derive(Debug, Clone, Default, PartialEq)]
        pub struct $name {
            /// The boolean variant.
            pub boolean: Option<bool>,
            /// The typed options-object variant.
            pub $field: Option<$ty>,
            /// The typed registration-options variant (object with `documentSelector`).
            pub registration_options: Option<$reg>,
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                match (self.boolean, &self.$field, &self.registration_options) {
                    (Some(b), None, None) => serializer.serialize_bool(b),
                    (None, Some(o), None) => o.serialize(serializer),
                    (None, None, Some(r)) => r.serialize(serializer),
                    _ => Err(serde::ser::Error::custom(concat!(
                        "exactly one element of ",
                        stringify!($name),
                        " should be set"
                    ))),
                }
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = serde_json::Value::deserialize(deserializer)?;
                let mut out = $name::default();
                if let Some(b) = value.as_bool() {
                    out.boolean = Some(b);
                } else if value.is_object() {
                    if value.get("documentSelector").is_some() {
                        out.registration_options =
                            Some(serde_json::from_value(value).map_err(de::Error::custom)?);
                    } else {
                        out.$field =
                            Some(serde_json::from_value(value).map_err(de::Error::custom)?);
                    }
                } else {
                    return Err(de::Error::custom($expecting));
                }
                Ok(out)
            }
        }
    };
}

/// A document URI. Mirrors Go `type DocumentUri string`: a string newtype that
/// (de)serializes as a plain JSON string.
///
/// # Examples
/// ```
/// let u: tsgo_lsproto::DocumentUri = serde_json::from_str("\"file:///a.ts\"").unwrap();
/// assert_eq!(u.0, "file:///a.ts");
/// ```
// Go: internal/lsp/lsproto/lsp.go:DocumentUri
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct DocumentUri(pub String);

impl Serialize for DocumentUri {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for DocumentUri {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(DocumentUri(String::deserialize(deserializer)?))
    }
}

/// A union of an integer or a string (LSP `integer | string`).
///
/// Exactly one field is set; mirrors Go's pointer-pair representation. On
/// deserialize, a JSON number yields [`IntegerOrString::integer`] and a JSON
/// string yields [`IntegerOrString::string`]; any other kind is an error.
///
/// # Examples
/// ```
/// let v: tsgo_lsproto::IntegerOrString = serde_json::from_str("42").unwrap();
/// assert_eq!(v.integer, Some(42));
/// ```
// Go: internal/lsp/lsproto/lsp_generated.go:IntegerOrString
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IntegerOrString {
    /// The integer variant, if the value was a JSON number.
    pub integer: Option<i32>,
    /// The string variant, if the value was a JSON string.
    pub string: Option<String>,
}

impl Serialize for IntegerOrString {
    // Go: internal/lsp/lsproto/lsp_generated.go:IntegerOrString.MarshalJSONTo
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (self.integer, &self.string) {
            (Some(i), None) => serializer.serialize_i32(i),
            (None, Some(s)) => serializer.serialize_str(s),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of IntegerOrString should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for IntegerOrString {
    // Go: internal/lsp/lsproto/lsp_generated.go:IntegerOrString.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = IntegerOrString;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an integer or a string")
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(IntegerOrString {
                    integer: Some(v as i32),
                    string: None,
                })
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(IntegerOrString {
                    integer: Some(v as i32),
                    string: None,
                })
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(IntegerOrString {
                    integer: None,
                    string: Some(v.to_string()),
                })
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// A union of an integer or `null` (LSP `integer | null`).
///
/// # Examples
/// ```
/// let v: tsgo_lsproto::IntegerOrNull = serde_json::from_str("null").unwrap();
/// assert_eq!(v.integer, None);
/// ```
// Go: internal/lsp/lsproto/lsp_generated.go:IntegerOrNull
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IntegerOrNull {
    /// The integer variant; `None` represents JSON `null`.
    pub integer: Option<i32>,
}

impl Serialize for IntegerOrNull {
    // Go: internal/lsp/lsproto/lsp_generated.go:IntegerOrNull.MarshalJSONTo
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.integer {
            Some(i) => serializer.serialize_i32(i),
            None => serializer.serialize_none(),
        }
    }
}

impl<'de> Deserialize<'de> for IntegerOrNull {
    // Go: internal/lsp/lsproto/lsp_generated.go:IntegerOrNull.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = IntegerOrNull;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an integer or null")
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(IntegerOrNull {
                    integer: Some(v as i32),
                })
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(IntegerOrNull {
                    integer: Some(v as i32),
                })
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(IntegerOrNull { integer: None })
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(IntegerOrNull { integer: None })
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// A union of a [`DocumentUri`] or `null` (LSP `DocumentUri | null`).
///
/// # Examples
/// ```
/// let v: tsgo_lsproto::DocumentUriOrNull = serde_json::from_str("null").unwrap();
/// assert_eq!(v.document_uri, None);
/// ```
// Go: internal/lsp/lsproto/lsp_generated.go:DocumentUriOrNull
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentUriOrNull {
    /// The URI variant; `None` represents JSON `null`.
    pub document_uri: Option<DocumentUri>,
}

impl Serialize for DocumentUriOrNull {
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentUriOrNull.MarshalJSONTo
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.document_uri {
            Some(uri) => uri.serialize(serializer),
            None => serializer.serialize_none(),
        }
    }
}

impl<'de> Deserialize<'de> for DocumentUriOrNull {
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentUriOrNull.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = DocumentUriOrNull;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a document URI or null")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(DocumentUriOrNull {
                    document_uri: Some(DocumentUri(v.to_string())),
                })
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(DocumentUriOrNull { document_uri: None })
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(DocumentUriOrNull { document_uri: None })
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// A union of a string or `null` (LSP `string | null`), used for the deprecated
/// [`InitializeParams`] `rootPath`. `None` represents JSON `null`.
// Go: internal/lsp/lsproto/lsp_generated.go:StringOrNull
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StringOrNull {
    /// The string variant; `None` represents JSON `null`.
    pub string: Option<String>,
}

impl Serialize for StringOrNull {
    // Go: internal/lsp/lsproto/lsp_generated.go:StringOrNull.MarshalJSONTo
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.string {
            Some(s) => serializer.serialize_str(s),
            None => serializer.serialize_none(),
        }
    }
}

impl<'de> Deserialize<'de> for StringOrNull {
    // Go: internal/lsp/lsproto/lsp_generated.go:StringOrNull.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = StringOrNull;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string or null")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StringOrNull {
                    string: Some(v.to_string()),
                })
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StringOrNull { string: None })
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StringOrNull { string: None })
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// Inlay hint kinds (LSP `InlayHintKind`, an integer enum).
///
/// # Examples
/// ```
/// assert_eq!(tsgo_lsproto::InlayHintKind::TYPE.to_string(), "Type");
/// ```
// Go: internal/lsp/lsproto/lsp_generated.go:InlayHintKind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InlayHintKind(pub u32);

impl InlayHintKind {
    /// An inlay hint for a type annotation.
    pub const TYPE: InlayHintKind = InlayHintKind(1);
    /// An inlay hint for a parameter.
    pub const PARAMETER: InlayHintKind = InlayHintKind(2);
}

impl fmt::Display for InlayHintKind {
    // Go: internal/lsp/lsproto/lsp_generated.go:InlayHintKind.String
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            1 => f.write_str("Type"),
            2 => f.write_str("Parameter"),
            n => write!(f, "InlayHintKind({n})"),
        }
    }
}

impl Serialize for InlayHintKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for InlayHintKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(InlayHintKind(u32::deserialize(deserializer)?))
    }
}

/// A folding-range kind (LSP `FoldingRangeKind`, a string enum).
// Go: internal/lsp/lsproto/lsp_generated.go:FoldingRangeKind
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FoldingRangeKind(pub String);

impl FoldingRangeKind {
    /// Folding range for a comment (`"comment"`).
    pub fn comment() -> FoldingRangeKind {
        FoldingRangeKind("comment".to_string())
    }
    /// Folding range for an import/include block (`"imports"`).
    pub fn imports() -> FoldingRangeKind {
        FoldingRangeKind("imports".to_string())
    }
    /// Folding range for a region, e.g. `#region` (`"region"`).
    pub fn region() -> FoldingRangeKind {
        FoldingRangeKind("region".to_string())
    }
}

impl Serialize for FoldingRangeKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for FoldingRangeKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(FoldingRangeKind(String::deserialize(deserializer)?))
    }
}

/// A union of a string label or an array of [`InlayHintLabelPart`]
/// (LSP `string | InlayHintLabelPart[]`).
///
/// # Examples
/// ```
/// let v: tsgo_lsproto::StringOrInlayHintLabelParts =
///     serde_json::from_str("\"x\"").unwrap();
/// assert_eq!(v.string.as_deref(), Some("x"));
/// ```
// Go: internal/lsp/lsproto/lsp_generated.go:StringOrInlayHintLabelParts
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StringOrInlayHintLabelParts {
    /// The plain-string variant.
    pub string: Option<String>,
    /// The structured-parts variant.
    pub inlay_hint_label_parts: Option<Vec<InlayHintLabelPart>>,
}

impl Serialize for StringOrInlayHintLabelParts {
    // Go: lsp_generated.go:StringOrInlayHintLabelParts.MarshalJSONTo
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (&self.string, &self.inlay_hint_label_parts) {
            (Some(s), None) => serializer.serialize_str(s),
            (None, Some(parts)) => parts.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of StringOrInlayHintLabelParts should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for StringOrInlayHintLabelParts {
    // Go: lsp_generated.go:StringOrInlayHintLabelParts.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = StringOrInlayHintLabelParts;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string or an array of inlay-hint label parts")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StringOrInlayHintLabelParts {
                    string: Some(v.to_string()),
                    inlay_hint_label_parts: None,
                })
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
                let parts = Vec::<InlayHintLabelPart>::deserialize(
                    de::value::SeqAccessDeserializer::new(seq),
                )?;
                Ok(StringOrInlayHintLabelParts {
                    string: None,
                    inlay_hint_label_parts: Some(parts),
                })
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// A position encoding kind (LSP `PositionEncodingKind`, a string enum).
///
/// Mirrors Go `type PositionEncodingKind string`. Modeled as a
/// [`std::borrow::Cow<'static, str>`] newtype so the predefined values are
/// `const`-constructible (matching the integer-kind associated-constant style,
/// e.g. [`SymbolKind::FILE`]) while still (de)serializing as a plain JSON
/// string.
// Go: internal/lsp/lsproto/lsp_generated.go:PositionEncodingKind
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct PositionEncodingKind(pub Cow<'static, str>);

impl PositionEncodingKind {
    /// Character offsets count UTF-8 code units (e.g. bytes).
    pub const UTF8: PositionEncodingKind = PositionEncodingKind(Cow::Borrowed("utf-8"));
    /// Character offsets count UTF-16 code units (the LSP default; always
    /// supported by servers).
    pub const UTF16: PositionEncodingKind = PositionEncodingKind(Cow::Borrowed("utf-16"));
    /// Character offsets count UTF-32 code units (i.e. Unicode codepoints).
    pub const UTF32: PositionEncodingKind = PositionEncodingKind(Cow::Borrowed("utf-32"));
}

impl Serialize for PositionEncodingKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PositionEncodingKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(PositionEncodingKind(Cow::Owned(String::deserialize(
            deserializer,
        )?)))
    }
}

/// A predefined language kind (LSP `LanguageKind`, a string enum).
///
/// Mirrors Go `type LanguageKind string`. Like [`PositionEncodingKind`] it is a
/// [`Cow<'static, str>`] newtype so the predefined ids are `const`-constructible
/// and unknown ids round-trip as their raw string. (De)serializes as a plain
/// JSON string.
// Go: internal/lsp/lsproto/lsp_generated.go:LanguageKind
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct LanguageKind(pub Cow<'static, str>);

impl LanguageKind {
    /// ABAP.
    pub const ABAP: LanguageKind = LanguageKind(Cow::Borrowed("abap"));
    /// Windows Bat.
    pub const WINDOWS_BAT: LanguageKind = LanguageKind(Cow::Borrowed("bat"));
    /// BibTeX.
    pub const BIB_TEX: LanguageKind = LanguageKind(Cow::Borrowed("bibtex"));
    /// Clojure.
    pub const CLOJURE: LanguageKind = LanguageKind(Cow::Borrowed("clojure"));
    /// Coffeescript.
    pub const COFFEESCRIPT: LanguageKind = LanguageKind(Cow::Borrowed("coffeescript"));
    /// C.
    pub const C: LanguageKind = LanguageKind(Cow::Borrowed("c"));
    /// C++.
    pub const CPP: LanguageKind = LanguageKind(Cow::Borrowed("cpp"));
    /// C#.
    pub const C_SHARP: LanguageKind = LanguageKind(Cow::Borrowed("csharp"));
    /// CSS.
    pub const CSS: LanguageKind = LanguageKind(Cow::Borrowed("css"));
    /// D (proposed).
    pub const D: LanguageKind = LanguageKind(Cow::Borrowed("d"));
    /// Delphi (proposed; aliased to `pascal`).
    pub const DELPHI: LanguageKind = LanguageKind(Cow::Borrowed("pascal"));
    /// Diff.
    pub const DIFF: LanguageKind = LanguageKind(Cow::Borrowed("diff"));
    /// Dart.
    pub const DART: LanguageKind = LanguageKind(Cow::Borrowed("dart"));
    /// Dockerfile.
    pub const DOCKERFILE: LanguageKind = LanguageKind(Cow::Borrowed("dockerfile"));
    /// Elixir.
    pub const ELIXIR: LanguageKind = LanguageKind(Cow::Borrowed("elixir"));
    /// Erlang.
    pub const ERLANG: LanguageKind = LanguageKind(Cow::Borrowed("erlang"));
    /// F#.
    pub const F_SHARP: LanguageKind = LanguageKind(Cow::Borrowed("fsharp"));
    /// Git commit.
    pub const GIT_COMMIT: LanguageKind = LanguageKind(Cow::Borrowed("git-commit"));
    /// Git rebase (aliased to `rebase`).
    pub const GIT_REBASE: LanguageKind = LanguageKind(Cow::Borrowed("rebase"));
    /// Go.
    pub const GO: LanguageKind = LanguageKind(Cow::Borrowed("go"));
    /// Groovy.
    pub const GROOVY: LanguageKind = LanguageKind(Cow::Borrowed("groovy"));
    /// Handlebars.
    pub const HANDLEBARS: LanguageKind = LanguageKind(Cow::Borrowed("handlebars"));
    /// Haskell.
    pub const HASKELL: LanguageKind = LanguageKind(Cow::Borrowed("haskell"));
    /// HTML.
    pub const HTML: LanguageKind = LanguageKind(Cow::Borrowed("html"));
    /// Ini.
    pub const INI: LanguageKind = LanguageKind(Cow::Borrowed("ini"));
    /// Java.
    pub const JAVA: LanguageKind = LanguageKind(Cow::Borrowed("java"));
    /// JavaScript.
    pub const JAVA_SCRIPT: LanguageKind = LanguageKind(Cow::Borrowed("javascript"));
    /// JavaScript React (JSX).
    pub const JAVA_SCRIPT_REACT: LanguageKind = LanguageKind(Cow::Borrowed("javascriptreact"));
    /// JSON.
    pub const JSON: LanguageKind = LanguageKind(Cow::Borrowed("json"));
    /// LaTeX.
    pub const LA_TEX: LanguageKind = LanguageKind(Cow::Borrowed("latex"));
    /// Less.
    pub const LESS: LanguageKind = LanguageKind(Cow::Borrowed("less"));
    /// Lua.
    pub const LUA: LanguageKind = LanguageKind(Cow::Borrowed("lua"));
    /// Makefile.
    pub const MAKEFILE: LanguageKind = LanguageKind(Cow::Borrowed("makefile"));
    /// Markdown.
    pub const MARKDOWN: LanguageKind = LanguageKind(Cow::Borrowed("markdown"));
    /// Objective-C.
    pub const OBJECTIVE_C: LanguageKind = LanguageKind(Cow::Borrowed("objective-c"));
    /// Objective-C++.
    pub const OBJECTIVE_CPP: LanguageKind = LanguageKind(Cow::Borrowed("objective-cpp"));
    /// Pascal (proposed).
    pub const PASCAL: LanguageKind = LanguageKind(Cow::Borrowed("pascal"));
    /// Perl.
    pub const PERL: LanguageKind = LanguageKind(Cow::Borrowed("perl"));
    /// Perl 6.
    pub const PERL6: LanguageKind = LanguageKind(Cow::Borrowed("perl6"));
    /// PHP.
    pub const PHP: LanguageKind = LanguageKind(Cow::Borrowed("php"));
    /// Powershell.
    pub const POWERSHELL: LanguageKind = LanguageKind(Cow::Borrowed("powershell"));
    /// Pug (aliased to `jade`).
    pub const PUG: LanguageKind = LanguageKind(Cow::Borrowed("jade"));
    /// Python.
    pub const PYTHON: LanguageKind = LanguageKind(Cow::Borrowed("python"));
    /// R.
    pub const R: LanguageKind = LanguageKind(Cow::Borrowed("r"));
    /// Razor.
    pub const RAZOR: LanguageKind = LanguageKind(Cow::Borrowed("razor"));
    /// Ruby.
    pub const RUBY: LanguageKind = LanguageKind(Cow::Borrowed("ruby"));
    /// Rust.
    pub const RUST: LanguageKind = LanguageKind(Cow::Borrowed("rust"));
    /// SCSS.
    pub const SCSS: LanguageKind = LanguageKind(Cow::Borrowed("scss"));
    /// SASS.
    pub const SASS: LanguageKind = LanguageKind(Cow::Borrowed("sass"));
    /// Scala.
    pub const SCALA: LanguageKind = LanguageKind(Cow::Borrowed("scala"));
    /// ShaderLab.
    pub const SHADER_LAB: LanguageKind = LanguageKind(Cow::Borrowed("shaderlab"));
    /// Shell script.
    pub const SHELL_SCRIPT: LanguageKind = LanguageKind(Cow::Borrowed("shellscript"));
    /// SQL.
    pub const SQL: LanguageKind = LanguageKind(Cow::Borrowed("sql"));
    /// Swift.
    pub const SWIFT: LanguageKind = LanguageKind(Cow::Borrowed("swift"));
    /// TypeScript.
    pub const TYPE_SCRIPT: LanguageKind = LanguageKind(Cow::Borrowed("typescript"));
    /// TypeScript React (TSX).
    pub const TYPE_SCRIPT_REACT: LanguageKind = LanguageKind(Cow::Borrowed("typescriptreact"));
    /// TeX.
    pub const TEX: LanguageKind = LanguageKind(Cow::Borrowed("tex"));
    /// Visual Basic (aliased to `vb`).
    pub const VISUAL_BASIC: LanguageKind = LanguageKind(Cow::Borrowed("vb"));
    /// XML.
    pub const XML: LanguageKind = LanguageKind(Cow::Borrowed("xml"));
    /// XSL.
    pub const XSL: LanguageKind = LanguageKind(Cow::Borrowed("xsl"));
    /// YAML.
    pub const YAML: LanguageKind = LanguageKind(Cow::Borrowed("yaml"));
}

impl Serialize for LanguageKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LanguageKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(LanguageKind(Cow::Owned(String::deserialize(deserializer)?)))
    }
}

lsp_object! {
    /// A position in a text document, expressed as zero-based line and
    /// character (column) offsets.
    Position {
        ["Zero-based line position in a document."]
        req line: u32 => "line",
        ["Zero-based character offset on a line."]
        req character: u32 => "character",
    }
}

lsp_object! {
    /// A range in a text document, from a start to an end [`Position`].
    Range {
        ["The range's start position."]
        req start: Position => "start",
        ["The range's end position."]
        req end: Position => "end",
    }
}

lsp_object! {
    /// A location inside a resource, such as a line inside a text file.
    Location {
        ["The resource URI."]
        req uri: DocumentUri => "uri",
        ["The range within the resource."]
        req range: Range => "range",
    }
}

lsp_object! {
    /// A textual edit applicable to a text document.
    TextEdit {
        ["The range of the document to be replaced."]
        req range: Range => "range",
        ["The replacement text."]
        req new_text: String => "newText",
    }
}

lsp_object! {
    /// A [`TextEdit`] carrying an additional change-annotation identifier.
    ///
    /// Since LSP 3.16.0. Mirrors a plain [`TextEdit`] plus the required
    /// `annotationId` referencing a `ChangeAnnotation` (Go models the
    /// identifier as a plain `string`).
    // Go: internal/lsp/lsproto/lsp_generated.go:AnnotatedTextEdit
    AnnotatedTextEdit {
        ["The range of the document to be replaced."]
        req range: Range => "range",
        ["The replacement text."]
        req new_text: String => "newText",
        ["The identifier of the change annotation."]
        req annotation_id: String => "annotationId",
    }
}

lsp_object! {
    /// A snippet string carrying its kind discriminator.
    ///
    /// The `kind` is always the literal `"snippet"` ([`StringLiteralSnippet`]);
    /// `value` holds the snippet text (LSP snippet syntax).
    // Go: internal/lsp/lsproto/lsp_generated.go:StringValue
    StringValue {
        ["The kind of string value (always `\"snippet\"`)."]
        req kind: StringLiteralSnippet => "kind",
        ["The snippet string."]
        req value: String => "value",
    }
}

lsp_object! {
    /// An interactive text edit whose replacement is a snippet [`StringValue`].
    ///
    /// Since LSP 3.18.0 (proposed). The `snippet` is required and rejects an
    /// explicit `null`; `annotationId` is optional and also rejects `null`.
    // Go: internal/lsp/lsproto/lsp_generated.go:SnippetTextEdit
    SnippetTextEdit {
        ["The range of the document to be replaced."]
        req range: Range => "range",
        ["The snippet to be inserted."]
        reqnn snippet: StringValue => "snippet",
        ["The identifier of the snippet edit's change annotation."]
        opt annotation_id: String => "annotationId",
    }
}

lsp_object! {
    /// A command reference (LSP `Command`): a title plus the command identifier
    /// and its arguments.
    ///
    /// `arguments` keeps the Go `*[]any` shape (`LSPAny`): each element is
    /// arbitrary JSON ([`serde_json::Value`]), intentionally untyped.
    // Go: internal/lsp/lsproto/lsp_generated.go:Command
    Command {
        ["Title of the command, e.g. `save`."]
        req title: String => "title",
        ["An optional tooltip."]
        opt tooltip: String => "tooltip",
        ["The identifier of the actual command handler."]
        req command: String => "command",
        ["Arguments the command handler is invoked with (arbitrary JSON, `LSPAny`)."]
        opt arguments: Vec<serde_json::Value> => "arguments",
    }
}

lsp_object! {
    /// An inlay-hint label part (LSP `InlayHintLabelPart`).
    InlayHintLabelPart {
        ["The value of this label part."]
        req value: String => "value",
        ["The tooltip text shown for this label part (string or markup)."]
        opt tooltip: StringOrMarkupContent => "tooltip",
        ["An optional source-code location for this label part."]
        opt location: Location => "location",
        ["An optional command executed when the label part is activated."]
        opt command: Command => "command",
    }
}

lsp_object! {
    /// An inlay hint: a label rendered inline at a [`Position`].
    InlayHint {
        ["The position of this hint."]
        req position: Position => "position",
        ["The label of this hint (string or structured parts)."]
        req label: StringOrInlayHintLabelParts => "label",
        ["The kind of this hint."]
        opt kind: InlayHintKind => "kind",
        ["Text edits performed when accepting this hint."]
        opt text_edits: Vec<TextEdit> => "textEdits",
        ["The tooltip text shown for this hint (string or markup)."]
        opt tooltip: StringOrMarkupContent => "tooltip",
        ["Render padding before the hint."]
        opt padding_left: bool => "paddingLeft",
        ["Render padding after the hint."]
        opt padding_right: bool => "paddingRight",
        ["A data entry preserved between request and resolve (deferred: raw JSON)."]
        opt data: serde_json::Value => "data",
    }
}

lsp_object! {
    /// A folding range in a text document.
    FoldingRange {
        ["The zero-based start line of the range to fold."]
        req start_line: u32 => "startLine",
        ["The zero-based start character offset."]
        opt start_character: u32 => "startCharacter",
        ["The zero-based end line of the range to fold."]
        req end_line: u32 => "endLine",
        ["The zero-based end character offset."]
        opt end_character: u32 => "endCharacter",
        ["The kind of the folding range."]
        opt kind: FoldingRangeKind => "kind",
        ["Text shown when the range is collapsed."]
        opt collapsed_text: String => "collapsedText",
    }
}

lsp_object! {
    /// A nested selection range: a [`Range`] plus an optional `parent` selection
    /// range that fully contains it.
    ///
    /// The `textDocument/selectionRange` result is one of these chains per
    /// requested position — walking the `parent` pointers expands the selection
    /// outward (innermost first), so `parent.range` always contains `this.range`.
    /// `parent` is boxed because the type is self-referential.
    // Go: internal/lsp/lsproto/lsp_generated.go:SelectionRange
    SelectionRange {
        ["The range of this selection range."]
        req range: Range => "range",
        ["The parent selection range containing this range (so `parent.range` contains `range`)."]
        opt parent: Box<SelectionRange> => "parent",
    }
}

lsp_object! {
    /// The result of a `textDocument/linkedEditingRange` request: a set of
    /// [`Range`]s that can be edited together (they must have identical length
    /// and identical text content and cannot overlap), plus an optional
    /// `word_pattern` regular expression describing valid contents for those
    /// ranges. If no pattern is provided the client's word pattern is used.
    // Go: internal/lsp/lsproto/lsp_generated.go:LinkedEditingRanges
    LinkedEditingRanges {
        ["A list of ranges that can be edited together (identical length + content, non-overlapping)."]
        reqnn ranges: Vec<Range> => "ranges",
        ["An optional word pattern (regular expression) describing valid contents for the ranges."]
        opt word_pattern: String => "wordPattern",
    }
}

/// Generates a string-literal type (LSP discriminator literals such as
/// `"create"`): serializes to the literal and rejects any other value.
macro_rules! string_literal {
    ($(#[$smeta:meta])* $name:ident, $lit:literal) => {
        $(#[$smeta])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub struct $name;

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str($lit)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = String::deserialize(deserializer)?;
                if s == $lit {
                    Ok($name)
                } else {
                    Err(de::Error::custom(format!(
                        concat!("expected ", stringify!($name), " value {:?}, got {:?}"),
                        $lit, s
                    )))
                }
            }
        }
    };
}

string_literal! {
    /// The `"begin"` discriminator literal for `WorkDoneProgressBegin`.
    StringLiteralBegin, "begin"
}
string_literal! {
    /// The `"report"` discriminator literal for `WorkDoneProgressReport`.
    StringLiteralReport, "report"
}
string_literal! {
    /// The `"end"` discriminator literal for `WorkDoneProgressEnd`.
    StringLiteralEnd, "end"
}
string_literal! {
    /// The `"create"` discriminator literal for `CreateFile`.
    StringLiteralCreate, "create"
}
string_literal! {
    /// The `"rename"` discriminator literal for `RenameFile`.
    StringLiteralRename, "rename"
}
string_literal! {
    /// The `"delete"` discriminator literal for `DeleteFile`.
    StringLiteralDelete, "delete"
}
string_literal! {
    /// The `"snippet"` discriminator literal for [`StringValue`].
    // Go: internal/lsp/lsproto/lsp_generated.go:StringLiteralSnippet
    StringLiteralSnippet, "snippet"
}

/// A symbol kind (LSP `SymbolKind`, an integer enum).
// Go: internal/lsp/lsproto/lsp_generated.go:SymbolKind
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SymbolKind(pub u32);

impl SymbolKind {
    /// A file symbol.
    pub const FILE: SymbolKind = SymbolKind(1);
    /// A function symbol.
    pub const FUNCTION: SymbolKind = SymbolKind(12);
    /// A variable symbol.
    pub const VARIABLE: SymbolKind = SymbolKind(13);
}

impl fmt::Display for SymbolKind {
    // Go: internal/lsp/lsproto/lsp_generated.go:SymbolKind.String
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.0 {
            1 => "File",
            2 => "Module",
            3 => "Namespace",
            4 => "Package",
            5 => "Class",
            6 => "Method",
            7 => "Property",
            8 => "Field",
            9 => "Constructor",
            10 => "Enum",
            11 => "Interface",
            12 => "Function",
            13 => "Variable",
            14 => "Constant",
            15 => "String",
            16 => "Number",
            17 => "Boolean",
            18 => "Array",
            19 => "Object",
            20 => "Key",
            21 => "Null",
            22 => "EnumMember",
            23 => "Struct",
            24 => "Event",
            25 => "Operator",
            26 => "TypeParameter",
            n => return write!(f, "SymbolKind({n})"),
        };
        f.write_str(name)
    }
}

impl Serialize for SymbolKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for SymbolKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(SymbolKind(u32::deserialize(deserializer)?))
    }
}

lsp_object! {
    /// Hover request options.
    HoverOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

lsp_object! {
    /// The `begin` payload of a `$/progress` work-done notification.
    WorkDoneProgressBegin {
        ["The discriminator (`\"begin\"`)."]
        req kind: StringLiteralBegin => "kind",
        ["Mandatory title of the progress operation."]
        req title: String => "title",
        ["Whether a cancel button should be shown."]
        opt cancellable: bool => "cancellable",
        ["More detailed progress message."]
        opt message: String => "message",
        ["Progress percentage in `[0, 100]`."]
        opt percentage: u32 => "percentage",
    }
}

lsp_object! {
    /// The `report` payload of a `$/progress` work-done notification.
    WorkDoneProgressReport {
        ["The discriminator (`\"report\"`)."]
        req kind: StringLiteralReport => "kind",
        ["Whether a cancel button should be shown."]
        opt cancellable: bool => "cancellable",
        ["More detailed progress message."]
        opt message: String => "message",
        ["Progress percentage in `[0, 100]`."]
        opt percentage: u32 => "percentage",
    }
}

lsp_object! {
    /// The `end` payload of a `$/progress` work-done notification.
    WorkDoneProgressEnd {
        ["The discriminator (`\"end\"`)."]
        req kind: StringLiteralEnd => "kind",
        ["A final message about the outcome."]
        opt message: String => "message",
    }
}

lsp_object! {
    /// An insert/replace edit (LSP `InsertReplaceEdit`).
    InsertReplaceEdit {
        ["The string to be inserted."]
        req new_text: String => "newText",
        ["The range if the insert is requested."]
        req insert: Range => "insert",
        ["The range if the replace is requested."]
        req replace: Range => "replace",
    }
}

lsp_object! {
    /// A text-document identifier with an optional (`integer | null`) version.
    OptionalVersionedTextDocumentIdentifier {
        ["The text document's URI."]
        req uri: DocumentUri => "uri",
        ["The version number, or `null` if unknown."]
        req version: IntegerOrNull => "version",
    }
}

lsp_object! {
    /// A grouped set of edits to a single text document.
    TextDocumentEdit {
        ["The text document to change."]
        req text_document: OptionalVersionedTextDocumentIdentifier => "textDocument",
        ["The edits to apply (`TextEdit | AnnotatedTextEdit | SnippetTextEdit`)."]
        reqnn edits: Vec<TextEditOrAnnotatedTextEditOrSnippetTextEdit> => "edits",
    }
}

lsp_object! {
    /// Options for a create-file resource operation (LSP `CreateFileOptions`).
    // Go: internal/lsp/lsproto/lsp_generated.go:CreateFileOptions
    CreateFileOptions {
        ["Overwrite an existing file (wins over `ignoreIfExists`)."]
        opt overwrite: bool => "overwrite",
        ["Ignore the operation if the file already exists."]
        opt ignore_if_exists: bool => "ignoreIfExists",
    }
}

lsp_object! {
    /// A create-file resource operation.
    CreateFile {
        ["The discriminator (`\"create\"`)."]
        req kind: StringLiteralCreate => "kind",
        ["An optional change-annotation identifier."]
        opt annotation_id: String => "annotationId",
        ["The resource to create."]
        req uri: DocumentUri => "uri",
        ["Additional create options."]
        opt options: CreateFileOptions => "options",
    }
}

lsp_object! {
    /// Options for a rename-file resource operation (LSP `RenameFileOptions`).
    // Go: internal/lsp/lsproto/lsp_generated.go:RenameFileOptions
    RenameFileOptions {
        ["Overwrite the target if it exists (wins over `ignoreIfExists`)."]
        opt overwrite: bool => "overwrite",
        ["Ignore the operation if the target already exists."]
        opt ignore_if_exists: bool => "ignoreIfExists",
    }
}

lsp_object! {
    /// A rename-file resource operation.
    RenameFile {
        ["The discriminator (`\"rename\"`)."]
        req kind: StringLiteralRename => "kind",
        ["An optional change-annotation identifier."]
        opt annotation_id: String => "annotationId",
        ["The old (existing) location."]
        req old_uri: DocumentUri => "oldUri",
        ["The new location."]
        req new_uri: DocumentUri => "newUri",
        ["Rename options."]
        opt options: RenameFileOptions => "options",
    }
}

lsp_object! {
    /// Options for a delete-file resource operation (LSP `DeleteFileOptions`).
    // Go: internal/lsp/lsproto/lsp_generated.go:DeleteFileOptions
    DeleteFileOptions {
        ["Delete the content recursively if a folder is denoted."]
        opt recursive: bool => "recursive",
        ["Ignore the operation if the file does not exist."]
        opt ignore_if_not_exists: bool => "ignoreIfNotExists",
    }
}

lsp_object! {
    /// A delete-file resource operation.
    DeleteFile {
        ["The discriminator (`\"delete\"`)."]
        req kind: StringLiteralDelete => "kind",
        ["An optional change-annotation identifier."]
        opt annotation_id: String => "annotationId",
        ["The file to delete."]
        req uri: DocumentUri => "uri",
        ["Delete options."]
        opt options: DeleteFileOptions => "options",
    }
}

/// A union of a boolean or [`HoverOptions`] (LSP `boolean | HoverOptions`).
// Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrHoverOptions
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BooleanOrHoverOptions {
    /// The boolean variant.
    pub boolean: Option<bool>,
    /// The options-object variant.
    pub hover_options: Option<HoverOptions>,
}

impl Serialize for BooleanOrHoverOptions {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (self.boolean, &self.hover_options) {
            (Some(b), None) => serializer.serialize_bool(b),
            (None, Some(o)) => o.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of BooleanOrHoverOptions should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for BooleanOrHoverOptions {
    // Go: lsp_generated.go:BooleanOrHoverOptions.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = BooleanOrHoverOptions;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a boolean or a hover-options object")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(BooleanOrHoverOptions {
                    boolean: Some(v),
                    hover_options: None,
                })
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                let opts = HoverOptions::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(BooleanOrHoverOptions {
                    boolean: None,
                    hover_options: Some(opts),
                })
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// A `kind`-discriminated union of work-done progress payloads
/// (LSP `WorkDoneProgressBegin | WorkDoneProgressReport | WorkDoneProgressEnd`).
// Go: internal/lsp/lsproto/lsp_generated.go:WorkDoneProgressBeginOrReportOrEnd
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkDoneProgressBeginOrReportOrEnd {
    /// The `begin` variant.
    pub begin: Option<WorkDoneProgressBegin>,
    /// The `report` variant.
    pub report: Option<WorkDoneProgressReport>,
    /// The `end` variant.
    pub end: Option<WorkDoneProgressEnd>,
}

impl Serialize for WorkDoneProgressBeginOrReportOrEnd {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(v) = &self.begin {
            v.serialize(serializer)
        } else if let Some(v) = &self.report {
            v.serialize(serializer)
        } else if let Some(v) = &self.end {
            v.serialize(serializer)
        } else {
            Err(serde::ser::Error::custom(
                "exactly one element of WorkDoneProgressBeginOrReportOrEnd should be set",
            ))
        }
    }
}

impl<'de> Deserialize<'de> for WorkDoneProgressBeginOrReportOrEnd {
    // Go: lsp_generated.go:WorkDoneProgressBeginOrReportOrEnd.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let kind = value
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let mut out = WorkDoneProgressBeginOrReportOrEnd::default();
        match kind.as_deref() {
            Some("begin") => {
                out.begin = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
            }
            Some("report") => {
                out.report = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
            }
            Some("end") => {
                out.end = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
            }
            _ => {
                return Err(de::Error::custom(
                    "invalid WorkDoneProgressBeginOrReportOrEnd",
                ))
            }
        }
        Ok(out)
    }
}

/// A presence-discriminated union of [`TextEdit`] or [`InsertReplaceEdit`].
///
/// A JSON object with an `insert` key is an [`InsertReplaceEdit`]; otherwise a
/// `range` key makes it a [`TextEdit`].
// Go: internal/lsp/lsproto/lsp_generated.go:TextEditOrInsertReplaceEdit
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextEditOrInsertReplaceEdit {
    /// The plain [`TextEdit`] variant.
    pub text_edit: Option<TextEdit>,
    /// The [`InsertReplaceEdit`] variant.
    pub insert_replace_edit: Option<InsertReplaceEdit>,
}

impl Serialize for TextEditOrInsertReplaceEdit {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (&self.text_edit, &self.insert_replace_edit) {
            (Some(v), None) => v.serialize(serializer),
            (None, Some(v)) => v.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of TextEditOrInsertReplaceEdit should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for TextEditOrInsertReplaceEdit {
    // Go: lsp_generated.go:TextEditOrInsertReplaceEdit.UnmarshalJSONFrom
    //
    // Go scans for the first object key matching `insert`/`range` in document
    // order; this port checks presence in that priority order. Equivalent for
    // all real inputs (the two keys are mutually exclusive in practice).
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let mut out = TextEditOrInsertReplaceEdit::default();
        if value.get("insert").is_some() {
            out.insert_replace_edit =
                Some(serde_json::from_value(value).map_err(de::Error::custom)?);
        } else if value.get("range").is_some() {
            out.text_edit = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
        } else {
            return Err(de::Error::custom("invalid TextEditOrInsertReplaceEdit"));
        }
        Ok(out)
    }
}

/// A presence-discriminated union of [`TextEdit`], [`AnnotatedTextEdit`], or
/// [`SnippetTextEdit`] (LSP `TextEdit | AnnotatedTextEdit | SnippetTextEdit`).
///
/// A JSON object carrying a `snippet` key is a [`SnippetTextEdit`]; otherwise an
/// `annotationId` key makes it an [`AnnotatedTextEdit`]; otherwise it is a plain
/// [`TextEdit`].
// Go: internal/lsp/lsproto/lsp_generated.go:TextEditOrAnnotatedTextEditOrSnippetTextEdit
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextEditOrAnnotatedTextEditOrSnippetTextEdit {
    /// The plain [`TextEdit`] variant.
    pub text_edit: Option<TextEdit>,
    /// The [`AnnotatedTextEdit`] variant.
    pub annotated_text_edit: Option<AnnotatedTextEdit>,
    /// The [`SnippetTextEdit`] variant.
    pub snippet_text_edit: Option<SnippetTextEdit>,
}

impl Serialize for TextEditOrAnnotatedTextEditOrSnippetTextEdit {
    // Go: lsp_generated.go:TextEditOrAnnotatedTextEditOrSnippetTextEdit.MarshalJSONTo
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (
            &self.text_edit,
            &self.annotated_text_edit,
            &self.snippet_text_edit,
        ) {
            (Some(v), None, None) => v.serialize(serializer),
            (None, Some(v), None) => v.serialize(serializer),
            (None, None, Some(v)) => v.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of TextEditOrAnnotatedTextEditOrSnippetTextEdit should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for TextEditOrAnnotatedTextEditOrSnippetTextEdit {
    // Go: lsp_generated.go:TextEditOrAnnotatedTextEditOrSnippetTextEdit.UnmarshalJSONFrom
    //
    // Go uses `jsonObjectHasKey(data, "snippet", "annotationId")`: a `snippet`
    // key selects `SnippetTextEdit`, else an `annotationId` key selects
    // `AnnotatedTextEdit`, else a plain `TextEdit`. This port checks presence in
    // that priority order, equivalent for all real inputs (a `SnippetTextEdit`
    // always carries `snippet`; an `AnnotatedTextEdit` carries `annotationId`
    // but no `snippet`).
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let mut out = TextEditOrAnnotatedTextEditOrSnippetTextEdit::default();
        if value.get("snippet").is_some() {
            out.snippet_text_edit = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
        } else if value.get("annotationId").is_some() {
            out.annotated_text_edit =
                Some(serde_json::from_value(value).map_err(de::Error::custom)?);
        } else {
            out.text_edit = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
        }
        Ok(out)
    }
}

/// A `kind`-discriminated union of document/resource edit operations
/// (LSP `TextDocumentEdit | CreateFile | RenameFile | DeleteFile`).
///
/// Absence of a `kind` key means [`TextDocumentEdit`]; otherwise the `kind`
/// value (`"create"`/`"rename"`/`"delete"`) selects the resource operation.
// Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile {
    /// The grouped text-document edit variant.
    pub text_document_edit: Option<TextDocumentEdit>,
    /// The create-file variant.
    pub create_file: Option<CreateFile>,
    /// The rename-file variant.
    pub rename_file: Option<RenameFile>,
    /// The delete-file variant.
    pub delete_file: Option<DeleteFile>,
}

impl Serialize for TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(v) = &self.text_document_edit {
            v.serialize(serializer)
        } else if let Some(v) = &self.create_file {
            v.serialize(serializer)
        } else if let Some(v) = &self.rename_file {
            v.serialize(serializer)
        } else if let Some(v) = &self.delete_file {
            v.serialize(serializer)
        } else {
            Err(serde::ser::Error::custom(
                "exactly one element of TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile should be set",
            ))
        }
    }
}

impl<'de> Deserialize<'de> for TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile {
    // Go: lsp_generated.go:TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let kind = value
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let mut out = TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile::default();
        match kind.as_deref() {
            Some("rename") => {
                out.rename_file = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
            }
            Some("create") => {
                out.create_file = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
            }
            Some("delete") => {
                out.delete_file = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
            }
            _ => {
                out.text_document_edit =
                    Some(serde_json::from_value(value).map_err(de::Error::custom)?);
            }
        }
        Ok(out)
    }
}

// `ClientCapabilities` (the pointer-based request tree) and its `resolve()` now
// live in `capabilities.rs`; `InitializeParams` references it via `crate::`.
// Go: internal/lsp/lsproto/lsp_generated.go:ClientCapabilities

/// Defines how the host (editor) should sync document changes to the server
/// (LSP `TextDocumentSyncKind`, an integer enum).
// Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentSyncKind
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextDocumentSyncKind(pub u32);

impl TextDocumentSyncKind {
    /// Documents should not be synced at all.
    pub const NONE: TextDocumentSyncKind = TextDocumentSyncKind(0);
    /// Documents are synced by always sending the full content of the document.
    pub const FULL: TextDocumentSyncKind = TextDocumentSyncKind(1);
    /// Documents are synced by sending the full content on open; after that
    /// only incremental updates are sent.
    pub const INCREMENTAL: TextDocumentSyncKind = TextDocumentSyncKind(2);
}

impl fmt::Display for TextDocumentSyncKind {
    // Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentSyncKind.String
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.0 {
            0 => "None",
            1 => "Full",
            2 => "Incremental",
            n => return write!(f, "TextDocumentSyncKind({n})"),
        };
        f.write_str(name)
    }
}

impl Serialize for TextDocumentSyncKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for TextDocumentSyncKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(TextDocumentSyncKind(u32::deserialize(deserializer)?))
    }
}

lsp_object! {
    /// Options to control whether the client includes text content on save.
    SaveOptions {
        ["The client is supposed to include the content on save."]
        opt include_text: bool => "includeText",
    }
}

/// A union of a boolean or [`SaveOptions`] (LSP `boolean | SaveOptions`).
// Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrSaveOptions
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BooleanOrSaveOptions {
    /// The boolean variant.
    pub boolean: Option<bool>,
    /// The options-object variant.
    pub save_options: Option<SaveOptions>,
}

impl Serialize for BooleanOrSaveOptions {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (self.boolean, &self.save_options) {
            (Some(b), None) => serializer.serialize_bool(b),
            (None, Some(o)) => o.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of BooleanOrSaveOptions should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for BooleanOrSaveOptions {
    // Go: lsp_generated.go:BooleanOrSaveOptions.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = BooleanOrSaveOptions;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a boolean or a save-options object")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(BooleanOrSaveOptions {
                    boolean: Some(v),
                    save_options: None,
                })
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                let opts = SaveOptions::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(BooleanOrSaveOptions {
                    boolean: None,
                    save_options: Some(opts),
                })
            }
        }
        deserializer.deserialize_any(V)
    }
}

lsp_object! {
    /// Detailed text-document sync options.
    TextDocumentSyncOptions {
        ["Open and close notifications are sent to the server."]
        opt open_close: bool => "openClose",
        ["How change notifications are sent to the server."]
        opt change: TextDocumentSyncKind => "change",
        ["Whether `willSave` notifications are sent to the server."]
        opt will_save: bool => "willSave",
        ["Whether `willSaveWaitUntil` requests are sent to the server."]
        opt will_save_wait_until: bool => "willSaveWaitUntil",
        ["Whether save notifications are sent to the server."]
        opt save: BooleanOrSaveOptions => "save",
    }
}

/// A union of [`TextDocumentSyncOptions`] or a bare [`TextDocumentSyncKind`]
/// (LSP `TextDocumentSyncOptions | TextDocumentSyncKind`).
// Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentSyncOptionsOrKind
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextDocumentSyncOptionsOrKind {
    /// The detailed-options variant.
    pub options: Option<TextDocumentSyncOptions>,
    /// The bare-kind variant.
    pub kind: Option<TextDocumentSyncKind>,
}

impl Serialize for TextDocumentSyncOptionsOrKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (&self.options, &self.kind) {
            (Some(o), None) => o.serialize(serializer),
            (None, Some(k)) => k.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of TextDocumentSyncOptionsOrKind should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for TextDocumentSyncOptionsOrKind {
    // Go: lsp_generated.go:TextDocumentSyncOptionsOrKind.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = TextDocumentSyncOptionsOrKind;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a text-document-sync options object or an integer kind")
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(TextDocumentSyncOptionsOrKind {
                    options: None,
                    kind: Some(TextDocumentSyncKind(v as u32)),
                })
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(TextDocumentSyncOptionsOrKind {
                    options: None,
                    kind: Some(TextDocumentSyncKind(v as u32)),
                })
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                let opts = TextDocumentSyncOptions::deserialize(
                    de::value::MapAccessDeserializer::new(map),
                )?;
                Ok(TextDocumentSyncOptionsOrKind {
                    options: Some(opts),
                    kind: None,
                })
            }
        }
        deserializer.deserialize_any(V)
    }
}

lsp_object! {
    /// Server support for resolving completion-item label details.
    ServerCompletionItemOptions {
        ["Whether the server resolves `CompletionItemLabelDetails` on resolve."]
        opt label_details_support: bool => "labelDetailsSupport",
    }
}

lsp_object! {
    /// Completion request options.
    CompletionOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["Characters that automatically trigger completion."]
        opt trigger_characters: Vec<String> => "triggerCharacters",
        ["Characters that commit a completion when no per-item set is given."]
        opt all_commit_characters: Vec<String> => "allCommitCharacters",
        ["Whether the server resolves additional completion-item information."]
        opt resolve_provider: bool => "resolveProvider",
        ["Server support for `CompletionItem`-specific capabilities."]
        opt completion_item: ServerCompletionItemOptions => "completionItem",
    }
}

lsp_object! {
    /// Signature-help request options.
    SignatureHelpOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["Characters that automatically trigger signature help."]
        opt trigger_characters: Vec<String> => "triggerCharacters",
        ["Characters that re-trigger signature help while it is showing."]
        opt retrigger_characters: Vec<String> => "retriggerCharacters",
    }
}

lsp_object! {
    /// Goto-definition request options.
    DefinitionOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options! {
    /// A union of a boolean or [`DefinitionOptions`].
    // Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrDefinitionOptions
    BooleanOrDefinitionOptions,
    definition_options: DefinitionOptions,
    "a boolean or a definition-options object"
}

lsp_object! {
    /// Find-references request options.
    ReferenceOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options! {
    /// A union of a boolean or [`ReferenceOptions`].
    // Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrReferenceOptions
    BooleanOrReferenceOptions,
    reference_options: ReferenceOptions,
    "a boolean or a reference-options object"
}

lsp_object! {
    /// Document-symbol request options.
    DocumentSymbolOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["A human-readable label shown when multiple outlines exist."]
        opt label: String => "label",
    }
}

boolean_or_options! {
    /// A union of a boolean or [`DocumentSymbolOptions`].
    // Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrDocumentSymbolOptions
    BooleanOrDocumentSymbolOptions,
    document_symbol_options: DocumentSymbolOptions,
    "a boolean or a document-symbol-options object"
}

lsp_object! {
    /// Code-action request options.
    CodeActionOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["The `CodeActionKind`s this server may return."]
        opt code_action_kinds: Vec<CodeActionKind> => "codeActionKinds",
        ["Static documentation for a class of code actions (deferred: raw JSON)."]
        // DEFER: `[]*CodeActionKindDocumentation` is a proposed/rare nested type;
        // kept as raw JSON. blocked-by: generator pass landing CodeActionKindDocumentation.
        opt documentation: serde_json::Value => "documentation",
        ["Whether the server resolves additional code-action information."]
        opt resolve_provider: bool => "resolveProvider",
    }
}

boolean_or_options! {
    /// A union of a boolean or [`CodeActionOptions`].
    // Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrCodeActionOptions
    BooleanOrCodeActionOptions,
    code_action_options: CodeActionOptions,
    "a boolean or a code-action-options object"
}

lsp_object! {
    /// Document-formatting request options.
    DocumentFormattingOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options! {
    /// A union of a boolean or [`DocumentFormattingOptions`].
    // Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrDocumentFormattingOptions
    BooleanOrDocumentFormattingOptions,
    document_formatting_options: DocumentFormattingOptions,
    "a boolean or a document-formatting-options object"
}

lsp_object! {
    /// Rename request options.
    RenameOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["Whether renames are checked/tested via a prepare step before execution."]
        opt prepare_provider: bool => "prepareProvider",
    }
}

boolean_or_options! {
    /// A union of a boolean or [`RenameOptions`].
    // Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrRenameOptions
    BooleanOrRenameOptions,
    rename_options: RenameOptions,
    "a boolean or a rename-options object"
}

lsp_object! {
    /// Workspace-symbol request options.
    WorkspaceSymbolOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["Whether the server resolves additional workspace-symbol information."]
        opt resolve_provider: bool => "resolveProvider",
    }
}

boolean_or_options! {
    /// A union of a boolean or [`WorkspaceSymbolOptions`].
    // Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrWorkspaceSymbolOptions
    BooleanOrWorkspaceSymbolOptions,
    workspace_symbol_options: WorkspaceSymbolOptions,
    "a boolean or a workspace-symbol-options object"
}

lsp_object! {
    /// The legend a server uses to encode semantic tokens.
    SemanticTokensLegend {
        ["The token types a server uses."]
        reqnn token_types: Vec<String> => "tokenTypes",
        ["The token modifiers a server uses."]
        reqnn token_modifiers: Vec<String> => "tokenModifiers",
    }
}

lsp_object! {
    /// Semantic-tokens options supporting deltas for full documents.
    SemanticTokensFullDelta {
        ["Whether the server supports deltas for full documents."]
        opt delta: bool => "delta",
    }
}

/// A union of a boolean or [`SemanticTokensFullDelta`]
/// (LSP `boolean | SemanticTokensFullDelta`).
// Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrSemanticTokensFullDelta
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BooleanOrSemanticTokensFullDelta {
    /// The boolean variant.
    pub boolean: Option<bool>,
    /// The full-delta options variant.
    pub semantic_tokens_full_delta: Option<SemanticTokensFullDelta>,
}

impl Serialize for BooleanOrSemanticTokensFullDelta {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (self.boolean, &self.semantic_tokens_full_delta) {
            (Some(b), None) => serializer.serialize_bool(b),
            (None, Some(o)) => o.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of BooleanOrSemanticTokensFullDelta should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for BooleanOrSemanticTokensFullDelta {
    // Go: lsp_generated.go:BooleanOrSemanticTokensFullDelta.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = BooleanOrSemanticTokensFullDelta;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a boolean or a semantic-tokens full-delta object")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(BooleanOrSemanticTokensFullDelta {
                    boolean: Some(v),
                    semantic_tokens_full_delta: None,
                })
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                let opts = SemanticTokensFullDelta::deserialize(
                    de::value::MapAccessDeserializer::new(map),
                )?;
                Ok(BooleanOrSemanticTokensFullDelta {
                    boolean: None,
                    semantic_tokens_full_delta: Some(opts),
                })
            }
        }
        deserializer.deserialize_any(V)
    }
}

lsp_object! {
    /// Semantic-tokens request options.
    SemanticTokensOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["The legend used by the server."]
        reqnn legend: SemanticTokensLegend => "legend",
        ["Whether the server provides range semantic tokens."]
        opt range: BooleanOrEmptyObject => "range",
        ["Whether the server provides full-document semantic tokens."]
        opt full: BooleanOrSemanticTokensFullDelta => "full",
    }
}

/// A union of [`SemanticTokensOptions`] or [`SemanticTokensRegistrationOptions`]
/// (LSP `SemanticTokensOptions | SemanticTokensRegistrationOptions`).
///
/// The registration-options variant (an object carrying a `documentSelector`)
/// decodes into the typed [`SemanticTokensRegistrationOptions`]; the plain
/// options variant decodes into [`SemanticTokensOptions`].
// Go: internal/lsp/lsproto/lsp_generated.go:SemanticTokensOptionsOrRegistrationOptions
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SemanticTokensOptionsOrRegistrationOptions {
    /// The plain options variant.
    pub options: Option<SemanticTokensOptions>,
    /// The typed registration-options variant.
    pub registration_options: Option<SemanticTokensRegistrationOptions>,
}

impl Serialize for SemanticTokensOptionsOrRegistrationOptions {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (&self.options, &self.registration_options) {
            (Some(o), None) => o.serialize(serializer),
            (None, Some(r)) => r.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of SemanticTokensOptionsOrRegistrationOptions should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for SemanticTokensOptionsOrRegistrationOptions {
    // Go: lsp_generated.go:SemanticTokensOptionsOrRegistrationOptions.UnmarshalJSONFrom
    //
    // Go dispatches on the presence of a `documentSelector` key: present means
    // the registration-options variant, otherwise the plain options variant.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let mut out = SemanticTokensOptionsOrRegistrationOptions::default();
        if value.get("documentSelector").is_some() {
            out.registration_options =
                Some(serde_json::from_value(value).map_err(de::Error::custom)?);
        } else {
            out.options = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
        }
        Ok(out)
    }
}

lsp_object! {
    /// Execute-command request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:ExecuteCommandOptions
    ExecuteCommandOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["The commands to be executed on the server."]
        reqnn commands: Vec<String> => "commands",
    }
}

lsp_object! {
    /// On-type formatting request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentOnTypeFormattingOptions
    DocumentOnTypeFormattingOptions {
        ["A character on which formatting should be triggered, like `{`."]
        req first_trigger_character: String => "firstTriggerCharacter",
        ["More trigger characters."]
        opt more_trigger_character: Vec<String> => "moreTriggerCharacter",
    }
}

lsp_object! {
    /// Code-lens request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:CodeLensOptions
    CodeLensOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["Whether code lens has a resolve provider as well."]
        opt resolve_provider: bool => "resolveProvider",
    }
}

lsp_object! {
    /// Document-link request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentLinkOptions
    DocumentLinkOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["Whether document links have a resolve provider as well."]
        opt resolve_provider: bool => "resolveProvider",
    }
}

lsp_object! {
    /// VS auto-insert request options (`textDocument/_vs_onAutoInsert`).
    // Go: internal/lsp/lsproto/lsp_generated.go:VsOnAutoInsertOptions
    VsOnAutoInsertOptions {
        ["List of trigger characters that trigger auto-insert."]
        reqnn vs_trigger_characters: Vec<String> => "_vs_triggerCharacters",
    }
}

lsp_object! {
    /// Document-highlight request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentHighlightOptions
    DocumentHighlightOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options! {
    /// A union of a boolean or [`DocumentHighlightOptions`].
    // Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrDocumentHighlightOptions
    BooleanOrDocumentHighlightOptions,
    document_highlight_options: DocumentHighlightOptions,
    "a boolean or a document-highlight-options object"
}

lsp_object! {
    /// Document-range-formatting request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentRangeFormattingOptions
    DocumentRangeFormattingOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["Whether the server supports formatting multiple ranges at once."]
        opt ranges_support: bool => "rangesSupport",
    }
}

boolean_or_options! {
    /// A union of a boolean or [`DocumentRangeFormattingOptions`].
    // Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrDocumentRangeFormattingOptions
    BooleanOrDocumentRangeFormattingOptions,
    document_range_formatting_options: DocumentRangeFormattingOptions,
    "a boolean or a document-range-formatting-options object"
}

lsp_object! {
    /// Inline-completion request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:InlineCompletionOptions
    InlineCompletionOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options! {
    /// A union of a boolean or [`InlineCompletionOptions`].
    // Go: internal/lsp/lsproto/lsp_generated.go:BooleanOrInlineCompletionOptions
    BooleanOrInlineCompletionOptions,
    inline_completion_options: InlineCompletionOptions,
    "a boolean or an inline-completion-options object"
}

lsp_object! {
    /// Pull-model diagnostic request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:DiagnosticOptions
    DiagnosticOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["An optional identifier under which diagnostics are managed by the client."]
        opt identifier: String => "identifier",
        ["Whether the language has inter-file dependencies."]
        req inter_file_dependencies: bool => "interFileDependencies",
        ["Whether the server provides support for workspace diagnostics as well."]
        req workspace_diagnostics: bool => "workspaceDiagnostics",
    }
}

// === registration-options base tree ===
//
// The shared building blocks every `*RegistrationOptions` is composed from:
// `StaticRegistrationOptions` (the `id` field), `DocumentSelectorOrNull` (the
// `documentSelector` field, a `[]DocumentFilter | null`), and
// `TextDocumentRegistrationOptions`. The Go meta-model flattens the embedded
// `StaticRegistrationOptions` / `TextDocumentRegistrationOptions` /
// `<Feature>Options` into each concrete `*RegistrationOptions`, so the ported
// structs are flat too (field order mirrors the Go declaration order).

lsp_object! {
    /// A workspace folder inside the server (LSP `WorkspaceFolder`); also the
    /// object arm of [`WorkspaceFolderOrURI`].
    // Go: internal/lsp/lsproto/lsp_generated.go:WorkspaceFolder
    WorkspaceFolder {
        ["The associated URI for this workspace folder."]
        req uri: crate::URI => "uri",
        ["The name of the workspace folder, used to refer to it in the UI."]
        req name: String => "name",
    }
}

/// A union of an array of [`WorkspaceFolder`] or `null`
/// (LSP `WorkspaceFolder[] | null`), the type of the [`InitializeParams`]
/// `workspaceFolders`. `None` represents JSON `null`.
// Go: internal/lsp/lsproto/lsp_generated.go:WorkspaceFoldersOrNull
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkspaceFoldersOrNull {
    /// The folder-array variant; `None` represents JSON `null`.
    pub workspace_folders: Option<Vec<WorkspaceFolder>>,
}

impl Serialize for WorkspaceFoldersOrNull {
    // Go: internal/lsp/lsproto/lsp_generated.go:WorkspaceFoldersOrNull.MarshalJSONTo
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.workspace_folders {
            Some(folders) => folders.serialize(serializer),
            None => serializer.serialize_none(),
        }
    }
}

impl<'de> Deserialize<'de> for WorkspaceFoldersOrNull {
    // Go: internal/lsp/lsproto/lsp_generated.go:WorkspaceFoldersOrNull.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = WorkspaceFoldersOrNull;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an array of workspace folders or null")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
                let folders = Vec::<WorkspaceFolder>::deserialize(
                    de::value::SeqAccessDeserializer::new(seq),
                )?;
                Ok(WorkspaceFoldersOrNull {
                    workspace_folders: Some(folders),
                })
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(WorkspaceFoldersOrNull {
                    workspace_folders: None,
                })
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(WorkspaceFoldersOrNull {
                    workspace_folders: None,
                })
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// A union of a [`WorkspaceFolder`] object or a bare `URI` string (LSP
/// `WorkspaceFolder | URI`), used as the `baseUri` of a [`RelativePattern`].
///
/// Exactly one field is set; mirrors Go's pointer-pair representation. On
/// deserialize, a JSON object yields [`WorkspaceFolderOrURI::workspace_folder`]
/// and a JSON string yields [`WorkspaceFolderOrURI::uri`]; any other kind is an
/// error.
// Go: internal/lsp/lsproto/lsp_generated.go:WorkspaceFolderOrURI
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkspaceFolderOrURI {
    /// The `WorkspaceFolder` object variant.
    pub workspace_folder: Option<WorkspaceFolder>,
    /// The bare `URI` string variant.
    pub uri: Option<crate::URI>,
}

impl Serialize for WorkspaceFolderOrURI {
    // Go: lsp_generated.go:WorkspaceFolderOrURI.MarshalJSONTo
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (&self.workspace_folder, &self.uri) {
            (Some(f), None) => f.serialize(serializer),
            (None, Some(u)) => u.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of WorkspaceFolderOrURI should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for WorkspaceFolderOrURI {
    // Go: lsp_generated.go:WorkspaceFolderOrURI.UnmarshalJSONFrom
    //
    // Go peeks the JSON kind: a `{` object is a WorkspaceFolder, a `"` string is
    // a bare URI; any other kind is an error.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let mut out = WorkspaceFolderOrURI::default();
        if value.is_object() {
            out.workspace_folder = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
        } else if let Some(s) = value.as_str() {
            out.uri = Some(crate::URI(s.to_string()));
        } else {
            return Err(de::Error::custom(
                "WorkspaceFolderOrURI: expected an object or a string",
            ));
        }
        Ok(out)
    }
}

lsp_object! {
    /// A relative glob pattern resolved against a base `WorkspaceFolder`/`URI`
    /// (LSP `RelativePattern`); the object arm of [`PatternOrRelativePattern`].
    // Go: internal/lsp/lsproto/lsp_generated.go:RelativePattern
    RelativePattern {
        ["The workspace folder or base URI this pattern is matched against."]
        req base_uri: WorkspaceFolderOrURI => "baseUri",
        ["The actual glob pattern."]
        req pattern: String => "pattern",
    }
}

/// A union of a plain glob `pattern` string or a [`RelativePattern`]
/// (LSP `Pattern | RelativePattern`). Exactly one variant is set.
// Go: internal/lsp/lsproto/lsp_generated.go:PatternOrRelativePattern
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PatternOrRelativePattern {
    /// The plain glob-pattern string variant.
    pub pattern: Option<String>,
    /// The `RelativePattern` object variant.
    pub relative_pattern: Option<RelativePattern>,
}

impl Serialize for PatternOrRelativePattern {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (&self.pattern, &self.relative_pattern) {
            (Some(p), None) => serializer.serialize_str(p),
            (None, Some(r)) => r.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of PatternOrRelativePattern should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for PatternOrRelativePattern {
    // Go: lsp_generated.go:PatternOrRelativePattern.UnmarshalJSONFrom
    //
    // Go peeks the JSON kind: a `"` string is the glob pattern, a `{` object is
    // a RelativePattern.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let mut out = PatternOrRelativePattern::default();
        if let Some(s) = value.as_str() {
            out.pattern = Some(s.to_string());
        } else if value.is_object() {
            out.relative_pattern = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
        } else {
            return Err(de::Error::custom(
                "PatternOrRelativePattern: expected a string or an object",
            ));
        }
        Ok(out)
    }
}

lsp_object! {
    /// Static registration options, mixed into a `*RegistrationOptions` to carry
    /// the registration `id` (used to later deregister the request).
    // Go: internal/lsp/lsproto/lsp_generated.go:StaticRegistrationOptions
    StaticRegistrationOptions {
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

lsp_object! {
    /// A document filter where `language` is the required field (one variant of
    /// the `DocumentFilter` union).
    // Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentFilterLanguage
    TextDocumentFilterLanguage {
        ["A language id, like `typescript`."]
        req language: String => "language",
        ["A Uri scheme, like `file` or `untitled`."]
        opt scheme: String => "scheme",
        ["A glob pattern, like `**/*.{ts,js}`."]
        opt pattern: PatternOrRelativePattern => "pattern",
    }
}

lsp_object! {
    /// A document filter where `scheme` is the required field (one variant of
    /// the `DocumentFilter` union).
    // Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentFilterScheme
    TextDocumentFilterScheme {
        ["A language id, like `typescript`."]
        opt language: String => "language",
        ["A Uri scheme, like `file` or `untitled`."]
        req scheme: String => "scheme",
        ["A glob pattern, like `**/*.{ts,js}`."]
        opt pattern: PatternOrRelativePattern => "pattern",
    }
}

lsp_object! {
    /// A document filter where `pattern` is the required field (one variant of
    /// the `DocumentFilter` union).
    // Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentFilterPattern
    TextDocumentFilterPattern {
        ["A language id, like `typescript`."]
        opt language: String => "language",
        ["A Uri scheme, like `file` or `untitled`."]
        opt scheme: String => "scheme",
        ["A glob pattern, like `**/*.{ts,js}`."]
        req pattern: PatternOrRelativePattern => "pattern",
    }
}

/// A `DocumentFilter`: the element type of a `DocumentSelector`, a union of the
/// three `TextDocumentFilter*` variants (LSP `TextDocumentFilter`).
///
/// On decode, the variants are tried in declaration order (language, scheme,
/// pattern) and the first whose required discriminator field is present wins,
/// mirroring the Go `json.Unmarshal`-and-fall-through logic. Exactly one variant
/// is set.
// Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentFilterLanguageOrSchemeOrPattern
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextDocumentFilterLanguageOrSchemeOrPattern {
    /// The `language`-required variant.
    pub language: Option<TextDocumentFilterLanguage>,
    /// The `scheme`-required variant.
    pub scheme: Option<TextDocumentFilterScheme>,
    /// The `pattern`-required variant.
    pub pattern: Option<TextDocumentFilterPattern>,
}

impl Serialize for TextDocumentFilterLanguageOrSchemeOrPattern {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (&self.language, &self.scheme, &self.pattern) {
            (Some(l), None, None) => l.serialize(serializer),
            (None, Some(s), None) => s.serialize(serializer),
            (None, None, Some(p)) => p.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of TextDocumentFilterLanguageOrSchemeOrPattern should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for TextDocumentFilterLanguageOrSchemeOrPattern {
    // Go: lsp_generated.go:TextDocumentFilterLanguageOrSchemeOrPattern.UnmarshalJSONFrom
    //
    // Go reads the raw value once and tries to decode it as each variant in
    // order, keeping the first that succeeds.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let mut out = TextDocumentFilterLanguageOrSchemeOrPattern::default();
        if let Ok(v) = serde_json::from_value::<TextDocumentFilterLanguage>(value.clone()) {
            out.language = Some(v);
        } else if let Ok(v) = serde_json::from_value::<TextDocumentFilterScheme>(value.clone()) {
            out.scheme = Some(v);
        } else if let Ok(v) = serde_json::from_value::<TextDocumentFilterPattern>(value) {
            out.pattern = Some(v);
        } else {
            return Err(de::Error::custom(
                "TextDocumentFilterLanguageOrSchemeOrPattern: no variant matched",
            ));
        }
        Ok(out)
    }
}

/// A `DocumentSelector` or `null` (LSP `DocumentSelector | null`).
///
/// `document_selector` is the present `[]DocumentFilter`; `None` serializes as
/// JSON `null` (the `*RegistrationOptions` convention meaning "use the
/// client-provided selector"). A nil selector in Go marshals to `null`, so the
/// default value serializes to `null` (not `[]`).
// Go: internal/lsp/lsproto/lsp_generated.go:DocumentSelectorOrNull
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocumentSelectorOrNull {
    /// The present selector (`Some`) or `null` (`None`).
    pub document_selector: Option<Vec<TextDocumentFilterLanguageOrSchemeOrPattern>>,
}

impl Serialize for DocumentSelectorOrNull {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.document_selector {
            Some(sel) => sel.serialize(serializer),
            None => serializer.serialize_none(),
        }
    }
}

impl<'de> Deserialize<'de> for DocumentSelectorOrNull {
    // Go: lsp_generated.go:DocumentSelectorOrNull.UnmarshalJSONFrom
    //
    // Go peeks the JSON kind: `null` leaves the pointer nil, `[` decodes the
    // selector array, anything else is an error.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let mut out = DocumentSelectorOrNull::default();
        if value.is_null() {
            // Leave `document_selector` as `None`.
        } else if value.is_array() {
            out.document_selector = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
        } else {
            return Err(de::Error::custom(
                "DocumentSelectorOrNull: expected an array or null",
            ));
        }
        Ok(out)
    }
}

lsp_object! {
    /// General text-document registration options: the `documentSelector` that
    /// scopes a dynamic registration. Mixed (flattened) into every concrete
    /// `*RegistrationOptions`.
    // Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentRegistrationOptions
    TextDocumentRegistrationOptions {
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
    }
}

// The concrete `*RegistrationOptions` for the `boolean | <Options> |
// <RegistrationOptions>` providers. Each flattens its feature `<Options>`
// (`workDoneProgress`), `TextDocumentRegistrationOptions` (`documentSelector`)
// and `StaticRegistrationOptions` (`id`); the field declaration order mirrors
// the Go struct exactly (it differs across these types, so serialization stays
// byte-for-byte identical to Go).

lsp_object! {
    /// Registration options for goto-declaration.
    // Go: internal/lsp/lsproto/lsp_generated.go:DeclarationRegistrationOptions
    DeclarationRegistrationOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

lsp_object! {
    /// Registration options for goto-type-definition.
    // Go: internal/lsp/lsproto/lsp_generated.go:TypeDefinitionRegistrationOptions
    TypeDefinitionRegistrationOptions {
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

lsp_object! {
    /// Registration options for goto-implementation.
    // Go: internal/lsp/lsproto/lsp_generated.go:ImplementationRegistrationOptions
    ImplementationRegistrationOptions {
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

lsp_object! {
    /// Registration options for document color.
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentColorRegistrationOptions
    DocumentColorRegistrationOptions {
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

lsp_object! {
    /// Registration options for folding range.
    // Go: internal/lsp/lsproto/lsp_generated.go:FoldingRangeRegistrationOptions
    FoldingRangeRegistrationOptions {
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

lsp_object! {
    /// Registration options for selection range.
    // Go: internal/lsp/lsproto/lsp_generated.go:SelectionRangeRegistrationOptions
    SelectionRangeRegistrationOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

lsp_object! {
    /// Registration options for call hierarchy.
    // Go: internal/lsp/lsproto/lsp_generated.go:CallHierarchyRegistrationOptions
    CallHierarchyRegistrationOptions {
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

lsp_object! {
    /// Registration options for linked editing range.
    // Go: internal/lsp/lsproto/lsp_generated.go:LinkedEditingRangeRegistrationOptions
    LinkedEditingRangeRegistrationOptions {
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

lsp_object! {
    /// Registration options for moniker (note: no `id` field in the Go model).
    // Go: internal/lsp/lsproto/lsp_generated.go:MonikerRegistrationOptions
    MonikerRegistrationOptions {
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

lsp_object! {
    /// Registration options for type hierarchy.
    // Go: internal/lsp/lsproto/lsp_generated.go:TypeHierarchyRegistrationOptions
    TypeHierarchyRegistrationOptions {
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

lsp_object! {
    /// Registration options for inline value.
    // Go: internal/lsp/lsproto/lsp_generated.go:InlineValueRegistrationOptions
    InlineValueRegistrationOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

lsp_object! {
    /// Registration options for inlay hint (adds `resolveProvider`).
    // Go: internal/lsp/lsproto/lsp_generated.go:InlayHintRegistrationOptions
    InlayHintRegistrationOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["Whether the server resolves additional inlay-hint information."]
        opt resolve_provider: bool => "resolveProvider",
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

lsp_object! {
    /// Registration options for pull-model diagnostics (flattens `DiagnosticOptions`
    /// whose required non-pointer bools are always serialized).
    // Go: internal/lsp/lsproto/lsp_generated.go:DiagnosticRegistrationOptions
    DiagnosticRegistrationOptions {
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["An optional identifier under which diagnostics are managed by the client."]
        opt identifier: String => "identifier",
        ["Whether the language has inter-file dependencies."]
        req inter_file_dependencies: bool => "interFileDependencies",
        ["Whether the server provides support for workspace diagnostics as well."]
        req workspace_diagnostics: bool => "workspaceDiagnostics",
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

lsp_object! {
    /// Registration options for semantic tokens (flattens `SemanticTokensOptions`
    /// including the required `legend`).
    // Go: internal/lsp/lsproto/lsp_generated.go:SemanticTokensRegistrationOptions
    SemanticTokensRegistrationOptions {
        ["A document selector to scope the registration (null = use the client's)."]
        req document_selector: DocumentSelectorOrNull => "documentSelector",
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["The legend used by the server."]
        reqnn legend: SemanticTokensLegend => "legend",
        ["Whether the server provides range semantic tokens."]
        opt range: BooleanOrEmptyObject => "range",
        ["Whether the server provides full-document semantic tokens."]
        opt full: BooleanOrSemanticTokensFullDelta => "full",
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

lsp_object! {
    /// Goto-declaration request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:DeclarationOptions
    DeclarationOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options_or_registration! {
    /// A triple union of a boolean, [`DeclarationOptions`], or registration options.
    // Go: lsp_generated.go:BooleanOrDeclarationOptionsOrDeclarationRegistrationOptions
    BooleanOrDeclarationOptionsOrDeclarationRegistrationOptions,
    declaration_options: DeclarationOptions,
    registration: DeclarationRegistrationOptions,
    "a boolean, declaration options, or declaration registration options"
}

lsp_object! {
    /// Goto-type-definition request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:TypeDefinitionOptions
    TypeDefinitionOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options_or_registration! {
    /// A triple union of a boolean, [`TypeDefinitionOptions`], or registration options.
    // Go: lsp_generated.go:BooleanOrTypeDefinitionOptionsOrTypeDefinitionRegistrationOptions
    BooleanOrTypeDefinitionOptionsOrTypeDefinitionRegistrationOptions,
    type_definition_options: TypeDefinitionOptions,
    registration: TypeDefinitionRegistrationOptions,
    "a boolean, type-definition options, or type-definition registration options"
}

lsp_object! {
    /// Goto-implementation request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:ImplementationOptions
    ImplementationOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options_or_registration! {
    /// A triple union of a boolean, [`ImplementationOptions`], or registration options.
    // Go: lsp_generated.go:BooleanOrImplementationOptionsOrImplementationRegistrationOptions
    BooleanOrImplementationOptionsOrImplementationRegistrationOptions,
    implementation_options: ImplementationOptions,
    registration: ImplementationRegistrationOptions,
    "a boolean, implementation options, or implementation registration options"
}

lsp_object! {
    /// Document-color request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:DocumentColorOptions
    DocumentColorOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options_or_registration! {
    /// A triple union of a boolean, [`DocumentColorOptions`], or registration options.
    // Go: lsp_generated.go:BooleanOrDocumentColorOptionsOrDocumentColorRegistrationOptions
    BooleanOrDocumentColorOptionsOrDocumentColorRegistrationOptions,
    document_color_options: DocumentColorOptions,
    registration: DocumentColorRegistrationOptions,
    "a boolean, document-color options, or document-color registration options"
}

lsp_object! {
    /// Folding-range request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:FoldingRangeOptions
    FoldingRangeOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options_or_registration! {
    /// A triple union of a boolean, [`FoldingRangeOptions`], or registration options.
    // Go: lsp_generated.go:BooleanOrFoldingRangeOptionsOrFoldingRangeRegistrationOptions
    BooleanOrFoldingRangeOptionsOrFoldingRangeRegistrationOptions,
    folding_range_options: FoldingRangeOptions,
    registration: FoldingRangeRegistrationOptions,
    "a boolean, folding-range options, or folding-range registration options"
}

lsp_object! {
    /// Selection-range request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:SelectionRangeOptions
    SelectionRangeOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options_or_registration! {
    /// A triple union of a boolean, [`SelectionRangeOptions`], or registration options.
    // Go: lsp_generated.go:BooleanOrSelectionRangeOptionsOrSelectionRangeRegistrationOptions
    BooleanOrSelectionRangeOptionsOrSelectionRangeRegistrationOptions,
    selection_range_options: SelectionRangeOptions,
    registration: SelectionRangeRegistrationOptions,
    "a boolean, selection-range options, or selection-range registration options"
}

lsp_object! {
    /// Call-hierarchy request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:CallHierarchyOptions
    CallHierarchyOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options_or_registration! {
    /// A triple union of a boolean, [`CallHierarchyOptions`], or registration options.
    // Go: lsp_generated.go:BooleanOrCallHierarchyOptionsOrCallHierarchyRegistrationOptions
    BooleanOrCallHierarchyOptionsOrCallHierarchyRegistrationOptions,
    call_hierarchy_options: CallHierarchyOptions,
    registration: CallHierarchyRegistrationOptions,
    "a boolean, call-hierarchy options, or call-hierarchy registration options"
}

lsp_object! {
    /// Linked-editing-range request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:LinkedEditingRangeOptions
    LinkedEditingRangeOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options_or_registration! {
    /// A triple union of a boolean, [`LinkedEditingRangeOptions`], or registration options.
    // Go: lsp_generated.go:BooleanOrLinkedEditingRangeOptionsOrLinkedEditingRangeRegistrationOptions
    BooleanOrLinkedEditingRangeOptionsOrLinkedEditingRangeRegistrationOptions,
    linked_editing_range_options: LinkedEditingRangeOptions,
    registration: LinkedEditingRangeRegistrationOptions,
    "a boolean, linked-editing-range options, or linked-editing-range registration options"
}

lsp_object! {
    /// Moniker request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:MonikerOptions
    MonikerOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options_or_registration! {
    /// A triple union of a boolean, [`MonikerOptions`], or registration options.
    // Go: lsp_generated.go:BooleanOrMonikerOptionsOrMonikerRegistrationOptions
    BooleanOrMonikerOptionsOrMonikerRegistrationOptions,
    moniker_options: MonikerOptions,
    registration: MonikerRegistrationOptions,
    "a boolean, moniker options, or moniker registration options"
}

lsp_object! {
    /// Type-hierarchy request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:TypeHierarchyOptions
    TypeHierarchyOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options_or_registration! {
    /// A triple union of a boolean, [`TypeHierarchyOptions`], or registration options.
    // Go: lsp_generated.go:BooleanOrTypeHierarchyOptionsOrTypeHierarchyRegistrationOptions
    BooleanOrTypeHierarchyOptionsOrTypeHierarchyRegistrationOptions,
    type_hierarchy_options: TypeHierarchyOptions,
    registration: TypeHierarchyRegistrationOptions,
    "a boolean, type-hierarchy options, or type-hierarchy registration options"
}

lsp_object! {
    /// Inline-value request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:InlineValueOptions
    InlineValueOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

boolean_or_options_or_registration! {
    /// A triple union of a boolean, [`InlineValueOptions`], or registration options.
    // Go: lsp_generated.go:BooleanOrInlineValueOptionsOrInlineValueRegistrationOptions
    BooleanOrInlineValueOptionsOrInlineValueRegistrationOptions,
    inline_value_options: InlineValueOptions,
    registration: InlineValueRegistrationOptions,
    "a boolean, inline-value options, or inline-value registration options"
}

lsp_object! {
    /// Inlay-hint request options.
    // Go: internal/lsp/lsproto/lsp_generated.go:InlayHintOptions
    InlayHintOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
        ["Whether the server resolves additional inlay-hint information."]
        opt resolve_provider: bool => "resolveProvider",
    }
}

boolean_or_options_or_registration! {
    /// A triple union of a boolean, [`InlayHintOptions`], or registration options.
    // Go: lsp_generated.go:BooleanOrInlayHintOptionsOrInlayHintRegistrationOptions
    BooleanOrInlayHintOptionsOrInlayHintRegistrationOptions,
    inlay_hint_options: InlayHintOptions,
    registration: InlayHintRegistrationOptions,
    "a boolean, inlay-hint options, or inlay-hint registration options"
}

/// A union of [`DiagnosticOptions`] or [`DiagnosticRegistrationOptions`]
/// (LSP `DiagnosticOptions | DiagnosticRegistrationOptions`).
///
/// The registration-options variant (an object carrying a `documentSelector`)
/// decodes into the typed [`DiagnosticRegistrationOptions`]; the plain options
/// variant decodes into [`DiagnosticOptions`].
// Go: internal/lsp/lsproto/lsp_generated.go:DiagnosticOptionsOrRegistrationOptions
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiagnosticOptionsOrRegistrationOptions {
    /// The plain options variant.
    pub options: Option<DiagnosticOptions>,
    /// The typed registration-options variant.
    pub registration_options: Option<DiagnosticRegistrationOptions>,
}

impl Serialize for DiagnosticOptionsOrRegistrationOptions {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (&self.options, &self.registration_options) {
            (Some(o), None) => o.serialize(serializer),
            (None, Some(r)) => r.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of DiagnosticOptionsOrRegistrationOptions should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for DiagnosticOptionsOrRegistrationOptions {
    // Go: lsp_generated.go:DiagnosticOptionsOrRegistrationOptions.UnmarshalJSONFrom
    //
    // Go dispatches on the presence of a `documentSelector` key: present means
    // the registration-options variant, otherwise the plain options variant.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let mut out = DiagnosticOptionsOrRegistrationOptions::default();
        if value.get("documentSelector").is_some() {
            out.registration_options =
                Some(serde_json::from_value(value).map_err(de::Error::custom)?);
        } else {
            out.options = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
        }
        Ok(out)
    }
}

// === WorkspaceOptions subtree (the `ServerCapabilities.workspace` field) ===
//
// The workspace-specific server capabilities tree: `WorkspaceOptions` and its
// `workspaceFolders` / `fileOperations` / `textDocumentContent` members. Each
// struct mirrors the Go declaration (field names, optionality, order) so the
// (de)serialization is byte-for-byte compatible.

/// A union of a string or a boolean (LSP `string | boolean`).
///
/// Exactly one field is set; mirrors Go's pointer-pair representation. On
/// deserialize, a JSON string yields [`StringOrBoolean::string`] and a JSON
/// boolean yields [`StringOrBoolean::boolean`]; any other kind is an error.
///
/// # Examples
/// ```
/// let v: tsgo_lsproto::StringOrBoolean = serde_json::from_str("\"id-1\"").unwrap();
/// assert_eq!(v.string.as_deref(), Some("id-1"));
/// ```
// Go: internal/lsp/lsproto/lsp_generated.go:StringOrBoolean
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StringOrBoolean {
    /// The string variant, if the value was a JSON string.
    pub string: Option<String>,
    /// The boolean variant, if the value was a JSON boolean.
    pub boolean: Option<bool>,
}

impl Serialize for StringOrBoolean {
    // Go: lsp_generated.go:StringOrBoolean.MarshalJSONTo
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (&self.string, self.boolean) {
            (Some(s), None) => serializer.serialize_str(s),
            (None, Some(b)) => serializer.serialize_bool(b),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of StringOrBoolean should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for StringOrBoolean {
    // Go: lsp_generated.go:StringOrBoolean.UnmarshalJSONFrom
    //
    // Go peeks the JSON kind: a `"` string fills `string`, a `t`/`f` boolean
    // fills `boolean`; any other kind is an error.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let mut out = StringOrBoolean::default();
        if let Some(s) = value.as_str() {
            out.string = Some(s.to_string());
        } else if let Some(b) = value.as_bool() {
            out.boolean = Some(b);
        } else {
            return Err(de::Error::custom(
                "StringOrBoolean: expected a string or a boolean",
            ));
        }
        Ok(out)
    }
}

lsp_object! {
    /// The server's support for workspace folders (LSP
    /// `WorkspaceFoldersServerCapabilities`).
    // Go: internal/lsp/lsproto/lsp_generated.go:WorkspaceFoldersServerCapabilities
    WorkspaceFoldersServerCapabilities {
        ["Whether the server has support for workspace folders."]
        opt supported: bool => "supported",
        ["Whether the server wants to receive workspace-folder change notifications (a registration id string or a boolean)."]
        opt change_notifications: StringOrBoolean => "changeNotifications",
    }
}

/// Whether a [`FileOperationPattern`] matches files or folders (LSP
/// `FileOperationPatternKind`, a string enum).
///
/// Mirrors Go `type FileOperationPatternKind string`. A [`Cow<'static, str>`]
/// newtype so the predefined kinds are `const`-constructible and unknown values
/// round-trip as their raw string; (de)serializes as a plain JSON string.
// Go: internal/lsp/lsproto/lsp_generated.go:FileOperationPatternKind
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct FileOperationPatternKind(pub Cow<'static, str>);

impl FileOperationPatternKind {
    /// The pattern matches a file only.
    pub const FILE: FileOperationPatternKind = FileOperationPatternKind(Cow::Borrowed("file"));
    /// The pattern matches a folder only.
    pub const FOLDER: FileOperationPatternKind = FileOperationPatternKind(Cow::Borrowed("folder"));
}

impl Serialize for FileOperationPatternKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for FileOperationPatternKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(FileOperationPatternKind(Cow::Owned(String::deserialize(
            deserializer,
        )?)))
    }
}

lsp_object! {
    /// Matching options for a [`FileOperationPattern`] (LSP
    /// `FileOperationPatternOptions`).
    // Go: internal/lsp/lsproto/lsp_generated.go:FileOperationPatternOptions
    FileOperationPatternOptions {
        ["Whether the pattern should be matched ignoring casing."]
        opt ignore_case: bool => "ignoreCase",
    }
}

lsp_object! {
    /// A glob pattern (with match kind/options) used by a [`FileOperationFilter`]
    /// (LSP `FileOperationPattern`).
    // Go: internal/lsp/lsproto/lsp_generated.go:FileOperationPattern
    FileOperationPattern {
        ["The glob pattern to match (e.g. `**/*.{ts,js}`)."]
        req glob: String => "glob",
        ["Whether to match files or folders with this pattern (both if absent)."]
        opt matches: FileOperationPatternKind => "matches",
        ["Additional options used during matching."]
        opt options: FileOperationPatternOptions => "options",
    }
}

lsp_object! {
    /// A filter to describe in which file-operation requests a server is
    /// interested (LSP `FileOperationFilter`).
    // Go: internal/lsp/lsproto/lsp_generated.go:FileOperationFilter
    FileOperationFilter {
        ["A Uri scheme like `file` or `untitled`."]
        opt scheme: String => "scheme",
        ["The actual file-operation pattern."]
        reqnn pattern: FileOperationPattern => "pattern",
    }
}

lsp_object! {
    /// The options to register for file operations (LSP
    /// `FileOperationRegistrationOptions`).
    // Go: internal/lsp/lsproto/lsp_generated.go:FileOperationRegistrationOptions
    FileOperationRegistrationOptions {
        ["The actual filters."]
        reqnn filters: Vec<FileOperationFilter> => "filters",
    }
}

lsp_object! {
    /// Options for the server's interest in file-operation notifications and
    /// requests (LSP `FileOperationOptions`).
    // Go: internal/lsp/lsproto/lsp_generated.go:FileOperationOptions
    FileOperationOptions {
        ["The server is interested in receiving `didCreateFiles` notifications."]
        opt did_create: FileOperationRegistrationOptions => "didCreate",
        ["The server is interested in receiving `willCreateFiles` requests."]
        opt will_create: FileOperationRegistrationOptions => "willCreate",
        ["The server is interested in receiving `didRenameFiles` notifications."]
        opt did_rename: FileOperationRegistrationOptions => "didRename",
        ["The server is interested in receiving `willRenameFiles` requests."]
        opt will_rename: FileOperationRegistrationOptions => "willRename",
        ["The server is interested in receiving `didDeleteFiles` notifications."]
        opt did_delete: FileOperationRegistrationOptions => "didDelete",
        ["The server is interested in receiving `willDeleteFiles` requests."]
        opt will_delete: FileOperationRegistrationOptions => "willDelete",
    }
}

lsp_object! {
    /// Options for the `workspace/textDocumentContent` request (LSP
    /// `TextDocumentContentOptions`).
    // Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentContentOptions
    TextDocumentContentOptions {
        ["The schemes for which the server provides content."]
        reqnn schemes: Vec<String> => "schemes",
    }
}

lsp_object! {
    /// Registration options for the `workspace/textDocumentContent` request (LSP
    /// `TextDocumentContentRegistrationOptions`).
    // Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentContentRegistrationOptions
    TextDocumentContentRegistrationOptions {
        ["The schemes for which the server provides content."]
        reqnn schemes: Vec<String> => "schemes",
        ["The id used to register the request (see `Registration#id`)."]
        opt id: String => "id",
    }
}

/// A union of [`TextDocumentContentOptions`] or
/// [`TextDocumentContentRegistrationOptions`] (LSP
/// `TextDocumentContentOptions | TextDocumentContentRegistrationOptions`).
///
/// On decode, the variants are tried in declaration order (plain options first,
/// then registration options), keeping the first that succeeds — mirroring the
/// Go `json.Unmarshal`-and-fall-through logic. Since the plain options variant
/// accepts any object carrying `schemes` (ignoring an extra `id`), it wins
/// whenever it decodes, exactly as in Go. Exactly one variant is set.
// Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentContentOptionsOrRegistrationOptions
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextDocumentContentOptionsOrRegistrationOptions {
    /// The plain options variant.
    pub options: Option<TextDocumentContentOptions>,
    /// The registration-options variant.
    pub registration_options: Option<TextDocumentContentRegistrationOptions>,
}

impl Serialize for TextDocumentContentOptionsOrRegistrationOptions {
    // Go: lsp_generated.go:TextDocumentContentOptionsOrRegistrationOptions.MarshalJSONTo
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (&self.options, &self.registration_options) {
            (Some(o), None) => o.serialize(serializer),
            (None, Some(r)) => r.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of TextDocumentContentOptionsOrRegistrationOptions should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for TextDocumentContentOptionsOrRegistrationOptions {
    // Go: lsp_generated.go:TextDocumentContentOptionsOrRegistrationOptions.UnmarshalJSONFrom
    //
    // Go reads the raw value once and tries to decode it as each variant in
    // order (options, then registration options), keeping the first success.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let mut out = TextDocumentContentOptionsOrRegistrationOptions::default();
        if let Ok(v) = serde_json::from_value::<TextDocumentContentOptions>(value.clone()) {
            out.options = Some(v);
        } else if let Ok(v) =
            serde_json::from_value::<TextDocumentContentRegistrationOptions>(value)
        {
            out.registration_options = Some(v);
        } else {
            return Err(de::Error::custom(
                "TextDocumentContentOptionsOrRegistrationOptions: no variant matched",
            ));
        }
        Ok(out)
    }
}

lsp_object! {
    /// Workspace-specific server capabilities (LSP `ServerCapabilities.workspace`,
    /// the `WorkspaceOptions` object).
    // Go: internal/lsp/lsproto/lsp_generated.go:WorkspaceOptions
    WorkspaceOptions {
        ["The server's support for workspace folders."]
        opt workspace_folders: WorkspaceFoldersServerCapabilities => "workspaceFolders",
        ["The server's interest in file-operation notifications/requests."]
        opt file_operations: FileOperationOptions => "fileOperations",
        ["The server's support for the `workspace/textDocumentContent` request."]
        opt text_document_content: TextDocumentContentOptionsOrRegistrationOptions => "textDocumentContent",
    }
}

lsp_object! {
    /// The capabilities the server provides (LSP `ServerCapabilities`).
    ///
    /// This is a *server-produced* value: every provider field is an optional
    /// pointer in Go (`json:",omitzero"`), so an absent provider is omitted on
    /// serialize and a `null` value is rejected on decode. Fields whose nested
    /// option type is not yet ported are modeled as raw JSON
    /// ([`serde_json::Value`]) with a `// DEFER` note, preserving the Go field
    /// name and optionality.
    // Go: internal/lsp/lsproto/lsp_generated.go:ServerCapabilities
    //
    // Field order mirrors the Go struct declaration so multi-field serialization
    // matches Go byte-for-byte. Providers whose nested option tree is not yet
    // ported keep the Go field name and optionality but carry a deferred raw
    // JSON ([`serde_json::Value`]) value (see the per-field `// DEFER` notes).
    ServerCapabilities {
        ["The position encoding the server picked (defaults to `utf-16`)."]
        opt position_encoding: PositionEncodingKind => "positionEncoding",
        ["Defines how text documents are synced (detailed options or a sync kind)."]
        opt text_document_sync: TextDocumentSyncOptionsOrKind => "textDocumentSync",
        ["The server provides completion support."]
        opt completion_provider: CompletionOptions => "completionProvider",
        ["The server provides hover support."]
        opt hover_provider: BooleanOrHoverOptions => "hoverProvider",
        ["The server provides signature-help support."]
        opt signature_help_provider: SignatureHelpOptions => "signatureHelpProvider",
        ["The server provides goto-declaration support."]
        opt declaration_provider: BooleanOrDeclarationOptionsOrDeclarationRegistrationOptions => "declarationProvider",
        ["The server provides goto-definition support."]
        opt definition_provider: BooleanOrDefinitionOptions => "definitionProvider",
        ["The server provides goto-type-definition support."]
        opt type_definition_provider: BooleanOrTypeDefinitionOptionsOrTypeDefinitionRegistrationOptions => "typeDefinitionProvider",
        ["The server provides goto-implementation support."]
        opt implementation_provider: BooleanOrImplementationOptionsOrImplementationRegistrationOptions => "implementationProvider",
        ["The server provides find-references support."]
        opt references_provider: BooleanOrReferenceOptions => "referencesProvider",
        ["The server provides document-highlight support."]
        opt document_highlight_provider: BooleanOrDocumentHighlightOptions => "documentHighlightProvider",
        ["The server provides document-symbol support."]
        opt document_symbol_provider: BooleanOrDocumentSymbolOptions => "documentSymbolProvider",
        ["The server provides code actions."]
        opt code_action_provider: BooleanOrCodeActionOptions => "codeActionProvider",
        ["The server provides code lens."]
        opt code_lens_provider: CodeLensOptions => "codeLensProvider",
        ["The server provides document-link support."]
        opt document_link_provider: DocumentLinkOptions => "documentLinkProvider",
        ["The server provides color support."]
        opt color_provider: BooleanOrDocumentColorOptionsOrDocumentColorRegistrationOptions => "colorProvider",
        ["The server provides workspace-symbol support."]
        opt workspace_symbol_provider: BooleanOrWorkspaceSymbolOptions => "workspaceSymbolProvider",
        ["The server provides document formatting."]
        opt document_formatting_provider: BooleanOrDocumentFormattingOptions => "documentFormattingProvider",
        ["The server provides document-range formatting."]
        opt document_range_formatting_provider: BooleanOrDocumentRangeFormattingOptions => "documentRangeFormattingProvider",
        ["The server provides on-type formatting."]
        opt document_on_type_formatting_provider: DocumentOnTypeFormattingOptions => "documentOnTypeFormattingProvider",
        ["The server provides rename support."]
        opt rename_provider: BooleanOrRenameOptions => "renameProvider",
        ["The server provides folding-range support."]
        opt folding_range_provider: BooleanOrFoldingRangeOptionsOrFoldingRangeRegistrationOptions => "foldingRangeProvider",
        ["The server provides selection-range support."]
        opt selection_range_provider: BooleanOrSelectionRangeOptionsOrSelectionRangeRegistrationOptions => "selectionRangeProvider",
        ["The server provides execute-command support."]
        opt execute_command_provider: ExecuteCommandOptions => "executeCommandProvider",
        ["The server provides call-hierarchy support."]
        opt call_hierarchy_provider: BooleanOrCallHierarchyOptionsOrCallHierarchyRegistrationOptions => "callHierarchyProvider",
        ["The server provides linked-editing-range support."]
        opt linked_editing_range_provider: BooleanOrLinkedEditingRangeOptionsOrLinkedEditingRangeRegistrationOptions => "linkedEditingRangeProvider",
        ["The server provides semantic-tokens support."]
        opt semantic_tokens_provider: SemanticTokensOptionsOrRegistrationOptions => "semanticTokensProvider",
        ["The server provides moniker support."]
        opt moniker_provider: BooleanOrMonikerOptionsOrMonikerRegistrationOptions => "monikerProvider",
        ["The server provides type-hierarchy support."]
        opt type_hierarchy_provider: BooleanOrTypeHierarchyOptionsOrTypeHierarchyRegistrationOptions => "typeHierarchyProvider",
        ["The server provides inline values."]
        opt inline_value_provider: BooleanOrInlineValueOptionsOrInlineValueRegistrationOptions => "inlineValueProvider",
        ["The server provides inlay hints."]
        opt inlay_hint_provider: BooleanOrInlayHintOptionsOrInlayHintRegistrationOptions => "inlayHintProvider",
        ["The server has support for pull-model diagnostics."]
        opt diagnostic_provider: DiagnosticOptionsOrRegistrationOptions => "diagnosticProvider",
        ["The server provides inline completions."]
        opt inline_completion_provider: BooleanOrInlineCompletionOptions => "inlineCompletionProvider",
        ["Workspace-specific server capabilities."]
        opt workspace: WorkspaceOptions => "workspace",
        ["Source-definition support via `custom/textDocument/sourceDefinition`."]
        opt custom_source_definition_provider: bool => "customSourceDefinitionProvider",
        ["VS auto-insert provider options."]
        opt vs_on_auto_insert_provider: VsOnAutoInsertOptions => "_vs_onAutoInsertProvider",
        ["VS-specific grouped references via `textDocument/_vs_references`."]
        opt vs_references_provider: bool => "_vs_referencesProvider",
        ["Multi-document highlight support via `custom/textDocument/multiDocumentHighlight`."]
        opt custom_multi_document_highlight_provider: bool => "customMultiDocumentHighlightProvider",
    }
}

lsp_open_object! {
    /// User-provided initialization options (typescript-go extension).
    ///
    /// Deferred: accepts any object (including `userPreferences: null`).
    // Go: internal/lsp/lsproto/lsp_generated.go:InitializationOptions
    InitializationOptions
}

lsp_object! {
    /// Work-done progress options.
    WorkDoneProgressOptions {
        ["Whether the server reports work-done progress."]
        opt work_done_progress: bool => "workDoneProgress",
    }
}

lsp_object! {
    /// A `kind`-tagged markup-content string (LSP `MarkupContent`): a
    /// `markdown`/`plaintext` `kind` plus the rendered `value`.
    // Go: internal/lsp/lsproto/lsp_generated.go:MarkupContent
    MarkupContent {
        ["The type of the markup."]
        req kind: MarkupKind => "kind",
        ["The content itself."]
        req value: String => "value",
    }
}

lsp_object! {
    /// A code block tagged with a language (LSP `MarkedStringWithLanguage`); the
    /// object arm of a marked string and the legacy `Hover` content shape.
    // Go: internal/lsp/lsproto/lsp_generated.go:MarkedStringWithLanguage
    MarkedStringWithLanguage {
        ["The language the `value` is written in (for syntax highlighting)."]
        req language: String => "language",
        ["The code block contents."]
        req value: String => "value",
    }
}

/// A union of a plain string or a [`MarkedStringWithLanguage`]
/// (LSP `string | MarkedStringWithLanguage`), the element type of the
/// `MarkedString[]` arm of [`Hover`] contents.
///
/// Exactly one field is set. On deserialize a JSON string yields
/// [`StringOrMarkedStringWithLanguage::string`] and a JSON object yields
/// [`StringOrMarkedStringWithLanguage::marked_string_with_language`]; any other
/// kind is an error.
// Go: internal/lsp/lsproto/lsp_generated.go:StringOrMarkedStringWithLanguage
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StringOrMarkedStringWithLanguage {
    /// The plain-string variant.
    pub string: Option<String>,
    /// The language-tagged marked-string variant.
    pub marked_string_with_language: Option<MarkedStringWithLanguage>,
}

impl Serialize for StringOrMarkedStringWithLanguage {
    // Go: lsp_generated.go:StringOrMarkedStringWithLanguage.MarshalJSONTo
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (&self.string, &self.marked_string_with_language) {
            (Some(s), None) => serializer.serialize_str(s),
            (None, Some(o)) => o.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of StringOrMarkedStringWithLanguage should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for StringOrMarkedStringWithLanguage {
    // Go: lsp_generated.go:StringOrMarkedStringWithLanguage.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = StringOrMarkedStringWithLanguage;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string or a language-tagged marked-string object")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StringOrMarkedStringWithLanguage {
                    string: Some(v.to_string()),
                    marked_string_with_language: None,
                })
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                let o = MarkedStringWithLanguage::deserialize(
                    de::value::MapAccessDeserializer::new(map),
                )?;
                Ok(StringOrMarkedStringWithLanguage {
                    string: None,
                    marked_string_with_language: Some(o),
                })
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// The hover-content union (LSP
/// `MarkupContent | string | MarkedStringWithLanguage | MarkedString[]`).
///
/// Exactly one field is set. Deserialization mirrors Go's `PeekKind`/
/// `jsonObjectHasKey` dispatch: an object with a `kind` key is a
/// [`MarkupContent`]; an object with a `language` key is a
/// [`MarkedStringWithLanguage`]; a JSON string is the bare `string` arm; a JSON
/// array is the `MarkedString[]` arm; anything else is an error.
// Go: internal/lsp/lsproto/lsp_generated.go:MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings {
    /// The [`MarkupContent`] variant (object with a `kind` key).
    pub markup_content: Option<MarkupContent>,
    /// The bare-string variant.
    pub string: Option<String>,
    /// The [`MarkedStringWithLanguage`] variant (object with a `language` key).
    pub marked_string_with_language: Option<MarkedStringWithLanguage>,
    /// The `MarkedString[]` variant.
    pub marked_strings: Option<Vec<StringOrMarkedStringWithLanguage>>,
}

impl Serialize for MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings {
    // Go: lsp_generated.go:MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings.MarshalJSONTo
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(v) = &self.markup_content {
            v.serialize(serializer)
        } else if let Some(v) = &self.string {
            serializer.serialize_str(v)
        } else if let Some(v) = &self.marked_string_with_language {
            v.serialize(serializer)
        } else if let Some(v) = &self.marked_strings {
            v.serialize(serializer)
        } else {
            Err(serde::ser::Error::custom(
                "exactly one element of MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings should be set",
            ))
        }
    }
}

impl<'de> Deserialize<'de> for MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings {
    // Go: lsp_generated.go:MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let mut out = MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings::default();
        match &value {
            serde_json::Value::Object(_) => {
                // Go dispatches on the first present key in `kind`, `language`
                // order (`jsonObjectHasKey`); `kind` wins when both are present.
                if value.get("kind").is_some() {
                    out.markup_content =
                        Some(serde_json::from_value(value).map_err(de::Error::custom)?);
                } else if value.get("language").is_some() {
                    out.marked_string_with_language =
                        Some(serde_json::from_value(value).map_err(de::Error::custom)?);
                } else {
                    return Err(de::Error::custom(
                        "invalid MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings",
                    ));
                }
            }
            serde_json::Value::String(_) => {
                out.string = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
            }
            serde_json::Value::Array(_) => {
                out.marked_strings =
                    Some(serde_json::from_value(value).map_err(de::Error::custom)?);
            }
            _ => {
                return Err(de::Error::custom(
                    "invalid MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings",
                ))
            }
        }
        Ok(out)
    }
}

/// A union of a plain string or a [`MarkupContent`]
/// (LSP `string | MarkupContent`), the shape of the various `*.documentation`
/// and inlay-hint `tooltip` fields.
///
/// Exactly one field is set. On deserialize a JSON string yields
/// [`StringOrMarkupContent::string`] and a JSON object yields
/// [`StringOrMarkupContent::markup_content`]; any other kind is an error.
// Go: internal/lsp/lsproto/lsp_generated.go:StringOrMarkupContent
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StringOrMarkupContent {
    /// The plain-string variant.
    pub string: Option<String>,
    /// The [`MarkupContent`] object variant.
    pub markup_content: Option<MarkupContent>,
}

impl Serialize for StringOrMarkupContent {
    // Go: lsp_generated.go:StringOrMarkupContent.MarshalJSONTo
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (&self.string, &self.markup_content) {
            (Some(s), None) => serializer.serialize_str(s),
            (None, Some(o)) => o.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of StringOrMarkupContent should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for StringOrMarkupContent {
    // Go: lsp_generated.go:StringOrMarkupContent.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = StringOrMarkupContent;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string or a markup-content object")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StringOrMarkupContent {
                    string: Some(v.to_string()),
                    markup_content: None,
                })
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                let o = MarkupContent::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(StringOrMarkupContent {
                    string: None,
                    markup_content: Some(o),
                })
            }
        }
        deserializer.deserialize_any(V)
    }
}

lsp_object! {
    /// The result of a hover request.
    Hover {
        ["The hover's content."]
        req contents: MarkupContentOrStringOrMarkedStringWithLanguageOrMarkedStrings => "contents",
        ["An optional range to visualize the hover."]
        opt range: Range => "range",
        ["Whether the verbosity level can be increased."]
        opt can_increase_verbosity: bool => "canIncreaseVerbosity",
    }
}

lsp_object! {
    /// An item of a call hierarchy (LSP `CallHierarchyItem`): a named symbol
    /// with its location and selection range.
    ///
    /// `data` keeps the Go `*CallHierarchyItemData` carrier as raw JSON; that
    /// nested type is deferred to the generator pass.
    // Go: internal/lsp/lsproto/lsp_generated.go:CallHierarchyItem
    CallHierarchyItem {
        ["The name of this item."]
        req name: String => "name",
        ["The kind of this item."]
        req kind: SymbolKind => "kind",
        ["Tags for this item."]
        opt tags: Vec<SymbolTag> => "tags",
        ["More detail for this item, e.g. the signature of a function."]
        opt detail: String => "detail",
        ["The resource identifier of this item."]
        req uri: DocumentUri => "uri",
        ["The range enclosing this symbol (including comments/code)."]
        req range: Range => "range",
        ["The range that should be selected and revealed (contained by `range`)."]
        req selection_range: Range => "selectionRange",
        // DEFER: `*CallHierarchyItemData` is a typescript-go carrier cookie; kept
        // as raw JSON. blocked-by: generator pass landing CallHierarchyItemData.
        ["A data entry preserved between prepare and incoming/outgoing calls (raw JSON)."]
        opt data: serde_json::Value => "data",
    }
}

lsp_object! {
    /// Parameters of a `callHierarchy/incomingCalls` request.
    CallHierarchyIncomingCallsParams {
        ["A work-done progress token."]
        opt work_done_token: IntegerOrString => "workDoneToken",
        ["A partial-result token."]
        opt partial_result_token: IntegerOrString => "partialResultToken",
        ["The call-hierarchy item to compute incoming calls for."]
        reqnn item: CallHierarchyItem => "item",
    }
}

lsp_object! {
    /// An incoming call in a call hierarchy.
    CallHierarchyIncomingCall {
        ["The item that makes the call."]
        reqnn from: CallHierarchyItem => "from",
        ["The ranges at which the calls appear."]
        reqnn from_ranges: Vec<Range> => "fromRanges",
    }
}

lsp_object! {
    /// A set of semantic tokens encoded as a flat integer array.
    SemanticTokens {
        ["An optional result id for delta updates."]
        opt result_id: String => "resultId",
        ["The actual tokens."]
        reqnn data: Vec<u32> => "data",
    }
}

/// The initial trace setting requested by the client (LSP `TraceValue`, a
/// string enum). Unknown values round-trip as their raw string.
// Go: internal/lsp/lsproto/lsp_generated.go:TraceValue
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraceValue(pub Cow<'static, str>);

impl TraceValue {
    /// Turn tracing off.
    pub const OFF: TraceValue = TraceValue(Cow::Borrowed("off"));
    /// Trace messages only.
    pub const MESSAGES: TraceValue = TraceValue(Cow::Borrowed("messages"));
    /// Verbose message tracing.
    pub const VERBOSE: TraceValue = TraceValue(Cow::Borrowed("verbose"));
}

impl Serialize for TraceValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for TraceValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(TraceValue(Cow::Owned(String::deserialize(deserializer)?)))
    }
}

lsp_object! {
    /// Information about the client as provided in [`InitializeParams`]
    /// (LSP `ClientInfo`).
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientInfo
    ClientInfo {
        ["The name of the client as defined by the client."]
        req name: String => "name",
        ["The client's version as defined by the client."]
        opt version: String => "version",
    }
}

lsp_object! {
    /// Information about the server as returned in [`InitializeResult`]
    /// (LSP `ServerInfo`).
    // Go: internal/lsp/lsproto/lsp_generated.go:ServerInfo
    ServerInfo {
        ["The name of the server as defined by the server."]
        req name: String => "name",
        ["The server's version as defined by the server."]
        opt version: String => "version",
    }
}

lsp_object! {
    /// Parameters of the `initialize` request.
    InitializeParams {
        ["A work-done progress token."]
        opt work_done_token: IntegerOrString => "workDoneToken",
        ["The parent process id, or `null`."]
        req process_id: IntegerOrNull => "processId",
        ["Information about the client."]
        opt client_info: ClientInfo => "clientInfo",
        ["The locale the client is showing the UI in."]
        opt locale: String => "locale",
        ["The deprecated root path, or `null`."]
        optn root_path: StringOrNull => "rootPath",
        ["The root URI of the workspace, or `null`."]
        req root_uri: DocumentUriOrNull => "rootUri",
        ["The capabilities provided by the client."]
        reqnn capabilities: ClientCapabilities => "capabilities",
        ["User-provided initialization options."]
        opt initialization_options: InitializationOptions => "initializationOptions",
        ["The initial trace setting (defaults to `off` when omitted)."]
        opt trace: TraceValue => "trace",
        ["The configured workspace folders, or `null`."]
        optn workspace_folders: WorkspaceFoldersOrNull => "workspaceFolders",
    }
}

lsp_object! {
    /// The result of the `initialize` request.
    InitializeResult {
        ["The capabilities the server provides."]
        reqnn capabilities: ServerCapabilities => "capabilities",
        ["Information about the server."]
        opt server_info: ServerInfo => "serverInfo",
    }
}

/// The kind of a completion item (LSP `CompletionItemKind`, an integer enum).
// Go: internal/lsp/lsproto/lsp_generated.go:CompletionItemKind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompletionItemKind(pub u32);

impl CompletionItemKind {
    /// A variable completion.
    pub const VARIABLE: CompletionItemKind = CompletionItemKind(6);
}

impl Serialize for CompletionItemKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for CompletionItemKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(CompletionItemKind(u32::deserialize(deserializer)?))
    }
}

/// Whether completion insert text is plain text or a snippet
/// (LSP `InsertTextFormat`, an integer enum).
// Go: internal/lsp/lsproto/lsp_generated.go:InsertTextFormat
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InsertTextFormat(pub u32);

impl InsertTextFormat {
    /// The insert text is a plain string.
    pub const PLAIN_TEXT: InsertTextFormat = InsertTextFormat(1);
    /// The insert text is a snippet.
    pub const SNIPPET: InsertTextFormat = InsertTextFormat(2);
}

impl Serialize for InsertTextFormat {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for InsertTextFormat {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(InsertTextFormat(u32::deserialize(deserializer)?))
    }
}

lsp_object! {
    /// Additional details for a completion-item label
    /// (LSP `CompletionItemLabelDetails`).
    // Go: internal/lsp/lsproto/lsp_generated.go:CompletionItemLabelDetails
    CompletionItemLabelDetails {
        ["A string rendered less prominently right after the label (e.g. a signature)."]
        opt detail: String => "detail",
        ["A string rendered less prominently after `detail` (e.g. a fully qualified name)."]
        opt description: String => "description",
    }
}

lsp_object! {
    /// A completion item presented in the editor.
    ///
    /// Fields whose precise nested types are deferred to the generator pass are
    /// modeled as raw JSON ([`serde_json::Value`]); their null/omit behavior is
    /// preserved.
    CompletionItem {
        ["The label of this completion item."]
        req label: String => "label",
        ["Additional details for the label."]
        opt label_details: CompletionItemLabelDetails => "labelDetails",
        ["The kind of this completion item."]
        opt kind: CompletionItemKind => "kind",
        ["Tags for this completion item."]
        opt tags: Vec<CompletionItemTag> => "tags",
        ["Additional human-readable detail."]
        opt detail: String => "detail",
        ["A doc-comment string or markup."]
        opt documentation: StringOrMarkupContent => "documentation",
        ["Whether this item is deprecated."]
        opt deprecated: bool => "deprecated",
        ["Whether to preselect this item."]
        opt preselect: bool => "preselect",
        ["The sort-comparison string."]
        opt sort_text: String => "sortText",
        ["The filter string."]
        opt filter_text: String => "filterText",
        ["The text inserted when selecting this item."]
        opt insert_text: String => "insertText",
        ["The format of the insert text."]
        opt insert_text_format: InsertTextFormat => "insertTextFormat",
        ["The whitespace/indentation insert mode."]
        opt insert_text_mode: InsertTextMode => "insertTextMode",
        ["An edit applied when selecting this item."]
        opt text_edit: TextEditOrInsertReplaceEdit => "textEdit",
        ["The edit text used with completion-list defaults."]
        opt text_edit_text: String => "textEditText",
        ["Additional edits applied alongside the main edit."]
        opt additional_text_edits: Vec<TextEdit> => "additionalTextEdits",
        ["Characters that commit this completion."]
        opt commit_characters: Vec<String> => "commitCharacters",
        ["A command run after inserting this completion."]
        opt command: Command => "command",
        ["Data preserved between request and resolve (deferred: raw JSON)."]
        opt data: serde_json::Value => "data",
    }
}

lsp_object! {
    /// An incremental change to a text document: a range and its replacement.
    ///
    /// Since: 3.18.0
    TextDocumentContentChangePartial {
        ["The range of the document that changed."]
        req range: Range => "range",
        ["The optional length of the range that got replaced (deprecated; use `range`)."]
        opt range_length: u32 => "rangeLength",
        ["The new text for the provided range."]
        req text: String => "text",
    }
}

lsp_object! {
    /// A change that replaces the whole content of a text document.
    TextDocumentContentChangeWholeDocument {
        ["The new text of the whole document."]
        req text: String => "text",
    }
}

/// A union of an incremental or whole-document text change
/// (LSP `TextDocumentContentChangePartial | TextDocumentContentChangeWholeDocument`).
///
/// A JSON object with a `range` key is a [`TextDocumentContentChangePartial`];
/// otherwise it is a [`TextDocumentContentChangeWholeDocument`].
// Go: internal/lsp/lsproto/lsp_generated.go:TextDocumentContentChangePartialOrWholeDocument
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextDocumentContentChangePartialOrWholeDocument {
    /// The incremental-change variant.
    pub partial: Option<TextDocumentContentChangePartial>,
    /// The whole-document-replacement variant.
    pub whole_document: Option<TextDocumentContentChangeWholeDocument>,
}

impl Serialize for TextDocumentContentChangePartialOrWholeDocument {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (&self.partial, &self.whole_document) {
            (Some(v), None) => v.serialize(serializer),
            (None, Some(v)) => v.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "exactly one element of TextDocumentContentChangePartialOrWholeDocument should be set",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for TextDocumentContentChangePartialOrWholeDocument {
    // Go: lsp_generated.go:TextDocumentContentChangePartialOrWholeDocument.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let mut out = TextDocumentContentChangePartialOrWholeDocument::default();
        if value.get("range").is_some() {
            out.partial = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
        } else {
            out.whole_document = Some(serde_json::from_value(value).map_err(de::Error::custom)?);
        }
        Ok(out)
    }
}

/// The severity of a diagnostic (LSP `DiagnosticSeverity`, an integer enum).
// Go: internal/lsp/lsproto/lsp_generated.go:DiagnosticSeverity
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticSeverity(pub u32);

impl DiagnosticSeverity {
    /// Reports an error.
    pub const ERROR: DiagnosticSeverity = DiagnosticSeverity(1);
    /// Reports a warning.
    pub const WARNING: DiagnosticSeverity = DiagnosticSeverity(2);
    /// Reports an information.
    pub const INFORMATION: DiagnosticSeverity = DiagnosticSeverity(3);
    /// Reports a hint.
    pub const HINT: DiagnosticSeverity = DiagnosticSeverity(4);
}

impl fmt::Display for DiagnosticSeverity {
    // Go: internal/lsp/lsproto/lsp_generated.go:DiagnosticSeverity.String
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.0 {
            1 => "Error",
            2 => "Warning",
            3 => "Information",
            4 => "Hint",
            n => return write!(f, "DiagnosticSeverity({n})"),
        };
        f.write_str(name)
    }
}

impl Serialize for DiagnosticSeverity {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for DiagnosticSeverity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(DiagnosticSeverity(u32::deserialize(deserializer)?))
    }
}

/// A diagnostic tag (LSP `DiagnosticTag`, an integer enum).
// Go: internal/lsp/lsproto/lsp_generated.go:DiagnosticTag
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticTag(pub u32);

impl DiagnosticTag {
    /// Unused or unnecessary code (rendered faded).
    pub const UNNECESSARY: DiagnosticTag = DiagnosticTag(1);
    /// Deprecated or obsolete code (rendered struck through).
    pub const DEPRECATED: DiagnosticTag = DiagnosticTag(2);
}

impl fmt::Display for DiagnosticTag {
    // Go: internal/lsp/lsproto/lsp_generated.go:DiagnosticTag.String
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.0 {
            1 => "Unnecessary",
            2 => "Deprecated",
            n => return write!(f, "DiagnosticTag({n})"),
        };
        f.write_str(name)
    }
}

impl Serialize for DiagnosticTag {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for DiagnosticTag {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(DiagnosticTag(u32::deserialize(deserializer)?))
    }
}

lsp_object! {
    /// A description for an error code (LSP `CodeDescription`).
    CodeDescription {
        ["A URI to open with more information about the diagnostic error."]
        req href: crate::URI => "href",
    }
}

lsp_open_object! {
    /// A placeholder for custom data preserved on a [`Diagnostic`] between a
    /// `textDocument/publishDiagnostics` notification and a
    /// `textDocument/codeAction` request.
    ///
    /// Deferred: Go models this as an empty struct; this port accepts any
    /// object and re-serializes to `{}`.
    // Go: internal/lsp/lsproto/lsp_generated.go:DiagnosticData
    DiagnosticData
}

lsp_object! {
    /// A related message and source-code location for a diagnostic
    /// (LSP `DiagnosticRelatedInformation`).
    DiagnosticRelatedInformation {
        ["The location of this related diagnostic information."]
        req location: Location => "location",
        ["The message of this related diagnostic information."]
        req message: String => "message",
    }
}

lsp_object! {
    /// A diagnostic (e.g. a compiler error or warning) at a [`Range`] in a
    /// document.
    Diagnostic {
        ["The range at which the message applies."]
        req range: Range => "range",
        ["The diagnostic's severity."]
        opt severity: DiagnosticSeverity => "severity",
        ["The diagnostic's code (appears in the user interface)."]
        opt code: IntegerOrString => "code",
        ["An optional description of the error code."]
        opt code_description: CodeDescription => "codeDescription",
        ["A human-readable source of this diagnostic, e.g. `typescript`."]
        opt source: String => "source",
        ["The diagnostic's message."]
        req message: String => "message",
        ["Additional metadata about the diagnostic."]
        opt tags: Vec<DiagnosticTag> => "tags",
        ["Related diagnostic information."]
        opt related_information: Vec<DiagnosticRelatedInformation> => "relatedInformation",
        ["Data preserved between publishDiagnostics and codeAction."]
        opt data: DiagnosticData => "data",
    }
}

#[cfg(test)]
#[path = "generated_test.rs"]
mod tests;
