use tsgo_ast::Kind;
use tsgo_astnav::NavSourceFile;
use tsgo_checker::get_symbol_at_location;

use crate::test_support::build_service;

// Go: internal/ls/languageservice.go:NewLanguageService + LanguageService.GetProgram
#[test]
fn language_service_wraps_one_file_program_and_exposes_program() {
    let ls = build_service(
        &[("/src/index.ts", "const x = 1;")],
        "/src",
        &["/src/index.ts"],
    );
    assert_eq!(ls.program().source_files().len(), 1);
    assert!(ls.program().get_source_file("/src/index.ts").is_some());
}

// Go: internal/ls/languageservice.go:LanguageService.toPath
#[test]
fn language_service_to_path_canonicalizes_against_program_cwd() {
    let ls = build_service(
        &[("/src/index.ts", "const x = 1;")],
        "/src",
        &["/src/index.ts"],
    );
    assert_eq!(ls.to_path("/src/index.ts").as_str(), "/src/index.ts");
    assert_eq!(ls.to_path("index.ts").as_str(), "/src/index.ts");
    assert_eq!(ls.project_path().as_str(), "/src");
}

// Go: internal/ls/languageservice.go:LanguageService.ReadFile / UseCaseSensitiveFileNames
#[test]
fn language_service_read_file_and_case_sensitivity_delegate_to_host() {
    let ls = build_service(
        &[("/src/index.ts", "const x = 1;")],
        "/src",
        &["/src/index.ts"],
    );
    assert_eq!(
        ls.read_file("/src/index.ts").as_deref(),
        Some("const x = 1;")
    );
    assert!(ls.read_file("/missing.ts").is_none());
    assert!(ls.use_case_sensitive_file_names());
}

// Go: internal/ls/languageservice.go:LanguageService.tryGetProgramAndFile (+ GetTypeCheckerForFile)
// The plumbing slice: the service exposes, for a file + position, a token (via
// astnav over the program's shared arena) and a checker that resolves it.
#[test]
fn file_check_context_exposes_a_checker_that_resolves_a_token_symbol() {
    let mut ls = build_service(
        &[("/src/index.ts", "const x = 1;")],
        "/src",
        &["/src/index.ts"],
    );
    let mut ctx = ls
        .file_check_context("/src/index.ts")
        .expect("a checking context for the file");

    // The view is the file's own SourceFile, so astnav node ids match the
    // checker's symbol tables.
    let nav = NavSourceFile::from_borrowed_arena(ctx.view.arena(), ctx.root, ctx.text.clone());
    assert_eq!(nav.kind(nav.root()), Kind::SourceFile);

    // Position 6 is the `x` binding in `const x = 1;`.
    let node = nav.get_touching_property_name(6);
    assert_eq!(nav.kind(node), Kind::Identifier);

    let globals = ctx.view.globals().cloned();
    let symbol =
        get_symbol_at_location(&mut ctx.checker, ctx.view.as_ref(), node, globals.as_ref())
            .expect("a symbol for the `x` binding");
    assert_eq!(ctx.view.symbol(symbol).name, "x");
}

// `file_check_context` returns `None` for a file the program does not have.
// Go: internal/ls/languageservice.go:LanguageService.tryGetProgramAndFile (nil file)
#[test]
fn file_check_context_is_none_for_unknown_file() {
    let mut ls = build_service(
        &[("/src/index.ts", "const x = 1;")],
        "/src",
        &["/src/index.ts"],
    );
    assert!(ls.file_check_context("/src/missing.ts").is_none());
}
