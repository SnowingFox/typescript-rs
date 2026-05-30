use std::sync::Arc;

use tsgo_core::compileroptions::CompilerOptions;
use tsgo_tsoptions::new_parsed_command_line;
use tsgo_tspath::ComparePathsOptions;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use super::*;
use crate::host::new_compiler_host;

fn program_for(files: &[(&str, &str)], cwd: &str, roots: &[&str]) -> Program {
    program_with(files, cwd, roots, CompilerOptions::default(), false)
}

fn program_with(
    files: &[(&str, &str)],
    cwd: &str,
    roots: &[&str],
    options: CompilerOptions,
    single_threaded: bool,
) -> Program {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(files.iter().copied(), true));
    let host = Arc::new(new_compiler_host(cwd, fs, "/lib"));
    let config = new_parsed_command_line(
        options,
        roots.iter().map(|s| s.to_string()).collect(),
        ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: cwd.to_string(),
        },
    );
    new_program(ProgramOptions {
        host,
        config: Arc::new(config),
        single_threaded,
    })
}

/// Tracer bullet: a program built from one in-memory file exposes that file and
/// round-trips its compiler options.
// Go: internal/compiler/program.go:NewProgram + Program.GetSourceFiles/Options
#[test]
fn builds_program_from_single_file() {
    let options = CompilerOptions {
        no_emit: tsgo_core::tristate::Tristate::True,
        ..Default::default()
    };
    let program = program_with(
        &[("/src/index.ts", "export const x = 1;")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    assert_eq!(program.source_files().len(), 1);
    assert_eq!(program.source_files()[0].file_name(), "/src/index.ts");
    assert!(program.options().no_emit.is_true());
}

/// `get_source_file` resolves a loaded file by name and returns `None` for one
/// not in the program.
// Go: internal/compiler/program.go:Program.GetSourceFile / GetSourceFileByPath
#[test]
fn looks_up_source_file_by_name() {
    let program = program_for(&[("/src/index.ts", "")], "/src", &["/src/index.ts"]);
    assert!(program.get_source_file("/src/index.ts").is_some());
    assert!(program.get_source_file("/src/other.ts").is_none());

    let path = program.to_path("/src/index.ts");
    assert_eq!(
        program
            .get_source_file_by_path(&path)
            .map(|f| f.file_name()),
        Some("/src/index.ts")
    );
}

/// A program with a resolvable import loads both files (referenced file first)
/// and sizes the checker pool to the file count.
// Go: internal/compiler/program.go:NewProgram + initCheckerPool
#[test]
fn builds_multi_file_program_and_sizes_pool() {
    let options = CompilerOptions {
        module_resolution: tsgo_core::compileroptions::ModuleResolutionKind::Bundler,
        ..Default::default()
    };
    let program = program_with(
        &[
            ("/src/index.ts", "import * as a from \"./a\";"),
            ("/src/a.ts", "export const a = 1;"),
        ],
        "/src",
        &["/src/index.ts"],
        options,
        false,
    );
    let names: Vec<&str> = program
        .source_files()
        .iter()
        .map(|f| f.file_name())
        .collect();
    assert_eq!(names, vec!["/src/a.ts", "/src/index.ts"]);
    // Not single-threaded, default 4 checkers, clamped to the 2 files.
    assert_eq!(program.checker_pool().checker_count(), 2);
}

/// `bind_source_files` binds every loaded file so its symbol table is queryable.
// Go: internal/compiler/program.go:BindSourceFiles
#[test]
fn bind_source_files_binds_every_file() {
    let mut program = program_for(
        &[("/src/index.ts", "var x; function f() {}")],
        "/src",
        &["/src/index.ts"],
    );
    program.bind_source_files();
    let file = &program.source_files()[0];
    let bind = file.bind_result().expect("file should be bound");
    assert!(bind.local(file.node(), "x").is_some());
    assert!(bind.local(file.node(), "f").is_some());
}

/// `create_checkers` builds the checker pool and associates files to checkers
/// round-robin (`i % K`), creating one real checker per slot.
// Go: internal/compiler/checkerpool.go:createCheckers
#[test]
fn create_checkers_associates_files_round_robin() {
    let options = CompilerOptions {
        checkers: Some(2),
        ..Default::default()
    };
    let mut program = program_with(
        &[("/a.ts", ""), ("/b.ts", ""), ("/c.ts", "")],
        "/",
        &["/a.ts", "/b.ts", "/c.ts"],
        options,
        false,
    );
    program.create_checkers();
    let pool = program.checker_pool();
    // 3 files, --checkers 2 => 2 checkers; files round-robin across them.
    assert_eq!(pool.created_checker_count(), 2);
    assert_eq!(pool.checker_index_for_file(0), Some(0));
    assert_eq!(pool.checker_index_for_file(1), Some(1));
    assert_eq!(pool.checker_index_for_file(2), Some(0));
    assert_eq!(pool.checker_index_for_file(3), None);
    // Grouped-iteration shape: each checker's file indices.
    assert_eq!(pool.files_for_checker(0, 3), vec![0, 2]);
    assert_eq!(pool.files_for_checker(1, 3), vec![1]);
}

/// `new_program` runs `verify_compiler_options`, exposing option-consistency
/// diagnostics on the program.
// Go: internal/compiler/program.go:NewProgram (verifyCompilerOptions)
#[test]
fn program_reports_option_diagnostics() {
    let options = CompilerOptions {
        out_file: "/dist/out.js".to_string(),
        ..Default::default()
    };
    let program = program_with(&[("/a.ts", "")], "/", &["/a.ts"], options, true);
    let diags = program.options_diagnostics();
    assert!(diags.iter().any(|d| std::ptr::eq(
        d.message,
        &tsgo_diagnostics::OPTION_0_HAS_BEEN_REMOVED_PLEASE_REMOVE_IT_FROM_YOUR_CONFIGURATION
    )));

    // A clean program reports none.
    let clean = program_for(&[("/a.ts", "")], "/", &["/a.ts"]);
    assert!(clean.options_diagnostics().is_empty());
}

/// End to end: a program over one file with an undefined identifier drives the
/// checker pool and surfaces the checker's "Cannot find name" (2304).
// Go: internal/compiler/program.go:GetSemanticDiagnostics (via checkerpool)
#[test]
fn program_collects_semantic_diagnostics() {
    let mut program = program_with(
        &[("/src/index.ts", "y;")],
        "/src",
        &["/src/index.ts"],
        CompilerOptions::default(),
        true,
    );
    let diags = program.semantic_diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

/// A single-threaded program uses one checker and reports its host/command line.
// Go: internal/compiler/program.go:Program.SingleThreaded / Host / CommandLine
#[test]
fn single_threaded_program_uses_one_checker() {
    let program = program_with(
        &[("/src/index.ts", "")],
        "/src",
        &["/src/index.ts"],
        CompilerOptions::default(),
        true,
    );
    assert!(program.single_threaded());
    assert_eq!(program.checker_pool().checker_count(), 1);
    assert_eq!(program.host().get_current_directory(), "/src");
    assert_eq!(
        program.command_line().file_names(),
        &["/src/index.ts".to_string()]
    );
}
