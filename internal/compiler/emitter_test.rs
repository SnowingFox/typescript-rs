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
