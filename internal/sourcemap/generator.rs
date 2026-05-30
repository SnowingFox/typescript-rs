//! Source map [`Generator`] and the serialized [`RawSourceMap`] structure.
//!
//! 1:1 port of Go `internal/sourcemap/generator.go`.

use base64::Engine;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use tsgo_core::Utf16Offset;
use tsgo_tspath::{get_relative_path_to_directory_or_url, ComparePathsOptions};

use crate::SourceMapError;

/// Index of a source file within a source map's `sources` array.
///
/// A value of `-1` (`sourceIndexNotSet`) denotes "no source".
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
// Go: internal/sourcemap/generator.go:SourceIndex
pub struct SourceIndex(pub i32);

const SOURCE_INDEX_NOT_SET: SourceIndex = SourceIndex(-1);
const NAME_INDEX_NOT_SET: NameIndex = NameIndex(-1);
const NOT_SET: i32 = -1;
const NOT_SET_UTF16: Utf16Offset = Utf16Offset(-1);

/// Index of a name within a source map's `names` array.
///
/// A value of `-1` (`nameIndexNotSet`) denotes "no name".
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
// Go: internal/sourcemap/generator.go:NameIndex
pub struct NameIndex(pub i32);

/// The deserialized Source Map v3 document.
///
/// Field declaration order matches the JSON key order Go emits, which the
/// serialization tests assert byte-for-byte: `version`, `file`, `sourceRoot`,
/// `sources`, `names`, `mappings`, `sourcesContent`.
///
/// `sources_content` is `None` when unset (the field is omitted entirely,
/// mirroring Go's `omitzero`), and `Some(..)` with `null` placeholders for
/// sources that have no inlined content.
///
/// # Examples
/// ```
/// use tsgo_sourcemap::RawSourceMap;
/// let map = RawSourceMap {
///     version: 3,
///     file: "main.js".to_string(),
///     source_root: "/".to_string(),
///     sources: vec![],
///     names: vec![],
///     mappings: String::new(),
///     sources_content: None,
/// };
/// assert_eq!(map.version, 3);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
// Go: internal/sourcemap/generator.go:RawSourceMap
pub struct RawSourceMap {
    /// Source map format version (always `3`).
    pub version: i32,
    /// Generated file name this map applies to.
    pub file: String,
    /// Path prepended to each entry in `sources`.
    pub source_root: String,
    /// Original source file paths, relative to `source_root`.
    pub sources: Vec<String>,
    /// Symbol names referenced by name-bearing mappings.
    pub names: Vec<String>,
    /// Base64 VLQ encoded mappings.
    pub mappings: String,
    /// Optional inlined source contents, parallel to `sources`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources_content: Option<Vec<Option<String>>>,
}

/// Incrementally builds a Source Map v3 from generated/source position pairs.
///
/// Positions are accumulated via the `add_*_mapping` methods and encoded lazily
/// into the `mappings` string; call [`Generator::raw_source_map`],
/// [`Generator::to_string`], or [`Generator::base64_data_url`] to obtain output.
///
/// # Examples
/// ```
/// use tsgo_sourcemap::Generator;
/// use tsgo_tspath::ComparePathsOptions;
/// let mut gen = Generator::new("main.js", "/", "/", ComparePathsOptions::default());
/// assert_eq!(gen.raw_source_map().version, 3);
/// ```
#[derive(Default)]
// Go: internal/sourcemap/generator.go:Generator
pub struct Generator {
    path_options: ComparePathsOptions,
    file: String,
    source_root: String,
    sources_directory_path: String,
    raw_sources: Vec<String>,
    sources: Vec<String>,
    source_to_source_index_map: FxHashMap<String, SourceIndex>,
    sources_content: Vec<Option<String>>,
    names: Vec<String>,
    name_to_name_index_map: FxHashMap<String, NameIndex>,
    mappings: String,
    last_generated_line: i32,
    last_generated_character: Utf16Offset,
    last_source_index: SourceIndex,
    last_source_line: i32,
    last_source_character: Utf16Offset,
    last_name_index: NameIndex,
    has_last: bool,
    pending_generated_line: i32,
    pending_generated_character: Utf16Offset,
    pending_source_index: SourceIndex,
    pending_source_line: i32,
    pending_source_character: Utf16Offset,
    pending_name_index: NameIndex,
    has_pending: bool,
    has_pending_source: bool,
    has_pending_name: bool,
}

impl Generator {
    /// Creates a generator for `file`, prefixing emitted sources with
    /// `source_root` and relativizing added sources against
    /// `sources_directory_path` using `options`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_sourcemap::Generator;
    /// use tsgo_tspath::ComparePathsOptions;
    /// let gen = Generator::new("main.js", "/", "/", ComparePathsOptions::default());
    /// assert!(gen.sources().is_empty());
    /// ```
    ///
    /// Side effects: none (pure constructor).
    // Go: internal/sourcemap/generator.go:NewGenerator
    pub fn new(
        file: &str,
        source_root: &str,
        sources_directory_path: &str,
        options: ComparePathsOptions,
    ) -> Generator {
        Generator {
            file: file.to_string(),
            source_root: source_root.to_string(),
            sources_directory_path: sources_directory_path.to_string(),
            path_options: options,
            ..Default::default()
        }
    }

    /// Returns the original (non-relativized) file names of added sources.
    ///
    /// Side effects: none (pure).
    // Go: internal/sourcemap/generator.go:Sources (returns `rawSources`, not the
    // relativized `sources`, by design).
    #[allow(clippy::misnamed_getters)]
    pub fn sources(&self) -> &[String] {
        &self.raw_sources
    }

    /// Adds a source to the map, returning its index. The relativized path (used
    /// for deduplication and in the `sources` array) is computed from
    /// `file_name` against the generator's sources directory; re-adding an
    /// equivalent path returns the existing index.
    ///
    /// # Examples
    /// ```
    /// use tsgo_sourcemap::{Generator, SourceIndex};
    /// use tsgo_tspath::ComparePathsOptions;
    /// let mut gen = Generator::new("main.js", "/", "/", ComparePathsOptions::default());
    /// assert_eq!(gen.add_source("/main.ts"), SourceIndex(0));
    /// assert_eq!(gen.add_source("/main.ts"), SourceIndex(0));
    /// ```
    ///
    /// Side effects: appends to the `sources`/`rawSources` lists on first use.
    // Go: internal/sourcemap/generator.go:AddSource
    pub fn add_source(&mut self, file_name: &str) -> SourceIndex {
        let source = get_relative_path_to_directory_or_url(
            &self.sources_directory_path,
            file_name,
            true, // is_absolute_path_an_url
            &self.path_options,
        );

        if let Some(&source_index) = self.source_to_source_index_map.get(&source) {
            return source_index;
        }

        let source_index = SourceIndex(self.sources.len() as i32);
        self.sources.push(source.clone());
        self.raw_sources.push(file_name.to_string());
        self.source_to_source_index_map.insert(source, source_index);
        source_index
    }

    /// Sets the inlined `content` for a previously added source.
    ///
    /// # Examples
    /// ```
    /// use tsgo_sourcemap::Generator;
    /// use tsgo_tspath::ComparePathsOptions;
    /// let mut gen = Generator::new("main.js", "/", "/", ComparePathsOptions::default());
    /// let idx = gen.add_source("/main.ts");
    /// assert!(gen.set_source_content(idx, "foo").is_ok());
    /// ```
    ///
    /// Side effects: extends and updates the `sourcesContent` list.
    // Go: internal/sourcemap/generator.go:SetSourceContent
    pub fn set_source_content(
        &mut self,
        source_index: SourceIndex,
        content: &str,
    ) -> Result<(), SourceMapError> {
        if source_index.0 < 0 || source_index.0 as usize >= self.sources.len() {
            return Err(SourceMapError("sourceIndex is out of range"));
        }
        while self.sources_content.len() <= source_index.0 as usize {
            self.sources_content.push(None);
        }
        self.sources_content[source_index.0 as usize] = Some(content.to_string());
        Ok(())
    }

    /// Declares a name in the map, returning its index; re-adding an existing
    /// name returns the previous index.
    ///
    /// # Examples
    /// ```
    /// use tsgo_sourcemap::{Generator, NameIndex};
    /// use tsgo_tspath::ComparePathsOptions;
    /// let mut gen = Generator::new("main.js", "/", "/", ComparePathsOptions::default());
    /// assert_eq!(gen.add_name("foo"), NameIndex(0));
    /// assert_eq!(gen.add_name("foo"), NameIndex(0));
    /// ```
    ///
    /// Side effects: appends to the `names` list on first use.
    // Go: internal/sourcemap/generator.go:AddName
    pub fn add_name(&mut self, name: &str) -> NameIndex {
        if let Some(&name_index) = self.name_to_name_index_map.get(name) {
            return name_index;
        }
        let name_index = NameIndex(self.names.len() as i32);
        self.names.push(name.to_string());
        self.name_to_name_index_map
            .insert(name.to_string(), name_index);
        name_index
    }

    /// Returns the accumulated state as a [`RawSourceMap`].
    ///
    /// Side effects: flushes any pending mapping into the `mappings` string.
    // Go: internal/sourcemap/generator.go:RawSourceMap
    pub fn raw_source_map(&mut self) -> RawSourceMap {
        self.commit_pending_mapping();
        RawSourceMap {
            version: 3,
            file: self.file.clone(),
            source_root: self.source_root.clone(),
            sources: self.sources.clone(),
            names: self.names.clone(),
            mappings: self.mappings.clone(),
            sources_content: if self.sources_content.is_empty() {
                None
            } else {
                Some(self.sources_content.clone())
            },
        }
    }

    fn is_new_generated_position(
        &self,
        generated_line: i32,
        generated_character: Utf16Offset,
    ) -> bool {
        !self.has_pending
            || self.pending_generated_line != generated_line
            || self.pending_generated_character != generated_character
    }

    fn is_backtracking_source_position(
        &self,
        source_index: SourceIndex,
        source_line: i32,
        source_character: Utf16Offset,
    ) -> bool {
        source_index != SOURCE_INDEX_NOT_SET
            && source_line != NOT_SET
            && source_character != NOT_SET_UTF16
            && self.pending_source_index == source_index
            && (self.pending_source_line > source_line
                || self.pending_source_line == source_line
                    && self.pending_source_character > source_character)
    }

    fn should_commit_mapping(&self) -> bool {
        self.has_pending
            && (!self.has_last
                || self.last_generated_line != self.pending_generated_line
                || self.last_generated_character != self.pending_generated_character
                || self.last_source_index != self.pending_source_index
                || self.last_source_line != self.pending_source_line
                || self.last_source_character != self.pending_source_character
                || self.last_name_index != self.pending_name_index)
    }

    fn append_mapping_char_code(&mut self, char_code: char) {
        self.mappings.push(char_code);
    }

    fn append_base64_vlq(&mut self, mut in_value: i32) {
        // Add a new least significant bit that has the sign of the value.
        // If negative, the appended least significant bit has value 1; otherwise 0.
        // e.g. -1 => binary 01[1] => 3; +1 => binary 01[0] => 2.
        if in_value < 0 {
            in_value = ((-in_value) << 1) + 1;
        } else {
            in_value <<= 1;
        }

        // Encode 5 bits at a time starting from least significant bits.
        loop {
            let mut current_digit = in_value & 31; // 11111
            in_value >>= 5;
            if in_value > 0 {
                // There are still more digits to encode, set the msb (6th bit).
                current_digit |= 32;
            }
            self.append_mapping_char_code(base64_format_encode(current_digit));
            if in_value <= 0 {
                break;
            }
        }
    }

    fn commit_pending_mapping(&mut self) {
        if !self.should_commit_mapping() {
            return;
        }

        // Line/Comma delimiters
        if self.last_generated_line < self.pending_generated_line {
            // Emit line delimiters
            loop {
                self.append_mapping_char_code(';');
                self.last_generated_line += 1;
                if self.last_generated_line >= self.pending_generated_line {
                    break;
                }
            }
            // Only need to set this once
            self.last_generated_character = Utf16Offset(0);
        } else {
            if self.last_generated_line != self.pending_generated_line {
                // panic rather than error as an invariant has been violated
                panic!("generatedLine cannot backtrack");
            }
            // Emit comma to separate the entry
            if self.has_last {
                self.append_mapping_char_code(',');
            }
        }

        // 1. Relative generated character
        self.append_base64_vlq(
            self.pending_generated_character.0 - self.last_generated_character.0,
        );
        self.last_generated_character = self.pending_generated_character;

        if self.has_pending_source {
            // 2. Relative sourceIndex
            self.append_base64_vlq(self.pending_source_index.0 - self.last_source_index.0);
            self.last_source_index = self.pending_source_index;

            // 3. Relative source line
            self.append_base64_vlq(self.pending_source_line - self.last_source_line);
            self.last_source_line = self.pending_source_line;

            // 4. Relative source character
            self.append_base64_vlq(self.pending_source_character.0 - self.last_source_character.0);
            self.last_source_character = self.pending_source_character;

            if self.has_pending_name {
                // 5. Relative nameIndex
                self.append_base64_vlq(self.pending_name_index.0 - self.last_name_index.0);
                self.last_name_index = self.pending_name_index;
            }
        }

        self.has_last = true;
    }

    #[allow(clippy::too_many_arguments)]
    fn add_mapping(
        &mut self,
        generated_line: i32,
        generated_character: Utf16Offset,
        source_index: SourceIndex,
        source_line: i32,
        source_character: Utf16Offset,
        name_index: NameIndex,
    ) {
        if self.is_new_generated_position(generated_line, generated_character)
            || self.is_backtracking_source_position(source_index, source_line, source_character)
        {
            self.commit_pending_mapping();
            self.pending_generated_line = generated_line;
            self.pending_generated_character = generated_character;
            self.has_pending_source = false;
            self.has_pending_name = false;
            self.has_pending = true;
        }

        if source_index != SOURCE_INDEX_NOT_SET
            && source_line != NOT_SET
            && source_character != NOT_SET_UTF16
        {
            self.pending_source_index = source_index;
            self.pending_source_line = source_line;
            self.pending_source_character = source_character;
            self.has_pending_source = true;
            if name_index != NAME_INDEX_NOT_SET {
                self.pending_name_index = name_index;
                self.has_pending_name = true;
            }
        }
    }

    /// Adds a mapping for a generated position with no source information.
    ///
    /// # Examples
    /// ```
    /// use tsgo_sourcemap::Generator;
    /// use tsgo_core::Utf16Offset;
    /// use tsgo_tspath::ComparePathsOptions;
    /// let mut gen = Generator::new("main.js", "/", "/", ComparePathsOptions::default());
    /// gen.add_generated_mapping(0, Utf16Offset(0)).unwrap();
    /// assert_eq!(gen.raw_source_map().mappings, "A");
    /// ```
    ///
    /// Side effects: updates pending mapping state (may flush a prior mapping).
    // Go: internal/sourcemap/generator.go:AddGeneratedMapping
    pub fn add_generated_mapping(
        &mut self,
        generated_line: i32,
        generated_character: Utf16Offset,
    ) -> Result<(), SourceMapError> {
        if generated_line < self.pending_generated_line {
            return Err(SourceMapError("generatedLine cannot backtrack"));
        }
        if generated_character.0 < 0 {
            return Err(SourceMapError("generatedCharacter cannot be negative"));
        }
        self.add_mapping(
            generated_line,
            generated_character,
            SOURCE_INDEX_NOT_SET,
            NOT_SET,       // source_line
            NOT_SET_UTF16, // source_character
            NAME_INDEX_NOT_SET,
        );
        Ok(())
    }

    /// Adds a mapping that ties a generated position to a source position.
    ///
    /// # Examples
    /// ```
    /// use tsgo_sourcemap::Generator;
    /// use tsgo_core::Utf16Offset;
    /// use tsgo_tspath::ComparePathsOptions;
    /// let mut gen = Generator::new("main.js", "/", "/", ComparePathsOptions::default());
    /// let idx = gen.add_source("/main.ts");
    /// gen.add_source_mapping(0, Utf16Offset(0), idx, 0, Utf16Offset(0)).unwrap();
    /// assert_eq!(gen.raw_source_map().mappings, "AAAA");
    /// ```
    ///
    /// Side effects: updates pending mapping state (may flush a prior mapping).
    // Go: internal/sourcemap/generator.go:AddSourceMapping
    pub fn add_source_mapping(
        &mut self,
        generated_line: i32,
        generated_character: Utf16Offset,
        source_index: SourceIndex,
        source_line: i32,
        source_character: Utf16Offset,
    ) -> Result<(), SourceMapError> {
        if generated_line < self.pending_generated_line {
            return Err(SourceMapError("generatedLine cannot backtrack"));
        }
        if generated_character.0 < 0 {
            return Err(SourceMapError("generatedCharacter cannot be negative"));
        }
        if source_index.0 < 0 || source_index.0 as usize >= self.sources.len() {
            return Err(SourceMapError("sourceIndex is out of range"));
        }
        if source_line < 0 {
            return Err(SourceMapError("sourceLine cannot be negative"));
        }
        if source_character.0 < 0 {
            return Err(SourceMapError("sourceCharacter cannot be negative"));
        }
        self.add_mapping(
            generated_line,
            generated_character,
            source_index,
            source_line,
            source_character,
            NAME_INDEX_NOT_SET,
        );
        Ok(())
    }

    /// Adds a mapping that ties a generated position to a source position and a
    /// name in the `names` array.
    ///
    /// # Examples
    /// ```
    /// use tsgo_sourcemap::Generator;
    /// use tsgo_core::Utf16Offset;
    /// use tsgo_tspath::ComparePathsOptions;
    /// let mut gen = Generator::new("main.js", "/", "/", ComparePathsOptions::default());
    /// let idx = gen.add_source("/main.ts");
    /// let name = gen.add_name("foo");
    /// gen.add_named_source_mapping(0, Utf16Offset(0), idx, 0, Utf16Offset(0), name).unwrap();
    /// assert_eq!(gen.raw_source_map().mappings, "AAAAA");
    /// ```
    ///
    /// Side effects: updates pending mapping state (may flush a prior mapping).
    // Go: internal/sourcemap/generator.go:AddNamedSourceMapping
    #[allow(clippy::too_many_arguments)]
    pub fn add_named_source_mapping(
        &mut self,
        generated_line: i32,
        generated_character: Utf16Offset,
        source_index: SourceIndex,
        source_line: i32,
        source_character: Utf16Offset,
        name_index: NameIndex,
    ) -> Result<(), SourceMapError> {
        if generated_line < self.pending_generated_line {
            return Err(SourceMapError("generatedLine cannot backtrack"));
        }
        if generated_character.0 < 0 {
            return Err(SourceMapError("generatedCharacter cannot be negative"));
        }
        if source_index.0 < 0 || source_index.0 as usize >= self.sources.len() {
            return Err(SourceMapError("sourceIndex is out of range"));
        }
        if source_line < 0 {
            return Err(SourceMapError("sourceLine cannot be negative"));
        }
        if source_character.0 < 0 {
            return Err(SourceMapError("sourceCharacter cannot be negative"));
        }
        if name_index.0 < 0 || name_index.0 as usize >= self.names.len() {
            return Err(SourceMapError("nameIndex is out of range"));
        }
        self.add_mapping(
            generated_line,
            generated_character,
            source_index,
            source_line,
            source_character,
            name_index,
        );
        Ok(())
    }

    fn bytes(&mut self) -> Vec<u8> {
        match tsgo_json::marshal(&self.raw_source_map()) {
            Ok(buf) => buf,
            Err(e) => panic!("{e}"),
        }
    }

    /// Returns the JSON string representation of the source map.
    ///
    /// Side effects: flushes any pending mapping into the `mappings` string.
    // Go: internal/sourcemap/generator.go:String
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&mut self) -> String {
        String::from_utf8(self.bytes()).expect("source map JSON is valid UTF-8")
    }

    /// Returns the source map as a `data:` URL with the JSON payload encoded as
    /// standard (padded) base64.
    ///
    /// # Examples
    /// ```
    /// use tsgo_sourcemap::Generator;
    /// use tsgo_tspath::ComparePathsOptions;
    /// let mut gen = Generator::new("main.js", "/", "/", ComparePathsOptions::default());
    /// assert!(gen.base64_data_url().starts_with("data:application/json;base64,"));
    /// ```
    ///
    /// Side effects: flushes any pending mapping into the `mappings` string.
    // Go: internal/sourcemap/generator.go:Base64DataURL
    pub fn base64_data_url(&mut self) -> String {
        const PREFIX: &str = "data:application/json;base64,";
        let data = self.bytes();
        let mut result = String::with_capacity(PREFIX.len() + data.len().div_ceil(3) * 4);
        result.push_str(PREFIX);
        base64::engine::general_purpose::STANDARD.encode_string(&data, &mut result);
        result
    }
}

fn base64_format_encode(value: i32) -> char {
    match value {
        0..=25 => (b'A' + value as u8) as char,
        26..=51 => (b'a' + (value - 26) as u8) as char,
        52..=61 => (b'0' + (value - 52) as u8) as char,
        62 => '+',
        63 => '/',
        _ => panic!("not a base64 value"),
    }
}

#[cfg(test)]
#[path = "generator_test.rs"]
mod tests;
