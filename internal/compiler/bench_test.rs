//! Performance benchmarks for key tsgo compiler operations.
//!
//! Run with: `cargo test -p tsgo_compiler -- bench --ignored`
//!
//! Each benchmark uses `std::time::Instant` for manual timing, prints results,
//! and asserts a generous upper bound (sized for debug builds; release should be
//! 5–20× faster).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::tristate::Tristate;
use tsgo_parser::SourceFileParseOptions;
use tsgo_tsoptions::new_parsed_command_line;
use tsgo_tspath::ComparePathsOptions;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use crate::host::{new_compiler_host, CompilerHost};
use crate::program::{new_program, ProgramOptions};
use crate::{EmitOptions, EmitOnly, Program};

// ─── Helpers ────────────────────────────────────────────────────────────────

fn gen_typed_source(lines: usize) -> String {
    let mut src = String::with_capacity(lines * 80);
    for i in 0..lines {
        match i % 5 {
            0 => src.push_str(&format!(
                "export function fn{i}(a: number, b: string): boolean {{ return a > {i}; }}\n"
            )),
            1 => src.push_str(&format!(
                "export interface I{i} {{ x: number; y: string; z?: boolean; }}\n"
            )),
            2 => src.push_str(&format!(
                "export type T{i} = {{ value: number; label: string }};\n"
            )),
            3 => src.push_str(&format!(
                "export const c{i}: number = {i} * 2 + 1;\n"
            )),
            _ => src.push_str(&format!(
                "export class C{i} {{ prop: string = \"v{i}\"; method(): number {{ return {i}; }} }}\n"
            )),
        }
    }
    src
}

fn gen_simple_source(lines: usize) -> String {
    let mut src = String::with_capacity(lines * 60);
    for i in 0..lines {
        src.push_str(&format!(
            "export function fn{i}(a: number, b: string): boolean {{ return a > 0; }}\n"
        ));
    }
    src
}

fn build_bench_program(files: &[(&str, &str)], roots: &[&str], options: CompilerOptions) -> Program {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(files.iter().copied(), true));
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

// ─── Benchmark 1: Parse 500-line TypeScript file ────────────────────────────

#[test]
#[ignore]
fn bench_parse_500_lines() {
    let src = gen_simple_source(500);

    let start = Instant::now();
    let fs: Arc<dyn Fs + Send + Sync> =
        Arc::new(MapFs::from_map([("/index.ts", src.as_str())], true));
    let host = new_compiler_host("/", fs, "/lib");
    let _parsed = host
        .get_source_file(&SourceFileParseOptions {
            file_name: "/index.ts".to_string(),
        })
        .expect("parse should succeed");
    let elapsed = start.elapsed();

    eprintln!(
        "[bench_parse_500_lines] elapsed: {elapsed:?} ({} ms)",
        elapsed.as_millis()
    );
    assert!(
        elapsed.as_millis() < 500,
        "parse 500 lines took {elapsed:?}, expected < 500ms"
    );
}

// ─── Benchmark 2: Parse + bind 500-line file ────────────────────────────────

#[test]
#[ignore]
fn bench_bind_500_lines() {
    let src = gen_simple_source(500);

    let fs: Arc<dyn Fs + Send + Sync> =
        Arc::new(MapFs::from_map([("/index.ts", src.as_str())], true));
    let host = new_compiler_host("/", fs, "/lib");

    let parse_start = Instant::now();
    let mut parsed = host
        .get_source_file(&SourceFileParseOptions {
            file_name: "/index.ts".to_string(),
        })
        .expect("parse should succeed");
    let parse_elapsed = parse_start.elapsed();

    let bind_start = Instant::now();
    parsed.bind();
    let bind_elapsed = bind_start.elapsed();

    let total = parse_elapsed + bind_elapsed;
    eprintln!(
        "[bench_bind_500_lines] parse: {parse_elapsed:?}, bind: {bind_elapsed:?}, total: {total:?}"
    );
    assert!(
        total.as_millis() < 1000,
        "parse+bind 500 lines took {total:?}, expected < 1000ms"
    );
}

// ─── Benchmark 3: Parse + bind + check 100-line typed file ──────────────────

#[test]
#[ignore]
fn bench_check_100_lines() {
    let src = gen_typed_source(100);

    let start = Instant::now();
    let mut program = build_bench_program(
        &[("/src/index.ts", &src)],
        &["/src/index.ts"],
        CompilerOptions::default(),
    );
    let _diags = program.semantic_diagnostics();
    let elapsed = start.elapsed();

    eprintln!(
        "[bench_check_100_lines] elapsed: {elapsed:?} ({} ms)",
        elapsed.as_millis()
    );
    assert!(
        elapsed.as_millis() < 5000,
        "check 100 typed lines took {elapsed:?}, expected < 5000ms"
    );
}

// ─── Benchmark 4: Full pipeline (parse+bind+check+emit) 200-line file ──────

#[test]
#[ignore]
fn bench_emit_200_lines() {
    let src = gen_typed_source(200);

    let start = Instant::now();
    let program = build_bench_program(
        &[("/src/index.ts", &src)],
        &["/src/index.ts"],
        CompilerOptions::default(),
    );
    let captured: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&captured);
    let result = program.emit(EmitOptions {
        target_source_file: None,
        emit_only: EmitOnly::All,
        write_file: Some(Box::new(move |name, text, _data| {
            sink.borrow_mut().push((name.to_string(), text.to_string()));
            Ok(())
        })),
    });
    let elapsed = start.elapsed();

    let captured = captured.borrow();
    eprintln!(
        "[bench_emit_200_lines] elapsed: {elapsed:?} ({} ms), files emitted: {}, emit_skipped: {}",
        elapsed.as_millis(),
        captured.len(),
        result.emit_skipped
    );
    assert!(
        !result.emit_skipped,
        "emit should not be skipped"
    );
    assert!(
        !captured.is_empty(),
        "should emit at least one file"
    );
    assert!(
        elapsed.as_millis() < 10000,
        "full pipeline 200 lines took {elapsed:?}, expected < 10000ms"
    );
}

// ─── Benchmark 5: Large file parse (5000 lines) ────────────────────────────

#[test]
#[ignore]
fn bench_parse_5000_lines() {
    let src = gen_simple_source(5000);

    let start = Instant::now();
    let fs: Arc<dyn Fs + Send + Sync> =
        Arc::new(MapFs::from_map([("/index.ts", src.as_str())], true));
    let host = new_compiler_host("/", fs, "/lib");
    let _parsed = host
        .get_source_file(&SourceFileParseOptions {
            file_name: "/index.ts".to_string(),
        })
        .expect("parse should succeed");
    let elapsed = start.elapsed();

    eprintln!(
        "[bench_parse_5000_lines] elapsed: {elapsed:?} ({} ms), source size: {} bytes",
        elapsed.as_millis(),
        src.len()
    );
    assert!(
        elapsed.as_millis() < 3000,
        "parse 5000 lines took {elapsed:?}, expected < 3000ms"
    );
}

// ─── Benchmark 6: Many files (100 small files) ─────────────────────────────

#[test]
#[ignore]
fn bench_many_files_100() {
    let mut files: Vec<(String, String)> = Vec::with_capacity(101);
    let mut main_src = String::new();
    for i in 0..100 {
        let path = format!("/src/mod{i}.ts");
        let content = format!(
            "export function fn{i}(x: number): number {{ return x + {i}; }}\n\
             export const val{i}: number = {i};\n\
             export interface I{i} {{ id: number; name: string; }}\n"
        );
        files.push((path, content));
        main_src.push_str(&format!("import {{ fn{i}, val{i} }} from \"./mod{i}\";\n"));
    }
    main_src.push_str("const sum = val0 + val50 + val99;\n");
    main_src.push_str("const r = fn0(1) + fn50(2) + fn99(3);\n");
    files.push(("/src/index.ts".to_string(), main_src));

    let file_refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let roots: Vec<&str> = vec!["/src/index.ts"];

    let start = Instant::now();
    let mut program = build_bench_program(&file_refs, &roots, CompilerOptions::default());
    let diags = program.semantic_diagnostics();
    let elapsed = start.elapsed();

    eprintln!(
        "[bench_many_files_100] elapsed: {elapsed:?} ({} ms), diagnostics: {}",
        elapsed.as_millis(),
        diags.len()
    );
    assert!(
        elapsed.as_millis() < 30000,
        "100 files compile took {elapsed:?}, expected < 30000ms"
    );
}

// ─── Benchmark 7: Declaration emit (.d.ts) for 200-line file ────────────────

#[test]
#[ignore]
fn bench_declaration_emit_200_lines() {
    let src = gen_typed_source(200);

    let options = CompilerOptions {
        declaration: Tristate::True,
        ..Default::default()
    };

    let start = Instant::now();
    let program = build_bench_program(
        &[("/src/index.ts", &src)],
        &["/src/index.ts"],
        options,
    );
    let captured: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&captured);
    let result = program.emit(EmitOptions {
        target_source_file: None,
        emit_only: EmitOnly::All,
        write_file: Some(Box::new(move |name, text, _data| {
            sink.borrow_mut().push((name.to_string(), text.to_string()));
            Ok(())
        })),
    });
    let elapsed = start.elapsed();

    let captured = captured.borrow();
    let dts_count = captured.iter().filter(|(n, _)| n.ends_with(".d.ts")).count();
    let js_count = captured.iter().filter(|(n, _)| n.ends_with(".js")).count();
    eprintln!(
        "[bench_declaration_emit_200_lines] elapsed: {elapsed:?} ({} ms), \
         .d.ts files: {dts_count}, .js files: {js_count}, emit_skipped: {}",
        elapsed.as_millis(),
        result.emit_skipped
    );
    assert!(
        !result.emit_skipped,
        "emit should not be skipped"
    );
    assert!(
        dts_count >= 1,
        "should emit at least one .d.ts file"
    );
    assert!(
        elapsed.as_millis() < 10000,
        "declaration emit 200 lines took {elapsed:?}, expected < 10000ms"
    );
}
