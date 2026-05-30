//! Source map mapping decoder: [`Mapping`] and [`MappingsDecoder`].
//!
//! 1:1 port of Go `internal/sourcemap/decoder.go`.

use tsgo_core::Utf16Offset;

use crate::{NameIndex, SourceIndex};

/// A single decoded mapping: a generated position optionally tied to a source
/// position and a name.
///
/// Absent fields use the `Missing*` sentinels (`-1`).
///
/// # Examples
/// ```
/// use tsgo_sourcemap::{decode_mappings, MISSING_NAME};
/// let mut decoder = decode_mappings("AAAA");
/// let m = decoder.values().next().unwrap();
/// assert_eq!(m.generated_line, 0);
/// assert_eq!(m.name_index, MISSING_NAME);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
// Go: internal/sourcemap/decoder.go:Mapping
pub struct Mapping {
    /// 0-based generated line.
    pub generated_line: i32,
    /// 0-based generated column, in UTF-16 code units.
    pub generated_character: Utf16Offset,
    /// Index into `sources`, or [`MISSING_SOURCE`].
    pub source_index: SourceIndex,
    /// 0-based source line, or [`MISSING_LINE_OR_COLUMN`].
    pub source_line: i32,
    /// 0-based source column, or [`MISSING_UTF16_COLUMN`].
    pub source_character: Utf16Offset,
    /// Index into `names`, or [`MISSING_NAME`].
    pub name_index: NameIndex,
}

/// Sentinel for a mapping with no source index.
// Go: internal/sourcemap/decoder.go
pub const MISSING_SOURCE: SourceIndex = SourceIndex(-1);
/// Sentinel for a mapping with no name index.
// Go: internal/sourcemap/decoder.go
pub const MISSING_NAME: NameIndex = NameIndex(-1);
/// Sentinel for a mapping with no source line/column.
// Go: internal/sourcemap/decoder.go
pub const MISSING_LINE_OR_COLUMN: i32 = -1;
/// Sentinel for a mapping with no source UTF-16 column.
// Go: internal/sourcemap/decoder.go
pub const MISSING_UTF16_COLUMN: Utf16Offset = Utf16Offset(-1);

impl Mapping {
    /// Reports whether two mappings have identical field values.
    ///
    /// # Examples
    /// ```
    /// use tsgo_sourcemap::decode_mappings;
    /// let m = decode_mappings("AAAA").values().next().unwrap();
    /// assert!(m.equals(&m));
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/sourcemap/decoder.go:Mapping.Equals
    pub fn equals(&self, other: &Mapping) -> bool {
        self == other
    }

    /// Reports whether this mapping carries source position information.
    ///
    /// # Examples
    /// ```
    /// use tsgo_sourcemap::decode_mappings;
    /// assert!(decode_mappings("AAAA").values().next().unwrap().is_source_mapping());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/sourcemap/decoder.go:Mapping.IsSourceMapping
    pub fn is_source_mapping(&self) -> bool {
        self.source_index != MISSING_SOURCE
            && self.source_line != MISSING_LINE_OR_COLUMN
            && self.source_character != MISSING_UTF16_COLUMN
    }
}

/// Streaming decoder over a Base64 VLQ `mappings` string.
///
/// Yields [`Mapping`]s via its [`Iterator`] implementation (or
/// [`MappingsDecoder::values`]); on malformed input it stops early and records
/// the reason in [`MappingsDecoder::error`].
///
/// # Examples
/// ```
/// use tsgo_sourcemap::decode_mappings;
/// let count = decode_mappings("AAAA,CAAC").values().count();
/// assert_eq!(count, 2);
/// ```
// Go: internal/sourcemap/decoder.go:MappingsDecoder
pub struct MappingsDecoder {
    mappings: String,
    done: bool,
    pos: usize,
    generated_line: i32,
    generated_character: Utf16Offset,
    source_index: SourceIndex,
    source_line: i32,
    source_character: Utf16Offset,
    name_index: NameIndex,
    error: Option<crate::SourceMapError>,
}

/// Creates a decoder over `mappings`.
///
/// # Examples
/// ```
/// use tsgo_sourcemap::decode_mappings;
/// let decoder = decode_mappings("AAAA");
/// assert_eq!(decoder.mappings_string(), "AAAA");
/// ```
///
/// Side effects: none (pure constructor).
// Go: internal/sourcemap/decoder.go:DecodeMappings
pub fn decode_mappings(mappings: &str) -> MappingsDecoder {
    MappingsDecoder {
        mappings: mappings.to_string(),
        done: false,
        pos: 0,
        generated_line: 0,
        generated_character: Utf16Offset(0),
        source_index: SourceIndex(0),
        source_line: 0,
        source_character: Utf16Offset(0),
        name_index: NameIndex(0),
        error: None,
    }
}

impl MappingsDecoder {
    /// Returns the original `mappings` string being decoded.
    ///
    /// Side effects: none (pure).
    // Go: internal/sourcemap/decoder.go:MappingsString
    pub fn mappings_string(&self) -> &str {
        &self.mappings
    }

    /// Returns the current byte offset within the `mappings` string.
    ///
    /// Side effects: none (pure).
    // Go: internal/sourcemap/decoder.go:Pos
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Returns the decode error, if any was recorded.
    ///
    /// Side effects: none (pure).
    // Go: internal/sourcemap/decoder.go:Error
    pub fn error(&self) -> Option<crate::SourceMapError> {
        self.error
    }

    /// Returns a [`Mapping`] capturing the full current decoder state (both
    /// source and name fields populated).
    ///
    /// Side effects: none (pure).
    // Go: internal/sourcemap/decoder.go:State
    pub fn state(&self) -> Mapping {
        self.capture_mapping(true, true)
    }

    /// Returns an iterator over the remaining mappings.
    ///
    /// Side effects: advances the decoder as the iterator is consumed.
    // Go: internal/sourcemap/decoder.go:Values
    pub fn values(&mut self) -> impl Iterator<Item = Mapping> + '_ {
        self.by_ref()
    }

    fn capture_mapping(&self, has_source: bool, has_name: bool) -> Mapping {
        Mapping {
            generated_line: self.generated_line,
            generated_character: self.generated_character,
            source_index: if has_source {
                self.source_index
            } else {
                MISSING_SOURCE
            },
            source_line: if has_source {
                self.source_line
            } else {
                MISSING_LINE_OR_COLUMN
            },
            source_character: if has_source {
                self.source_character
            } else {
                MISSING_UTF16_COLUMN
            },
            name_index: if has_name {
                self.name_index
            } else {
                MISSING_NAME
            },
        }
    }

    fn stop_iterating(&mut self) -> Option<Mapping> {
        self.done = true;
        None
    }

    fn set_error(&mut self, err: &'static str) {
        self.error = Some(crate::SourceMapError(err));
    }

    fn set_error_and_stop_iterating(&mut self, err: &'static str) -> Option<Mapping> {
        self.set_error(err);
        self.stop_iterating()
    }

    fn has_reported_error(&self) -> bool {
        self.error.is_some()
    }

    fn is_source_mapping_segment_end(&self) -> bool {
        self.pos == self.mappings.len()
            || self.mappings.as_bytes()[self.pos] == b','
            || self.mappings.as_bytes()[self.pos] == b';'
    }

    fn base64_vlq_format_decode(&mut self) -> i32 {
        let mut more_digits = true;
        let mut shift_count = 0;
        let mut value: i32 = 0;
        while more_digits {
            if self.pos >= self.mappings.len() {
                self.set_error("Error in decoding base64VLQFormatDecode, past the mapping string");
                return -1;
            }

            // 6 digit number
            let current_byte = base64_format_decode(self.mappings.as_bytes()[self.pos]);
            if current_byte == -1 {
                self.set_error("Invalid character in VLQ");
                return -1;
            }

            // If msb is set, we still have more bits to continue
            more_digits = (current_byte & 32) != 0;

            // Least significant 5 bits are the next msbs in the final value.
            value |= (current_byte & 31) << shift_count;
            shift_count += 5;
            self.pos += 1;
        }

        // Least significant bit of 1 represents a negative; rest is the magnitude.
        if (value & 1) == 0 {
            value >> 1
        } else {
            -(value >> 1)
        }
    }
}

impl Iterator for MappingsDecoder {
    type Item = Mapping;

    // Go: internal/sourcemap/decoder.go:Next
    fn next(&mut self) -> Option<Mapping> {
        while !self.done && self.pos < self.mappings.len() {
            let ch = self.mappings.as_bytes()[self.pos];
            if ch == b';' {
                // new line
                self.generated_line += 1;
                self.generated_character = Utf16Offset(0);
                self.pos += 1;
                continue;
            }

            if ch == b',' {
                // Next entry is on same line - no action needed
                self.pos += 1;
                continue;
            }

            let mut has_source = false;
            let mut has_name = false;
            self.generated_character =
                Utf16Offset(self.generated_character.0 + self.base64_vlq_format_decode());
            if self.has_reported_error() {
                return self.stop_iterating();
            }
            if self.generated_character.0 < 0 {
                return self.set_error_and_stop_iterating("Invalid generatedCharacter found");
            }

            if !self.is_source_mapping_segment_end() {
                has_source = true;

                self.source_index =
                    SourceIndex(self.source_index.0 + self.base64_vlq_format_decode());
                if self.has_reported_error() {
                    return self.stop_iterating();
                }
                if self.source_index.0 < 0 {
                    return self.set_error_and_stop_iterating("Invalid sourceIndex found");
                }
                if self.is_source_mapping_segment_end() {
                    return self.set_error_and_stop_iterating(
                        "Unsupported Format: No entries after sourceIndex",
                    );
                }

                self.source_line += self.base64_vlq_format_decode();
                if self.has_reported_error() {
                    return self.stop_iterating();
                }
                if self.source_line < 0 {
                    return self.set_error_and_stop_iterating("Invalid sourceLine found");
                }
                if self.is_source_mapping_segment_end() {
                    return self.set_error_and_stop_iterating(
                        "Unsupported Format: No entries after sourceLine",
                    );
                }

                self.source_character =
                    Utf16Offset(self.source_character.0 + self.base64_vlq_format_decode());
                if self.has_reported_error() {
                    return self.stop_iterating();
                }
                if self.source_character.0 < 0 {
                    return self.set_error_and_stop_iterating("Invalid sourceCharacter found");
                }

                if !self.is_source_mapping_segment_end() {
                    has_name = true;
                    self.name_index =
                        NameIndex(self.name_index.0 + self.base64_vlq_format_decode());
                    if self.has_reported_error() {
                        return self.stop_iterating();
                    }
                    if self.name_index.0 < 0 {
                        return self.set_error_and_stop_iterating("Invalid nameIndex found");
                    }

                    if !self.is_source_mapping_segment_end() {
                        return self.set_error_and_stop_iterating(
                            "Unsupported Error Format: Entries after nameIndex",
                        );
                    }
                }
            }

            return Some(self.capture_mapping(has_source, has_name));
        }

        self.stop_iterating()
    }
}

fn base64_format_decode(ch: u8) -> i32 {
    match ch {
        b'A'..=b'Z' => (ch - b'A') as i32,
        b'a'..=b'z' => (ch - b'a') as i32 + 26,
        b'0'..=b'9' => (ch - b'0') as i32 + 52,
        b'+' => 62,
        b'/' => 63,
        _ => -1,
    }
}

#[cfg(test)]
#[path = "decoder_test.rs"]
mod tests;
