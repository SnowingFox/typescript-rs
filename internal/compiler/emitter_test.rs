use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use tsgo_core::compileroptions::{CompilerOptions, NewLineKind};
use tsgo_core::tristate::Tristate;
use tsgo_tsoptions::new_parsed_command_line;
use tsgo_tspath::ComparePathsOptions;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use crate::{new_compiler_host, new_program, EmitOptions, EmitResult, Program, ProgramOptions};

/// Captures `(file_name, text)` pairs written by an emit, so tests can assert on
/// the emitter's output without touching the file system.
type Captured = Rc<RefCell<Vec<(String, String)>>>;

/// Builds a single-threaded program over `files` rooted at `roots` under
/// `/src`, applying `options`.
fn build_program(files: &[(&str, &str)], roots: &[&str], options: CompilerOptions) -> Program {
    let map: Vec<(String, String)> = files
        .iter()
        .map(|(name, text)| (name.to_string(), text.to_string()))
        .collect();
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(
        map.iter().map(|(n, t)| (n.as_str(), t.as_str())),
        true,
    ));
    let host = Arc::new(new_compiler_host("/src", fs, "/lib"));
    let config = new_parsed_command_line(
        options,
        roots.iter().map(|r| r.to_string()).collect(),
        ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: "/src".into(),
        },
    );
    new_program(ProgramOptions {
        host,
        config: Arc::new(config),
        single_threaded: true,
    })
}

/// Emits `program`, capturing every written file into `captured`.
fn emit_capturing(program: &Program, captured: &Captured) -> EmitResult {
    let sink = Rc::clone(captured);
    program.emit(EmitOptions {
        target_source_file: None,
        emit_only: EmitOnly::All,
        write_file: Some(Box::new(move |name, text, _data| {
            sink.borrow_mut().push((name.to_string(), text.to_string()));
            Ok(())
        })),
    })
}

// Go: internal/compiler/emit_test.go (functional equivalent of BenchmarkEmit*)
// Tracer bullet: a single TypeScript file runs the transformer pipeline (type
// eraser) -> printer end-to-end and emits JavaScript text.
#[test]
fn emit_single_js_basic() {
    let program = build_program(
        &[("/src/index.ts", "const x: number = 1;")],
        &["/src/index.ts"],
        CompilerOptions::default(),
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    assert_eq!(result.emitted_files, vec!["/src/index.js".to_string()]);
    let captured = captured.borrow();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].0, "/src/index.js");
    assert_eq!(captured[0].1, "const x = 1;\n");
}

// Go: internal/compiler/emitter.go:emitter.emitJSFile (`options.NoEmit == TSTrue`)
#[test]
fn emit_skipped_when_no_emit() {
    let options = CompilerOptions {
        no_emit: Tristate::True,
        ..Default::default()
    };
    let program = build_program(
        &[("/src/index.ts", "const x: number = 1;")],
        &["/src/index.ts"],
        options,
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(result.emit_skipped);
    assert!(result.emitted_files.is_empty());
    assert!(captured.borrow().is_empty());
}

// Go: internal/compiler/emitter.go:emitter.printSourceFile (`options.EmitBOM`)
#[test]
fn emit_prepends_bom_when_emit_bom() {
    let options = CompilerOptions {
        emit_bom: Tristate::True,
        ..Default::default()
    };
    let program = build_program(
        &[("/src/index.ts", "const x: number = 1;")],
        &["/src/index.ts"],
        options,
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    emit_capturing(&program, &captured);

    let captured = captured.borrow();
    assert_eq!(captured[0].1, "\u{FEFF}const x = 1;\n");
}

// Go: internal/compiler/program.go:Program.Emit + CombineEmitResults
// Determinism: multiple files emit in input order and the combined result
// preserves that order (the PORTING.md determinism rule for emit).
#[test]
fn emit_combines_multiple_files_in_input_order() {
    let program = build_program(
        &[
            ("/src/a.ts", "const a: number = 1;"),
            ("/src/index.ts", "const b: number = 2;"),
        ],
        &["/src/a.ts", "/src/index.ts"],
        CompilerOptions::default(),
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert_eq!(
        result.emitted_files,
        vec!["/src/a.js".to_string(), "/src/index.js".to_string()]
    );
    let captured = captured.borrow();
    assert_eq!(
        captured
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("/src/a.js", "const a = 1;\n"),
            ("/src/index.js", "const b = 2;\n"),
        ]
    );
}

// Go: internal/compiler/emitter.go:sourceFileMayBeEmitted (declaration files)
#[test]
fn emit_skips_declaration_files() {
    let program = build_program(
        &[
            ("/src/a.d.ts", "declare const y: number;"),
            ("/src/index.ts", "const x: number = 1;"),
        ],
        &["/src/a.d.ts", "/src/index.ts"],
        CompilerOptions::default(),
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert_eq!(result.emitted_files, vec!["/src/index.js".to_string()]);
}

// Go: internal/compiler/program.go:EmitOptions.TargetSourceFile
#[test]
fn emit_target_source_file_emits_only_that_file() {
    let program = build_program(
        &[
            ("/src/a.ts", "const a: number = 1;"),
            ("/src/index.ts", "const b: number = 2;"),
        ],
        &["/src/a.ts", "/src/index.ts"],
        CompilerOptions::default(),
    );
    let result = program.emit(EmitOptions {
        target_source_file: Some("/src/index.ts".to_string()),
        emit_only: EmitOnly::All,
        write_file: None,
    });

    assert_eq!(result.emitted_files, vec!["/src/index.js".to_string()]);
}

// Go: internal/compiler/emitter.go:emitter.writeText (host.WriteFile fallback)
#[test]
fn emit_writes_through_host_fs_by_default() {
    let program = build_program(
        &[("/src/index.ts", "const x: number = 1;")],
        &["/src/index.ts"],
        CompilerOptions::default(),
    );
    let result = program.emit(EmitOptions {
        target_source_file: None,
        emit_only: EmitOnly::All,
        write_file: None,
    });

    assert_eq!(result.emitted_files, vec!["/src/index.js".to_string()]);
    let written = program.host().fs().read_file("/src/index.js");
    assert_eq!(written.as_deref(), Some("const x = 1;\n"));
}

// Slice 2: `--sourceMap` writes `out.js` + `out.js.map` and appends a
// `//# sourceMappingURL=out.js.map` comment to the JS.
//
// Go ground truth (`cmd/tsgo --sourceMap --module esnext`) for
// `const x: number = 1;` emits a leading `"use strict";` (module pipeline) and
// the map `;AAAA,MAAM,CAAC,GAAW,CAAC,CAAC`. The reachable Rust subset runs only
// the type eraser (no module transform), so it omits `"use strict";` and the
// mappings lose the leading `;` (the generated-line shift).
// Go: internal/compiler/emitter.go:printSourceFile (SourceMap branch)
#[test]
fn emit_source_map_writes_map_file_and_url_comment() {
    let options = CompilerOptions {
        source_map: Tristate::True,
        ..Default::default()
    };
    let program = build_program(
        &[("/src/index.ts", "const x: number = 1;")],
        &["/src/index.ts"],
        options,
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    // The `.map` is written before the `.js` (Go EmittedFiles order).
    assert_eq!(
        result.emitted_files,
        vec!["/src/index.js.map".to_string(), "/src/index.js".to_string()]
    );
    let captured = captured.borrow();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].0, "/src/index.js.map");
    assert_eq!(captured[1].0, "/src/index.js");

    // JS gets the trailing URL comment (no trailing newline after it).
    assert_eq!(
        captured[1].1,
        "const x = 1;\n//# sourceMappingURL=index.js.map"
    );

    // The `.map` JSON matches Go exactly (key order + relativized source), with
    // the mappings string lacking the leading `;` (no `"use strict";`).
    assert_eq!(
        captured[0].1,
        r#"{"version":3,"file":"index.js","sourceRoot":"","sources":["index.ts"],"names":[],"mappings":"AAAA,MAAM,CAAC,GAAW,CAAC,CAAC"}"#
    );

    // A source-map emit result is recorded with the raw (un-relativized) source.
    assert_eq!(result.source_maps.len(), 1);
    assert_eq!(result.source_maps[0].generated_file, "/src/index.js");
    assert_eq!(
        result.source_maps[0].input_source_file_names,
        vec!["/src/index.ts".to_string()]
    );
}

// Slice 3: `--inlineSourceMap` appends a `data:application/json;base64,<...>`
// URL inline and writes no separate `.map` file. The base64 decodes to the
// same JSON the file-mode map carries.
// Go: internal/compiler/emitter.go:getSourceMappingURL (InlineSourceMap)
#[test]
fn emit_inline_source_map_appends_base64_data_url() {
    let options = CompilerOptions {
        inline_source_map: Tristate::True,
        ..Default::default()
    };
    let program = build_program(
        &[("/src/index.ts", "const x: number = 1;")],
        &["/src/index.ts"],
        options,
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    // Only the `.js` is written; no separate `.map`.
    assert_eq!(result.emitted_files, vec!["/src/index.js".to_string()]);
    let captured = captured.borrow();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].0, "/src/index.js");

    // The data URL's base64 payload is the same JSON as file mode (verified
    // against the base64 of the Go-derived JSON).
    assert_eq!(
        captured[0].1,
        concat!(
            "const x = 1;\n//# sourceMappingURL=data:application/json;base64,",
            "eyJ2ZXJzaW9uIjozLCJmaWxlIjoiaW5kZXguanMiLCJzb3VyY2VSb290IjoiIiwi",
            "c291cmNlcyI6WyJpbmRleC50cyJdLCJuYW1lcyI6W10sIm1hcHBpbmdzIjoiQUFB",
            "QSxNQUFNLENBQUMsR0FBVyxDQUFDLENBQUMifQ=="
        )
    );
    // No separate source-map emit result file path beyond the inline payload,
    // but the result still records the source map (Go appends it for inline).
    assert_eq!(result.source_maps.len(), 1);
    assert_eq!(result.source_maps[0].generated_file, "/src/index.js");
}

// Slice 5: without `--sourceMap`/`--inlineSourceMap` the output is byte-for-byte
// the pre-P6-7 emit — no `//# sourceMappingURL=` comment, no `.map` file, and no
// recorded source map (the plain path must be unchanged).
// Go: internal/compiler/emitter.go:printSourceFile (sourceMapGenerator == nil)
#[test]
fn emit_without_source_map_has_no_url_or_map() {
    let program = build_program(
        &[("/src/index.ts", "const x: number = 1;")],
        &["/src/index.ts"],
        CompilerOptions::default(),
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert_eq!(result.emitted_files, vec!["/src/index.js".to_string()]);
    assert!(result.source_maps.is_empty());
    let captured = captured.borrow();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].1, "const x = 1;\n");
    assert!(!captured[0].1.contains("//# sourceMappingURL="));
}

// Go: internal/compiler/emitter.go:emitter.emitJSFile (PrinterOptions.NewLine)
#[test]
fn emit_honors_crlf_newline_option() {
    let options = CompilerOptions {
        new_line: NewLineKind::Crlf,
        ..Default::default()
    };
    let program = build_program(
        &[("/src/index.ts", "const x: number = 1;")],
        &["/src/index.ts"],
        options,
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    emit_capturing(&program, &captured);

    let captured = captured.borrow();
    assert_eq!(captured[0].1, "const x = 1;\r\n");
}
