//! Bidirectional generated/source position mapping ([`DocumentPositionMapper`]).
//!
//! 1:1 port of Go `internal/sourcemap/source_mapper.go`.

use base64::Engine;
use rustc_hash::FxHashMap;
use tsgo_tspath as tspath;

use crate::{decode_mappings, EcmaLineInfo, NameIndex, RawSourceMap, SourceIndex, MISSING_SOURCE};

/// Filesystem/line-info source for resolving and parsing source maps.
// Go: internal/sourcemap/source_mapper.go:Host
pub trait Host {
    /// Reports whether file names are compared case-sensitively.
    fn use_case_sensitive_file_names(&self) -> bool;
    /// Returns line-start information for `file_name`, or `None` if unavailable.
    fn get_ecma_line_info(&self, file_name: &str) -> Option<EcmaLineInfo>;
    /// Reads `file_name`, returning its contents or `None` if it is missing.
    fn read_file(&self, file_name: &str) -> Option<String>;
}

const MISSING_POSITION: i32 = -1;

/// A mapping expressed with absolute byte positions rather than line/column.
// Go: internal/sourcemap/source_mapper.go:MappedPosition
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MappedPosition {
    generated_position: i32,
    source_position: i32,
    source_index: SourceIndex,
    name_index: NameIndex,
}

impl MappedPosition {
    fn is_source_mapped_position(&self) -> bool {
        self.source_index != MISSING_SOURCE && self.source_position != MISSING_POSITION
    }
}

/// Alias for a [`MappedPosition`] grouped under its source.
// Go: internal/sourcemap/source_mapper.go:SourceMappedPosition
pub type SourceMappedPosition = MappedPosition;

/// Maps positions in a generated file to original sources and vice versa.
// Go: internal/sourcemap/source_mapper.go:DocumentPositionMapper
pub struct DocumentPositionMapper {
    use_case_sensitive_file_names: bool,
    source_file_absolute_paths: Vec<String>,
    source_to_source_index_map: FxHashMap<String, SourceIndex>,
    generated_absolute_file_path: String,
    generated_mappings: Vec<MappedPosition>,
    source_mappings: FxHashMap<SourceIndex, Vec<SourceMappedPosition>>,
}

/// A resolved position within a specific file.
// Go: internal/sourcemap/source_mapper.go:DocumentPosition
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentPosition {
    /// Absolute path of the file the position refers to.
    pub file_name: String,
    /// Byte offset within `file_name`.
    pub pos: i32,
}

fn create_document_position_mapper(
    host: &dyn Host,
    source_map: &RawSourceMap,
    map_path: &str,
) -> DocumentPositionMapper {
    let map_directory = tspath::get_directory_path(map_path);
    let source_root = if !source_map.source_root.is_empty() {
        tspath::get_normalized_absolute_path(&source_map.source_root, &map_directory)
    } else {
        map_directory.clone()
    };
    let generated_absolute_file_path =
        tspath::get_normalized_absolute_path(&source_map.file, &map_directory);
    let source_file_absolute_paths: Vec<String> = source_map
        .sources
        .iter()
        .map(|source| tspath::get_normalized_absolute_path(source, &source_root))
        .collect();
    let use_case_sensitive_file_names = host.use_case_sensitive_file_names();
    let mut source_to_source_index_map: FxHashMap<String, SourceIndex> =
        FxHashMap::with_capacity_and_hasher(source_file_absolute_paths.len(), Default::default());
    for (i, source) in source_file_absolute_paths.iter().enumerate() {
        source_to_source_index_map.insert(
            tspath::get_canonical_file_name(source, use_case_sensitive_file_names),
            SourceIndex(i as i32),
        );
    }

    let mut decoded_mappings: Vec<MappedPosition> = Vec::new();

    // getDecodedMappings()
    let mut decoder = decode_mappings(&source_map.mappings);
    for mapping in decoder.by_ref() {
        // processMapping()
        let mut generated_position = -1;
        if let Some(line_info) = host.get_ecma_line_info(&generated_absolute_file_path) {
            generated_position = tsgo_scanner::compute_position_of_line_and_utf16_character(
                line_info.line_starts(),
                mapping.generated_line,
                mapping.generated_character,
                line_info.text(),
                true, // allow_edits
            );
        }

        let mut source_position = -1;
        if mapping.is_source_mapping() {
            if let Some(line_info) = host
                .get_ecma_line_info(&source_file_absolute_paths[mapping.source_index.0 as usize])
            {
                source_position = tsgo_scanner::compute_position_of_line_and_utf16_character(
                    line_info.line_starts(),
                    mapping.source_line,
                    mapping.source_character,
                    line_info.text(),
                    true, // allow_edits
                );
            }
        }

        decoded_mappings.push(MappedPosition {
            generated_position,
            source_index: mapping.source_index,
            source_position,
            name_index: mapping.name_index,
        });
    }
    if decoder.error().is_some() {
        decoded_mappings = Vec::new();
    }

    // getSourceMappings()
    let mut source_mappings: FxHashMap<SourceIndex, Vec<SourceMappedPosition>> =
        FxHashMap::default();
    for mapping in &decoded_mappings {
        if !mapping.is_source_mapped_position() {
            continue;
        }
        let source_index = mapping.source_index;
        source_mappings
            .entry(source_index)
            .or_default()
            .push(SourceMappedPosition {
                generated_position: mapping.generated_position,
                source_index,
                source_position: mapping.source_position,
                name_index: mapping.name_index,
            });
    }
    for list in source_mappings.values_mut() {
        list.sort_by(|a, b| {
            tsgo_debug::assert(
                a.source_index == b.source_index,
                Some("All source mappings should have the same source index"),
            );
            a.source_position.cmp(&b.source_position)
        });
        *list = tsgo_core::deduplicate_sorted(list, |a, b| {
            a.generated_position == b.generated_position
                && a.source_index == b.source_index
                && a.source_position == b.source_position
        });
    }

    // getGeneratedMappings()
    let mut generated_mappings = decoded_mappings;
    generated_mappings.sort_by(|a, b| a.generated_position.cmp(&b.generated_position));
    let generated_mappings = tsgo_core::deduplicate_sorted(&generated_mappings, |a, b| {
        a.generated_position == b.generated_position
            && a.source_index == b.source_index
            && a.source_position == b.source_position
    });

    DocumentPositionMapper {
        use_case_sensitive_file_names,
        source_file_absolute_paths,
        source_to_source_index_map,
        generated_absolute_file_path,
        generated_mappings,
        source_mappings,
    }
}

impl DocumentPositionMapper {
    /// Maps a generated position back to its closest original source position.
    ///
    /// Side effects: none (pure).
    // Go: internal/sourcemap/source_mapper.go:GetSourcePosition
    pub fn get_source_position(&self, loc: &DocumentPosition) -> Option<DocumentPosition> {
        if self.generated_mappings.is_empty() {
            return None;
        }

        let target_index = match self
            .generated_mappings
            .binary_search_by(|m| m.generated_position.cmp(&loc.pos))
        {
            Ok(i) | Err(i) => i,
        };

        if target_index >= self.generated_mappings.len() {
            return None;
        }

        let mapping = &self.generated_mappings[target_index];
        if !mapping.is_source_mapped_position() {
            return None;
        }

        // Closest position
        Some(DocumentPosition {
            file_name: self.source_file_absolute_paths[mapping.source_index.0 as usize].clone(),
            pos: mapping.source_position,
        })
    }

    /// Maps an original source position to its closest generated position.
    ///
    /// Side effects: none (pure).
    // Go: internal/sourcemap/source_mapper.go:GetGeneratedPosition
    pub fn get_generated_position(&self, loc: &DocumentPosition) -> Option<DocumentPosition> {
        let source_index =
            *self
                .source_to_source_index_map
                .get(&tspath::get_canonical_file_name(
                    &loc.file_name,
                    self.use_case_sensitive_file_names,
                ))?;
        if source_index.0 < 0 || source_index.0 as usize >= self.source_mappings.len() {
            return None;
        }
        let source_mappings = self
            .source_mappings
            .get(&source_index)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let target_index =
            match source_mappings.binary_search_by(|m| m.source_position.cmp(&loc.pos)) {
                Ok(i) | Err(i) => i,
            };

        if target_index >= source_mappings.len() {
            return None;
        }

        let mapping = &source_mappings[target_index];
        if mapping.source_index != source_index {
            return None;
        }

        // Closest position
        Some(DocumentPosition {
            file_name: self.generated_absolute_file_path.clone(),
            pos: mapping.generated_position,
        })
    }
}

/// Resolves and parses the source map for `generated_file_name`, returning a
/// [`DocumentPositionMapper`] when a valid map is found.
///
/// Side effects: reads files and source-map URLs via `host`.
// Go: internal/sourcemap/source_mapper.go:GetDocumentPositionMapper
pub fn get_document_position_mapper(
    host: &dyn Host,
    generated_file_name: &str,
) -> Option<DocumentPositionMapper> {
    let mut map_file_name = try_get_source_mapping_url(host, generated_file_name);
    if !map_file_name.is_empty() {
        let (base64_object, matched) = try_parse_base64_url(&map_file_name);
        if matched {
            if !base64_object.is_empty() {
                if let Ok(decoded) =
                    base64::engine::general_purpose::STANDARD.decode(&base64_object)
                {
                    if let Ok(decoded) = String::from_utf8(decoded) {
                        return convert_document_to_source_mapper(
                            host,
                            &decoded,
                            generated_file_name,
                        );
                    }
                }
            }
            // Not a data URL we can parse, skip it
            map_file_name = String::new();
        }
    }

    let mut possible_map_locations: Vec<String> = Vec::new();
    if !map_file_name.is_empty() {
        possible_map_locations.push(map_file_name);
    }
    possible_map_locations.push(format!("{generated_file_name}.map"));
    for location in &possible_map_locations {
        let resolved = tspath::get_normalized_absolute_path(
            location,
            &tspath::get_directory_path(generated_file_name),
        );
        if let Some(map_file_contents) = host.read_file(&resolved) {
            return convert_document_to_source_mapper(host, &map_file_contents, &resolved);
        }
    }
    None
}

fn convert_document_to_source_mapper(
    host: &dyn Host,
    contents: &str,
    map_file_name: &str,
) -> Option<DocumentPositionMapper> {
    let source_map = try_parse_raw_source_map(contents)?;
    if source_map.sources.is_empty() || source_map.file.is_empty() || source_map.mappings.is_empty()
    {
        // invalid map
        return None;
    }

    // Don't support source maps that contain inlined sources
    if let Some(contents) = &source_map.sources_content {
        if contents.iter().any(|s| s.is_some()) {
            return None;
        }
    }

    Some(create_document_position_mapper(
        host,
        &source_map,
        map_file_name,
    ))
}

fn try_parse_raw_source_map(contents: &str) -> Option<RawSourceMap> {
    let source_map: RawSourceMap = tsgo_json::unmarshal(contents.as_bytes()).ok()?;
    if source_map.version != 3 {
        return None;
    }
    Some(source_map)
}

fn try_get_source_mapping_url(host: &dyn Host, file_name: &str) -> String {
    let line_info = host.get_ecma_line_info(file_name);
    crate::try_get_source_mapping_url(line_info.as_ref())
}

// Equivalent to /^data:(?:application\/json;(?:charset=[uU][tT][fF]-8;)?base64,([A-Za-z0-9+/=]+)$)?/
fn try_parse_base64_url(url: &str) -> (String, bool) {
    let Some(url) = url.strip_prefix("data:") else {
        return (String::new(), false);
    };
    let Some(url) = url.strip_prefix("application/json;") else {
        return (String::new(), true);
    };
    let url = if let Some(rest) = url.strip_prefix("charset=") {
        const UTF8: &str = "utf-8;";
        if rest.len() < UTF8.len() || !rest[..UTF8.len()].eq_ignore_ascii_case(UTF8) {
            return (String::new(), true);
        }
        &rest[UTF8.len()..]
    } else {
        url
    };
    let Some(url) = url.strip_prefix("base64,") else {
        return (String::new(), true);
    };
    for r in url.chars() {
        if !(tsgo_stringutil::is_ascii_letter(r)
            || tsgo_stringutil::is_digit(r)
            || r == '+'
            || r == '/'
            || r == '=')
        {
            return (String::new(), true);
        }
    }
    (url.to_string(), true)
}

#[cfg(test)]
#[path = "source_mapper_test.rs"]
mod tests;
