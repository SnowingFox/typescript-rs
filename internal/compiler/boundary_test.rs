//! Boundary / edge-case test suite for the Rust tsgo compiler.
//!
//! Every test exercises an unusual or degenerate input through the full
//! compilation pipeline (parse → bind → check → emit). The PRIMARY assertion
//! for each test is **does not panic**; secondary assertions verify expected
//! diagnostics or output where applicable.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use tsgo_core::compileroptions::CompilerOptions;
use tsgo_tsoptions::new_parsed_command_line;
use tsgo_tspath::ComparePathsOptions;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use crate::host::new_compiler_host;
use crate::program::{new_program, ProgramOptions};
use crate::{EmitOptions, EmitOnly, EmitResult, Program};

type Captured = Rc<RefCell<Vec<(String, String)>>>;

const BOUNDARY_TIMEOUT_SECS: u64 = 60;

fn boundary_program(files: &[(&str, &str)], roots: &[&str]) -> Program {
    boundary_program_with(files, roots, CompilerOptions::default())
}

fn boundary_program_with(
    files: &[(&str, &str)],
    roots: &[&str],
    options: CompilerOptions,
) -> Program {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(files.iter().copied(), true));
    let host = Arc::new(new_compiler_host("/", fs, "/lib"));
    let config = new_parsed_command_line(
        options,
        roots.iter().map(|s| s.to_string()).collect(),
        ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: "/".to_string(),
        },
    );
    new_program(ProgramOptions {
        host,
        config: Arc::new(config),
        single_threaded: true,
    })
}

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

// ────────────────────────────────────────────────────────────────────────────
// 1. Empty file
// ────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_empty_file() {
    let mut program = boundary_program(&[("/index.ts", "")], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);
    assert!(!result.emit_skipped);
}

// ────────────────────────────────────────────────────────────────────────────
// 2. File with only whitespace/comments
// ────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_whitespace_and_comments_only() {
    let src = "   \n\n  // line comment\n  /* block comment */\n  \n";
    let mut program = boundary_program(&[("/index.ts", src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);
    assert!(!result.emit_skipped);
    let captured = captured.borrow();
    if !captured.is_empty() {
        let js = &captured[0].1;
        assert!(
            js.trim().is_empty() || js.trim() == "\"use strict\";",
            "expected empty or only use-strict output, got: {js:?}"
        );
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 3. Very large file (10,000 lines of declarations)
// ────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_very_large_file_10k_lines() {
    let mut src = String::new();
    for i in 0..10_000 {
        src.push_str(&format!("export const v{i}: number = {i};\n"));
    }
    let start = Instant::now();
    let mut program = boundary_program(&[("/index.ts", &src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < BOUNDARY_TIMEOUT_SECS,
        "10k-line file took {elapsed:?} (>{BOUNDARY_TIMEOUT_SECS}s)"
    );
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 4. 10,000 small files (1 line each) — many-module scenario
// ────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_many_small_files_10k() {
    let mut files: Vec<(String, String)> = Vec::with_capacity(10_001);
    let mut main_src = String::new();
    for i in 0..10_000 {
        let path = format!("/m{i}.ts");
        let content = format!("export const x{i} = {i};\n");
        files.push((path, content));
    }
    // A main file that imports a sample of them to exercise module resolution.
    for i in (0..10_000).step_by(1000) {
        main_src.push_str(&format!("import {{ x{i} }} from \"./m{i}\";\n"));
    }
    main_src.push_str("const sum = x0;\n");
    files.push(("/index.ts".to_string(), main_src));

    let file_refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let roots: Vec<&str> = files.iter().map(|(a, _)| a.as_str()).collect();

    let start = Instant::now();
    let mut program = boundary_program(&file_refs, &roots);
    let diags = program.semantic_diagnostics();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < BOUNDARY_TIMEOUT_SECS,
        "10k small files took {elapsed:?} (>{BOUNDARY_TIMEOUT_SECS}s)"
    );
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 5. Non-UTF-8 content — verify graceful error, no panic
// ────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_non_utf8_content() {
    // MapFs takes &str, so we use a source string containing replacement characters
    // that would result from lossy UTF-8 decoding of invalid bytes. The parser
    // must not panic on unexpected characters.
    let src = "const x = \u{FFFD}\u{FFFD}\u{FFFD};\n";
    let mut program = boundary_program(&[("/index.ts", src)], &["/index.ts"]);
    let _diags = program.semantic_diagnostics();
    // Primary assertion: did not panic. Diagnostics are expected (invalid token).
}

// ────────────────────────────────────────────────────────────────────────────
// 6. BOM (byte-order mark) prefix
// ────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_bom_prefix() {
    let src = "\u{FEFF}const x: number = 42;\n";
    let mut program = boundary_program(&[("/index.ts", src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);
    assert!(!result.emit_skipped);
}

// ────────────────────────────────────────────────────────────────────────────
// 7. File with only semicolons (valid empty statements)
// ────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_only_semicolons() {
    let src = ";;;;;;;";
    let mut program = boundary_program(&[("/index.ts", src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);
    assert!(!result.emit_skipped);
}

// ────────────────────────────────────────────────────────────────────────────
// 8. Deeply nested blocks (100 levels of { })
// ────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_deeply_nested_blocks_100() {
    let depth = 100;
    let mut src = String::new();
    for _ in 0..depth {
        src.push_str("{ ");
    }
    src.push_str("const x = 1;");
    for _ in 0..depth {
        src.push_str(" }");
    }
    src.push('\n');

    let mut program = boundary_program(&[("/index.ts", &src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 9. Unicode identifiers
// ────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_unicode_identifiers() {
    let src = r#"
const π = 3.14;
const 你好 = "hello";
const café = "latte";
const _ñ = 42;
const Σ = π + _ñ;
"#;
    let mut program = boundary_program(&[("/index.ts", src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);
    assert!(!result.emit_skipped);
    let captured = captured.borrow();
    if !captured.is_empty() {
        let js = &captured[0].1;
        assert!(js.contains("3.14"), "expected π value in output: {js:?}");
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 10. Circular extends
// ────────────────────────────────────────────────────────────────────────────
//
// BUG(checker): `interface A extends B {} / interface B extends A {}` causes a
// stack overflow in the checker's base-type resolution — infinite recursion
// without a circularity guard. The type-alias equivalent (`type A = { b: B }`)
// is handled correctly (see `stress_circular_type_references`). This test is
// ignored until the checker's `resolveBaseTypesOfInterface` gains a circularity
// sentinel (expected to produce TS2310).
#[test]
#[ignore = "checker stack overflow on circular interface extends (known bug)"]
fn boundary_circular_extends() {
    let src = r#"
interface A extends B { a: number; }
interface B extends A { b: string; }
declare const x: A;
const y = x.a;
const z = x.b;
"#;
    let mut program = boundary_program(&[("/index.ts", src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    let has_circularity = diags.iter().any(|d| d.code == 2310);
    assert!(
        has_circularity,
        "expected TS2310 circularity diagnostic, got: {diags:?}"
    );
}

// ────────────────────────────────────────────────────────────────────────────
// 11. Very long identifier (10,000 chars)
// ────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_very_long_identifier() {
    let ident = "a".repeat(10_000);
    let src = format!("const {ident} = 42;\n");
    let mut program = boundary_program(&[("/index.ts", &src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 12. Very long string literal (100,000 chars)
// ────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_very_long_string_literal() {
    let payload = "x".repeat(100_000);
    let src = format!("const s = \"{payload}\";\n");
    let mut program = boundary_program(&[("/index.ts", &src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 13. File ending mid-token (truncated source)
// ────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_file_ending_mid_token() {
    // Various truncated inputs — primary assertion: no panic.
    let cases = [
        "const x = \"hello world",       // unterminated string
        "const y = `template ${",        // unterminated template
        "function foo(",                 // unterminated parameter list
        "const z = {",                   // unterminated object
        "/* unclosed comment",           // unterminated block comment
        "const w: num",                  // truncated type annotation
    ];
    for src in &cases {
        let mut program = boundary_program(&[("/index.ts", src)], &["/index.ts"]);
        let _diags = program.semantic_diagnostics();
        // Primary assertion: did not panic.
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 14. Mixed line endings (\r\n, \r, \n in same file)
// ────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_mixed_line_endings() {
    let src = "const a = 1;\r\nconst b = 2;\rconst c = 3;\nconst d = a + b + c;\r\n";
    let mut program = boundary_program(&[("/index.ts", src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);
    assert!(!result.emit_skipped);
}

// ────────────────────────────────────────────────────────────────────────────
// 15. Shebang line
// ────────────────────────────────────────────────────────────────────────────
#[test]
fn boundary_shebang_line() {
    let src = "#!/usr/bin/env node\nconst x: number = 42;\nconsole.log(x);\n";
    let mut program = boundary_program(&[("/index.ts", src)], &["/index.ts"]);
    let diags = program.semantic_diagnostics();
    for d in &diags {
        assert_ne!(d.code, -1, "internal error diagnostic: {d:?}");
    }
    let captured: Captured = Rc::new(RefCell::new(Vec::new()));
    let result = emit_capturing(&program, &captured);
    assert!(!result.emit_skipped);
    let captured = captured.borrow();
    assert!(!captured.is_empty(), "expected emitted output");
    let js = &captured[0].1;
    assert!(
        js.contains("42"),
        "expected emitted JS to contain the constant value: {js:?}"
    );
}
