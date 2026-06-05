//! API protocol types (`proto.go`): method names, handles, request/response DTOs.

use std::fmt;

use serde::de::{self, Deserializer};
use serde::Serialize;
use tsgo_ls_lsconv::file_name_to_document_uri;
use tsgo_lsproto::DocumentUri;
use tsgo_tspath::get_normalized_absolute_path;

/// Identifies a document by either a file name (plain JSON string) or a URI
/// object (`{ "uri": "..." }` on the wire).
///
/// Mirrors Go's dual wire form: unmarshaling accepts a string (→ [`file_name`])
/// or an object with a `uri` field (unknown object fields are ignored).
///
/// # Examples
/// ```
/// use tsgo_api::DocumentIdentifier;
/// use tsgo_json::unmarshal;
/// use tsgo_lsproto::DocumentUri;
///
/// let from_str: DocumentIdentifier = unmarshal(br#""foo.ts""#).unwrap();
/// assert_eq!(from_str.file_name, "foo.ts");
///
/// let from_obj: DocumentIdentifier =
///     unmarshal(br#"{"uri":"file:///foo.ts"}"#).unwrap();
/// assert_eq!(from_obj.uri, DocumentUri("file:///foo.ts".into()));
/// ```
///
/// Side effects: none (pure data).
// Go: internal/api/proto.go:DocumentIdentifier
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct DocumentIdentifier {
    /// Local file name when the wire value was a plain string.
    #[serde(rename = "fileName", skip_serializing_if = "String::is_empty", default)]
    pub file_name: String,
    /// Document URI when the wire value was a `{ "uri": ... }` object.
    #[serde(rename = "uri", skip_serializing_if = "document_uri_is_empty", default)]
    pub uri: DocumentUri,
}

fn document_uri_is_empty(uri: &DocumentUri) -> bool {
    uri.0.is_empty()
}

impl<'de> serde::Deserialize<'de> for DocumentIdentifier {
    // Go: internal/api/proto.go:DocumentIdentifier.UnmarshalJSONFrom
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if let Some(s) = value.as_str() {
            return Ok(DocumentIdentifier {
                file_name: s.to_string(),
                ..Default::default()
            });
        }
        if let Some(obj) = value.as_object() {
            let uri = obj
                .get("uri")
                .and_then(|v| v.as_str())
                .map(|s| DocumentUri(s.to_string()))
                .unwrap_or_default();
            return Ok(DocumentIdentifier {
                uri,
                ..Default::default()
            });
        }
        Err(de::Error::custom(format!(
            "DocumentIdentifier: expected string or object, got {}",
            json_value_kind(&value)
        )))
    }
}

fn json_value_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

impl DocumentIdentifier {
    /// Returns the local file name for this identifier.
    ///
    /// When [`uri`] is set, delegates to [`DocumentUri::file_name`]; otherwise
    /// returns [`file_name`].
    ///
    /// # Examples
    /// ```
    /// use tsgo_api::DocumentIdentifier;
    /// use tsgo_lsproto::DocumentUri;
    ///
    /// let d = DocumentIdentifier {
    ///     uri: DocumentUri("file:///foo.ts".into()),
    ///     ..Default::default()
    /// };
    /// assert_eq!(d.to_file_name(), "/foo.ts");
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/api/proto.go:DocumentIdentifier.ToFileName
    pub fn to_file_name(&self) -> String {
        if !self.uri.0.is_empty() {
            return self.uri.file_name();
        }
        self.file_name.clone()
    }

    /// Returns the LSP document URI for this identifier.
    ///
    /// When [`uri`] is set, returns it unchanged; otherwise converts
    /// [`file_name`] via [`file_name_to_document_uri`].
    ///
    /// # Examples
    /// ```
    /// use tsgo_api::DocumentIdentifier;
    /// use tsgo_lsproto::DocumentUri;
    ///
    /// let d = DocumentIdentifier {
    ///     file_name: "/path/to/file.ts".into(),
    ///     ..Default::default()
    /// };
    /// assert_eq!(d.to_uri(), DocumentUri("file:///path/to/file.ts".into()));
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/api/proto.go:DocumentIdentifier.ToURI
    pub fn to_uri(&self) -> DocumentUri {
        if !self.uri.0.is_empty() {
            return self.uri.clone();
        }
        file_name_to_document_uri(&self.file_name)
    }

    /// Returns an absolute normalized file path for this identifier.
    ///
    /// When [`uri`] is set, returns [`DocumentUri::file_name`]; otherwise
    /// normalizes [`file_name`] against `cwd`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_api::DocumentIdentifier;
    ///
    /// let d = DocumentIdentifier {
    ///     file_name: "bar.ts".into(),
    ///     ..Default::default()
    /// };
    /// assert_eq!(d.to_absolute_file_name("/cwd"), "/cwd/bar.ts");
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/api/proto.go:DocumentIdentifier.ToAbsoluteFileName
    pub fn to_absolute_file_name(&self, cwd: &str) -> String {
        if !self.uri.0.is_empty() {
            return self.uri.file_name();
        }
        get_normalized_absolute_path(&self.file_name, cwd)
    }
}

impl fmt::Display for DocumentIdentifier {
    // Go: internal/api/proto.go:DocumentIdentifier.String
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.uri.0.is_empty() {
            return write!(f, "{}", self.uri.0);
        }
        write!(f, "{}", self.file_name)
    }
}

#[cfg(test)]
#[path = "proto_test.rs"]
mod tests;
