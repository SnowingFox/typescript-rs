use super::*;

use tsgo_core::tristate::Tristate;
use tsgo_vfs::vfstest::MapFs;

use crate::tsoptionstest::VfsParseConfigHost;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

// Go: internal/tsoptions/commandlineparser_test.go:TestResponseFileDoesNotPanic
#[test]
fn response_file_empty_does_not_panic() {
    let fs = MapFs::from_map(std::iter::empty::<(String, String)>(), true);
    let parsed = parse_command_line_test_worker(None, &s(&["@"]), Some(&fs), "/tmp");
    assert!(
        !parsed.errors.is_empty(),
        "expected an error for empty response file name"
    );
}

#[test]
fn response_file_relative_missing_does_not_panic() {
    let fs = MapFs::from_map(std::iter::empty::<(String, String)>(), true);
    let parsed = parse_command_line_test_worker(None, &s(&["@blah"]), Some(&fs), "/tmp");
    assert!(
        !parsed.errors.is_empty(),
        "expected an error for missing response file"
    );
}

// Go: internal/tsoptions/commandlineparser_test.go:TestParseCommandLineTypeRootsRelativePath
#[test]
fn type_roots_relative_to_absolute() {
    let host = VfsParseConfigHost::new(
        &[("/home/project/bug.ts", "let x = 1;")],
        "/home/project",
        true,
    );
    let cmd = parse_command_line(&s(&["--typeRoots", "t", "bug.ts"]), &host);
    let type_roots = cmd
        .compiler_options()
        .type_roots
        .as_ref()
        .expect("type_roots set");
    assert_eq!(type_roots.len(), 1);
    assert!(tsgo_tspath::is_rooted_disk_path(&type_roots[0]));
    assert!(type_roots[0].ends_with("/t"), "got: {}", type_roots[0]);
}

// Behavior-level (Go TestCommandLineParseResult is golden/P10; here we assert
// the parsed values directly).
#[test]
fn parse_single_lib_flag() {
    let host = VfsParseConfigHost::new(&[("/p/0.ts", "")], "/p", true);
    let cmd = parse_command_line(&s(&["--lib", "es6", "0.ts"]), &host);
    assert_eq!(cmd.file_names(), &["0.ts".to_string()]);
    assert_eq!(
        cmd.compiler_options().lib,
        vec!["lib.es2015.d.ts".to_string()]
    );
}

#[test]
fn parse_explicit_boolean_false() {
    let host = VfsParseConfigHost::new(&[("/p/0.ts", "")], "/p", true);
    let cmd = parse_command_line(&s(&["--strictNullChecks", "false", "0.ts"]), &host);
    assert_eq!(cmd.compiler_options().strict_null_checks, Tristate::False);
    assert_eq!(cmd.file_names(), &["0.ts".to_string()]);
}

#[test]
fn parse_tsconfig_only_option_null() {
    let host = VfsParseConfigHost::new(&[("/p/0.ts", "")], "/p", true);
    let cmd = parse_command_line(
        &s(&["--composite", "null", "-tsBuildInfoFile", "null", "0.ts"]),
        &host,
    );
    assert_eq!(cmd.file_names(), &["0.ts".to_string()]);
    // tsconfig-only `composite` set to null is allowed (no error).
    assert!(cmd.errors().is_empty(), "errors: {:?}", cmd.errors());
    assert_eq!(cmd.compiler_options().composite, Tristate::Unknown);
}

#[test]
fn parse_missing_enum_argument_errors() {
    let host = VfsParseConfigHost::new(&[("/p/0.ts", "")], "/p", true);
    let cmd = parse_command_line(&s(&["0.ts", "--fallbackPolling"]), &host);
    assert!(!cmd.errors().is_empty());
}

#[test]
fn parse_unknown_option_errors() {
    let host = VfsParseConfigHost::new(&[("/p/0.ts", "")], "/p", true);
    let cmd = parse_command_line(&s(&["--unknownXyz"]), &host);
    assert!(!cmd.errors().is_empty());
    assert_eq!(cmd.errors()[0].message.code(), 5023);
}

// Go: internal/tsoptions/commandlineparser_test.go:TestParseBuildCommandLine
#[test]
fn build_no_options_defaults_to_dot() {
    let host = VfsParseConfigHost::new(&[], "/", true);
    let parsed = parse_build_command_line(&[], &host);
    assert_eq!(parsed.projects, vec![".".to_string()]);
}

#[test]
fn build_clean_and_force_invalid() {
    let host = VfsParseConfigHost::new(&[], "/", true);
    let parsed = parse_build_command_line(&s(&["--clean", "--force"]), &host);
    assert!(!parsed.errors.is_empty());
}

#[test]
fn build_builders_parsed() {
    let host = VfsParseConfigHost::new(&[], "/", true);
    let parsed = parse_build_command_line(&s(&["--builders", "2"]), &host);
    assert_eq!(parsed.build_options.builders, Some(2));
}

#[test]
fn build_singlethreaded_and_builders() {
    let host = VfsParseConfigHost::new(&[], "/", true);
    let parsed = parse_build_command_line(&s(&["--singleThreaded", "--builders", "2"]), &host);
    assert_eq!(parsed.build_options.builders, Some(2));
    assert_eq!(parsed.compiler_options.single_threaded, Tristate::True);
}

#[test]
fn build_builders_zero_errors() {
    let host = VfsParseConfigHost::new(&[], "/", true);
    let parsed = parse_build_command_line(&s(&["--builders", "0"]), &host);
    assert!(!parsed.errors.is_empty());
    assert_eq!(parsed.build_options.builders, None);
}

#[test]
fn build_builders_negative_errors() {
    let host = VfsParseConfigHost::new(&[], "/", true);
    let parsed = parse_build_command_line(&s(&["--builders", "-1"]), &host);
    assert!(!parsed.errors.is_empty());
}

#[test]
fn build_builders_invalid_type_errors() {
    let host = VfsParseConfigHost::new(&[], "/", true);
    let parsed = parse_build_command_line(&s(&["--builders", "invalid"]), &host);
    assert!(!parsed.errors.is_empty());
}
