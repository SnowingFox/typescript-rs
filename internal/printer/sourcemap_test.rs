//! Source-map emit tests for the printer (driving `tsgo_sourcemap::Generator`).
//!
//! Ground truth captured with `cmd/tsgo --sourceMap --module esnext` (Go). The
//! reachable Rust emit subset runs only the type eraser (no module transform),
//! so it does **not** prepend the `"use strict";` prologue Go's full pipeline
//! adds. The mappings therefore match Go's exactly *minus* the leading `;` (the
//! generated line shift from the absent prologue): Go emits
//! `;AAAA,MAAM,CAAC,GAAG,CAAC,CAAC` for `const x = 1;`, this emits
//! `AAAA,MAAM,CAAC,GAAG,CAAC,CAAC`.

use crate::test_support::emit_with_source_map;
use tsgo_core::Utf16Offset;
use tsgo_sourcemap::{decode_mappings, SourceIndex};

// Slice 1 tracer: emitting `const x = 1;` with source maps enabled records the
// first mapping gen(0,0) -> src(0,0) and produces the expected VLQ string.
// Go: internal/printer/printer.go:Write (sourceMapGenerator) + emitPos
#[test]
fn source_map_records_first_mapping() {
    let (text, mut generator) =
        emit_with_source_map("const x = 1;", "/main.ts", "main.js", "/", false);
    assert_eq!(text, "const x = 1;\n");

    let map = generator.raw_source_map();
    assert_eq!(map.version, 3);
    assert_eq!(map.file, "main.js");
    assert_eq!(map.source_root, "");
    assert_eq!(map.sources, vec!["main.ts".to_string()]);
    assert!(map.names.is_empty());
    assert!(map.sources_content.is_none());

    // First decoded mapping ties generated (0,0) to source (0,0).
    let first = decode_mappings(&map.mappings).values().next().unwrap();
    assert_eq!(first.generated_line, 0);
    assert_eq!(first.generated_character, Utf16Offset(0));
    assert_eq!(first.source_index, SourceIndex(0));
    assert_eq!(first.source_line, 0);
    assert_eq!(first.source_character, Utf16Offset(0));

    // Exact VLQ string vs cmd/tsgo's `.map` (Go `;AAAA,...` minus the leading
    // `;` from the absent `"use strict";` prologue).
    assert_eq!(map.mappings, "AAAA,MAAM,CAAC,GAAG,CAAC,CAAC");
}

// Slice 4: a 2-line input produces mappings whose decoded segments point at the
// correct source lines (statement 1 -> source line 0, statement 2 -> line 1).
// Go ground truth (`cmd/tsgo --sourceMap --module esnext`):
//   `;AAAA,MAAM,CAAC,GAAG,CAAC,CAAC;AACZ,MAAM,CAAC,GAAG,CAAC,CAAC`
// minus the leading `;` (no `"use strict";` in the reachable subset).
// Go: internal/printer/printer.go:Write (multi-statement mappings)
#[test]
fn source_map_multi_statement_lines() {
    let (text, mut generator) = emit_with_source_map(
        "const x = 1;\nconst y = 2;",
        "/main.ts",
        "main.js",
        "/",
        false,
    );
    assert_eq!(text, "const x = 1;\nconst y = 2;\n");

    let map = generator.raw_source_map();
    assert_eq!(
        map.mappings,
        "AAAA,MAAM,CAAC,GAAG,CAAC,CAAC;AACZ,MAAM,CAAC,GAAG,CAAC,CAAC"
    );

    let mappings: Vec<_> = decode_mappings(&map.mappings).values().collect();

    // First generated line maps to source line 0; the leading mapping is the
    // statement start at column 0 -> source (0,0).
    let line0: Vec<_> = mappings.iter().filter(|m| m.generated_line == 0).collect();
    assert!(!line0.is_empty());
    assert_eq!(line0[0].source_line, 0);
    assert_eq!(line0[0].generated_character, Utf16Offset(0));
    assert_eq!(line0[0].source_character, Utf16Offset(0));
    assert!(line0.iter().all(|m| m.source_line == 0));

    // Second generated line maps to source line 1, restarting at column 0.
    let line1: Vec<_> = mappings.iter().filter(|m| m.generated_line == 1).collect();
    assert!(!line1.is_empty());
    assert_eq!(line1[0].source_line, 1);
    assert_eq!(line1[0].generated_character, Utf16Offset(0));
    assert_eq!(line1[0].source_character, Utf16Offset(0));
    assert!(line1.iter().all(|m| m.source_line == 1));
}

// Slice 4 follow-up: `inline_sources` embeds the full source text into the
// map's `sourcesContent` (Go `--inlineSources`).
// Go: internal/printer/printer.go:setSourceMapSource (InlineSources)
#[test]
fn source_map_inline_sources_embeds_content() {
    let input = "const x = 1;";
    let (_text, mut generator) = emit_with_source_map(input, "/main.ts", "main.js", "/", true);
    let map = generator.raw_source_map();
    assert_eq!(
        map.sources_content,
        Some(vec![Some(input.to_string())]),
        "inline_sources should embed the verbatim source text"
    );
}
