use super::*;
use tsgo_core::Utf16Offset;
use tsgo_tspath::ComparePathsOptions;

// Go: internal/sourcemap/generator_test.go base = NewGenerator("main.js","/","/",{})
fn base() -> Generator {
    Generator::new("main.js", "/", "/", ComparePathsOptions::default())
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_Empty
#[test]
fn empty() {
    let mut gen = base();
    let source_map = gen.raw_source_map();
    assert_eq!(
        source_map,
        RawSourceMap {
            version: 3,
            file: "main.js".to_string(),
            source_root: "/".to_string(),
            sources: vec![],
            names: vec![],
            mappings: String::new(),
            sources_content: None,
        }
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_Empty_Serialized
#[test]
fn empty_serialized() {
    let mut gen = base();
    let actual = gen.to_string();
    let expected =
        r#"{"version":3,"file":"main.js","sourceRoot":"/","sources":[],"names":[],"mappings":""}"#;
    assert_eq!(actual, expected);
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddSource
#[test]
fn add_source() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    let source_map = gen.raw_source_map();
    assert_eq!(source_index, SourceIndex(0));
    assert_eq!(
        source_map,
        RawSourceMap {
            version: 3,
            file: "main.js".to_string(),
            source_root: "/".to_string(),
            sources: vec!["main.ts".to_string()],
            names: vec![],
            mappings: String::new(),
            sources_content: None,
        }
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_SetSourceContent
#[test]
fn set_source_content() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    assert!(gen.set_source_content(source_index, "foo").is_ok());
    let source_map = gen.raw_source_map();
    assert_eq!(source_index, SourceIndex(0));
    assert_eq!(
        source_map,
        RawSourceMap {
            version: 3,
            file: "main.js".to_string(),
            source_root: "/".to_string(),
            sources: vec!["main.ts".to_string()],
            names: vec![],
            mappings: String::new(),
            sources_content: Some(vec![Some("foo".to_string())]),
        }
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_SetSourceContent_ForSecondSourceOnly
#[test]
fn set_source_content_for_second_source_only() {
    let mut gen = base();
    gen.add_source("/skipped.ts");
    let source_index = gen.add_source("/main.ts");
    assert!(gen.set_source_content(source_index, "foo").is_ok());
    let source_map = gen.raw_source_map();
    assert_eq!(source_index, SourceIndex(1));
    assert_eq!(
        source_map,
        RawSourceMap {
            version: 3,
            file: "main.js".to_string(),
            source_root: "/".to_string(),
            sources: vec!["skipped.ts".to_string(), "main.ts".to_string()],
            names: vec![],
            mappings: String::new(),
            sources_content: Some(vec![None, Some("foo".to_string())]),
        }
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_SetSourceContent_SourceIndexOutOfRange
#[test]
fn set_source_content_out_of_range() {
    let mut gen = base();
    assert_eq!(
        gen.set_source_content(SourceIndex(-1), "")
            .unwrap_err()
            .to_string(),
        "sourceIndex is out of range",
    );
    assert_eq!(
        gen.set_source_content(SourceIndex(0), "")
            .unwrap_err()
            .to_string(),
        "sourceIndex is out of range",
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_SetSourceContent_ForSecondSourceOnly_Serialized
#[test]
fn set_source_content_for_second_source_only_serialized() {
    let mut gen = base();
    gen.add_source("/skipped.ts");
    let source_index = gen.add_source("/main.ts");
    assert!(gen.set_source_content(source_index, "foo").is_ok());
    let actual = gen.to_string();
    let expected = r#"{"version":3,"file":"main.js","sourceRoot":"/","sources":["skipped.ts","main.ts"],"names":[],"mappings":"","sourcesContent":[null,"foo"]}"#;
    assert_eq!(actual, expected);
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddName
#[test]
fn add_name() {
    let mut gen = base();
    let name_index = gen.add_name("foo");
    let source_map = gen.raw_source_map();
    assert_eq!(name_index, NameIndex(0));
    assert_eq!(
        source_map,
        RawSourceMap {
            version: 3,
            file: "main.js".to_string(),
            source_root: "/".to_string(),
            sources: vec![],
            names: vec!["foo".to_string()],
            mappings: String::new(),
            sources_content: None,
        }
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddGeneratedMapping
#[test]
fn add_generated_mapping() {
    let mut gen = base();
    assert!(gen.add_generated_mapping(0, Utf16Offset(0)).is_ok());
    assert_eq!(gen.raw_source_map().mappings, "A");
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddGeneratedMapping_OnSecondLineOnly
#[test]
fn add_generated_mapping_on_second_line_only() {
    let mut gen = base();
    assert!(gen.add_generated_mapping(1, Utf16Offset(0)).is_ok());
    assert_eq!(gen.raw_source_map().mappings, ";A");
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddSourceMapping
#[test]
fn add_source_mapping() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    assert!(gen
        .add_source_mapping(0, Utf16Offset(0), source_index, 0, Utf16Offset(0))
        .is_ok());
    assert_eq!(gen.raw_source_map().mappings, "AAAA");
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddSourceMapping_NextGeneratedCharacter
#[test]
fn add_source_mapping_next_generated_character() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    assert!(gen
        .add_source_mapping(0, Utf16Offset(0), source_index, 0, Utf16Offset(0))
        .is_ok());
    assert!(gen
        .add_source_mapping(0, Utf16Offset(1), source_index, 0, Utf16Offset(0))
        .is_ok());
    assert_eq!(gen.raw_source_map().mappings, "AAAA,CAAA");
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddSourceMapping_NextGeneratedAndSourceCharacter
#[test]
fn add_source_mapping_next_generated_and_source_character() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    assert!(gen
        .add_source_mapping(0, Utf16Offset(0), source_index, 0, Utf16Offset(0))
        .is_ok());
    assert!(gen
        .add_source_mapping(0, Utf16Offset(1), source_index, 0, Utf16Offset(1))
        .is_ok());
    assert_eq!(gen.raw_source_map().mappings, "AAAA,CAAC");
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddSourceMapping_NextGeneratedLine
#[test]
fn add_source_mapping_next_generated_line() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    assert!(gen
        .add_source_mapping(0, Utf16Offset(0), source_index, 0, Utf16Offset(0))
        .is_ok());
    assert!(gen
        .add_source_mapping(1, Utf16Offset(0), source_index, 0, Utf16Offset(0))
        .is_ok());
    assert_eq!(gen.raw_source_map().mappings, "AAAA;AAAA");
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddSourceMapping_PreviousSourceCharacter
#[test]
fn add_source_mapping_previous_source_character() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    assert!(gen
        .add_source_mapping(0, Utf16Offset(0), source_index, 0, Utf16Offset(1))
        .is_ok());
    assert!(gen
        .add_source_mapping(0, Utf16Offset(1), source_index, 0, Utf16Offset(0))
        .is_ok());
    assert_eq!(gen.raw_source_map().mappings, "AAAC,CAAD");
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddNamedSourceMapping
#[test]
fn add_named_source_mapping() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    let name_index = gen.add_name("foo");
    assert!(gen
        .add_named_source_mapping(
            0,
            Utf16Offset(0),
            source_index,
            0,
            Utf16Offset(0),
            name_index
        )
        .is_ok());
    let source_map = gen.raw_source_map();
    assert_eq!(source_map.mappings, "AAAAA");
    assert_eq!(source_map.names, vec!["foo".to_string()]);
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddNamedSourceMapping_WithPreviousName
#[test]
fn add_named_source_mapping_with_previous_name() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    let name_index1 = gen.add_name("foo");
    let name_index2 = gen.add_name("bar");
    assert!(gen
        .add_named_source_mapping(
            0,
            Utf16Offset(0),
            source_index,
            0,
            Utf16Offset(0),
            name_index2
        )
        .is_ok());
    assert!(gen
        .add_named_source_mapping(
            0,
            Utf16Offset(1),
            source_index,
            0,
            Utf16Offset(0),
            name_index1
        )
        .is_ok());
    let source_map = gen.raw_source_map();
    assert_eq!(source_map.mappings, "AAAAC,CAAAD");
    assert_eq!(source_map.names, vec!["foo".to_string(), "bar".to_string()]);
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddGeneratedMapping_GeneratedLineCannotBacktrack
#[test]
fn add_generated_mapping_line_cannot_backtrack() {
    let mut gen = base();
    assert!(gen.add_generated_mapping(1, Utf16Offset(0)).is_ok());
    assert_eq!(
        gen.add_generated_mapping(0, Utf16Offset(0))
            .unwrap_err()
            .to_string(),
        "generatedLine cannot backtrack",
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddGeneratedMapping_GeneratedCharacterCannotBeNegative
#[test]
fn add_generated_mapping_char_cannot_be_negative() {
    let mut gen = base();
    assert!(gen.add_generated_mapping(0, Utf16Offset(0)).is_ok());
    assert_eq!(
        gen.add_generated_mapping(0, Utf16Offset(-1))
            .unwrap_err()
            .to_string(),
        "generatedCharacter cannot be negative",
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddSourceMapping_GeneratedLineCannotBacktrack
#[test]
fn add_source_mapping_line_cannot_backtrack() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    assert!(gen
        .add_source_mapping(1, Utf16Offset(0), source_index, 0, Utf16Offset(0))
        .is_ok());
    assert_eq!(
        gen.add_source_mapping(0, Utf16Offset(0), source_index, 0, Utf16Offset(0))
            .unwrap_err()
            .to_string(),
        "generatedLine cannot backtrack",
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddSourceMapping_GeneratedCharacterCannotBeNegative
#[test]
fn add_source_mapping_char_cannot_be_negative() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    assert!(gen
        .add_source_mapping(0, Utf16Offset(0), source_index, 0, Utf16Offset(0))
        .is_ok());
    assert_eq!(
        gen.add_source_mapping(0, Utf16Offset(-1), source_index, 0, Utf16Offset(0))
            .unwrap_err()
            .to_string(),
        "generatedCharacter cannot be negative",
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddSourceMapping_SourceIndexIsOutOfRange
#[test]
fn add_source_mapping_source_index_out_of_range() {
    let mut gen = base();
    assert_eq!(
        gen.add_source_mapping(0, Utf16Offset(0), SourceIndex(-1), 0, Utf16Offset(0))
            .unwrap_err()
            .to_string(),
        "sourceIndex is out of range",
    );
    assert_eq!(
        gen.add_source_mapping(0, Utf16Offset(0), SourceIndex(0), 0, Utf16Offset(0))
            .unwrap_err()
            .to_string(),
        "sourceIndex is out of range",
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddSourceMapping_SourceLineCannotBeNegative
#[test]
fn add_source_mapping_source_line_cannot_be_negative() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    assert_eq!(
        gen.add_source_mapping(0, Utf16Offset(0), source_index, -1, Utf16Offset(0))
            .unwrap_err()
            .to_string(),
        "sourceLine cannot be negative",
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddSourceMapping_SourceCharacterCannotBeNegative
#[test]
fn add_source_mapping_source_char_cannot_be_negative() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    assert_eq!(
        gen.add_source_mapping(0, Utf16Offset(0), source_index, 0, Utf16Offset(-1))
            .unwrap_err()
            .to_string(),
        "sourceCharacter cannot be negative",
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddNamedSourceMapping_GeneratedLineCannotBacktrack
#[test]
fn add_named_source_mapping_line_cannot_backtrack() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    let name_index = gen.add_name("foo");
    assert!(gen
        .add_named_source_mapping(
            1,
            Utf16Offset(0),
            source_index,
            0,
            Utf16Offset(0),
            name_index
        )
        .is_ok());
    assert_eq!(
        gen.add_named_source_mapping(
            0,
            Utf16Offset(0),
            source_index,
            0,
            Utf16Offset(0),
            name_index
        )
        .unwrap_err()
        .to_string(),
        "generatedLine cannot backtrack",
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddNamedSourceMapping_GeneratedCharacterCannotBeNegative
#[test]
fn add_named_source_mapping_char_cannot_be_negative() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    let name_index = gen.add_name("foo");
    assert!(gen
        .add_named_source_mapping(
            0,
            Utf16Offset(0),
            source_index,
            0,
            Utf16Offset(0),
            name_index
        )
        .is_ok());
    assert_eq!(
        gen.add_named_source_mapping(
            0,
            Utf16Offset(-1),
            source_index,
            0,
            Utf16Offset(0),
            name_index
        )
        .unwrap_err()
        .to_string(),
        "generatedCharacter cannot be negative",
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddNamedSourceMapping_SourceIndexIsOutOfRange
#[test]
fn add_named_source_mapping_source_index_out_of_range() {
    let mut gen = base();
    let name_index = gen.add_name("foo");
    assert_eq!(
        gen.add_named_source_mapping(
            0,
            Utf16Offset(0),
            SourceIndex(-1),
            0,
            Utf16Offset(0),
            name_index
        )
        .unwrap_err()
        .to_string(),
        "sourceIndex is out of range",
    );
    assert_eq!(
        gen.add_named_source_mapping(
            0,
            Utf16Offset(0),
            SourceIndex(0),
            0,
            Utf16Offset(0),
            name_index
        )
        .unwrap_err()
        .to_string(),
        "sourceIndex is out of range",
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddNamedSourceMapping_SourceLineCannotBeNegative
#[test]
fn add_named_source_mapping_source_line_cannot_be_negative() {
    let mut gen = base();
    let name_index = gen.add_name("foo");
    let source_index = gen.add_source("/main.ts");
    assert_eq!(
        gen.add_named_source_mapping(
            0,
            Utf16Offset(0),
            source_index,
            -1,
            Utf16Offset(0),
            name_index
        )
        .unwrap_err()
        .to_string(),
        "sourceLine cannot be negative",
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddNamedSourceMapping_SourceCharacterCannotBeNegative
#[test]
fn add_named_source_mapping_source_char_cannot_be_negative() {
    let mut gen = base();
    let name_index = gen.add_name("foo");
    let source_index = gen.add_source("/main.ts");
    assert_eq!(
        gen.add_named_source_mapping(
            0,
            Utf16Offset(0),
            source_index,
            0,
            Utf16Offset(-1),
            name_index
        )
        .unwrap_err()
        .to_string(),
        "sourceCharacter cannot be negative",
    );
}

// Go: internal/sourcemap/generator_test.go:TestSourceMapGenerator_AddNamedSourceMapping_NameIndexIsOutOfRange
#[test]
fn add_named_source_mapping_name_index_out_of_range() {
    let mut gen = base();
    let source_index = gen.add_source("/main.ts");
    assert_eq!(
        gen.add_named_source_mapping(
            0,
            Utf16Offset(0),
            source_index,
            0,
            Utf16Offset(0),
            NameIndex(-1)
        )
        .unwrap_err()
        .to_string(),
        "nameIndex is out of range",
    );
    assert_eq!(
        gen.add_named_source_mapping(
            0,
            Utf16Offset(0),
            source_index,
            0,
            Utf16Offset(0),
            NameIndex(0)
        )
        .unwrap_err()
        .to_string(),
        "nameIndex is out of range",
    );
}
