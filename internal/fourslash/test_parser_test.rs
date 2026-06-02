use super::*;

// Go: internal/fourslash/test_parser.go:ParseTestData (headline — a named marker)
#[test]
fn parse_test_data_extracts_a_named_marker() {
    let data = parse_test_data("/*a*/const x = 1;", "test.ts").unwrap();
    assert_eq!(data.files.len(), 1);
    assert_eq!(data.files[0].content, "const x = 1;");
    assert_eq!(data.files[0].file_name(), "/test.ts");

    assert_eq!(data.markers.len(), 1);
    assert_eq!(data.markers[0].name.as_deref(), Some("a"));
    assert_eq!(data.markers[0].position, 0);
    assert_eq!(
        data.markers[0].ls_position,
        Position {
            line: 0,
            character: 0
        }
    );

    let marker = data.marker_positions.get("a").expect("marker `a` indexed");
    assert_eq!(marker.position, 0);
}

// `/**/` is an empty-named marker (Go: trimmed name is the empty string).
// Go: internal/fourslash/test_parser.go:parseFileContent (slash marker)
#[test]
fn parse_test_data_anonymous_slash_marker_has_empty_name() {
    let data = parse_test_data("const x = 1; /**/", "test.ts").unwrap();
    assert_eq!(data.files[0].content, "const x = 1; ");
    assert_eq!(data.markers.len(), 1);
    assert_eq!(data.markers[0].name.as_deref(), Some(""));
    assert_eq!(data.markers[0].position, 13);
    assert!(data.marker_positions.contains_key(""));
}

// Go: internal/fourslash/test_parser.go:parseFileContent (range start/end)
#[test]
fn parse_test_data_extracts_a_range() {
    let data = parse_test_data("[|ranged|]", "test.ts").unwrap();
    assert_eq!(data.files[0].content, "ranged");
    assert!(data.markers.is_empty());

    assert_eq!(data.ranges.len(), 1);
    let r = &data.ranges[0];
    assert_eq!(r.range, TextRange::new(0, 6));
    assert_eq!(
        r.ls_range.start,
        Position {
            line: 0,
            character: 0
        }
    );
    assert_eq!(
        r.ls_range.end,
        Position {
            line: 0,
            character: 6
        }
    );
    assert!(r.marker.is_none());
}

// A range may carry an embedded marker at its start: `[|/*m*/foo|]`.
// Go: internal/fourslash/test_parser.go:parseFileContent (openRanges[...].marker)
#[test]
fn parse_test_data_range_with_embedded_marker() {
    let data = parse_test_data("[|/*m*/foo|]", "test.ts").unwrap();
    assert_eq!(data.files[0].content, "foo");
    assert_eq!(data.markers.len(), 1);
    assert_eq!(data.markers[0].name.as_deref(), Some("m"));
    assert_eq!(data.markers[0].position, 0);

    assert_eq!(data.ranges.len(), 1);
    let r = &data.ranges[0];
    assert_eq!(r.range, TextRange::new(0, 3));
    let embedded = r.marker.as_ref().expect("embedded marker");
    assert_eq!(embedded.name.as_deref(), Some("m"));
    assert_eq!(r.get_name(), Some("m"));
}

// `// @filename:` splits the case into multiple files.
// Go: internal/fourslash/test_parser.go:ParseTestData (multi-file)
#[test]
fn parse_test_data_splits_multiple_files() {
    let content = "// @filename: a.ts\nconst a = 1;\n// @filename: b.ts\nconst b = 2;";
    let data = parse_test_data(content, "test.ts").unwrap();
    assert_eq!(data.files.len(), 2);
    assert_eq!(data.files[0].file_name(), "/a.ts");
    assert_eq!(data.files[0].content, "const a = 1;");
    assert_eq!(data.files[1].file_name(), "/b.ts");
    assert_eq!(data.files[1].content, "const b = 2;");
}

// `// @<option>: <value>` directives (other than `@filename`) become global
// options.
// Go: internal/fourslash/test_parser.go:ParseTestData (globalOptions)
#[test]
fn parse_test_data_records_global_options() {
    let data = parse_test_data("// @target: esnext\nconst x = 1;", "test.ts").unwrap();
    assert_eq!(
        data.global_options.get("target").map(String::as_str),
        Some("esnext")
    );
    assert_eq!(data.files[0].content, "const x = 1;");
}

// `{| "name": "foo", ... |}` is a named object marker carrying JSON data.
// Go: internal/fourslash/test_parser.go:getObjectMarker
#[test]
fn parse_test_data_named_object_marker_carries_data() {
    let data =
        parse_test_data("{| \"name\": \"foo\", \"kind\": \"value\" |}bar", "test.ts").unwrap();
    assert_eq!(data.files[0].content, "bar");
    assert_eq!(data.markers.len(), 1);
    let marker = &data.markers[0];
    assert_eq!(marker.name.as_deref(), Some("foo"));
    assert_eq!(marker.position, 0);
    let map = marker.data.as_ref().expect("object marker data");
    assert_eq!(map.get("name").and_then(|v| v.as_str()), Some("foo"));
    assert_eq!(map.get("kind").and_then(|v| v.as_str()), Some("value"));
    assert!(data.marker_positions.contains_key("foo"));
}

// An object marker without a `"name"` field is anonymous: kept in `markers`
// but never indexed by name.
// Go: internal/fourslash/test_parser.go:ParseTestData (anonymous object marker)
#[test]
fn parse_test_data_anonymous_object_marker_is_not_indexed() {
    let data = parse_test_data("{| \"kind\": \"value\" |}bar", "test.ts").unwrap();
    assert_eq!(data.markers.len(), 1);
    assert!(data.markers[0].name.is_none());
    assert!(data.markers[0].data.is_some());
    assert!(data.marker_positions.is_empty());
}

// A marker on a later line gets the correct multi-line LSP position.
// Go: internal/fourslash/test_parser.go:parseFileContent (LSPosition)
#[test]
fn parse_test_data_marker_ls_position_is_multiline() {
    let data = parse_test_data("a\n/*m*/b", "test.ts").unwrap();
    assert_eq!(data.files[0].content, "a\nb");
    assert_eq!(data.markers[0].position, 2);
    assert_eq!(
        data.markers[0].ls_position,
        Position {
            line: 1,
            character: 0
        }
    );
}

// Ranges are sorted by (pos asc, end desc).
// Go: internal/fourslash/test_parser.go:parseFileContent (SortStableFunc)
#[test]
fn parse_test_data_ranges_sorted_by_pos_then_end_desc() {
    // Outer range [0,6) "abcdef" wraps an inner range [0,3) "abc".
    let data = parse_test_data("[|[|abc|]def|]", "test.ts").unwrap();
    assert_eq!(data.files[0].content, "abcdef");
    assert_eq!(data.ranges.len(), 2);
    // Same start (0): the longer range (end 6) sorts first.
    assert_eq!(data.ranges[0].range, TextRange::new(0, 6));
    assert_eq!(data.ranges[1].range, TextRange::new(0, 3));
}

// A `/* ... */` with non-marker characters is treated as a block comment and
// kept verbatim in the output (no marker recorded).
// Go: internal/fourslash/test_parser.go:parseFileContent (block-comment bail-out)
#[test]
fn parse_test_data_block_comment_is_not_a_marker() {
    let data = parse_test_data("/* hello */const x = 1;", "test.ts").unwrap();
    assert_eq!(data.files[0].content, "/* hello */const x = 1;");
    assert!(data.markers.is_empty());
}

// `chompLeadingSpace`: when every non-empty line starts with a space, one
// leading space is stripped from each.
// Go: internal/fourslash/test_parser.go:chompLeadingSpace
#[test]
fn parse_test_data_chomps_uniform_leading_space() {
    let data = parse_test_data(" /*a*/const x = 1;\n const y = 2;", "test.ts").unwrap();
    assert_eq!(data.files[0].content, "const x = 1;\nconst y = 2;");
    assert_eq!(data.markers[0].position, 0);
}

// A duplicate marker name is a parse error.
// Go: internal/fourslash/test_parser.go:ParseTestData (Duplicate marker name)
#[test]
fn parse_test_data_duplicate_marker_name_errors() {
    let err = parse_test_data("/*a*/x/*a*/y", "test.ts").unwrap_err();
    assert!(
        err.0.contains("Duplicate marker name"),
        "unexpected error: {}",
        err.0
    );
}

// An unterminated range is a parse error.
// Go: internal/fourslash/test_parser.go:parseFileContent (Unterminated range)
#[test]
fn parse_test_data_unterminated_range_errors() {
    let err = parse_test_data("[|abc", "test.ts").unwrap_err();
    assert!(
        err.0.contains("Unterminated range"),
        "unexpected error: {}",
        err.0
    );
}

// An unterminated marker is a parse error.
// Go: internal/fourslash/test_parser.go:parseFileContent (Unterminated marker)
#[test]
fn parse_test_data_unterminated_marker_errors() {
    let err = parse_test_data("/*abc", "test.ts").unwrap_err();
    assert!(
        err.0.contains("Unterminated marker"),
        "unexpected error: {}",
        err.0
    );
}

// A `|]` with no open range is a parse error.
// Go: internal/fourslash/test_parser.go:parseFileContent (Found range end with no matching start)
#[test]
fn parse_test_data_range_end_without_start_errors() {
    let err = parse_test_data("abc|]", "test.ts").unwrap_err();
    assert!(
        err.0.contains("Found range end with no matching start"),
        "unexpected error: {}",
        err.0
    );
}

// `is_state_baselining_enabled` reads the `@statebaseline` global option.
// Go: internal/fourslash/test_parser.go:TestData.isStateBaseliningEnabled
#[test]
fn parse_test_data_state_baselining_flag() {
    let enabled = parse_test_data("// @statebaseline: true\nconst x = 1;", "test.ts").unwrap();
    assert!(enabled.is_state_baselining_enabled());
    let disabled = parse_test_data("const x = 1;", "test.ts").unwrap();
    assert!(!disabled.is_state_baselining_enabled());
}

// `marker_with_symlink` re-homes a marker onto another file, preserving its
// position and name.
// Go: internal/fourslash/test_parser.go:Marker.MakerWithSymlink
#[test]
fn marker_with_symlink_rehomes_marker() {
    let data = parse_test_data("/*a*/const x = 1;", "test.ts").unwrap();
    let original = data.marker_positions.get("a").unwrap();
    let moved = original.marker_with_symlink("/other.ts");
    assert_eq!(moved.file_name(), "/other.ts");
    assert_eq!(moved.position, original.position);
    assert_eq!(moved.name, original.name);
}

// `Marker` implements `MarkerOrRange` (file name, LSP position, name).
// Go: internal/fourslash/test_parser.go:Marker (MarkerOrRange)
#[test]
fn marker_marker_or_range_accessors() {
    let data = parse_test_data("a\n/*m*/b", "test.ts").unwrap();
    let marker = data.marker_positions.get("m").unwrap();
    assert_eq!(MarkerOrRange::file_name(marker), "/test.ts");
    assert_eq!(
        MarkerOrRange::ls_pos(marker),
        Position {
            line: 1,
            character: 0
        }
    );
    assert_eq!(marker.get_name(), Some("m"));
}

// `RangeMarker` implements `MarkerOrRange`: its caret position is the range
// start, and an anonymous range has no name.
// Go: internal/fourslash/test_parser.go:RangeMarker (MarkerOrRange / LSLocation)
#[test]
fn range_marker_or_range_accessors_and_location() {
    let data = parse_test_data("ab[|cd|]", "test.ts").unwrap();
    let r = &data.ranges[0];
    assert_eq!(MarkerOrRange::file_name(r), "/test.ts");
    assert_eq!(
        r.ls_pos(),
        Position {
            line: 0,
            character: 2
        }
    );
    assert_eq!(r.get_name(), None);

    let loc = r.ls_location();
    assert_eq!(loc.uri.0, "file:///test.ts");
    assert_eq!(
        loc.range.start,
        Position {
            line: 0,
            character: 2
        }
    );
    assert_eq!(
        loc.range.end,
        Position {
            line: 0,
            character: 4
        }
    );
}

// `TestFileInfo::emit` is `false` in this round (the `emitthisfile` directive
// is deferred).
// Go: internal/fourslash/test_parser.go:TestFileInfo (emit)
#[test]
fn test_file_info_emit_defaults_false() {
    let data = parse_test_data("const x = 1;", "test.ts").unwrap();
    assert!(!data.files[0].emit());
}
