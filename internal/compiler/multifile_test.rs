use tsgo_ast::NodeData;
use tsgo_parser::SourceFileParseOptions;

use super::*;
use crate::host::{parse_file, ParsedFile};

/// Parses and binds `text` as `file_name`, yielding a bound [`ParsedFile`] the
/// multi-file program can join.
fn bound(file_name: &str, text: &str) -> ParsedFile {
    let opts = SourceFileParseOptions {
        file_name: file_name.to_string(),
    };
    let mut parsed = parse_file(opts, text.to_string());
    parsed.bind();
    parsed
}

/// The program exposes one collision-free source-file handle per file, and a
/// per-file view resolves that file's own `SourceFile` root through its own
/// arena (the tracer for the multi-file view).
// Go: internal/compiler/program.go:Program.SourceFiles + per-file checking context
#[test]
fn exposes_one_collision_free_view_per_file() {
    let files = vec![
        bound("/a.ts", "interface Foo {}"),
        bound("/b.ts", "var b = 1;"),
    ];
    let program = MultiFileBoundProgram::new(&files);

    let handles = program.source_files();
    assert_eq!(handles.len(), 2);
    assert_ne!(handles[0], handles[1], "file handles must not collide");

    for &handle in &handles {
        let view = program.file_view(handle).expect("a view per handle");
        assert!(
            matches!(view.arena().data(view.root()), NodeData::SourceFile(_)),
            "the view's root must be its own SourceFile node"
        );
        assert_eq!(view.file_handle(), handle, "the view's handle is its key");
    }
}

/// `globals()` is the cross-file union of every file's top-level declarations
/// (Go's `Checker.globals`), so a name declared in one file is visible to the
/// program-wide global table.
// Go: internal/checker/checker.go:Checker.globals (merge of every global file's Locals)
#[test]
fn globals_merge_top_level_declarations_across_files() {
    let files = vec![
        bound("/lib.d.ts", "interface String { length: number; }"),
        bound("/index.ts", "var s = 1;"),
    ];
    let program = MultiFileBoundProgram::new(&files);

    let globals = program.globals().expect("merged globals");
    assert!(globals.contains_key("String"), "String from /lib.d.ts");
    assert!(globals.contains_key("s"), "s from /index.ts");
    assert!(!globals.contains_key("nope"), "undeclared names are absent");
}

/// P6-options S1: a multi-file program built with explicit compiler options
/// surfaces the program's REAL options through
/// [`BoundProgram::compiler_options`] (overriding the trait's all-defaults
/// default added in checker round 4al), so the checker's option-gated
/// diagnostics read the program's actual `--target`/`--downlevelIteration`. The
/// per-file views carry the same options.
// Go: internal/compiler/program.go:Program.Options
#[test]
fn compiler_options_reflects_program_options() {
    use std::rc::Rc;
    use tsgo_core::compileroptions::{CompilerOptions, ScriptTarget};
    let files = vec![bound("/a.ts", "var a = 1;")];
    let options = Rc::new(CompilerOptions {
        target: ScriptTarget::Es2015,
        ..CompilerOptions::default()
    });

    let program = MultiFileBoundProgram::new_with_options(&files, Rc::clone(&options));
    assert_eq!(program.compiler_options().target, ScriptTarget::Es2015);

    // The per-file view returned for checking carries the same real options.
    let handle = program.source_files()[0];
    let view = program.file_view(handle).expect("a view per handle");
    assert_eq!(view.compiler_options().target, ScriptTarget::Es2015);

    // The plain `new` overload keeps all-defaults options (additive: existing
    // callers are unchanged).
    let default_program = MultiFileBoundProgram::new(&files);
    assert_eq!(
        default_program.compiler_options().target,
        ScriptTarget::None
    );
}

/// Cross-file DECLARATION MERGING: a global `interface` declared in more than
/// one file (the lib pattern — `ObjectConstructor` is augmented across
/// `lib.es5.d.ts`/`lib.es2015.core.d.ts`/`lib.es2017.object.d.ts`) is merged
/// into a single global symbol whose member table is the UNION of every
/// declaration's members. This is the member-table half of Go's `mergeSymbol`.
// Go: internal/checker/checker.go:Checker.mergeGlobalSymbol / mergeSymbol
#[test]
fn cross_file_interface_members_merge_into_one_global_symbol() {
    let files = vec![
        bound("/a.d.ts", "interface Foo { a: number; }"),
        bound("/b.d.ts", "interface Foo { b: string; }"),
    ];
    let program = MultiFileBoundProgram::new(&files);
    let globals = program.globals().expect("merged globals");

    let foo = *globals.get("Foo").expect("merged Foo global");
    let members = &program.symbol(foo).members;
    assert!(
        members.contains_key("a"),
        "member from the first declaration is kept: {:?}",
        members.keys().collect::<Vec<_>>()
    );
    assert!(
        members.contains_key("b"),
        "member from the second declaration is merged in: {:?}",
        members.keys().collect::<Vec<_>>()
    );
}

/// Guard for the merge gate (`getExcludedSymbolFlags`): a NON-mergeable
/// same-named collision (two block-scoped `let`s) is NOT merged — the first
/// file's symbol wins (the duplicate-identifier diagnostic is DEFER'd), so the
/// merge does not blanket-combine every collision.
// Go: internal/checker/checker.go:getExcludedSymbolFlags (BlockScopedVariableExcludes)
#[test]
fn cross_file_non_mergeable_collision_keeps_first_symbol() {
    let files = vec![
        bound("/a.ts", "let dup = 1;"),
        bound("/b.ts", "let dup = 2;"),
    ];
    let program = MultiFileBoundProgram::new(&files);
    let handles = program.source_files();
    let globals = program.globals().expect("merged globals");

    let dup = *globals.get("dup").expect("dup global");
    let dup_view = program.view_for_symbol(dup).expect("declaring view");
    assert_eq!(
        dup_view.file_handle(),
        handles[0],
        "a non-mergeable collision keeps the FIRST file's symbol (no merge)"
    );
}

/// `view_for_symbol(merged_id)` returns the view of the file that *declares* the
/// symbol, so the checker can build a global type against the arena that owns
/// its declaration nodes (Go: a symbol's declaring file).
// Go: internal/compiler/program.go:Program (a symbol's declaring file)
#[test]
fn view_for_symbol_returns_declaring_file_view() {
    let files = vec![
        bound("/lib.d.ts", "interface String { length: number; }"),
        bound("/index.ts", "var s = 1;"),
    ];
    let program = MultiFileBoundProgram::new(&files);
    let handles = program.source_files();
    let globals = program.globals().expect("merged globals");

    let string_id = *globals.get("String").expect("String global");
    let s_id = *globals.get("s").expect("s global");

    let string_view = program.view_for_symbol(string_id).expect("declaring view");
    assert_eq!(
        string_view.file_handle(),
        handles[0],
        "String is in /lib.d.ts"
    );

    let s_view = program.view_for_symbol(s_id).expect("declaring view");
    assert_eq!(s_view.file_handle(), handles[1], "s is in /index.ts");
}
