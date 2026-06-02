use std::sync::Arc;

use tsgo_core::compileroptions::CompilerOptions;
use tsgo_tsoptions::new_parsed_command_line;
use tsgo_tspath::ComparePathsOptions;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use super::*;
use crate::host::new_compiler_host;
use crate::BoundFile;

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

/// Builds a program whose host file system serves the embedded `bundled:///`
/// libs (so the automatic default-lib include can read the real `lib.*.d.ts`),
/// rooting the default library at [`tsgo_bundled::lib_path`].
fn program_with_bundled_libs(
    files: &[(&str, &str)],
    cwd: &str,
    roots: &[&str],
    options: CompilerOptions,
    single_threaded: bool,
) -> Program {
    let inner = MapFs::from_map(files.iter().copied(), true);
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(tsgo_bundled::wrap_fs(inner));
    let host = Arc::new(new_compiler_host(cwd, fs, tsgo_bundled::lib_path()));
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

/// Builds a program from a real tsconfig file (parsed so it carries its
/// `references[]` + `configFilePath`), the way a config-driven program is built.
fn program_from_config(files: &[(&str, &str)], cwd: &str, config_file_name: &str) -> Program {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(files.iter().copied(), true));
    let host = Arc::new(new_compiler_host(cwd, fs, "/lib"));
    let config = crate::get_resolved_project_reference(host.as_ref(), config_file_name)
        .expect("root config parses");
    new_program(ProgramOptions {
        host,
        config: Arc::new(config),
        single_threaded: true,
    })
}

/// A config-driven program resolves its `references[]`, surfaces the composite
/// requirement (`TS6306`) for a non-composite reference, and exposes the
/// topological build order — the machinery a `--build` (P9) drives.
// Go: internal/compiler/program.go:NewProgram (verifyProjectReferences + GetResolvedProjectReferences)
#[test]
fn program_surfaces_project_reference_diagnostics_and_build_order() {
    let program = program_from_config(
        &[
            (
                "/app/tsconfig.json",
                r#"{ "compilerOptions": { "composite": true }, "files": ["a.ts"], "references": [{ "path": "../lib" }] }"#,
            ),
            ("/app/a.ts", "export const a = 1;"),
            (
                "/lib/tsconfig.json",
                r#"{ "compilerOptions": {}, "files": ["index.ts"] }"#,
            ),
            ("/lib/index.ts", "export const x = 1;"),
        ],
        "/app",
        "/app/tsconfig.json",
    );

    let refs = program.get_resolved_project_references();
    assert_eq!(refs.len(), 1, "root has one resolved reference");
    assert!(refs[0].is_some(), "../lib resolved");

    let diags = program.project_reference_diagnostics();
    assert_eq!(diags.len(), 1, "non-composite reference reported");
    assert_eq!(diags[0].code(), 6306);
    assert_eq!(
        diags[0].text(),
        "Referenced project '/lib' must have setting \"composite\": true."
    );

    let order = program.build_order();
    assert_eq!(
        order.order,
        vec![
            "/lib/tsconfig.json".to_string(),
            "/app/tsconfig.json".to_string(),
        ]
    );
    assert!(order.circular_diagnostics.is_empty());
}

/// A program with no config file (constructed from a raw command line) has no
/// references: the accessors are empty and construction is unchanged.
// Go: internal/compiler/program.go:Program (no ConfigFile -> no references)
#[test]
fn program_without_config_file_has_no_references() {
    let program = program_for(&[("/src/index.ts", "export const x = 1;")], "/src", &[]);
    assert!(program.get_resolved_project_references().is_empty());
    assert!(program.project_reference_diagnostics().is_empty());
    assert!(program.build_order().order.is_empty());
}

/// With root files and `noLib` off (and no explicit `--lib`), the program
/// automatically includes the default library file resolved from the emit
/// target (`lib.d.ts` for ES5), reads it from the bundled embed, and binds it
/// along with the rest of the program.
// Go: internal/compiler/fileloader.go:processAllProgramFiles (default-lib include)
#[test]
fn loads_and_binds_default_lib_file() {
    let options = CompilerOptions {
        target: tsgo_core::compileroptions::ScriptTarget::Es5,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.ts", "export const x = 1;")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    // ES5 is not in the target->full-lib map, so Go falls back to `lib.d.ts`.
    let lib_name = "bundled:///libs/lib.d.ts";
    assert!(
        program
            .source_files()
            .iter()
            .any(|f| f.file_name() == lib_name),
        "default lib should be part of the program"
    );
    // The lib participates in binding like any other source file.
    program.bind_source_files();
    let lib = program
        .source_files()
        .iter()
        .find(|f| f.file_name() == lib_name)
        .expect("lib file loaded");
    assert!(lib.is_bound());
}

/// An explicit `--lib` list includes the named lib file(s) (resolved by short
/// name, e.g. `es5` -> `lib.es5.d.ts`) instead of the target's default lib.
/// Unlike the reference-only default aggregators, `lib.es5.d.ts` carries the
/// real `Array`/`String`/`Object` declarations.
// Go: internal/compiler/fileloader.go:processAllProgramFiles (--lib branch)
#[test]
fn loads_explicit_lib_files() {
    let options = CompilerOptions {
        lib: vec!["es5".to_string()],
        ..Default::default()
    };
    let program = program_with_bundled_libs(
        &[("/src/index.ts", "export const x = 1;")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    assert!(
        program
            .source_files()
            .iter()
            .any(|f| f.file_name() == "bundled:///libs/lib.es5.d.ts"),
        "explicit --lib es5 should load lib.es5.d.ts"
    );
}

/// P6-8: with the DEFAULT lib (no explicit `--lib`), the program follows the
/// aggregator `lib.d.ts`'s `/// <reference lib>` directives and pulls in the
/// real declaration libs it references (`lib.es5.d.ts`, `lib.dom.d.ts`, ...),
/// not just the reference-only aggregator. The parser does not expose lib
/// reference directives yet, so the loader scans the lib's leading trivia for
/// them (see `FileLoader::resolve_lib_references`).
// Go: internal/compiler/filesparser.go:load (file.LibReferenceDirectives -> pathForLibFile)
#[test]
fn default_lib_expands_reference_graph() {
    let options = CompilerOptions {
        target: tsgo_core::compileroptions::ScriptTarget::Es5,
        ..Default::default()
    };
    let program = program_with_bundled_libs(
        &[("/src/index.ts", "export const x = 1;")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    let names: Vec<&str> = program
        .source_files()
        .iter()
        .map(|f| f.file_name())
        .collect();
    // The aggregator itself is present (ES5 -> `lib.d.ts`)...
    assert!(
        names.contains(&"bundled:///libs/lib.d.ts"),
        "aggregator present: {names:?}"
    );
    // ...and so are the libs it references via `/// <reference lib>`.
    assert!(
        names.contains(&"bundled:///libs/lib.es5.d.ts"),
        "default lib must pull in lib.es5.d.ts: {names:?}"
    );
    assert!(
        names.contains(&"bundled:///libs/lib.dom.d.ts"),
        "default lib must pull in lib.dom.d.ts: {names:?}"
    );
}

/// End to end: a program with the default lib loaded resolves the real
/// `Array`/`String`/`Object` globals through the checker's round-4z global
/// machinery. The lib file's top-level declarations are the program's globals
/// (see `BoundFile::globals`), so a checker built over the lib's bound view
/// finds them via `get_global_symbol`/`get_global_type`.
///
/// DEFER(P6): checking a *separate* source file (e.g. `s.length`) against these
/// lib globals (so `.length` resolves with no 2339) needs a multi-file program
/// view that merges the lib globals with the checked file's own scope.
/// blocked-by: multi-file `compiler.Program` `BoundProgram` view + cross-file
/// global merge (P6-8).
// Go: internal/checker/checker.go:Checker.getGlobalSymbol/getGlobalType (over the program's lib globals)
#[test]
fn resolves_real_lib_globals_end_to_end() {
    use std::rc::Rc;
    use tsgo_ast::SymbolFlags;
    use tsgo_checker::{BoundProgram, Checker};

    let options = CompilerOptions {
        lib: vec!["es5".to_string()],
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.ts", "var s: string = \"a\";\ns.length;")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    // Bind every file (including the lib) and build the pool's checkers.
    program.create_checkers();

    // The loaded lib's bound view carries the program's global symbol table.
    let lib = program.default_lib_file().expect("default lib loaded");
    let view: Rc<dyn BoundProgram> = Rc::new(BoundFile::for_file(lib).expect("lib is bound"));
    let mut checker = Checker::new_checker(view);

    // Real lib globals resolve through the checker's 4z global lookup.
    assert!(
        checker
            .get_global_symbol("Array", SymbolFlags::TYPE)
            .is_some(),
        "global type `Array`"
    );
    assert!(
        checker
            .get_global_symbol("String", SymbolFlags::TYPE)
            .is_some(),
        "global type `String`"
    );
    assert!(
        checker
            .get_global_symbol("Object", SymbolFlags::VALUE)
            .is_some(),
        "global value `Object`"
    );
    // A name absent from the lib globals stays unresolved.
    assert!(checker
        .get_global_symbol("NotAGlobal", SymbolFlags::VALUE)
        .is_none());
    // And a real lib interface type builds via `get_global_type`.
    assert!(
        checker.get_global_type("Object").is_some(),
        "global interface type `Object` builds"
    );
}

/// End to end (P6-6): a program over a real source file plus a second file that
/// declares the `String` global resolves `s.length` *across files* — there is
/// NO 2339, because the multi-file `BoundProgram` merges the two files' globals
/// and the checker resolves `length` against the other file's `String`
/// interface. The negative control (no `String` file) reports 2339, so the
/// resolution genuinely comes from the cross-file global.
///
/// This realizes the P6-5 deferral (`s.length` cross-file). The full real
/// default-lib graph (the `/// <reference lib>` aggregator -> real declaration
/// libs) is still DEFER(P6-8); a synthetic-but-real `String`-declaring file
/// proves the cross-file merge through the same multi-file program the real lib
/// flows through.
// Go: internal/checker/checker.go:Checker.getApparentType (cross-file lib `String`)
#[test]
fn resolves_string_length_across_files_end_to_end() {
    let options = CompilerOptions {
        no_lib: tsgo_core::tristate::Tristate::True,
        ..Default::default()
    };
    // Positive: the `String` global lives in a *separate* file from `s.length`.
    let mut program = program_with(
        &[
            ("/lib.ts", "interface String {\n  length: number;\n}"),
            ("/index.ts", "var s: string = \"a\";\ns.length;"),
        ],
        "/",
        &["/lib.ts", "/index.ts"],
        options.clone(),
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags.is_empty(),
        "cross-file `String.length` must resolve (no 2339): {diags:?}"
    );

    // Negative control: without the `String`-declaring file, `length` does not
    // resolve and the access reports 2339.
    let mut program = program_with(
        &[("/index.ts", "var s: string = \"a\";\ns.length;")],
        "/",
        &["/index.ts"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert_eq!(diags.len(), 1, "no lib -> 2339: {diags:?}");
    assert_eq!(diags[0].code, 2339);
}

/// End to end with the REAL bundled lib: a program over `--lib es5` plus a
/// source file resolves `s.length` against the real `interface String` declared
/// in `lib.es5.d.ts` — across files, through the multi-file `BoundProgram`'s
/// merged globals — so there is NO 2339. This upgrades the P6-5
/// `resolves_real_lib_globals_end_to_end` deferral: checking a *separate* source
/// file against the real lib globals now works.
// Go: internal/checker/checker.go:Checker.getApparentType (cross-file lib `String`)
#[test]
fn resolves_string_length_via_real_lib_es5() {
    let options = CompilerOptions {
        lib: vec!["es5".to_string()],
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.ts", "var s: string = \"a\";\ns.length;")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags.iter().all(|d| d.code != 2339),
        "`s.length` must resolve against the real lib `String` (no 2339): {diags:?}"
    );
}

/// Panic-robustness (P10 corpus triage, category d): checking a property access
/// whose property is a method DECLARED IN A LIB FILE (`array.push(...)`, where
/// `array: any[]`) must not panic. The `push` symbol's declarations live in
/// `lib.es5.d.ts`'s arena, not the file-under-check's arena; the value-type
/// builder (`getTypeOfFuncClassEnumModule` -> `getSignaturesOfSymbol`) read them
/// through `program.arena()` (the wrong arena) and indexed out of bounds
/// (`index out of bounds: the len is 44 but the index is 3028`). The fix
/// switches to the symbol's owning file view first, mirroring the owning-view
/// switch already used by `getDeclaredTypeOfSymbol` /
/// `getConstraintOfTypeParameter`. Reproduces the corpus case
/// `classExpressionWithComputedPropertyInLoop.ts`.
// Go: internal/checker/checker.go:Checker.getTypeOfFuncClassEnumModule (resolved against the symbol's declaring file)
#[test]
fn property_access_on_lib_declared_method_does_not_panic() {
    let options = CompilerOptions {
        target: tsgo_core::compileroptions::ScriptTarget::Es2015,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[(
            "/src/index.ts",
            "const array: any[] = [];\nconst key = \"myKey\";\nfor (let i = 0; i < 3; i++) {\n    array.push(class C {\n        [key] = i;\n        #field = i;\n    });\n}\n",
        )],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    // Previously panicked with a cross-arena out-of-bounds index; now it returns.
    // The exact diagnostics are not asserted (this is a panic-robustness slice);
    // only that resolving `array.push` does not abort.
    let _ = program.semantic_diagnostics();
}

/// Round 11 (esnext/DOM lib bind panic fix): binding the FULL `@target: esnext`
/// default lib — which expands to the DOM lib plus the whole `es20xx` chain,
/// including `lib.es2025.iterator.d.ts`'s `declare global { … }` augmentation —
/// must complete WITHOUT panicking, and the real globals must resolve.
///
/// The `declare global` block is an ambient module. The binder used to DEFER its
/// symbol creation, then panic at `declareModuleMember`'s
/// `symbol_of(container).unwrap()` (`internal/binder/symbols.rs`) when binding
/// the block's `interface`/`var` members — `bind_source_files` swallowed the
/// panic and left `lib.es2025.iterator.d.ts` UNBOUND. With the root fix (the
/// ambient module now owns a symbol before its members bind, matching Go's
/// `bindModuleDeclaration`), every esnext+dom lib binds and contributes globals.
// Go: internal/binder/binder.go:bindModuleDeclaration (ambient-module symbol creation)
#[test]
fn binds_full_esnext_dom_lib_without_panic() {
    let options = CompilerOptions {
        target: tsgo_core::compileroptions::ScriptTarget::EsNext,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[(
            "/src/index.ts",
            "let o: Object;\nlet el: HTMLElement;\nlet p: Promise<number>;\n",
        )],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    // `@target: esnext` expands the default-lib reference graph to the full set,
    // including the `declare global` augmentation lib and the DOM lib.
    let names: Vec<String> = program
        .source_files()
        .iter()
        .map(|f| f.file_name().to_string())
        .collect();
    assert!(
        names
            .iter()
            .any(|n| n == "bundled:///libs/lib.es2025.iterator.d.ts"),
        "esnext pulls in the `declare global` augmentation lib: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "bundled:///libs/lib.dom.d.ts"),
        "esnext pulls in the DOM lib: {names:?}"
    );

    // Bind every file. `bind_source_files` swallows per-file binder panics
    // (leaving the file unbound); the regression left
    // `lib.es2025.iterator.d.ts` UNBOUND. With the fix, every lib binds.
    program.bind_source_files();
    let unbound: Vec<&str> = program
        .source_files()
        .iter()
        .filter(|f| !f.is_bound())
        .map(|f| f.file_name())
        .collect();
    assert!(
        unbound.is_empty(),
        "every esnext+dom lib must bind without panicking; unbound: {unbound:?}"
    );

    // The merged globals from the full lib resolve: no `TS2304 Cannot find name`
    // for the `es5`/`dom`/`es2015` globals referenced above (each lives in a lib
    // that now binds and contributes to the multi-file program's globals).
    let diags = program.semantic_diagnostics();
    let unresolved: Vec<&str> = diags
        .iter()
        .filter(|d| d.code == 2304)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        unresolved.is_empty(),
        "real esnext+dom globals (Object/HTMLElement/Promise) must resolve: {unresolved:?}"
    );
}

/// End to end (P6-8): a program with the DEFAULT lib (no explicit `--lib`)
/// resolves a real global declared in a *referenced* lib. The aggregator
/// `lib.d.ts` pulls in `lib.es5.d.ts` via `/// <reference lib>`, so the merged
/// globals carry the real `interface String` and `s.length` resolves with no
/// 2339 — the same cross-file resolution P6-6 proved with `--lib es5`, now via
/// the DEFAULT lib.
///
/// The partial binder `panic!`s on some real lib constructs (e.g.
/// `lib.dom.d.ts`'s `[Symbol.x]` computed property names); such libs are skipped
/// (left unbound, excluded from the checker view). `String` lives in the
/// bindable `lib.es5.d.ts`, so it still resolves.
// Go: internal/checker/checker.go:Checker.getApparentType (cross-file lib `String` via the default lib)
#[test]
fn resolves_real_global_via_default_lib_end_to_end() {
    let options = CompilerOptions {
        target: tsgo_core::compileroptions::ScriptTarget::Es5,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.ts", "var s: string = \"a\";\ns.length;")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    // `s.length` must resolve against the real `String` from the referenced
    // `lib.es5.d.ts` (pulled in by the default `lib.d.ts` aggregator).
    assert!(
        !diags
            .iter()
            .any(|d| d.code == 2339 && d.message.contains("'length'")),
        "`s.length` must resolve against the default lib's real `String` (no 2339): {diags:?}"
    );
}

/// P6-8: the loaded lib set is deterministically ordered — lib files come first,
/// sorted by `getDefaultLibFilePriority` (by each lib's short-name position in
/// `tsgo_tsoptions::LIBS`), ahead of the source files — independent of discovery
/// or list order. Here `--lib` lists `scripthost` before `es5`, but `es5`
/// (priority 1) sorts before `scripthost`, and the source file comes last.
// Go: internal/compiler/fileloader.go:sortLibs/getDefaultLibFilePriority + filesparser.go:getProcessedFiles (libs first)
#[test]
fn lib_set_is_sorted_by_priority_and_precedes_sources() {
    let options = CompilerOptions {
        lib: vec!["scripthost".to_string(), "es5".to_string()],
        ..Default::default()
    };
    let program = program_with_bundled_libs(
        &[("/src/index.ts", "export const x = 1;")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    let names: Vec<&str> = program
        .source_files()
        .iter()
        .map(|f| f.file_name())
        .collect();
    let pos = |needle: &str| {
        names
            .iter()
            .position(|n| *n == needle)
            .unwrap_or_else(|| panic!("{needle} must be present in {names:?}"))
    };
    // es5 (Libs index 0 -> priority 1) sorts before scripthost.
    assert!(
        pos("bundled:///libs/lib.es5.d.ts") < pos("bundled:///libs/lib.scripthost.d.ts"),
        "libs sorted by priority: {names:?}"
    );
    // The source file follows all libs (Go appends `files` after sorted `libs`).
    assert_eq!(
        *names.last().unwrap(),
        "/src/index.ts",
        "source file follows libs: {names:?}"
    );
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

/// The for-of `2802` repro: a `[Symbol.iterator]`-bearing object (neither an
/// array nor a string) iterated by for-of. The file self-declares `Symbol`,
/// `Iterator`, `It`, and `it`, so it needs no lib and binds end-to-end through
/// the compiler. Under a `--target` below `es2015` with no `--downlevelIteration`
/// the iteration reports `2802`; once an option permits downlevelling it does
/// not (the option-gated behavior this round wires end-to-end).
const FOR_OF_SYMBOL_ITERATOR_SRC: &str = "interface SymbolConstructor { readonly iterator: unique symbol; }\ndeclare var Symbol: SymbolConstructor;\ninterface Iterator<T> { next(): { value: T }; }\ninterface It { [Symbol.iterator](): Iterator<string>; }\ndeclare const it: It;\nfor (const x of it) {\n}";

/// End to end (P6-options): a program built with `--target es2015` over the
/// for-of `[Symbol.iterator]` repro does NOT report `2802`, because the checker
/// now reads the program's REAL `target` through
/// [`BoundProgram::compiler_options`] (round 4al's gating). Before this round
/// the pool built the checker's program with all-defaults options (`target`
/// unset, i.e. below `es2015`), so the gating fired regardless of the program's
/// `--target` — this test is the RED that drove threading the real options
/// through the pool.
// Go: internal/checker/checker.go:Checker.getIterationDiagnosticDetails (target gating)
#[test]
fn program_for_of_iterable_with_es2015_target_no_2802() {
    use tsgo_core::compileroptions::ScriptTarget;
    let options = CompilerOptions {
        no_lib: tsgo_core::tristate::Tristate::True,
        target: ScriptTarget::Es2015,
        ..Default::default()
    };
    let mut program = program_with(
        &[("/a.ts", FOR_OF_SYMBOL_ITERATOR_SRC)],
        "/",
        &["/a.ts"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags.iter().all(|d| d.code != 2802),
        "es2015 target must not gate the iteration (no 2802): {diags:?}"
    );
}

/// End to end (P6-options) — positive control for the gating direction: the
/// SAME repro under `--target es5` (below `es2015`, no `--downlevelIteration`)
/// DOES report `2802`. This proves the for-of is genuinely checked end-to-end
/// (so `program_for_of_iterable_with_es2015_target_no_2802`'s clean result is
/// the real `es2015` allowance, not a file that silently failed to bind/check),
/// and that the gating reads the program's REAL `--target`.
// Go: internal/checker/checker.go:Checker.getIterationDiagnosticDetails (target gating)
#[test]
fn program_for_of_iterable_with_es5_target_reports_2802() {
    use tsgo_core::compileroptions::ScriptTarget;
    let options = CompilerOptions {
        no_lib: tsgo_core::tristate::Tristate::True,
        target: ScriptTarget::Es5,
        ..Default::default()
    };
    let mut program = program_with(
        &[("/a.ts", FOR_OF_SYMBOL_ITERATOR_SRC)],
        "/",
        &["/a.ts"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert_eq!(diags.len(), 1, "es5 target gates the iteration: {diags:?}");
    assert_eq!(diags[0].code, 2802);
    assert_eq!(
        diags[0].message,
        "Type 'It' can only be iterated through when using the '--downlevelIteration' flag or with a '--target' of 'es2015' or higher."
    );
}

/// End to end (P6-options): the SAME repro under `--downlevelIteration` (with an
/// `es5` target) does NOT report `2802` — the other half of the option-gated
/// behavior difference, proving the checker reads the program's REAL
/// `--downlevelIteration` through [`BoundProgram::compiler_options`].
// Go: internal/checker/checker.go:Checker.getIterationDiagnosticDetails (downlevelIteration gating)
#[test]
fn program_for_of_iterable_with_downlevel_iteration_no_2802() {
    use tsgo_core::compileroptions::ScriptTarget;
    let options = CompilerOptions {
        no_lib: tsgo_core::tristate::Tristate::True,
        target: ScriptTarget::Es5,
        downlevel_iteration: tsgo_core::tristate::Tristate::True,
        ..Default::default()
    };
    let mut program = program_with(
        &[("/a.ts", FOR_OF_SYMBOL_ITERATOR_SRC)],
        "/",
        &["/a.ts"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags.iter().all(|d| d.code != 2802),
        "--downlevelIteration must permit the iteration (no 2802): {diags:?}"
    );
}

/// End to end with the REAL bundled lib: a bare reference to a lib global
/// VALUE (`Error`, `Object`, `Date` — each `declare var` in `lib.es5.d.ts`)
/// resolves through `checkIdentifier` against the program's merged globals, so
/// there is NO spurious `TS2304` ("Cannot find name"). Before the fix
/// `checkIdentifier` passed `None` for the globals scope, so every global-value
/// reference cascaded into a 2304 (and a follow-on 2339 on its `error`-typed
/// members) — the dominant P10 corpus false-positive. tsc reports no error for
/// these references (the `cmd/tsgo` ground truth).
// Go: internal/checker/checker.go:Checker.checkIdentifier -> resolveName (consults c.globals)
#[test]
fn bare_lib_global_value_reference_resolves_no_2304() {
    let options = CompilerOptions {
        target: tsgo_core::compileroptions::ScriptTarget::Es5,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.ts", "Error;\nObject;\nDate;\n")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags.iter().all(|d| d.code != 2304),
        "lib global values must resolve against the real lib (no 2304): {diags:?}"
    );
}

/// Guard for the merged-globals identifier fix: a bare reference to a name that
/// is NOT a lib global (and not declared anywhere) still reports `TS2304`,
/// proving the fix resolves real globals rather than blanket-muting the
/// "Cannot find name" diagnostic.
// Go: internal/checker/checker.go:Checker.checkIdentifier (Cannot_find_name_0)
#[test]
fn bare_undefined_name_still_reports_2304_with_real_lib() {
    let options = CompilerOptions {
        target: tsgo_core::compileroptions::ScriptTarget::Es5,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.ts", "thisGlobalDoesNotExist;\n")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags
            .iter()
            .any(|d| d.code == 2304 && d.message.contains("thisGlobalDoesNotExist")),
        "a genuinely undefined name must still report 2304: {diags:?}"
    );
}

/// End to end with the REAL bundled lib: an UNRESOLVED bare name accessed via
/// property reports ONLY the single `TS2304` ("Cannot find name") — there is NO
/// follow-on `TS2339` on its `error`-typed receiver. The receiver's `error`
/// type carries the `Any` flag, so `checkPropertyAccessExpressionOrQualifiedName`
/// short-circuits it. This is the cascade amplifier behind the dominant P10
/// corpus `extra TS2339` (a property access on an `error`/`any` receiver).
// Go: internal/checker/checker.go:Checker.checkPropertyAccessExpressionOrQualifiedName (isAnyLike on errorType)
#[test]
fn unresolved_name_property_access_reports_only_2304_no_cascade() {
    let options = CompilerOptions {
        target: tsgo_core::compileroptions::ScriptTarget::Es5,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.ts", "notDefinedAtAll.member;\n")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert_eq!(diags.len(), 1, "only the 2304, no cascade: {diags:?}");
    assert_eq!(diags[0].code, 2304);
    assert!(
        diags.iter().all(|d| d.code != 2339),
        "no 2339 on the error-typed receiver: {diags:?}"
    );
}

/// End to end with the REAL bundled lib: a chain of property accesses on a
/// value typed `any` (`a.b.c.d`) produces NO diagnostics — each step yields
/// `any`, never a 2339.
// Go: internal/checker/checker.go:Checker.checkPropertyAccessExpressionOrQualifiedName (isAnyLike)
#[test]
fn property_access_chain_on_any_reports_no_2339() {
    let options = CompilerOptions {
        target: tsgo_core::compileroptions::ScriptTarget::Es5,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.ts", "declare const a: any;\na.b.c.d;\n")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags.iter().all(|d| d.code != 2339),
        "property access chain on `any` must not report 2339: {diags:?}"
    );
}

/// End to end with the REAL bundled lib: `Object.entries(...)` resolves the
/// `entries` member on `ObjectConstructor`, which is declared across MULTIPLE
/// lib files (`lib.es5.d.ts` declares `ObjectConstructor` with `keys`/...; the
/// `entries`/`values` members live in `lib.es2017.object.d.ts`). With
/// `@target: esnext` the whole lib chain is loaded, so tsc merges every
/// `interface ObjectConstructor` declaration and resolves `entries` — producing
/// NO `TS2339`. Before the cross-file declaration-merge fix the program's
/// merged globals kept only the FIRST file's `ObjectConstructor` symbol
/// (`lib.es5.d.ts`), so the es2017 `entries`/`values` members were lost and we
/// reported a spurious `TS2339` (the P10 `objectSubtypeReduction` false
/// positive).
// Go: internal/checker/checker.go:Checker.mergeGlobalSymbol / mergeSymbol (cross-file declaration merging)
#[test]
fn object_entries_resolves_via_cross_file_lib_interface_merge() {
    // `lib: ["es2017"]` pulls the es5 -> es2015.core -> es2017.object chain (so
    // `ObjectConstructor` is declared in three separate lib files) WITHOUT the
    // DOM/full aggregate, a minimal/deterministic repro of the cross-file
    // interface merge.
    let options = CompilerOptions {
        lib: vec!["es2017".to_string()],
        strict: tsgo_core::tristate::Tristate::True,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.ts", "Object.entries({});\n")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags.iter().all(|d| d.code != 2339),
        "Object.entries (es2017 member of the multiply-declared ObjectConstructor) \
         must resolve via cross-file interface merge (no 2339): {diags:?}"
    );
}

/// GUARD (NOT over-resolution): a property that is genuinely absent from the
/// merged `ObjectConstructor` STILL reports `TS2339`. This proves the cross-file
/// merge resolves real members rather than blanket-muting the diagnostic on the
/// receiver.
// Go: internal/checker/checker.go:Checker.reportNonexistentProperty (TS2339)
#[test]
fn absent_object_constructor_property_still_reports_2339_after_merge() {
    let options = CompilerOptions {
        lib: vec!["es2017".to_string()],
        strict: tsgo_core::tristate::Tristate::True,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.ts", "Object.thisIsNotARealMethod;\n")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags
            .iter()
            .any(|d| d.code == 2339 && d.message.contains("thisIsNotARealMethod")),
        "a genuinely-absent member must still report 2339: {diags:?}"
    );
}

/// GUARD (no regression of base members): a BASE (`lib.es5.d.ts`)
/// `ObjectConstructor` member (`keys`) still resolves after the cross-file merge
/// — the merge ADDS later-lib members without dropping the first declaration's.
// Go: internal/checker/checker.go:Checker.mergeSymbol (target members are kept)
#[test]
fn object_keys_es5_base_member_still_resolves_after_merge() {
    let options = CompilerOptions {
        lib: vec!["es2017".to_string()],
        strict: tsgo_core::tristate::Tristate::True,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.ts", "Object.keys({});\n")],
        "/src",
        &["/src/index.ts"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags.iter().all(|d| d.code != 2339),
        "the es5 base member `Object.keys` must still resolve after the merge: {diags:?}"
    );
}

/// End to end with the REAL bundled lib (the path the P10 corpus parity runner
/// exercises): a CommonJS `require(...)` call in a checked JS file resolves the
/// bare `require` callee to the synthetic `require` symbol (type `any`), so
/// `const a = require("./x")` produces NO `TS2304: Cannot find name 'require'`.
/// This clears the `require` sub-cluster of the P10 `extra TS2304` false
/// positives. (`module`/`exports` are a *separate* CommonJS-module-binding root
/// that is still deferred — see the worklog; this slice covers only `require`.)
// Go: internal/binder/nameresolver.go:Resolve (RequireSymbol branch)
#[test]
fn require_call_in_js_file_resolves_no_2304_with_real_lib() {
    let options = CompilerOptions {
        lib: vec!["es5".to_string()],
        check_js: tsgo_core::tristate::Tristate::True,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.js", "const a = require(\"./x\");\n")],
        "/src",
        &["/src/index.js"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags.iter().all(|d| d.code != 2304),
        "require(...) callee resolves through the real-lib program (no 2304): {diags:?}"
    );
}

/// End to end with the REAL bundled lib (the path the P10 corpus parity runner
/// exercises, mirroring `jsxMultilineAttributeStringValues2`): an intrinsic
/// `.tsx` element with `@jsx: preserve` and NO `JSX.IntrinsicElements` in scope
/// is implicitly `any` and reports TS7026 under the default `noImplicitAny`. A
/// SELF-CLOSING element reports it exactly ONCE — and nothing else (no spurious
/// cascade), matching tsc's committed baseline shape for the corpus case.
// Go: internal/checker/jsx.go:Checker.getIntrinsicTagSymbol (c.noImplicitAny -> TS7026)
#[test]
fn jsx_intrinsic_self_closing_reports_one_ts7026_with_real_lib() {
    let options = CompilerOptions {
        jsx: tsgo_core::compileroptions::JsxEmit::Preserve,
        target: tsgo_core::compileroptions::ScriptTarget::Es2015,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.tsx", "const a = <div className=\"foo\" />;\n")],
        "/src",
        &["/src/index.tsx"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert_eq!(
        diags.len(),
        1,
        "self-closing intrinsic with no JSX.IntrinsicElements -> exactly one TS7026, no cascade: {diags:?}"
    );
    assert_eq!(diags[0].code, 7026);
    assert_eq!(
        diags[0].message,
        "JSX element implicitly has type 'any' because no interface 'JSX.IntrinsicElements' exists."
    );
}

/// End to end with the REAL bundled lib: a PAIRED intrinsic `<div></div>` with
/// no `JSX.IntrinsicElements` reports TS7026 TWICE (opening + closing element),
/// the exact count tsc's byte-compared baseline expects for paired intrinsic
/// elements (`checkJsxElementDeferred` resolves both tags).
// Go: internal/checker/jsx.go:Checker.checkJsxElementDeferred (open + close TS7026)
#[test]
fn jsx_intrinsic_paired_reports_two_ts7026_with_real_lib() {
    let options = CompilerOptions {
        jsx: tsgo_core::compileroptions::JsxEmit::Preserve,
        target: tsgo_core::compileroptions::ScriptTarget::Es2015,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.tsx", "const a = <div></div>;\n")],
        "/src",
        &["/src/index.tsx"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    let ts7026 = diags.iter().filter(|d| d.code == 7026).count();
    assert_eq!(
        ts7026, 2,
        "paired intrinsic element reports TS7026 on the opening AND closing element: {diags:?}"
    );
    assert!(
        diags.iter().all(|d| d.code == 7026),
        "no spurious diagnostics beyond the two TS7026: {diags:?}"
    );
}

/// End to end with the REAL bundled lib (the path the P10 corpus parity runner
/// exercises): a CommonJS `module.exports = X` assignment in a checked JS file
/// makes the binder declare `module`/`exports` as file locals, so they resolve
/// through the normal scope walk and report NO `TS2591`/`TS2304` — and the
/// `module.exports` member access does not spuriously report `TS2339` (the
/// CJS-declared symbol's type is benign / `any`-like). This clears the
/// `module`/`exports` sub-cluster of the P10 false positives.
// Go: internal/binder/binder.go:declareCommonJSVariable
#[test]
fn js_module_exports_assignment_resolves_no_2591_with_real_lib() {
    let options = CompilerOptions {
        lib: vec!["es5".to_string()],
        check_js: tsgo_core::tristate::Tristate::True,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.js", "module.exports = { a: 1 };\n")],
        "/src",
        &["/src/index.js"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags
            .iter()
            .all(|d| d.code != 2591 && d.code != 2304 && d.code != 2339),
        "module.exports CommonJS file resolves `module` through the real-lib program \
         (no 2591/2304/2339): {diags:?}"
    );
}

/// End to end with the REAL bundled lib: an `exports.x = Y` CommonJS assignment
/// resolves `exports` (no 2304) and the `exports.x` member access does not
/// report 2339.
// Go: internal/binder/binder.go:declareCommonJSVariable / bindExportsOrObjectDefineProperty
#[test]
fn js_exports_property_resolves_no_2304_with_real_lib() {
    let options = CompilerOptions {
        lib: vec!["es5".to_string()],
        check_js: tsgo_core::tristate::Tristate::True,
        ..Default::default()
    };
    let mut program = program_with_bundled_libs(
        &[("/src/index.js", "exports.foo = 1;\n")],
        "/src",
        &["/src/index.js"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags
            .iter()
            .all(|d| d.code != 2591 && d.code != 2304 && d.code != 2339),
        "exports.foo CommonJS file resolves `exports` (no 2591/2304/2339): {diags:?}"
    );
}

/// GUARD (over-resolution): the CommonJS `module`/`exports` file locals one
/// JS file declares must NOT leak into the program globals — a sibling `.ts`
/// file in the same program keeps `module` unresolved (TS2591), exactly as tsc.
/// This proves the multi-file globals merge excludes `SymbolFlagsModuleExports`
/// symbols, so resolving `module`/`exports` is strictly per-file.
// Go: internal/checker/checker.go:Checker.globals (excludes IsExternalOrCommonJSModule files)
#[test]
fn commonjs_module_locals_do_not_leak_into_sibling_ts_globals() {
    let options = CompilerOptions {
        no_lib: tsgo_core::tristate::Tristate::True,
        check_js: tsgo_core::tristate::Tristate::True,
        ..Default::default()
    };
    let mut program = program_with(
        &[("/a.js", "module.exports = {};"), ("/b.ts", "module;")],
        "/",
        &["/a.js", "/b.ts"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    // `/a.js` resolves `module` via its own CommonJS locals (no error there);
    // `/b.ts` must still report the unresolved `module` (TS2591), proving no
    // cross-file leak.
    assert!(
        diags.iter().any(|d| d.code == 2591),
        "sibling TS file must keep `module` unresolved (no globals leak): {diags:?}"
    );
}

/// `SkipTypeChecking`/`canIncludeBindAndCheckDiagnostics`: a JS file compiled
/// with `checkJs: false` is NOT bind-and-checked, so it produces ZERO semantic
/// diagnostics (the `module` reference is not even resolved). This mirrors Go's
/// gate: with `checkJs == false` a `.js`/`.jsx` file is neither plain JS
/// (`checkJs` would have to be unset) nor checked JS (`checkJs` would have to be
/// true), so `canIncludeBindAndCheckDiagnostics` returns false.
// Go: internal/compiler/program.go:Program.canIncludeBindAndCheckDiagnostics
#[test]
fn js_file_with_check_js_false_is_not_checked() {
    let options = CompilerOptions {
        no_lib: tsgo_core::tristate::Tristate::True,
        check_js: tsgo_core::tristate::Tristate::False,
        ..Default::default()
    };
    let mut program = program_with(
        &[("/index.js", "module.exports = {};")],
        "/",
        &["/index.js"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags.is_empty(),
        "checkJs:false JS file must be skipped (no semantic diagnostics): {diags:?}"
    );
}

/// Guard (Go-faithful, NOT over-suppression): a *plain* JS file — `checkJs`
/// unset and no `// @ts-check`/`@ts-nocheck` — IS bind-and-checked in Go
/// (`IsPlainJSFile` -> the `isPlainJS` branch of
/// `canIncludeBindAndCheckDiagnostics` is true), so a genuinely undefined name
/// still reports 2304. The gate skips a JS file ONLY when `checkJs` is
/// explicitly false (or a `@ts-nocheck` directive is present); it must NOT
/// blanket-mute plain JS.
///
/// The witness is a plain undefined identifier (NOT `module`): as of Round 8
/// the binder declares `module`/`exports` as CommonJS file locals once it sees
/// an indicator, so `module.exports = {}` now resolves and emits nothing — it
/// would no longer prove the file is checked. A bare undefined name does.
// Go: internal/ast/utilities.go:IsPlainJSFile + Program.canIncludeBindAndCheckDiagnostics
#[test]
fn plain_js_file_is_still_checked() {
    let options = CompilerOptions {
        no_lib: tsgo_core::tristate::Tristate::True,
        ..Default::default()
    };
    let mut program = program_with(
        &[("/index.js", "nonExistentName;")],
        "/",
        &["/index.js"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "plain JS (checkJs unset) is checked in Go, so an undefined name reports 2304: {diags:?}"
    );
}

/// Guard: a JS file compiled with `checkJs: true` IS bind-and-checked (the
/// `isCheckJS` branch), so the gate is conditioned on the `checkJs` state and
/// does not blanket-skip JS. The witness is a bare undefined name (not
/// `module`, which now resolves via CommonJS binding — see Round 8).
// Go: internal/ast/utilities.go:IsCheckJSEnabledForFile + Program.canIncludeBindAndCheckDiagnostics
#[test]
fn js_file_with_check_js_true_is_checked() {
    let options = CompilerOptions {
        no_lib: tsgo_core::tristate::Tristate::True,
        check_js: tsgo_core::tristate::Tristate::True,
        ..Default::default()
    };
    let mut program = program_with(
        &[("/index.js", "nonExistentName;")],
        "/",
        &["/index.js"],
        options,
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "checkJs:true JS is checked (undefined name -> TS2304): {diags:?}"
    );
}

/// Guard: a TS file is ALWAYS bind-and-checked, regardless of `checkJs` — the
/// JS gate only applies to `.js`/`.jsx` script kinds. Even with `checkJs:
/// false`, an unresolved name in a `.ts` file reports 2304.
// Go: internal/compiler/program.go:Program.canIncludeBindAndCheckDiagnostics (ScriptKindTS -> true)
#[test]
fn ts_file_is_checked_regardless_of_check_js() {
    let options = CompilerOptions {
        no_lib: tsgo_core::tristate::Tristate::True,
        check_js: tsgo_core::tristate::Tristate::False,
        ..Default::default()
    };
    let mut program = program_with(&[("/index.ts", "y;")], "/", &["/index.ts"], options, true);
    let diags = program.semantic_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == 2304),
        "TS file is checked regardless of checkJs: {diags:?}"
    );
}

/// Round 13 (surface binder diagnostics): a duplicate class declaration is a
/// binder merge conflict that emits `DUPLICATE_IDENTIFIER_0` (TS2300) on BOTH
/// the existing and the new name. Before this round
/// [`Program::semantic_diagnostics`] collected only checker diagnostics and
/// dropped the binder's `bindDiagnostics`, so these never surfaced — this is
/// the RED that drove merging bind diagnostics into the program's bind-and-check
/// pass (Go's `getBindAndCheckDiagnosticsWithChecker` = `BindDiagnostics()` ++
/// `checker.GetDiagnostics()`). The spans are trivia-skipped to match Go's
/// `GetErrorRangeForNode` (the default case for a name node), so they byte-match
/// tsc: `class C {} class C {}` flags the two `C` names at columns 7 and 18.
// Go: internal/compiler/program.go:getBindAndCheckDiagnosticsWithChecker (BindDiagnostics ++ check)
#[test]
fn binder_duplicate_identifier_surfaces_ts2300() {
    let mut program = program_with(
        &[("/index.ts", "class C {} class C {}")],
        "/",
        &["/index.ts"],
        CompilerOptions {
            no_lib: tsgo_core::tristate::Tristate::True,
            ..Default::default()
        },
        true,
    );
    let diags = program.semantic_diagnostics();
    let mut dup: Vec<(i32, i32)> = diags
        .iter()
        .filter(|d| d.code == 2300)
        .map(|d| (d.start, d.length))
        .collect();
    dup.sort();
    // Both `C` names: the first at byte 6 (`skipTrivia` past the space after
    // `class`), the second at byte 17, each one character wide.
    assert_eq!(
        dup,
        vec![(6, 1), (17, 1)],
        "duplicate-class binder TS2300 must surface on both names with trivia-skipped spans: {diags:?}"
    );
    assert!(
        diags
            .iter()
            .filter(|d| d.code == 2300)
            .all(|d| d.message == "Duplicate identifier 'C'."),
        "TS2300 message is localized with the name argument: {diags:?}"
    );

    // The per-file collector attributes both to the file that owns them.
    let by_file = program.semantic_diagnostics_by_file();
    let owner = by_file
        .iter()
        .find(|(name, _)| name == "/index.ts")
        .expect("the duplicate-identifier diagnostics are attributed to /index.ts");
    assert_eq!(
        owner.1.iter().filter(|d| d.code == 2300).count(),
        2,
        "both TS2300 are attributed to /index.ts: {by_file:?}"
    );
}

/// Round 13: a duplicate block-scoped `const` is the OTHER binder merge-conflict
/// arm — the existing symbol carries `BLOCK_SCOPED_VARIABLE`, so the binder
/// reports `CANNOT_REDECLARE_BLOCK_SCOPED_VARIABLE_0` (TS2451) rather than
/// TS2300. It must surface through the program now too.
// Go: internal/compiler/program.go:getBindAndCheckDiagnosticsWithChecker (BindDiagnostics)
#[test]
fn binder_block_scoped_redeclare_surfaces_ts2451() {
    let mut program = program_with(
        &[("/index.ts", "const x = 1; const x = 2;")],
        "/",
        &["/index.ts"],
        CompilerOptions {
            no_lib: tsgo_core::tristate::Tristate::True,
            ..Default::default()
        },
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags
            .iter()
            .any(|d| d.code == 2451 && d.message == "Cannot redeclare block-scoped variable 'x'."),
        "block-scoped redeclare must surface the binder's TS2451: {diags:?}"
    );
    // It is NOT a TS2300 (that arm is for non-block-scoped duplicates).
    assert!(
        diags.iter().all(|d| d.code != 2300),
        "a block-scoped redeclare is TS2451, never TS2300: {diags:?}"
    );
}

/// Round 13 GUARD (no over-report): legal declaration MERGES must produce NO
/// duplicate-identifier diagnostic. `tsc` merges interface+interface,
/// namespace+namespace, function+namespace, and enum+namespace, so the binder's
/// excludes/merge rules must not flag any of them — surfacing bind diagnostics
/// must not turn a legal merge into a spurious TS2300/TS2451.
// Go: internal/binder/binder.go:declareSymbolEx (no conflict when excludes don't intersect)
#[test]
fn legal_merges_produce_no_duplicate_identifier() {
    for src in [
        "interface I {} interface I {}",
        "namespace N {} namespace N {}",
        "function f() {} namespace f {}",
        "enum E {} namespace E {}",
    ] {
        let mut program = program_with(
            &[("/index.ts", src)],
            "/",
            &["/index.ts"],
            CompilerOptions {
                no_lib: tsgo_core::tristate::Tristate::True,
                ..Default::default()
            },
            true,
        );
        let diags = program.semantic_diagnostics();
        assert!(
            diags.iter().all(|d| d.code != 2300 && d.code != 2451),
            "legal merge `{src}` must not report a duplicate-identifier diagnostic: {diags:?}"
        );
    }
}

/// Round 13 GATE (default-library exclusion preserved): a program built over the
/// REAL bundled libs must never attribute ANY semantic diagnostic — checker OR
/// binder — to a `lib.*.d.ts` file. Bind diagnostics flow through the SAME
/// default-lib exclusion mask as checker diagnostics, so a (legal) lib-internal
/// declaration merge never leaks a diagnostic positioned in a lib file (which
/// `tsc` does not report and which would slice out of bounds when rendered
/// against a user file).
// Go: internal/compiler/program.go:SkipTypeChecking (IsSourceFileDefaultLibrary)
#[test]
fn binder_diagnostics_in_default_lib_are_not_reported() {
    let mut program = program_with_bundled_libs(
        &[("/src/index.ts", "export const ok = 1;\n")],
        "/src",
        &["/src/index.ts"],
        CompilerOptions {
            lib: vec!["es5".to_string()],
            ..Default::default()
        },
        true,
    );
    let by_file = program.semantic_diagnostics_by_file();
    assert!(
        by_file
            .iter()
            .all(|(name, _)| !name.starts_with("bundled:///libs/")),
        "no semantic diagnostic (bind or check) may be attributed to a default-lib file: {by_file:?}"
    );
}

/// Round 13 GATE (JS skip): a `.js` file compiled with `checkJs: false` is
/// skipped by `canIncludeBindAndCheckDiagnostics`, so its binder
/// duplicate-identifier diagnostics are NOT surfaced — the bind-and-check skip
/// gate applies to bind diagnostics exactly as it does to checker diagnostics.
// Go: internal/compiler/program.go:getBindAndCheckDiagnosticsWithChecker (SkipTypeChecking -> nil)
#[test]
fn check_js_false_suppresses_binder_duplicate() {
    let mut program = program_with(
        &[("/index.js", "class C {} class C {}")],
        "/",
        &["/index.js"],
        CompilerOptions {
            no_lib: tsgo_core::tristate::Tristate::True,
            check_js: tsgo_core::tristate::Tristate::False,
            ..Default::default()
        },
        true,
    );
    let diags = program.semantic_diagnostics();
    assert!(
        diags.is_empty(),
        "checkJs:false JS file is skipped, so its binder TS2300 is not surfaced: {diags:?}"
    );
}

/// Round 13 GATE (plain-JS filter, Go-faithful): a PLAIN JS file (`checkJs`
/// unset) IS bind-and-checked, but Go keeps only the `plainJSErrors` codes of
/// the combined bind+check diagnostics. `Duplicate_identifier` (TS2300) is NOT
/// in that set, while `Cannot_redeclare_block_scoped_variable` (TS2451) IS — so
/// a duplicate class in plain JS surfaces NO TS2300, but a block-scoped
/// redeclare still surfaces TS2451. This prevents a plain-JS bind over-report.
// Go: internal/compiler/program.go:getBindAndCheckDiagnosticsWithChecker (isPlainJS -> plainJSErrors)
#[test]
fn plain_js_filters_ts2300_but_keeps_ts2451() {
    let mut dup_class = program_with(
        &[("/index.js", "class C {} class C {}")],
        "/",
        &["/index.js"],
        CompilerOptions {
            no_lib: tsgo_core::tristate::Tristate::True,
            ..Default::default()
        },
        true,
    );
    let diags = dup_class.semantic_diagnostics();
    assert!(
        diags.iter().all(|d| d.code != 2300),
        "plain JS filters the binder TS2300 (not in plainJSErrors): {diags:?}"
    );

    let mut dup_const = program_with(
        &[("/index.js", "let x = 1; let x = 2;")],
        "/",
        &["/index.js"],
        CompilerOptions {
            no_lib: tsgo_core::tristate::Tristate::True,
            ..Default::default()
        },
        true,
    );
    let diags = dup_const.semantic_diagnostics();
    assert!(
        diags.iter().any(|d| d.code == 2451),
        "plain JS keeps the binder TS2451 (in plainJSErrors): {diags:?}"
    );
}

/// Round 13 (related info round-trips): two `export default` statements are a
/// binder merge conflict that emits `A_module_cannot_have_multiple_default_exports`
/// (TS2528) with a related `The_first_export_default_is_here` (TS2752) pointing
/// at the first export. The conversion bridge must carry the binder
/// diagnostic's `related` list into the checker diagnostic's
/// `related_information` recursively.
// Go: internal/binder/binder.go:declareSymbolEx (multiple-default-exports related info)
#[test]
fn binder_multiple_default_exports_carries_related_info() {
    let mut program = program_with(
        &[("/index.ts", "export default 1;\nexport default 2;")],
        "/",
        &["/index.ts"],
        CompilerOptions {
            no_lib: tsgo_core::tristate::Tristate::True,
            ..Default::default()
        },
        true,
    );
    let diags = program.semantic_diagnostics();
    let with_related = diags
        .iter()
        .find(|d| d.code == 2528 && !d.related_information.is_empty())
        .expect("a TS2528 carries the 'first export default is here' related info");
    assert!(
        with_related
            .related_information
            .iter()
            .any(|r| r.code == 2752),
        "the related info is TS2752 (The first export default is here): {with_related:?}"
    );
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
