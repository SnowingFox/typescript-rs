use super::*;
use crate::{create_ecma_line_info, Generator};
use std::collections::HashMap;
use tsgo_core::{compute_ecma_line_starts, Utf16Offset};
use tsgo_tspath::ComparePathsOptions;

// Go: internal/sourcemap/source_mapper.go:tryParseBase64Url
#[test]
fn try_parse_base64_url_ok() {
    let (parseable, is_base64) = try_parse_base64_url("data:application/json;base64,AAA=");
    assert_eq!(parseable, "AAA=");
    assert!(is_base64);
}

// Go: internal/sourcemap/source_mapper.go:tryParseBase64Url (not a data url)
#[test]
fn try_parse_base64_url_not_data() {
    let (parseable, is_base64) = try_parse_base64_url("https://example.com/a.js.map");
    assert_eq!(parseable, "");
    assert!(!is_base64);
}

// Go: internal/sourcemap/source_mapper.go:tryParseBase64Url (data, but not json/base64)
#[test]
fn try_parse_base64_url_not_json() {
    let (parseable, is_base64) = try_parse_base64_url("data:text/plain,hello");
    assert_eq!(parseable, "");
    assert!(is_base64);
}

struct FakeHost {
    files: HashMap<String, String>,
}

impl Host for FakeHost {
    fn use_case_sensitive_file_names(&self) -> bool {
        false
    }
    fn get_ecma_line_info(&self, file_name: &str) -> Option<EcmaLineInfo> {
        self.files
            .get(file_name)
            .map(|text| create_ecma_line_info(text, compute_ecma_line_starts(text)))
    }
    fn read_file(&self, file_name: &str) -> Option<String> {
        self.files.get(file_name).cloned()
    }
}

// Go: internal/sourcemap/source_mapper.go:GetDocumentPositionMapper / GetSourcePosition / GetGeneratedPosition
#[test]
fn document_position_mapper_roundtrip() {
    // Build a real source map: generated "/dir/out.js" (0,0) -> source "/dir/in.ts" (0,0).
    let mut gen = Generator::new("out.js", "", "/dir", ComparePathsOptions::default());
    let idx = gen.add_source("/dir/in.ts");
    gen.add_source_mapping(0, Utf16Offset(0), idx, 0, Utf16Offset(0))
        .unwrap();
    let map_json = gen.to_string();

    let mut files = HashMap::new();
    files.insert("/dir/out.js".to_string(), "ab".to_string());
    files.insert("/dir/in.ts".to_string(), "ab".to_string());
    files.insert("/dir/out.js.map".to_string(), map_json);
    let host = FakeHost { files };

    let mapper = get_document_position_mapper(&host, "/dir/out.js").expect("mapper");

    let source = mapper
        .get_source_position(&DocumentPosition {
            file_name: "/dir/out.js".to_string(),
            pos: 0,
        })
        .expect("source position");
    assert_eq!(source.file_name, "/dir/in.ts");
    assert_eq!(source.pos, 0);

    let generated = mapper
        .get_generated_position(&DocumentPosition {
            file_name: "/dir/in.ts".to_string(),
            pos: 0,
        })
        .expect("generated position");
    assert_eq!(generated.file_name, "/dir/out.js");
    assert_eq!(generated.pos, 0);
}
