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
// Tracer bullet: a single TypeScript file runs the full transformer pipeline
// (type eraser + runtime syntax + ES downlevel + use strict + module) and emits
// JavaScript text. The full chain prepends `"use strict";` matching Go's output.
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
    // Full chain: type eraser strips `: number`, use-strict adds the directive.
    assert_eq!(captured[0].1, "\"use strict\";\nconst x = 1;\n");
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
    assert_eq!(captured[0].1, "\u{FEFF}\"use strict\";\nconst x = 1;\n");
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
            ("/src/a.js", "\"use strict\";\nconst a = 1;\n"),
            ("/src/index.js", "\"use strict\";\nconst b = 2;\n"),
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
    assert_eq!(written.as_deref(), Some("\"use strict\";\nconst x = 1;\n"));
}

// Slice 2: `--sourceMap` writes `out.js` + `out.js.map` and appends a
// `//# sourceMappingURL=out.js.map` comment to the JS.
//
// With the full transform chain the use-strict transform prepends `"use strict";`
// on line 1. The source mappings now start with `;` (empty first generated line
// in the mapping), matching Go's ground truth exactly:
// `cmd/tsgo --sourceMap` → `;AAAA,MAAM,CAAC,GAAW,CAAC,CAAC`.
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

    // JS gets `"use strict"` + the code + trailing URL comment.
    assert_eq!(
        captured[1].1,
        "\"use strict\";\nconst x = 1;\n//# sourceMappingURL=index.js.map"
    );

    // The `.map` JSON: the leading `;` reflects the synthesized `"use strict";`
    // line (no source mapping for it), matching Go's ground truth.
    assert_eq!(
        captured[0].1,
        r#"{"version":3,"file":"index.js","sourceRoot":"","sources":["index.ts"],"names":[],"mappings":";AAAA,MAAM,CAAC,GAAW,CAAC,CAAC"}"#
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
// URL inline and writes no separate `.map` file. With the full chain the
// `"use strict";` line shifts the mappings (leading `;`), changing the base64.
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

    // The JS text now starts with `"use strict";` and ends with the inline data URL.
    let text = &captured[0].1;
    assert!(text.starts_with(
        "\"use strict\";\nconst x = 1;\n//# sourceMappingURL=data:application/json;base64,"
    ));
    // The data URL encodes the map JSON with `;AAAA,...` (the leading `;` for
    // the synthesized `"use strict";` line), matching Go's ground truth.
    assert!(text.contains("sourceMappingURL=data:application/json;base64,"));

    assert_eq!(result.source_maps.len(), 1);
    assert_eq!(result.source_maps[0].generated_file, "/src/index.js");
}

// Slice 5: without `--sourceMap`/`--inlineSourceMap` the output has no
// `//# sourceMappingURL=` comment, no `.map` file, and no recorded source map.
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
    assert_eq!(captured[0].1, "\"use strict\";\nconst x = 1;\n");
    assert!(!captured[0].1.contains("//# sourceMappingURL="));
}

// Go: internal/compiler/emitter.go:getScriptTransformers (module transform chain)
// With `module: CommonJS`, the full chain fires: type eraser → runtime syntax →
// use strict → CJS module transform. `export const x = 1;` becomes CJS output
// with `"use strict"`, the `__esModule` marker, and `exports.x = 1;`.
#[test]
fn emit_cjs_module_fires_full_chain() {
    let options = CompilerOptions {
        module: tsgo_core::compileroptions::ModuleKind::CommonJs,
        ..Default::default()
    };
    let program = build_program(
        &[("/src/index.ts", "export const x = 1;")],
        &["/src/index.ts"],
        options,
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    let captured = captured.borrow();
    assert_eq!(captured.len(), 1);
    let text = &captured[0].1;
    // CJS transform must fire: `export const` syntax is gone.
    assert!(
        !text.contains("export const"),
        "CJS transform should have lowered the ES export: {text}"
    );
    // The `__esModule` marker is present.
    assert!(
        text.contains("__esModule"),
        "CJS output should contain __esModule marker: {text}"
    );
    // The exported binding is assigned.
    assert!(
        text.contains("exports.x = 1"),
        "CJS output should contain exports.x = 1: {text}"
    );
    // The use-strict transform fires (directive is present in the output).
    assert!(
        text.contains("\"use strict\""),
        "CJS output should contain \"use strict\" directive: {text}"
    );
}

// Go: internal/compiler/emitter.go:getScriptTransformers (JSX transform)
// With `jsx: react` and a `.tsx` file, the JSX transform fires and lowers
// `<div/>` to `React.createElement("div", null)`.
#[test]
fn emit_jsx_react_lowers_element() {
    let options = CompilerOptions {
        jsx: tsgo_core::compileroptions::JsxEmit::React,
        ..Default::default()
    };
    let program = build_program(
        &[("/src/app.tsx", "const el = <div/>;")],
        &["/src/app.tsx"],
        options,
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    let captured = captured.borrow();
    assert_eq!(captured.len(), 1);
    let text = &captured[0].1;
    // JSX transform should lower `<div/>` to a createElement call.
    assert!(
        !text.contains("<div"),
        "JSX syntax should be lowered: {text}"
    );
    assert!(
        text.contains("React.createElement"),
        "JSX should lower to React.createElement: {text}"
    );
}

// Go: internal/compiler/emitter.go:getScriptTransformers (module transform chain)
// With `module: CommonJS`, `import { x } from "./m"; console.log(x);` lowers the
// import to `require("./m")` and rewrites the use-site `x` to `m_1.x` (the CJS
// module transform's textual name-match fallback, since no resolver is threaded
// through the emitter yet). `"use strict"` is prepended (module < ES2015).
#[test]
fn emit_cjs_import_and_use_lowers_to_require_and_member_access() {
    let options = CompilerOptions {
        module: tsgo_core::compileroptions::ModuleKind::CommonJs,
        ..Default::default()
    };
    let program = build_program(
        &[(
            "/src/index.ts",
            "import { x } from \"./m\";\nconsole.log(x);",
        )],
        &["/src/index.ts"],
        options,
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    let captured = captured.borrow();
    assert_eq!(captured.len(), 1);
    let text = &captured[0].1;
    // CJS transform fires: `import` syntax is gone.
    assert!(
        !text.contains("import {"),
        "CJS transform should have lowered the ES import: {text}"
    );
    // A `require("./m")` call is present.
    assert!(
        text.contains("require(\"./m\")"),
        "CJS output should contain require(\"./m\"): {text}"
    );
    // The use-site `x` is rewritten to a qualified member access on the alias.
    assert!(
        text.contains(".x"),
        "CJS output should rewrite the use of `x` to a member access: {text}"
    );
    // `"use strict"` is prepended (module < ES2015).
    assert!(
        text.contains("\"use strict\""),
        "CJS output should contain \"use strict\" directive: {text}"
    );
}

// Go: internal/compiler/emitter.go:getScriptTransformers (ESM passthrough)
// With `module: EsNext`, `export default function() {}` is preserved as-is: the
// implied module transformer dispatches to the ES module transform which is a
// passthrough for simple exports. `"use strict"` is skipped because the file is
// an external module emitted as ESM (module >= ES2015).
#[test]
fn emit_esnext_export_default_function_is_preserved() {
    let options = CompilerOptions {
        module: tsgo_core::compileroptions::ModuleKind::EsNext,
        ..Default::default()
    };
    let program = build_program(
        &[("/src/index.ts", "export default function() {}")],
        &["/src/index.ts"],
        options,
    );
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);

    assert!(!result.emit_skipped);
    let captured = captured.borrow();
    assert_eq!(captured.len(), 1);
    let text = &captured[0].1;
    // ESM passthrough: `export default` syntax is preserved.
    assert!(
        text.contains("export default function"),
        "ESM passthrough should preserve export default function: {text}"
    );
    // No `"use strict"` — ESM is implicitly strict (module >= ES2015 + external module).
    assert!(
        !text.contains("\"use strict\""),
        "ESM output should NOT contain \"use strict\" (ESM is implicitly strict): {text}"
    );
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
    assert_eq!(captured[0].1, "\"use strict\";\r\nconst x = 1;\r\n");
}
