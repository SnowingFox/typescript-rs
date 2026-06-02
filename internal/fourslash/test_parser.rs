//! Port of Go `internal/fourslash/test_parser.go`: the fourslash markup parser.
//!
//! A fourslash test case is a TypeScript source file annotated with "magic"
//! markup that the parser strips out, recording the positions it occupied:
//!
//! - `/*name*/` — a **named marker** (an empty name for `/**/`), a caret point.
//! - `[|ranged text|]` — a **range marker**, a text selection; a marker may be
//!   embedded inside a range (`[|/*m*/foo|]`).
//! - `{| "key": value |}` — an **object marker** carrying JSON data; it may be
//!   anonymous (no `"name"` field) or named.
//! - `// @filename: x.ts` / `// @symlink:` / `// @option: value` directives are
//!   handled by the shared [`tsgo_testrunner`] splitter (multi-file split,
//!   symlinks, and global options).
//!
//! [`parse_test_data`] produces a [`TestData`] (the marker-stripped files plus
//! the markers and ranges with both byte offsets and LSP positions).
//!
//! # Divergence from Go
//! - Go threads [`parse_file_content`] as the `parseFile` callback into
//!   `testrunner.ParseTestFilesAndSymlinksWithOptions`, so file splitting and
//!   marker extraction interleave. The Rust [`tsgo_testrunner`] splitter does
//!   not take a callback (it yields `(name, content)` units), so this port
//!   splits first, then runs [`parse_file_content`] over each unit's content.
//!   The per-unit content is identical to Go's, so the result is the same. The
//!   only dropped facet is the per-file `emitthisfile` option (always `false`
//!   here); see [`TestFileInfo`].
//! - `RangeMarker::marker` stores an owned clone of the embedded marker (with
//!   its computed LSP position) rather than Go's shared `*Marker` pointer.
//!   Markers are immutable after parsing, so this is behaviorally identical.

use indexmap::IndexMap;
use std::rc::Rc;

use serde_json::{Map as JsonMap, Value as JsonValue};

use tsgo_core::text::{TextPos, TextRange};
use tsgo_ls_lsconv::{compute_lsp_line_starts, Converters, PositionEncodingKind, Script};
use tsgo_lsproto::{Position, Range};
use tsgo_stringutil::{is_ascii_letter, is_digit};
use tsgo_testrunner::{parse_test_files_and_symlinks_with_options, ParseTestFilesOptions};
use tsgo_tspath::get_normalized_absolute_path;

/// An error raised while parsing fourslash markup (Go reports these via
/// `t.Fatalf`; this port returns them as an error).
///
/// Side effects: none (plain data).
// Go: internal/fourslash/test_parser.go:fourslashError
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FourslashError(pub String);

impl std::fmt::Display for FourslashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FourslashError {}

/// A text selection inserted by surrounding the desired text with `[|` and `|]`,
/// e.g. `[|text in range|]` selects `text in range`.
///
/// Side effects: none (plain data).
// Go: internal/fourslash/test_parser.go:RangeMarker
#[derive(Debug, Clone, PartialEq)]
pub struct RangeMarker {
    file_name: String,
    /// The selection's span in internal UTF-8 byte offsets.
    pub range: TextRange,
    /// The selection's span as an LSP range.
    pub ls_range: Range,
    /// The marker embedded at the start of the range, if any (`[|/*m*/foo|]`).
    pub marker: Option<Box<Marker>>,
}

impl RangeMarker {
    /// The LSP location (URI + range) of this selection.
    ///
    /// Side effects: none (pure).
    // Go: internal/fourslash/test_parser.go:RangeMarker.LSLocation
    pub fn ls_location(&self) -> tsgo_lsproto::Location {
        tsgo_lsproto::Location {
            uri: tsgo_ls_lsconv::file_name_to_document_uri(&self.file_name),
            range: self.ls_range.clone(),
        }
    }
}

/// A caret point produced by `/*name*/`, `/**/`, or a `{| ... |}` object marker.
///
/// `name` is `None` only for anonymous object markers (a `{| ... |}` without a
/// `"name"` field); `/**/` yields an empty-string name.
///
/// Side effects: none (plain data).
// Go: internal/fourslash/test_parser.go:Marker
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    file_name: String,
    /// The marker's position in internal UTF-8 byte offsets.
    pub position: i32,
    /// The marker's position as an LSP position.
    pub ls_position: Position,
    /// The marker name (`None` for anonymous `{| ... |}` object markers).
    pub name: Option<String>,
    /// The parsed JSON data for a `{| ... |}` object marker, else `None`.
    pub data: Option<JsonMap<String, JsonValue>>,
}

impl Marker {
    /// Returns a copy of this marker re-homed onto `file_name` (used when a
    /// symlinked file mirrors another's markers).
    ///
    /// Side effects: none (pure).
    // Go: internal/fourslash/test_parser.go:Marker.MakerWithSymlink
    pub fn marker_with_symlink(&self, file_name: &str) -> Marker {
        Marker {
            file_name: file_name.to_string(),
            position: self.position,
            ls_position: self.ls_position.clone(),
            name: self.name.clone(),
            data: self.data.clone(),
        }
    }
}

/// A marker or a range: the common accessors used to drive the caret to either.
///
/// Side effects: none (pure).
// Go: internal/fourslash/test_parser.go:MarkerOrRange
pub trait MarkerOrRange {
    /// The file the marker/range lives in.
    fn file_name(&self) -> &str;
    /// The LSP position the caret should move to (a range starts at its start).
    fn ls_pos(&self) -> Position;
    /// The marker name, if any.
    fn get_name(&self) -> Option<&str>;
}

impl MarkerOrRange for Marker {
    // Go: internal/fourslash/test_parser.go:Marker.FileName
    fn file_name(&self) -> &str {
        &self.file_name
    }

    // Go: internal/fourslash/test_parser.go:Marker.LSPos
    fn ls_pos(&self) -> Position {
        self.ls_position.clone()
    }

    // Go: internal/fourslash/test_parser.go:Marker.GetName
    fn get_name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl MarkerOrRange for RangeMarker {
    // Go: internal/fourslash/test_parser.go:RangeMarker.FileName
    fn file_name(&self) -> &str {
        &self.file_name
    }

    // Go: internal/fourslash/test_parser.go:RangeMarker.LSPos
    fn ls_pos(&self) -> Position {
        self.ls_range.start.clone()
    }

    // Go: internal/fourslash/test_parser.go:RangeMarker.GetName
    fn get_name(&self) -> Option<&str> {
        self.marker.as_ref().and_then(|m| m.name.as_deref())
    }
}

/// One marker-stripped file of a fourslash test case.
///
/// The `emit` flag (Go's `emitthisfile` per-file directive) is always `false`
/// in this round; see the module divergence note.
///
/// Side effects: none (plain data).
// Go: internal/fourslash/test_parser.go:TestFileInfo
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFileInfo {
    file_name: String,
    /// The file contents with all markup (markers, ranges) stripped out.
    pub content: String,
    emit: bool,
}

impl TestFileInfo {
    /// The file's normalized absolute name.
    ///
    /// Side effects: none (pure).
    // Go: internal/fourslash/test_parser.go:TestFileInfo.FileName
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Whether this file is marked for emit (`// @emitthisfile:`); always
    /// `false` this round.
    ///
    /// Side effects: none (pure).
    // Go: internal/fourslash/test_parser.go:TestFileInfo (emit)
    pub fn emit(&self) -> bool {
        self.emit
    }
}

impl Script for TestFileInfo {
    // Go: internal/fourslash/test_parser.go:TestFileInfo.FileName (lsconv.Script)
    fn file_name(&self) -> &str {
        &self.file_name
    }

    // Go: internal/fourslash/test_parser.go:TestFileInfo.Text (lsconv.Script)
    fn text(&self) -> &[u8] {
        self.content.as_bytes()
    }
}

/// The parsed fourslash test case: the marker-stripped files plus all markers
/// and ranges (with byte offsets and LSP positions), the named-marker index,
/// and the directive-derived symlinks / global options.
///
/// Side effects: none (plain data).
// Go: internal/fourslash/test_parser.go:TestData
#[derive(Debug, Clone, Default)]
pub struct TestData {
    /// The marker-stripped files, in declaration order.
    pub files: Vec<TestFileInfo>,
    /// Named markers indexed by name (anonymous object markers are excluded).
    pub marker_positions: IndexMap<String, Marker>,
    /// All markers (named and anonymous), in source order across files.
    pub markers: Vec<Marker>,
    /// Symlinks declared via `// @symlink:` / `// @link:` directives.
    pub symlinks: IndexMap<String, String>,
    /// Global `// @<option>:` directives.
    pub global_options: IndexMap<String, String>,
    /// All ranges, sorted by `(pos, -end)` within each file.
    pub ranges: Vec<RangeMarker>,
}

impl TestData {
    /// Whether the `// @statebaseline: true` directive is set.
    ///
    /// Side effects: none (pure).
    // Go: internal/fourslash/test_parser.go:TestData.isStateBaseliningEnabled
    pub fn is_state_baselining_enabled(&self) -> bool {
        is_state_baselining_enabled(&self.global_options)
    }
}

/// Whether the global options enable the `@statebaseline` mode.
///
/// Side effects: none (pure).
// Go: internal/fourslash/test_parser.go:isStateBaseliningEnabled
fn is_state_baselining_enabled(global_options: &IndexMap<String, String>) -> bool {
    global_options.get("statebaseline").map(String::as_str) == Some("true")
}

/// Parses fourslash markup in `contents` into a [`TestData`].
///
/// `file_name` names the implicit first file (content before the first
/// `// @filename:` directive). Marker names must be unique (a duplicate or an
/// unnamed non-object marker is an error).
///
/// # Examples
/// ```
/// use tsgo_fourslash::parse_test_data;
/// let data = parse_test_data("/*a*/const x = 1;", "test.ts").unwrap();
/// assert_eq!(data.files[0].content, "const x = 1;");
/// assert!(data.marker_positions.contains_key("a"));
/// assert_eq!(data.marker_positions["a"].position, 0);
/// ```
///
/// Side effects: none (pure).
// Go: internal/fourslash/test_parser.go:ParseTestData
pub fn parse_test_data(contents: &str, file_name: &str) -> Result<TestData, FourslashError> {
    let mut files: Vec<TestFileInfo> = Vec::new();
    let mut marker_positions: IndexMap<String, Marker> = IndexMap::new();
    let mut markers: Vec<Marker> = Vec::new();
    let mut ranges: Vec<RangeMarker> = Vec::new();

    // Split into named units (+ symlinks + global options). Go passes
    // `parse_file_content` as the splitter callback; the Rust splitter has no
    // callback, so we split first and parse each unit's content below.
    let parsed = parse_test_files_and_symlinks_with_options(
        contents,
        file_name,
        ParseTestFilesOptions {
            allow_implicit_first_file: true,
        },
    );

    let mut has_tsconfig = false;
    for unit in &parsed.units {
        let file = parse_file_content(&unit.name, &unit.content)?;
        has_tsconfig = has_tsconfig || is_config_file(file.file.file_name());
        files.push(file.file);
        for marker in file.markers {
            match &marker.name {
                None => {
                    if marker.data.is_some() {
                        // Anonymous object marker: kept in `markers` but never
                        // indexed by name.
                        markers.push(marker);
                        continue;
                    }
                    return Err(FourslashError(format!(
                        "Marker at position {} is unnamed",
                        marker.position
                    )));
                }
                Some(name) => {
                    if let Some(existing) = marker_positions.get(name) {
                        return Err(FourslashError(format!(
                            "Duplicate marker name: \"{}\" at {} and {}",
                            name, marker.position, existing.position
                        )));
                    }
                    marker_positions.insert(name.clone(), marker.clone());
                    markers.push(marker);
                }
            }
        }
        ranges.extend(file.ranges);
    }

    if has_tsconfig
        && has_unsupported_global_options_with_config(&parsed.global_options)
        && !is_state_baselining_enabled(&parsed.global_options)
    {
        return Err(FourslashError(
            "It is not allowed to use global options along with config files.".to_string(),
        ));
    }

    Ok(TestData {
        files,
        marker_positions,
        markers,
        symlinks: parsed.symlinks,
        global_options: parsed.global_options,
        ranges,
    })
}

/// Whether the global options contain any option not permitted alongside a
/// `tsconfig.json` (only `symlink`/`link`/`usecasesensitivefilenames` are
/// allowed with a config file).
///
/// Side effects: none (pure).
// Go: internal/fourslash/test_parser.go:hasUnsupportedGlobalOptionsWithConfig
fn has_unsupported_global_options_with_config(global_options: &IndexMap<String, String>) -> bool {
    for option in global_options.keys() {
        match option.to_lowercase().as_str() {
            "symlink" | "link" | "usecasesensitivefilenames" => continue,
            _ => return true,
        }
    }
    false
}

/// Whether `file_name` is a `tsconfig.json` / `jsconfig.json`.
///
/// Side effects: none (pure).
// Go: internal/fourslash/test_parser.go:isConfigFile
fn is_config_file(file_name: &str) -> bool {
    let file_name = file_name.to_lowercase();
    file_name.ends_with("tsconfig.json") || file_name.ends_with("jsconfig.json")
}

/// The marker-stripped file plus the markers and ranges extracted from one unit.
///
/// Side effects: none (plain data).
// Go: internal/fourslash/test_parser.go:testFileWithMarkers
struct TestFileWithMarkers {
    file: TestFileInfo,
    markers: Vec<Marker>,
    ranges: Vec<RangeMarker>,
}

/// Source-location bookkeeping for an open marker / range (byte offset, the
/// metacharacter-stripped position, and 1-based line/column for error
/// messages).
// Go: internal/fourslash/test_parser.go:locationInformation
#[derive(Clone)]
struct LocationInformation {
    position: i32,
    source_position: usize,
    source_line: i32,
    source_column: i32,
}

/// An open (unclosed) range, plus the index (into the per-file `markers` Vec) of
/// any marker embedded at its start.
// Go: internal/fourslash/test_parser.go:rangeLocationInformation
struct OpenRange {
    location: LocationInformation,
    marker_index: Option<usize>,
}

/// A closed range carrying its byte span and the index of any embedded marker.
struct ClosedRange {
    file_name: String,
    range: TextRange,
    marker_index: Option<usize>,
}

/// The marker-parser state machine state.
// Go: internal/fourslash/test_parser.go:parserState
#[derive(Clone, Copy, PartialEq, Eq)]
enum ParserState {
    None,
    InSlashStarMarker,
    InObjectMarker,
}

/// Go `utf8.RuneError` (the replacement character `U+FFFD`).
const RUNE_ERROR: char = '\u{FFFD}';

/// Decodes the rune at byte offset `i` in `s`, returning `(char, byte_width)`.
///
/// Mirrors Go `utf8.DecodeRuneInString(s[i:])` for the valid-UTF-8 inputs the
/// parser sees (`&str` is always valid UTF-8); an out-of-range/empty tail
/// decodes as `(U+FFFD, 0)`.
fn decode_rune_at(s: &str, i: usize) -> (char, usize) {
    match s[i..].chars().next() {
        Some(c) => (c, c.len_utf8()),
        None => (RUNE_ERROR, 0),
    }
}

/// Extracts markers and ranges from one file's `content`, returning the
/// stripped contents (with markup removed) plus the markers/ranges with byte
/// offsets and LSP positions.
///
/// This is the core DSL state machine: it scans rune-by-rune tracking the
/// running count of stripped metacharacters (`difference`) so that each
/// marker/range records its position in the *stripped* output.
///
/// Side effects: none (pure).
// Go: internal/fourslash/test_parser.go:parseFileContent
fn parse_file_content(
    file_name: &str,
    content: &str,
) -> Result<TestFileWithMarkers, FourslashError> {
    let file_name = get_normalized_absolute_path(file_name, "/");
    let content = chomp_leading_space(content);

    // The file content (minus metacharacters) so far.
    let mut output = String::new();

    let mut markers: Vec<Marker> = Vec::new();

    // A stack of the open range markers that are still unclosed.
    let mut open_ranges: Vec<OpenRange> = Vec::new();
    // A list of closed ranges we've collected so far.
    let mut range_markers: Vec<ClosedRange> = Vec::new();

    // The total number of metacharacters removed from the file (so far).
    let mut difference: i32 = 0;

    // One-based current position data.
    let mut line: i32 = 1;
    let mut column: i32 = 1;

    // The current marker (or maybe multi-line comment?) we're parsing.
    let mut open_marker: Option<LocationInformation> = None;

    // The latest position of the start of an unflushed plain text area.
    let mut last_normal_char_position: usize = 0;

    let bytes = content.as_bytes();
    let flush = |output: &mut String, from: usize, to: Option<usize>| match to {
        Some(end) => output.push_str(&content[from..end]),
        None => output.push_str(&content[from..]),
    };

    let mut state = ParserState::None;
    let (mut previous_character, mut i) = decode_rune_at(&content, 0);
    let mut size: usize;
    while i < content.len() {
        let (current_character, csize) = decode_rune_at(&content, i);
        size = csize;
        match state {
            ParserState::None => {
                if previous_character == '[' && current_character == '|' {
                    // Found a range start.
                    open_ranges.push(OpenRange {
                        location: LocationInformation {
                            position: (i as i32 - 1) - difference,
                            source_position: i - 1,
                            source_line: line,
                            source_column: column,
                        },
                        marker_index: None,
                    });
                    // Copy all text up to marker position.
                    flush(&mut output, last_normal_char_position, Some(i - 1));
                    last_normal_char_position = i + 1;
                    difference += 2;
                } else if previous_character == '|' && current_character == ']' {
                    // Found a range end.
                    let range_start = match open_ranges.pop() {
                        Some(r) => r,
                        None => {
                            return Err(report_error(
                                &file_name,
                                line,
                                column,
                                "Found range end with no matching start.",
                            ))
                        }
                    };

                    range_markers.push(ClosedRange {
                        file_name: file_name.clone(),
                        range: TextRange::new(
                            range_start.location.position,
                            (i as i32 - 1) - difference,
                        ),
                        marker_index: range_start.marker_index,
                    });

                    // Copy all text up to range marker position.
                    flush(&mut output, last_normal_char_position, Some(i - 1));
                    last_normal_char_position = i + 1;
                    difference += 2;
                } else if previous_character == '/' && current_character == '*' {
                    // Found a possible marker start.
                    state = ParserState::InSlashStarMarker;
                    open_marker = Some(LocationInformation {
                        position: (i as i32 - 1) - difference,
                        source_position: i - 1,
                        source_line: line,
                        source_column: column - 1,
                    });
                } else if previous_character == '{' && current_character == '|' {
                    // Found an object marker start.
                    state = ParserState::InObjectMarker;
                    open_marker = Some(LocationInformation {
                        position: (i as i32 - 1) - difference,
                        source_position: i - 1,
                        source_line: line,
                        source_column: column,
                    });
                    flush(&mut output, last_normal_char_position, Some(i - 1));
                }
            }
            ParserState::InObjectMarker => {
                // Object markers are only ever terminated by |} and have no
                // content restrictions.
                if previous_character == '|' && current_character == '}' {
                    let open = open_marker.as_ref().expect("open object marker");
                    let object_marker_data =
                        content[open.source_position + 2..i - 1].trim().to_string();
                    let marker = get_object_marker(&file_name, open, &object_marker_data)?;

                    let marker_index = markers.len();
                    if let Some(open_range) = open_ranges.last_mut() {
                        open_range.marker_index = Some(marker_index);
                    }
                    markers.push(marker);

                    // Set the current start to point to the end of the current
                    // marker to ignore its text.
                    last_normal_char_position = i + 1;
                    difference += i as i32 + 1 - open.source_position as i32;

                    // Reset the state.
                    open_marker = None;
                    state = ParserState::None;
                }
            }
            ParserState::InSlashStarMarker => {
                if previous_character == '*' && current_character == '/' {
                    // Record the marker. start + 2 to ignore the /*, -1 on the
                    // end to ignore the * (/ is next).
                    let open = open_marker.as_ref().expect("open slash-star marker");
                    let marker_name_text =
                        content[open.source_position + 2..i - 1].trim().to_string();
                    let marker = Marker {
                        file_name: file_name.clone(),
                        position: open.position,
                        ls_position: Position::default(),
                        name: Some(marker_name_text),
                        data: None,
                    };
                    let marker_index = markers.len();
                    if let Some(open_range) = open_ranges.last_mut() {
                        open_range.marker_index = Some(marker_index);
                    }
                    markers.push(marker);

                    // Set the current start to point to the end of the current
                    // marker to ignore its text.
                    flush(
                        &mut output,
                        last_normal_char_position,
                        Some(open.source_position),
                    );
                    last_normal_char_position = i + 1;
                    difference += i as i32 + 1 - open.source_position as i32;

                    // Reset the state.
                    open_marker = None;
                    state = ParserState::None;
                } else if !(is_digit(current_character)
                    || is_ascii_letter(current_character)
                    || current_character == '$'
                    || current_character == '_')
                {
                    // Invalid marker character.
                    if current_character == '*' && i < content.len() - 1 && bytes[i + 1] == b'/' {
                        // The marker is about to be closed, ignore the
                        // 'invalid' char.
                    } else {
                        // We've hit a non-valid marker character, so we were
                        // actually in a block comment. Bail out the text we've
                        // gathered so far back into the output.
                        flush(&mut output, last_normal_char_position, Some(i));
                        last_normal_char_position = i;
                        open_marker = None;
                        state = ParserState::None;
                    }
                }
            }
        }
        if current_character == '\n' && previous_character == '\r' {
            // Ignore trailing \n after \r.
            i += size;
            continue;
        } else if current_character == '\n' || current_character == '\r' {
            line += 1;
            column = 1;
            i += size;
            continue;
        }
        column += 1;
        if i >= last_normal_char_position {
            previous_character = current_character;
        } else {
            // Reset to avoid accidentally reusing marker delimiters as part of
            // other markers.
            previous_character = RUNE_ERROR;
        }
        i += size;
    }

    // Add the remaining text.
    flush(&mut output, last_normal_char_position, None);

    if let Some(open_range) = open_ranges.first() {
        return Err(report_error(
            &file_name,
            open_range.location.source_line,
            open_range.location.source_column,
            "Unterminated range.",
        ));
    }

    if let Some(open) = open_marker {
        return Err(report_error(
            &file_name,
            open.source_line,
            open.source_column,
            "Unterminated marker.",
        ));
    }

    let output_string = output;
    // Set LS positions for markers.
    let line_map = Rc::new(compute_lsp_line_starts(output_string.as_bytes()));
    let converters = Converters::new(PositionEncodingKind::utf8(), {
        let line_map = Rc::clone(&line_map);
        move |_| Rc::clone(&line_map)
    });

    // DEFER(P10): the per-file `emitthisfile` directive (Go reads it from the
    // splitter's per-unit `fileOptions`). The Rust testrunner splitter does not
    // surface per-unit options, and `emit` is only consumed by baseline emit
    // (also deferred), so it is always `false` this round.
    let test_file_info = TestFileInfo {
        file_name,
        content: output_string,
        emit: false,
    };

    for marker in &mut markers {
        marker.ls_position =
            converters.position_to_line_and_character(&test_file_info, TextPos(marker.position));
    }

    // Sort by (pos asc, end desc), stably (mirrors Go's SortStableFunc).
    range_markers.sort_by(|a, b| {
        a.range
            .pos()
            .cmp(&b.range.pos())
            .then(b.range.end().cmp(&a.range.end()))
    });

    let ranges: Vec<RangeMarker> = range_markers
        .into_iter()
        .map(|closed| RangeMarker {
            file_name: closed.file_name,
            range: closed.range,
            ls_range: Range {
                start: converters
                    .position_to_line_and_character(&test_file_info, TextPos(closed.range.pos())),
                end: converters
                    .position_to_line_and_character(&test_file_info, TextPos(closed.range.end())),
            },
            marker: closed
                .marker_index
                .map(|idx| Box::new(markers[idx].clone())),
        })
        .collect();

    Ok(TestFileWithMarkers {
        file: test_file_info,
        markers,
        ranges,
    })
}

/// Parses a `{| ... |}` object marker's body as JSON (`{ <text> }`), returning
/// a marker carrying the data (and a name if the object has a non-empty string
/// `"name"` field).
///
/// Side effects: none (pure).
// Go: internal/fourslash/test_parser.go:getObjectMarker
fn get_object_marker(
    file_name: &str,
    location: &LocationInformation,
    text: &str,
) -> Result<Marker, FourslashError> {
    // Attempt to parse the marker value as JSON.
    let wrapped = format!("{{ {text} }}");
    let value: JsonValue = serde_json::from_str(&wrapped).map_err(|_| {
        report_error(
            file_name,
            location.source_line,
            location.source_column,
            &format!("Unable to parse marker text {text}"),
        )
    })?;

    let marker_value = match value {
        JsonValue::Object(map) if !map.is_empty() => map,
        _ => {
            return Err(report_error(
                file_name,
                location.source_line,
                location.source_column,
                "Object markers can not be empty",
            ))
        }
    };

    let mut marker = Marker {
        file_name: file_name.to_string(),
        position: location.position,
        ls_position: Position::default(),
        name: None,
        data: Some(marker_value.clone()),
    };

    // Object markers can be anonymous.
    if let Some(JsonValue::String(name)) = marker_value.get("name") {
        if !name.is_empty() {
            marker.name = Some(name.clone());
        }
    }

    Ok(marker)
}

/// Formats a parse error as Go's `reportError` (`<file> (<line>,<col>): <msg>`).
///
/// Side effects: none (pure).
// Go: internal/fourslash/test_parser.go:reportError
fn report_error(file_name: &str, line: i32, col: i32, message: &str) -> FourslashError {
    FourslashError(format!("{file_name} ({line},{col}): {message}"))
}

/// Strips a single leading space from every non-empty line when *all* non-empty
/// lines start with a space; otherwise returns the content unchanged.
///
/// Side effects: none (pure).
// Go: internal/fourslash/test_parser.go:chompLeadingSpace
fn chomp_leading_space(content: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    for line in &lines {
        if !line.is_empty() && line.as_bytes()[0] != b' ' {
            return content.to_string();
        }
    }

    let result: Vec<&str> = lines
        .iter()
        .map(|line| if line.is_empty() { "" } else { &line[1..] })
        .collect();
    result.join("\n")
}

#[cfg(test)]
#[path = "test_parser_test.rs"]
mod tests;
