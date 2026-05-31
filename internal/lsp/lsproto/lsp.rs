//! Port of Go `internal/lsp/lsproto/lsp.go` (hand-written protocol helpers:
//! the `DocumentUri` URI<->file-name conversion and friends).

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A generic URI string. Mirrors Go `type URI string`: a string newtype that
/// (de)serializes as a plain JSON string (used e.g. for [`crate::CodeDescription`]).
///
/// # Examples
/// ```
/// let u: tsgo_lsproto::URI = serde_json::from_str("\"https://x\"").unwrap();
/// assert_eq!(u.0, "https://x");
/// ```
// Go: internal/lsp/lsproto/lsp.go:URI
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct URI(pub String);

impl Serialize for URI {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for URI {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(URI(String::deserialize(deserializer)?))
    }
}

impl crate::DocumentUri {
    /// Converts this URI to a local file name (the inverse of
    /// `FileNameToDocumentURI`).
    ///
    /// Mirrors Go's `DocumentUri.FileName`: bundled URIs pass through, `file://`
    /// URIs are parsed (host preserved, path percent-decoded, Windows drive
    /// letters normalized), and every other scheme is mapped into the
    /// `^/<scheme>/<authority>/<path>` virtual form while staying escaped so it
    /// round-trips.
    ///
    /// # Examples
    /// ```
    /// let uri = tsgo_lsproto::DocumentUri("file:///path/to/file.ts".to_string());
    /// assert_eq!(uri.file_name(), "/path/to/file.ts");
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/lsp/lsproto/lsp.go:DocumentUri.FileName
    pub fn file_name(&self) -> String {
        let uri = &self.0;
        if tsgo_bundled::is_bundled(uri) {
            return uri.clone();
        }
        if uri.starts_with("file://") {
            let (host, path) = parse_file_uri(uri);
            if !host.is_empty() {
                return format!("//{host}{path}");
            }
            return fix_windows_uri_path(&path);
        }

        // Leave all other URIs escaped so we can round-trip them.
        let (scheme, path) = uri
            .split_once(':')
            .unwrap_or_else(|| panic!("invalid URI: {uri}"));

        let mut authority = "ts-nul-authority";
        let mut path = path;
        if let Some(rest) = path.strip_prefix("//") {
            let (auth, rest_path) = rest
                .split_once('/')
                .unwrap_or_else(|| panic!("invalid URI: {uri}"));
            authority = auth;
            path = rest_path;
        }

        format!("^/{scheme}/{authority}/{path}")
    }

    /// Returns this URI's canonicalized [`tsgo_tspath::Path`], built from
    /// [`DocumentUri::file_name`].
    ///
    /// `use_case_sensitive_file_names` controls whether the resulting path is
    /// case-folded (matching the host file system's case sensitivity).
    ///
    /// # Examples
    /// ```
    /// let uri = tsgo_lsproto::DocumentUri("file:///A/B.ts".to_string());
    /// assert_eq!(uri.path(false).as_str(), "/a/b.ts");
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/lsp/lsproto/lsp.go:DocumentUri.Path
    pub fn path(&self, use_case_sensitive_file_names: bool) -> tsgo_tspath::Path {
        tsgo_tspath::to_path(&self.file_name(), "", use_case_sensitive_file_names)
    }
}

/// Parses a `file://` URI into its host and path, mirroring the subset of Go's
/// `net/url.Parse` behavior that `DocumentUri.FileName` relies on.
fn parse_file_uri(uri: &str) -> (String, String) {
    // `url.Parse` splits the fragment (`#...`) off before parsing the rest.
    let without_fragment = uri.split_once('#').map_or(uri, |(before, _)| before);
    // Everything after the `file:` scheme; starts with `//`.
    let rest = &without_fragment["file:".len()..];
    // Drop a query component (`?...`) if present.
    let rest = rest.split_once('?').map_or(rest, |(before, _)| before);
    // The authority spans from after `//` up to the next `/` (which begins the
    // path). Go: `authority, rest = split(rest[2:], '/', false)`.
    let after_slashes = &rest[2..];
    let (authority, path) = match after_slashes.find('/') {
        Some(i) => (&after_slashes[..i], &after_slashes[i..]),
        None => (after_slashes, ""),
    };
    // `net/url` percent-decodes both the host and the path; an ill-formed
    // escape makes `url.Parse` fail, which Go turns into a panic.
    let host = percent_decode(authority).unwrap_or_else(|| panic!("invalid file URI: {uri}"));
    let path = percent_decode(path).unwrap_or_else(|| panic!("invalid file URI: {uri}"));
    (host, path)
}

/// Normalizes a decoded `file://` path: a leading-`/` path that begins with a
/// DOS volume (`/c:/...`) drops the leading slash and lowercases the drive
/// (`c:/...`); anything else is returned unchanged.
// Go: internal/lsp/lsproto/lsp.go:fixWindowsURIPath
fn fix_windows_uri_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix('/') {
        let (volume, rest, ok) = tsgo_tspath::split_volume_path(rest);
        if ok {
            return format!("{volume}{rest}");
        }
    }
    path.to_string()
}

/// Percent-decodes `%XX` byte escapes, mirroring Go `net/url.unescape` in
/// `encodePath` mode: `+` is preserved (only query mode maps it to a space) and
/// an ill-formed `%` escape yields `None`.
///
/// Unlike Go (whose strings may hold arbitrary bytes), the decoded bytes must
/// be valid UTF-8; otherwise `None` is returned.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            out.push((unhex(bytes[i + 1])? << 4) | unhex(bytes[i + 2])?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Decodes a single hex digit, or `None` if `b` is not `[0-9a-fA-F]`.
fn unhex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
#[path = "lsp_test.rs"]
mod tests;
