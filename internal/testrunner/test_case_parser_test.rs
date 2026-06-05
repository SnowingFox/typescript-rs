use super::*;

// Slice 1 (RED->GREEN): a single `// @option:` directive plus one
// `// @filename:` split yields one unit, and the option is extracted as a
// setting.
// Go: internal/testrunner/test_case_parser_test.go:TestMakeUnitsFromTest (single-file shape)
#[test]
fn parse_single_directive_and_filename() {
    let code = "// @strict: true\n// @filename: a.ts\nconst x = 1;";
    let content = make_units_from_test(code, "simpleTest.ts");
    assert_eq!(content.test_unit_data.len(), 1);
    assert_eq!(content.test_unit_data[0].name, "a.ts");
    assert_eq!(content.test_unit_data[0].content, "const x = 1;");
    assert!(content.symlinks.is_empty());

    let settings = extract_compiler_settings(code);
    assert_eq!(settings.get("strict").map(String::as_str), Some("true"));
}

// Slice 2 (RED->GREEN): two `// @filename:` directives produce two units, with
// content (including non-directive comment lines) routed to the right unit.
// This mirrors Go's table-free `TestMakeUnitsFromTest` exactly.
// Go: internal/testrunner/test_case_parser_test.go:TestMakeUnitsFromTest
#[test]
fn make_units_from_test_multi_file() {
    let code = "// @strict: true\n\
// @noEmit: true\n\
// @filename: firstFile.ts\n\
function foo() { return \"a\"; }\n\
// normal comment\n\
// @filename: secondFile.ts\n\
// some other comment\n\
function bar() { return \"b\"; }";

    let content = make_units_from_test(code, "simpleTest.ts");
    assert_eq!(content.test_unit_data.len(), 2);
    assert_eq!(
        content.test_unit_data[0],
        TestUnit {
            content: "function foo() { return \"a\"; }\n// normal comment".to_string(),
            name: "firstFile.ts".to_string(),
        }
    );
    assert_eq!(
        content.test_unit_data[1],
        TestUnit {
            content: "// some other comment\nfunction bar() { return \"b\"; }".to_string(),
            name: "secondFile.ts".to_string(),
        }
    );
    assert!(content.ts_config.is_none());
    assert!(content.symlinks.is_empty());
}

// The global directives that precede the first `@filename` are collected as
// global options, not folded into any unit's content.
// Go: internal/testrunner/test_case_parser.go:ParseTestFilesAndSymlinksWithOptions (globalOptions)
#[test]
fn global_options_collected_before_first_filename() {
    let code = "// @strict: true\n// @noEmit: true\n// @filename: a.ts\nconst x = 1;";
    let parsed = parse_test_files_and_symlinks(code, "test.ts");
    assert_eq!(parsed.units.len(), 1);
    assert_eq!(
        parsed.global_options.get("strict").map(String::as_str),
        Some("true")
    );
    assert_eq!(
        parsed.global_options.get("noemit").map(String::as_str),
        Some("true")
    );
}

// With no `@filename` directive at all, the single unit is named from the
// base of the `file_name` argument.
// Go: internal/testrunner/test_case_parser.go:ParseTestFilesAndSymlinksWithOptions (single-file normalize)
#[test]
fn single_file_without_filename_directive_uses_base_name() {
    let parsed = parse_test_files_and_symlinks("const x = 1;", "/dir/sub/test.ts");
    assert_eq!(parsed.units.len(), 1);
    assert_eq!(parsed.units[0].name, "test.ts");
    assert_eq!(parsed.units[0].content, "const x = 1;");
}

// `// @currentDirectory:` is recorded separately from the units.
// Go: internal/testrunner/test_case_parser.go:ParseTestFilesAndSymlinksWithOptions (currentDirectory)
#[test]
fn current_directory_directive_is_captured() {
    let code = "// @currentDirectory: /work\n// @filename: a.ts\nx;";
    let parsed = parse_test_files_and_symlinks(code, "test.ts");
    assert_eq!(parsed.current_directory, "/work");
}

// `// @link: <source> -> <target>` declares a symlink keyed by the target.
// Go: internal/testrunner/test_case_parser.go:parseSymlinkFromTest
#[test]
fn link_directive_records_symlink() {
    let code = "// @link: /real/node_modules -> /app/node_modules\n// @filename: a.ts\nx;";
    let parsed = parse_test_files_and_symlinks(code, "test.ts");
    assert_eq!(
        parsed.symlinks.get("/app/node_modules").map(String::as_str),
        Some("/real/node_modules")
    );
    // The link line is consumed (not part of any unit's content).
    assert_eq!(parsed.units[0].content, "x;");
}

// `// @symlink:` after a `@filename` maps each comma-separated link to the
// current file.
// Go: internal/testrunner/test_case_parser.go:ParseTestFilesAndSymlinksWithOptions (symlink)
#[test]
fn symlink_directive_maps_links_to_current_file() {
    let code = "// @filename: real.ts\nx;\n// @symlink: a.ts, b.ts";
    let parsed = parse_test_files_and_symlinks(code, "test.ts");
    assert_eq!(
        parsed.symlinks.get("a.ts").map(String::as_str),
        Some("real.ts")
    );
    assert_eq!(
        parsed.symlinks.get("b.ts").map(String::as_str),
        Some("real.ts")
    );
}

// An embedded `tsconfig.json` unit is parsed into `ts_config` and removed from
// the compile unit list (mirroring Go `makeUnitsFromTest`).
#[test]
fn make_units_from_test_parses_tsconfig() {
    let code = "// @filename: tsconfig.json\n{}\n// @filename: a.ts\nexport const x = 1;";
    let content = make_units_from_test(code, "test.ts");
    assert_eq!(content.test_unit_data.len(), 1);
    assert_eq!(content.test_unit_data[0].name, "a.ts");
    assert!(content.ts_config.is_some());
    assert_eq!(
        content.ts_config_file_unit.as_ref().map(|u| u.name.as_str()),
        Some("tsconfig.json")
    );
}

// `extract_compiler_settings` lowercases option names and strips a trailing
// `;` from values, scanning the whole content.
// Go: internal/testrunner/test_case_parser.go:extractCompilerSettings
#[test]
fn extract_compiler_settings_lowercases_and_strips_semicolon() {
    let settings = extract_compiler_settings("// @Strict: true\n// @Target: ES2015;\nconst x = 1;");
    assert_eq!(settings.get("strict").map(String::as_str), Some("true"));
    assert_eq!(settings.get("target").map(String::as_str), Some("ES2015"));
}

// A later duplicate of the same option overwrites the earlier value (Go's
// duplicate-detection panic is a deliberate no-op).
// Go: internal/testrunner/test_case_parser.go:ParseTestFilesAndSymlinksWithOptions (duplicate global option)
#[test]
fn duplicate_global_option_last_wins() {
    let code = "// @target: es5\n// @target: esnext\n// @filename: a.ts\nx;";
    let parsed = parse_test_files_and_symlinks(code, "test.ts");
    assert_eq!(
        parsed.global_options.get("target").map(String::as_str),
        Some("esnext")
    );
}

// A non-directive comment that is not an `@option` is treated as content.
// Go: internal/testrunner/test_case_parser.go:ParseTestFilesAndSymlinksWithOptions (content lines)
#[test]
fn plain_comment_line_is_content_not_directive() {
    let code = "// @filename: a.ts\n// just a comment\nconst x = 1;";
    let parsed = parse_test_files_and_symlinks(code, "test.ts");
    assert_eq!(parsed.units.len(), 1);
    assert_eq!(parsed.units[0].content, "// just a comment\nconst x = 1;");
}

// `parse_symlink_from_test` returns false (and does not mutate) for a
// non-link line.
// Go: internal/testrunner/test_case_parser.go:parseSymlinkFromTest
#[test]
fn parse_symlink_from_test_rejects_non_link() {
    let mut symlinks = indexmap::IndexMap::new();
    assert!(!parse_symlink_from_test("const x = 1;", &mut symlinks));
    assert!(symlinks.is_empty());
    assert!(parse_symlink_from_test("// @link: /a -> /b", &mut symlinks));
    assert_eq!(symlinks.get("/b").map(String::as_str), Some("/a"));
}

// Comment-only content before the first `@filename` is allowed (skip_trivia
// treats it as trivia), so no panic occurs.
// Go: internal/testrunner/test_case_parser.go:ParseTestFilesAndSymlinksWithOptions (hasContentBeforeFirstFilename)
#[test]
fn comment_only_content_before_first_filename_is_allowed() {
    let code = "// a leading comment\n// @filename: a.ts\nconst x = 1;";
    let parsed = parse_test_files_and_symlinks(code, "test.ts");
    assert_eq!(parsed.units.len(), 1);
    assert_eq!(parsed.units[0].name, "a.ts");
    assert_eq!(parsed.units[0].content, "const x = 1;");
}

// Real code before the first `@filename` (without implicit-first-file) is a
// hard error, mirroring Go's panic.
// Go: internal/testrunner/test_case_parser.go:ParseTestFilesAndSymlinksWithOptions (panic)
#[test]
#[should_panic(expected = "Non-comment test content appears before the first")]
fn content_before_first_filename_panics() {
    let code = "const leaked = 1;\n// @filename: a.ts\nconst x = 1;";
    let _ = parse_test_files_and_symlinks(code, "test.ts");
}

// With `allow_implicit_first_file`, leading content is collected into an
// implicit first file named by the `file_name` argument.
// Go: internal/testrunner/test_case_parser.go:ParseTestFilesAndSymlinksWithOptions (AllowImplicitFirstFile)
#[test]
fn implicit_first_file_collects_leading_content() {
    let code = "leading content\n// @filename: a.ts\nconst x = 1;";
    let parsed = parse_test_files_and_symlinks_with_options(
        code,
        "implicit.ts",
        ParseTestFilesOptions {
            allow_implicit_first_file: true,
        },
    );
    assert_eq!(parsed.units.len(), 2);
    assert_eq!(parsed.units[0].name, "implicit.ts");
    assert_eq!(parsed.units[0].content, "leading content");
    assert_eq!(parsed.units[1].name, "a.ts");
    assert_eq!(parsed.units[1].content, "const x = 1;");
}
