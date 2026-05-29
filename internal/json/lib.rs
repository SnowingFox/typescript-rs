//! `tsgo_json` — thin wrapper for the whole repo's JSON
//! serialization/deserialization (serde_json backend).
//!
//! 1:1 port of Go `internal/json/json.go`. The Go side wraps
//! `go-json-experiment/json`, unifying defaults (tolerate invalid UTF-8 by
//! default, optional deterministic output, indent control) and re-exporting
//! codec types. The Rust side implements the main path with `serde` /
//! `serde_json`; the streaming `Decoder`/`Encoder` token API is deferred until
//! LSP is wired up.
//!
//! # Divergence from Go
//! - Tolerating invalid UTF-8 (Go's default `AllowInvalidUTF8(true)`): Rust
//!   `String`/`serde_json` require valid UTF-8, so this is handled upstream
//!   (vfs/scanner) and the end-to-end impact is covered by P10 parity.
//! - `...json.Options` variadics -> named functions (`marshal_indent` /
//!   `marshal_deterministic`).

use std::io;

use serde::de::DeserializeOwned;
use serde::Serialize;

pub use serde;
pub use serde_json;
pub use serde_json::Error;
pub use serde_json::Value;

/// Serializes compactly to JSON bytes (no extra whitespace), matching Go's v2
/// JSON compact output.
///
/// # Examples
/// ```
/// #[derive(serde::Serialize)]
/// struct S { a: i32, b: String }
/// let out = tsgo_json::marshal(&S { a: 1, b: "x".into() }).unwrap();
/// assert_eq!(String::from_utf8(out).unwrap(), r#"{"a":1,"b":"x"}"#);
/// ```
///
/// Side effects: none (pure; returns a newly-allocated `Vec<u8>`).
// Go: internal/json/json.go:Marshal
pub fn marshal<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    serde_json::to_vec(value)
}

/// Writes the JSON serialization to any [`io::Write`].
///
/// # Examples
/// ```
/// #[derive(serde::Serialize)]
/// struct S { a: i32 }
/// let mut buf = Vec::new();
/// tsgo_json::marshal_write(&mut buf, &S { a: 1 }).unwrap();
/// assert_eq!(String::from_utf8(buf).unwrap(), r#"{"a":1}"#);
/// ```
///
/// Side effects: writes bytes to `w` (nothing else).
// Go: internal/json/json.go:MarshalWrite
pub fn marshal_write<W: io::Write, T: Serialize>(w: &mut W, value: &T) -> Result<(), Error> {
    serde_json::to_writer(w, value)
}

/// Serializes with indentation/prefix. When `prefix==""&&indent==""` it falls
/// back to compact [`marshal`] (matching Go's explicit branch).
///
/// Otherwise it indents over multiple lines using `indent`; if `prefix` is
/// non-empty, it is appended after each newline (matching Go's `MarshalIndent`
/// semantics).
///
/// # Examples
/// ```
/// #[derive(serde::Serialize)]
/// struct S { a: i32 }
/// let out = tsgo_json::marshal_indent(&S { a: 1 }, "", "  ").unwrap();
/// assert_eq!(String::from_utf8(out).unwrap(), "{\n  \"a\": 1\n}");
/// ```
///
/// Side effects: none (pure).
// Go: internal/json/json.go:MarshalIndent
pub fn marshal_indent<T: Serialize>(
    value: &T,
    prefix: &str,
    indent: &str,
) -> Result<Vec<u8>, Error> {
    if prefix.is_empty() && indent.is_empty() {
        // WithIndentPrefix/WithIndent imply multi-line; if both are empty,
        // skip them and emit compact output.
        return marshal(value);
    }
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    value.serialize(&mut ser)?;
    if prefix.is_empty() {
        return Ok(buf);
    }
    let mut out = Vec::with_capacity(buf.len());
    for &b in &buf {
        out.push(b);
        if b == b'\n' {
            out.extend_from_slice(prefix.as_bytes());
        }
    }
    Ok(out)
}

/// Deterministic serialization: object keys are emitted in stable lexical order
/// (matching Go `Deterministic(true)`'s reproducible output).
///
/// Implementation: convert to [`serde_json::Value`] first (whose object is a
/// sorted `BTreeMap` underneath, so keys are naturally ordered), then serialize.
///
/// # Examples
/// ```
/// use std::collections::HashMap;
/// let mut m = HashMap::new();
/// m.insert("b", 2);
/// m.insert("a", 1);
/// let out = tsgo_json::marshal_deterministic(&m).unwrap();
/// assert_eq!(String::from_utf8(out).unwrap(), r#"{"a":1,"b":2}"#);
/// ```
///
/// Side effects: none (pure).
// Go: internal/json/json.go:Deterministic
pub fn marshal_deterministic<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    let v: Value = serde_json::to_value(value)?;
    serde_json::to_vec(&v)
}

/// Deserializes `T` from JSON bytes.
///
/// # Examples
/// ```
/// #[derive(serde::Deserialize, PartialEq, Debug)]
/// struct S { x: i32 }
/// let s: S = tsgo_json::unmarshal(br#"{"x":3}"#).unwrap();
/// assert_eq!(s, S { x: 3 });
/// ```
///
/// Side effects: none (pure).
// Go: internal/json/json.go:Unmarshal
pub fn unmarshal<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(bytes)
}

/// Deserializes `T` from any [`io::Read`].
///
/// # Examples
/// ```
/// #[derive(serde::Deserialize, PartialEq, Debug)]
/// struct S { x: i32 }
/// let s: S = tsgo_json::unmarshal_read(&br#"{"x":7}"#[..]).unwrap();
/// assert_eq!(s, S { x: 7 });
/// ```
///
/// Side effects: reads bytes from `r` (nothing else).
// Go: internal/json/json.go:UnmarshalRead
pub fn unmarshal_read<R: io::Read, T: DeserializeOwned>(r: R) -> Result<T, Error> {
    serde_json::from_reader(r)
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
