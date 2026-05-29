//! Shared helpers for VFS implementations: rooted-path splitting and byte/BOM
//! decoding.
//!
//! 1:1 port of Go `internal/vfs/internal/internal.go`.
//!
//! DIVERGENCE(port): Go's `Common` struct delegates to an `io/fs.FS` sub-FS.
//! Rust has no `io/fs`, so the in-memory and OS FSes implement lookups directly;
//! only the pure helpers (`root_length`, `split_path`, BOM decoding) are shared
//! here.

use tsgo_tspath::{get_encoded_root_length, normalize_path, remove_trailing_directory_separator};

/// Returns the length of the rooted prefix of `p` (e.g. `1` for `/`, `3` for
/// `c:/`).
///
/// # Examples
/// ```
/// use tsgo_vfs::internal::root_length;
/// assert_eq!(root_length("/foo/bar"), 1);
/// assert_eq!(root_length("c:/foo"), 3);
/// ```
///
/// Side effects: none (pure).
///
/// # Panics
/// Panics with `vfs: path "<p>" is not absolute` if `p` has no root.
// Go: internal/vfs/internal/internal.go:RootLength
pub fn root_length(p: &str) -> usize {
    let l = get_encoded_root_length(p);
    if l == 0 {
        panic!("vfs: path {p:?} is not absolute");
    } else if l < 0 {
        // Negative means the root was detected but is not "well-formed"
        // (Go encodes this as the bitwise complement).
        (!l) as usize
    } else {
        l as usize
    }
}

/// Splits a rooted path into its root prefix and the remaining (normalized,
/// trailing-separator-trimmed) path.
///
/// # Examples
/// ```
/// use tsgo_vfs::internal::split_path;
/// assert_eq!(split_path("/foo/bar/"), ("/".to_string(), "foo/bar".to_string()));
/// assert_eq!(split_path("c:/foo"), ("c:/".to_string(), "foo".to_string()));
/// ```
///
/// Side effects: none (pure).
///
/// # Panics
/// Panics if `p` is not absolute (via [`root_length`]).
// Go: internal/vfs/internal/internal.go:SplitPath
pub fn split_path(p: &str) -> (String, String) {
    let normalized = normalize_path(p);
    let l = root_length(&normalized);
    let root_name = normalized[..l].to_string();
    let rest = remove_trailing_directory_separator(&normalized[l..]).to_string();
    (root_name, rest)
}

/// Decodes raw file bytes into a string, handling UTF-16 LE/BE and UTF-8 byte
/// order marks.
///
/// A UTF-16 LE (`FF FE`) or BE (`FE FF`) BOM selects 16-bit decoding; a UTF-8
/// BOM (`EF BB BF`) is stripped; otherwise the bytes are taken as UTF-8.
///
/// # Examples
/// ```
/// use tsgo_vfs::internal::decode_bytes;
/// assert_eq!(decode_bytes(b"\xEF\xBB\xBFhi"), "hi");
/// assert_eq!(decode_bytes(b"\xFF\xFEh\x00i\x00"), "hi");
/// ```
///
/// Side effects: none (pure).
// Go: internal/vfs/internal/internal.go:decodeBytes
pub fn decode_bytes(bytes: &[u8]) -> String {
    if bytes.len() >= 2 {
        match (bytes[0], bytes[1]) {
            (0xFF, 0xFE) => return decode_utf16(&bytes[2..], ByteOrder::Little),
            (0xFE, 0xFF) => return decode_utf16(&bytes[2..], ByteOrder::Big),
            _ => {}
        }
    }
    let body = if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        &bytes[3..]
    } else {
        bytes
    };
    // PERF(port): Go uses `unsafe.String` to view the bytes without a copy; here
    // we copy into an owned String. Invalid UTF-8 is replaced lossily.
    String::from_utf8_lossy(body).into_owned()
}

/// Byte order for UTF-16 decoding.
///
/// # Examples
/// ```
/// use tsgo_vfs::internal::ByteOrder;
/// assert_ne!(ByteOrder::Little, ByteOrder::Big);
/// ```
///
/// Side effects: none (pure value type).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteOrder {
    /// Little-endian (`FF FE`).
    Little,
    /// Big-endian (`FE FF`).
    Big,
}

/// Decodes UTF-16 bytes (without BOM) using the given byte order.
///
/// # Examples
/// ```
/// use tsgo_vfs::internal::{decode_utf16, ByteOrder};
/// assert_eq!(decode_utf16(b"\x00h\x00i", ByteOrder::Big), "hi");
/// ```
///
/// Side effects: none (pure).
// Go: internal/vfs/internal/internal.go:decodeUtf16
pub fn decode_utf16(bytes: &[u8], order: ByteOrder) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| match order {
            ByteOrder::Little => u16::from_le_bytes([pair[0], pair[1]]),
            ByteOrder::Big => u16::from_be_bytes([pair[0], pair[1]]),
        })
        .collect();
    char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
