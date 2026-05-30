use super::*;
use crate::{create_ecma_line_info, EcmaLineInfo};
use tsgo_core::compute_ecma_line_starts;

fn line_info(text: &str) -> EcmaLineInfo {
    create_ecma_line_info(text, compute_ecma_line_starts(text))
}

// Go: internal/sourcemap/util.go:TryGetSourceMappingURL (found)
#[test]
fn try_get_source_mapping_url_found() {
    let info = line_info("var x = 1;\n//# sourceMappingURL=a.js.map");
    assert_eq!(try_get_source_mapping_url(Some(&info)), "a.js.map");
}

// Go: internal/sourcemap/util.go:TryGetSourceMappingURL (none)
#[test]
fn try_get_source_mapping_url_none() {
    let info = line_info("var x = 1;\nconsole.log(x);");
    assert_eq!(try_get_source_mapping_url(Some(&info)), "");
}

// Go: internal/sourcemap/util.go:TryGetSourceMappingURL (nil line info)
#[test]
fn try_get_source_mapping_url_nil() {
    assert_eq!(try_get_source_mapping_url(None), "");
}
