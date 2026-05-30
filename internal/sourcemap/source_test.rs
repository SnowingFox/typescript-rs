use super::*;
use tsgo_core::text::TextPos;

struct FakeSource {
    text: String,
    file_name: String,
    line_map: Vec<TextPos>,
}

impl Source for FakeSource {
    fn text(&self) -> &str {
        &self.text
    }
    fn file_name(&self) -> &str {
        &self.file_name
    }
    fn ecma_line_map(&self) -> &[TextPos] {
        &self.line_map
    }
}

// Go: internal/sourcemap/source.go:Source
#[test]
fn source_trait_getters() {
    let s = FakeSource {
        text: "abc".to_string(),
        file_name: "a.ts".to_string(),
        line_map: vec![TextPos(0)],
    };
    assert_eq!(s.text(), "abc");
    assert_eq!(s.file_name(), "a.ts");
    assert_eq!(s.ecma_line_map(), &[TextPos(0)]);
}
