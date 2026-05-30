use std::sync::Arc;

use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use super::*;

fn host_with(files: &[(&str, &str)], cwd: &str) -> CompilerHostImpl {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(files.iter().copied(), true));
    new_compiler_host(cwd, fs, "/lib")
}

/// Tracer bullet: a host over an in-memory file system returns the current
/// directory and the contents of a file it holds.
// Go: internal/compiler/host.go:compilerHost.GetCurrentDirectory/FS
#[test]
fn host_returns_cwd_and_file_contents() {
    let host = host_with(&[("/src/index.ts", "export const x = 1;")], "/src");
    assert_eq!(host.get_current_directory(), "/src");
    assert_eq!(
        host.fs().read_file("/src/index.ts").as_deref(),
        Some("export const x = 1;")
    );
}

/// `get_source_file` reads and parses a file the host holds, exposing its
/// normalized file name and original text.
// Go: internal/compiler/host.go:compilerHost.GetSourceFile
#[test]
fn host_parses_source_file() {
    let host = host_with(&[("/src/index.ts", "export const x = 1;")], "/src");
    let opts = SourceFileParseOptions {
        file_name: "/src/index.ts".to_string(),
    };
    let parsed = host.get_source_file(&opts).expect("file should parse");
    assert_eq!(parsed.file_name(), "/src/index.ts");
    assert_eq!(parsed.text(), "export const x = 1;");
}

/// `get_source_file` returns `None` for a file the host does not hold.
// Go: internal/compiler/host.go:compilerHost.GetSourceFile (ReadFile miss)
#[test]
fn host_missing_source_file_is_none() {
    let host = host_with(&[("/src/index.ts", "")], "/src");
    let opts = SourceFileParseOptions {
        file_name: "/src/missing.ts".to_string(),
    };
    assert!(host.get_source_file(&opts).is_none());
}

/// Binding a parsed file produces its file-scope symbol table.
// Go: internal/compiler/program.go:BindSourceFiles (per file)
#[test]
fn binding_a_file_yields_its_symbol_table() {
    let host = host_with(&[("/src/index.ts", "var x; function f() {}")], "/src");
    let opts = SourceFileParseOptions {
        file_name: "/src/index.ts".to_string(),
    };
    let mut parsed = host.get_source_file(&opts).expect("file should parse");
    assert!(!parsed.is_bound());
    let node = parsed.node();
    let result = parsed.bind();
    assert!(result.local(node, "x").is_some());
    assert!(result.local(node, "f").is_some());
    assert!(parsed.is_bound());
}

/// A parsed file exposes its arena/root node and (for valid input) reports no
/// syntactic diagnostics.
// Go: internal/ast/ast.go:SourceFile (arena/root/diagnostics)
#[test]
fn parsed_file_exposes_arena_root_and_diagnostics() {
    let host = host_with(&[("/src/index.ts", "export const x = 1;")], "/src");
    let opts = SourceFileParseOptions {
        file_name: "/src/index.ts".to_string(),
    };
    let parsed = host.get_source_file(&opts).expect("file should parse");
    assert_eq!(
        parsed.arena().kind(parsed.node()),
        tsgo_ast::Kind::SourceFile
    );
    assert!(parsed.diagnostics().is_empty());
}
