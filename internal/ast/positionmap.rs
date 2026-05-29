//! Bidirectional mapping between UTF-8 byte offsets and UTF-16 code units.

/// One entry per multi-byte character, recording where it ends and the running
/// `utf8 - utf16` offset difference accumulated through it.
#[derive(Clone, Copy, Debug)]
struct PositionMapEntry {
    /// UTF-8 byte offset immediately after this multi-byte character.
    utf8_pos: i32,
    /// Cumulative `utf8 - utf16` difference through this character.
    delta: i32,
}

/// Bidirectional map between UTF-8 byte offsets (used by Go/Rust) and UTF-16
/// code-unit offsets (used by JavaScript/TypeScript).
///
/// For ASCII-only text the two coincide; for non-ASCII text they diverge because
/// multi-byte UTF-8 sequences map to a different number of UTF-16 code units
/// (notably astral characters take 4 UTF-8 bytes but 2 UTF-16 code units).
/// Conversions are `O(log n)` via binary search over the recorded entries.
///
/// # Examples
/// ```
/// use tsgo_ast::positionmap::compute_position_map;
/// let pm = compute_position_map("café");
/// assert!(!pm.is_ascii_only());
/// // The byte after `é` (5) maps to UTF-16 code unit 4.
/// assert_eq!(pm.utf8_to_utf16(5), 4);
/// assert_eq!(pm.utf16_to_utf8(4), 5);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/ast/positionmap.go:PositionMap
pub struct PositionMap {
    ascii_only: bool,
    entries: Vec<PositionMapEntry>,
}

/// Builds a [`PositionMap`] for `text` by scanning it once.
///
/// # Examples
/// ```
/// use tsgo_ast::positionmap::compute_position_map;
/// assert!(compute_position_map("const x = 1;").is_ascii_only());
/// ```
///
/// Side effects: none (pure).
// Go: internal/ast/positionmap.go:ComputePositionMap
pub fn compute_position_map(text: &str) -> PositionMap {
    let bytes = text.as_bytes();
    let mut entries: Vec<PositionMapEntry> = Vec::new();
    let mut delta = 0i32;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] < 0x80 {
            i += 1;
            continue;
        }
        // `i` is at a UTF-8 boundary, so the next char decodes cleanly.
        let ch = text[i..].chars().next().unwrap();
        let size = ch.len_utf8();
        let utf16_size = if (ch as u32) >= 0x10000 { 2 } else { 1 };
        delta += size as i32 - utf16_size;
        entries.push(PositionMapEntry {
            utf8_pos: (i + size) as i32,
            delta,
        });
        i += size;
    }
    let ascii_only = entries.is_empty();
    PositionMap {
        ascii_only,
        entries,
    }
}

impl PositionMap {
    /// Reports whether the text is ASCII-only (so UTF-8 and UTF-16 offsets agree).
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/positionmap.go:IsAsciiOnly
    pub fn is_ascii_only(&self) -> bool {
        self.ascii_only
    }

    /// Converts a UTF-8 byte offset to a UTF-16 code-unit offset.
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/positionmap.go:UTF8ToUTF16
    pub fn utf8_to_utf16(&self, utf8_offset: i32) -> i32 {
        if self.ascii_only {
            return utf8_offset;
        }
        // Find the last entry whose `utf8_pos <= utf8_offset`.
        let mut lo = 0usize;
        let mut hi = self.entries.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.entries[mid].utf8_pos <= utf8_offset {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return utf8_offset;
        }
        utf8_offset - self.entries[lo - 1].delta
    }

    /// Converts a UTF-16 code-unit offset to a UTF-8 byte offset.
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/positionmap.go:UTF16ToUTF8
    pub fn utf16_to_utf8(&self, utf16_offset: i32) -> i32 {
        if self.ascii_only {
            return utf16_offset;
        }
        // Find the last entry whose UTF-16 offset (`utf8_pos - delta`) is `<=`.
        let mut lo = 0usize;
        let mut hi = self.entries.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let utf16_pos = self.entries[mid].utf8_pos - self.entries[mid].delta;
            if utf16_pos <= utf16_offset {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return utf16_offset;
        }
        utf16_offset + self.entries[lo - 1].delta
    }
}

#[cfg(test)]
#[path = "positionmap_test.rs"]
mod tests;
