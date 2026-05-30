use super::*;

use crate::commandlineoption::{CommandLineOption, CommandLineOptionKind};

// Go: internal/tsoptions/tsconfigparsing.go:invalidTrailingRecursion
#[test]
fn invalid_trailing_recursion_cases() {
    assert!(invalid_trailing_recursion("**"));
    assert!(invalid_trailing_recursion("a/**"));
    assert!(invalid_trailing_recursion("a/**/"));
    assert!(!invalid_trailing_recursion("a/**/b"));
    assert!(!invalid_trailing_recursion("a**b"));
}

// Go: internal/tsoptions/tsconfigparsing.go:invalidDotDotAfterRecursiveWildcard
#[test]
fn invalid_dot_dot_after_recursive_wildcard_cases() {
    assert!(invalid_dot_dot_after_recursive_wildcard("**/.."));
    assert!(invalid_dot_dot_after_recursive_wildcard("a/**/../b"));
    assert!(!invalid_dot_dot_after_recursive_wildcard("../a/**"));
    assert!(!invalid_dot_dot_after_recursive_wildcard("a/b/c"));
}

// Go: internal/tsoptions/tsconfigparsing.go:specToDiagnostic
#[test]
fn spec_to_diagnostic_invalid_dot_dot() {
    // `**/../*` has a `/..` after `**/` -> invalid (non-recursion-disallowed mode).
    assert!(spec_to_diagnostic("**/../*", false).is_some());
    assert!(spec_to_diagnostic("src/**/*.ts", false).is_none());
    // trailing recursion only flagged when disallowed.
    assert!(spec_to_diagnostic("src/**", true).is_some());
    assert!(spec_to_diagnostic("src/**", false).is_none());
}

// Go: internal/tsoptions/errors.go:getCompilerOptionValueTypeString
#[test]
fn compiler_option_value_type_string() {
    let s = CommandLineOption {
        name: "x",
        kind: CommandLineOptionKind::String,
        ..Default::default()
    };
    assert_eq!(get_compiler_option_value_type_string(&s), "string");
    let list = CommandLineOption {
        name: "lib",
        kind: CommandLineOptionKind::List,
        ..Default::default()
    };
    assert_eq!(get_compiler_option_value_type_string(&list), "Array");
}

// Go: internal/tsoptions/errors.go:formatEnumTypeKeys
#[test]
fn format_enum_type_keys_drops_deprecated() {
    let module = CommandLineOption {
        name: "module",
        kind: CommandLineOptionKind::Enum,
        ..Default::default()
    };
    let keys = ["commonjs", "amd", "esnext"];
    let formatted = format_enum_type_keys(&module, &keys);
    // `amd` is deprecated for `module` and is dropped.
    assert!(formatted.contains("commonjs"));
    assert!(formatted.contains("esnext"));
    assert!(!formatted.contains("amd"));
}

// Go: internal/tsoptions/errors.go:createUnknownOptionError
#[test]
fn create_unknown_option_error_basic() {
    let diag = create_unknown_option_error(
        "unknownXyz",
        &tsgo_diagnostics::UNKNOWN_COMPILER_OPTION_0,
        "",
        None,
    );
    assert_eq!(diag.message.code(), 5023);
    assert_eq!(diag.args, vec!["unknownXyz".to_string()]);
}
