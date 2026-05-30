//! Port of Go `internal/lsp/lsproto/lsp_generated.go` (the LSP meta-model
//! generated types).

use std::fmt;

use serde::de::{self, DeserializeOwned, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
    /// An inlay-hint label part (LSP `InlayHintLabelPart`).
    InlayHintLabelPart {
        ["The value of this label part."]
        req value: String => "value",
        ["The tooltip text (deferred: raw JSON)."]
        opt tooltip: serde_json::Value => "tooltip",
        ["An optional source-code location for this label part."]
        opt location: Location => "location",
        ["An optional command (deferred: raw JSON)."]
        opt command: serde_json::Value => "command",
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
        ["The tooltip text (deferred: raw JSON)."]
        opt tooltip: serde_json::Value => "tooltip",
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

/// A symbol kind (LSP `SymbolKind`, an integer enum).
// Go: internal/lsp/lsproto/lsp_generated.go:SymbolKind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        ["The edits to apply (deferred element type: raw JSON)."]
        reqnn edits: Vec<serde_json::Value> => "edits",
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
        ["Additional options (deferred: raw JSON)."]
        opt options: serde_json::Value => "options",
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
        ["Rename options (deferred: raw JSON)."]
        opt options: serde_json::Value => "options",
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
        ["Delete options (deferred: raw JSON)."]
        opt options: serde_json::Value => "options",
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

lsp_open_object! {
    /// The capabilities the client supports.
    ///
    /// Deferred: the full LSP field tree is large and generated later; this
    /// port accepts any object and re-serializes to `{}`.
    // Go: internal/lsp/lsproto/lsp_generated.go:ClientCapabilities
    ClientCapabilities
}

lsp_open_object! {
    /// The capabilities the server provides.
    ///
    /// Deferred: see [`ClientCapabilities`].
    // Go: internal/lsp/lsproto/lsp_generated.go:ServerCapabilities
    ServerCapabilities
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
    /// The result of a hover request.
    Hover {
        ["The hover's content (deferred union: raw JSON)."]
        req contents: serde_json::Value => "contents",
        ["An optional range to visualize the hover."]
        opt range: Range => "range",
        ["Whether the verbosity level can be increased."]
        opt can_increase_verbosity: bool => "canIncreaseVerbosity",
    }
}

lsp_object! {
    /// Parameters of a `callHierarchy/incomingCalls` request.
    CallHierarchyIncomingCallsParams {
        ["A work-done progress token."]
        opt work_done_token: IntegerOrString => "workDoneToken",
        ["A partial-result token."]
        opt partial_result_token: IntegerOrString => "partialResultToken",
        ["The call-hierarchy item (deferred: raw JSON)."]
        reqnn item: serde_json::Value => "item",
    }
}

lsp_object! {
    /// An incoming call in a call hierarchy.
    CallHierarchyIncomingCall {
        ["The item that makes the call (deferred: raw JSON)."]
        reqnn from: serde_json::Value => "from",
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

lsp_object! {
    /// Parameters of the `initialize` request.
    InitializeParams {
        ["A work-done progress token."]
        opt work_done_token: IntegerOrString => "workDoneToken",
        ["The parent process id, or `null`."]
        req process_id: IntegerOrNull => "processId",
        ["Information about the client (deferred: raw JSON)."]
        opt client_info: serde_json::Value => "clientInfo",
        ["The locale the client is showing the UI in."]
        opt locale: String => "locale",
        ["The deprecated root path, or `null` (deferred: raw JSON)."]
        optn root_path: serde_json::Value => "rootPath",
        ["The root URI of the workspace, or `null`."]
        req root_uri: DocumentUriOrNull => "rootUri",
        ["The capabilities provided by the client."]
        reqnn capabilities: ClientCapabilities => "capabilities",
        ["User-provided initialization options."]
        opt initialization_options: InitializationOptions => "initializationOptions",
        ["The initial trace setting (deferred: raw JSON)."]
        opt trace: serde_json::Value => "trace",
        ["The configured workspace folders, or `null` (deferred: raw JSON)."]
        optn workspace_folders: serde_json::Value => "workspaceFolders",
    }
}

lsp_object! {
    /// The result of the `initialize` request.
    InitializeResult {
        ["The capabilities the server provides."]
        reqnn capabilities: ServerCapabilities => "capabilities",
        ["Information about the server (deferred: raw JSON)."]
        opt server_info: serde_json::Value => "serverInfo",
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
    /// A completion item presented in the editor.
    ///
    /// Fields whose precise nested types are deferred to the generator pass are
    /// modeled as raw JSON ([`serde_json::Value`]); their null/omit behavior is
    /// preserved.
    CompletionItem {
        ["The label of this completion item."]
        req label: String => "label",
        ["Additional label details (deferred: raw JSON)."]
        opt label_details: serde_json::Value => "labelDetails",
        ["The kind of this completion item."]
        opt kind: CompletionItemKind => "kind",
        ["Tags for this completion item (deferred: raw JSON)."]
        opt tags: serde_json::Value => "tags",
        ["Additional human-readable detail."]
        opt detail: String => "detail",
        ["A doc-comment string (deferred: raw JSON)."]
        opt documentation: serde_json::Value => "documentation",
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
        ["The whitespace/indentation insert mode (deferred: raw JSON)."]
        opt insert_text_mode: serde_json::Value => "insertTextMode",
        ["An edit applied when selecting this item."]
        opt text_edit: TextEditOrInsertReplaceEdit => "textEdit",
        ["The edit text used with completion-list defaults."]
        opt text_edit_text: String => "textEditText",
        ["Additional edits applied alongside the main edit."]
        opt additional_text_edits: Vec<TextEdit> => "additionalTextEdits",
        ["Characters that commit this completion."]
        opt commit_characters: Vec<String> => "commitCharacters",
        ["A command run after inserting (deferred: raw JSON)."]
        opt command: serde_json::Value => "command",
        ["Data preserved between request and resolve (deferred: raw JSON)."]
        opt data: serde_json::Value => "data",
    }
}

#[cfg(test)]
#[path = "generated_test.rs"]
mod tests;
