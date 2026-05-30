use std::sync::Arc;

use tsgo_parser::SourceFileParseOptions;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use super::*;
use crate::host::{new_compiler_host, CompilerHost};

/// Parses and binds `text` as `file_name`, yielding a bound [`ParsedFile`] the
/// pool can seed its shared program from.
fn bound_file(file_name: &str, text: &str) -> ParsedFile {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map([(file_name, text)], true));
    let host = new_compiler_host("/", fs, "/lib");
    let mut parsed = host
        .get_source_file(&SourceFileParseOptions {
            file_name: file_name.to_string(),
        })
        .expect("file should parse");
    parsed.bind();
    parsed
}

/// The default pool uses 4 checkers when not single-threaded and the file count
/// allows it.
// Go: internal/compiler/checkerpool.go:newCheckerPoolWithTracing (default 4)
#[test]
fn defaults_to_four_checkers() {
    assert_eq!(checker_count(false, None, 10), 4);
}

/// The checker count is clamped to the number of files.
// Go: internal/compiler/checkerpool.go (min(checkerCount, len(files)))
#[test]
fn clamps_to_file_count() {
    assert_eq!(checker_count(false, Some(8), 2), 2);
    assert_eq!(checker_count(false, None, 3), 3);
}

/// Single-threaded programs always use a single checker.
// Go: internal/compiler/checkerpool.go (singleThreaded => 1)
#[test]
fn single_threaded_uses_one() {
    assert_eq!(checker_count(true, Some(8), 10), 1);
}

/// The count is at least one even with no files, and never exceeds 256.
// Go: internal/compiler/checkerpool.go (max(min(..., 256), 1))
#[test]
fn clamps_to_floor_and_ceiling() {
    assert_eq!(checker_count(false, None, 0), 1);
    assert_eq!(checker_count(false, Some(1000), 1000), 256);
}

/// The configured `--checkers` value overrides the default.
// Go: internal/compiler/checkerpool.go (options.Checkers)
#[test]
fn honors_configured_count() {
    assert_eq!(checker_count(false, Some(2), 10), 2);
}

/// The pool reports the count it was sized with.
// Go: internal/compiler/checkerpool.go:checkerPool (len(checkers))
#[test]
fn pool_reports_checker_count() {
    let pool = CompilerCheckerPool::new(false, Some(3), 10);
    assert_eq!(pool.checker_count(), 3);
}

/// The pool drives its checker over the shared program and surfaces the
/// "Cannot find name" (2304) diagnostic for an undefined identifier.
// Go: internal/compiler/checkerpool.go:createCheckers + program.go:getDiagnostics (Cannot_find_name_0)
#[test]
fn collects_undefined_identifier_diagnostic() {
    let files = vec![bound_file("/a.ts", "y;")];
    let mut pool = CompilerCheckerPool::new(true, None, files.len());
    pool.create_checkers(&files);

    let diags = pool.collect_diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2304);
    assert_eq!(diags[0].message, "Cannot find name 'y'.");
}

/// The pool drives the checker's full reachable semantics, not a special case:
/// a property access on an interface that lacks the member surfaces the 2339
/// "property does not exist" diagnostic.
// Go: internal/compiler/program.go:getSemanticDiagnostics (Property_0_does_not_exist_on_type_1)
#[test]
fn collects_property_does_not_exist_diagnostic() {
    let files = vec![bound_file(
        "/a.ts",
        "interface Foo {\n  bar: string;\n}\ndeclare const foo: Foo;\nfoo.baz;",
    )];
    let mut pool = CompilerCheckerPool::new(true, None, files.len());
    pool.create_checkers(&files);

    let diags = pool.collect_diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2339);
    assert_eq!(
        diags[0].message,
        "Property 'baz' does not exist on type 'Foo'."
    );
}
