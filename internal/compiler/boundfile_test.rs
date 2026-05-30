use std::rc::Rc;
use std::sync::Arc;

use tsgo_checker::BoundProgram;
use tsgo_parser::SourceFileParseOptions;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use super::*;
use crate::host::{new_compiler_host, CompilerHost};

fn bound_file_for(text: &str) -> ParsedFile {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map([("/a.ts", text)], true));
    let host = new_compiler_host("/", fs, "/lib");
    let mut parsed = host
        .get_source_file(&SourceFileParseOptions {
            file_name: "/a.ts".to_string(),
        })
        .expect("file should parse");
    parsed.bind();
    parsed
}

/// A `BoundFile` exposes the bound file's arena, root, and file-scope symbols to
/// the checker via the `BoundProgram` surface.
// Go: internal/compiler/program.go:Program (bound-file query surface)
#[test]
fn bound_file_exposes_arena_root_and_symbols() {
    let parsed = bound_file_for("var x;");
    let view = BoundFile::for_file(&parsed).expect("file is bound");

    assert!(view.arena().node_count() > 0);
    let root = view.root();
    assert_eq!(root, parsed.node());

    let table = view.locals(root).expect("source file has a locals table");
    let x = *table.get("x").expect("x is a file local");
    assert_eq!(view.symbol(x).name, "x");
}

/// A `BoundFile` is owned (`'static`), so it can be placed in
/// `Rc<dyn BoundProgram>` and shared by `Rc::clone` — the exact shape K checkers
/// use to share one program (round 4l's `NewChecker(program)` retain model).
// Go: internal/checker/checker.go:NewChecker (one *Program shared by the pool)
#[test]
fn bound_file_is_shareable_as_rc_program() {
    let parsed = bound_file_for("var x;");
    let view = BoundFile::for_file(&parsed).expect("file is bound");

    let program: Rc<dyn BoundProgram> = Rc::new(view);
    let shared = Rc::clone(&program);
    assert_eq!(Rc::strong_count(&program), 2);
    // Both handles see the same retained program.
    assert_eq!(program.root(), parsed.node());
    assert_eq!(shared.root(), parsed.node());
}

/// An unbound file yields no `BoundFile`.
// Go: internal/compiler/program.go:Program (bind precedes checking)
#[test]
fn unbound_file_has_no_bound_view() {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map([("/a.ts", "var x;")], true));
    let host = new_compiler_host("/", fs, "/lib");
    let parsed = host
        .get_source_file(&SourceFileParseOptions {
            file_name: "/a.ts".to_string(),
        })
        .expect("file should parse");
    assert!(BoundFile::for_file(&parsed).is_none());
}
