//! Source-map URL discovery ([`try_get_source_mapping_url`]).
//!
//! 1:1 port of Go `internal/sourcemap/util.go`.

use crate::EcmaLineInfo;

/// Scans `line_info` from its last line backwards for a trailing
/// `//# sourceMappingURL=` (or `//@ sourceMappingURL=`) comment and returns the
/// referenced URL, or an empty string if none is present.
///
/// # Examples
/// ```
/// use tsgo_sourcemap::{create_ecma_line_info, try_get_source_mapping_url};
/// use tsgo_core::compute_ecma_line_starts;
/// let text = "var x = 1;\n//# sourceMappingURL=a.js.map";
/// let info = create_ecma_line_info(text, compute_ecma_line_starts(text));
/// assert_eq!(try_get_source_mapping_url(Some(&info)), "a.js.map");
/// ```
///
/// Side effects: none (pure).
// Go: internal/sourcemap/util.go:TryGetSourceMappingURL
pub fn try_get_source_mapping_url(line_info: Option<&EcmaLineInfo>) -> String {
    if let Some(line_info) = line_info {
        for index in (0..line_info.line_count()).rev() {
            let line = line_info.line_text(index);
            let line = line.trim_start_matches(|c: char| c.is_whitespace());
            let line = line.trim_end_matches(tsgo_stringutil::is_line_break);
            if line.is_empty() {
                continue;
            }
            let bytes = line.as_bytes();
            if line.len() < 4
                || !line.starts_with("//")
                || (bytes[2] != b'#' && bytes[2] != b'@')
                || bytes[3] != b' '
            {
                break;
            }
            if let Some(url) = line[4..].strip_prefix("sourceMappingURL=") {
                return url
                    .trim_end_matches(|c: char| c.is_whitespace())
                    .to_string();
            }
        }
    }
    String::new()
}

#[cfg(test)]
#[path = "util_test.rs"]
mod tests;
