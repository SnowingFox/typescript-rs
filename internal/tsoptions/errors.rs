//! Diagnostic construction for option parsing: unknown options, invalid enum
//! values, type-mismatch help text, and `files`/glob spec validation.
//!
//! 1:1 port of Go `internal/tsoptions/errors.go` (plus the spec/locale value
//! validation from `tsconfigparsing.go`).
//!
//! DIVERGENCE(port): command-line parsing never has a source file, so the
//! `CreateDiagnosticForNodeInSourceFile*` family collapses to the
//! compiler-diagnostic form here; the node-anchored form lands with tsconfig
//! parsing.

use tsgo_core::text::TextRange;
use tsgo_diagnostics::{self as diagnostics, Message};
use tsgo_parser::Diagnostic;

use crate::commandlineoption::{CommandLineOption, CommandLineOptionKind};
use crate::diagnostics::AlternateModeDiagnostics;
use crate::OptionValue;

/// Builds a file-less compiler diagnostic (mirrors `ast.NewCompilerDiagnostic`).
///
/// Side effects: none (pure).
// Go: internal/ast/diagnostic.go:NewCompilerDiagnostic
pub fn new_compiler_diagnostic(message: &'static Message, args: Vec<String>) -> Diagnostic {
    Diagnostic {
        loc: TextRange::new(-1, -1),
        message,
        args,
    }
}

/// Builds a diagnostic for a value that is not a valid enum key for `opt`.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/errors.go:createDiagnosticForInvalidEnumType
pub fn create_diagnostic_for_invalid_enum_type(opt: &CommandLineOption) -> Diagnostic {
    let names_of_type: Vec<&str> = opt
        .enum_map()
        .map(|m| m.keys().copied().collect())
        .unwrap_or_default();
    let string_names = format_enum_type_keys(opt, &names_of_type);
    let opt_name = format!("--{}", opt.name);
    new_compiler_diagnostic(
        &diagnostics::ARGUMENT_FOR_0_OPTION_MUST_BE_COLON_1,
        vec![opt_name, string_names],
    )
}

/// Formats an enum option's value keys as `'a', 'b'`, dropping deprecated keys.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/errors.go:formatEnumTypeKeys
pub fn format_enum_type_keys(opt: &CommandLineOption, keys: &[&str]) -> String {
    let deprecated = opt.deprecated_keys();
    let kept: Vec<&str> = keys
        .iter()
        .copied()
        .filter(|k| deprecated.is_none_or(|d| !d.has(k)))
        .collect();
    format!("'{}'", kept.join("', '"))
}

/// Returns the user-facing type name for an option (`"string"`, `"Array"`, ...).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/errors.go:getCompilerOptionValueTypeString
pub fn get_compiler_option_value_type_string(option: &CommandLineOption) -> String {
    match option.kind {
        CommandLineOptionKind::ListOrElement => match option.elements() {
            Some(el) => format!("{} or Array", get_compiler_option_value_type_string(el)),
            None => "Array".to_string(),
        },
        CommandLineOptionKind::List => "Array".to_string(),
        kind => kind_str(kind).to_string(),
    }
}

/// Returns the lowercase kind string Go produces from `string(option.Kind)`.
fn kind_str(kind: CommandLineOptionKind) -> &'static str {
    match kind {
        CommandLineOptionKind::String => "string",
        CommandLineOptionKind::Number => "number",
        CommandLineOptionKind::Boolean => "boolean",
        CommandLineOptionKind::Object => "object",
        CommandLineOptionKind::List => "list",
        CommandLineOptionKind::ListOrElement => "listOrElement",
        CommandLineOptionKind::Enum => "enum",
    }
}

/// Builds the diagnostic for an unknown option, accounting for the alternate
/// mode (an option that belongs to `tsc -b` vs `tsc`, or `--build` first).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/errors.go:createUnknownOptionError
pub fn create_unknown_option_error(
    unknown_option: &str,
    unknown_option_diagnostic: &'static Message,
    unknown_option_error_text: &str,
    alternate_mode: Option<&AlternateModeDiagnostics>,
) -> Diagnostic {
    if let Some(alt) = alternate_mode {
        if let Some(other_option) = alt.options_name_map.get(&unknown_option.to_lowercase()) {
            let mut diagnostic = alt.diagnostic;
            if other_option.name == "build" {
                diagnostic = &diagnostics::OPTION_BUILD_MUST_BE_THE_FIRST_COMMAND_LINE_ARGUMENT;
            }
            return new_compiler_diagnostic(diagnostic, vec![unknown_option.to_string()]);
        }
    }
    let text = if unknown_option_error_text.is_empty() {
        unknown_option
    } else {
        unknown_option_error_text
    };
    new_compiler_diagnostic(unknown_option_diagnostic, vec![text.to_string()])
}

/// The unknown-option diagnostic message for an option container name.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/errors.go:extraKeyDiagnostics
pub fn extra_key_diagnostics(s: &str) -> Option<&'static Message> {
    match s {
        "compilerOptions" => Some(&diagnostics::UNKNOWN_COMPILER_OPTION_0),
        "watchOptions" => Some(&diagnostics::UNKNOWN_WATCH_OPTION_0),
        "typeAcquisition" => Some(&diagnostics::UNKNOWN_TYPE_ACQUISITION_OPTION_0),
        "buildOptions" => Some(&diagnostics::UNKNOWN_BUILD_OPTION_0),
        _ => None,
    }
}

/// The "did you mean" unknown-option diagnostic for an option container name.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/errors.go:extraKeyDidYouMeanDiagnostics
pub fn extra_key_did_you_mean_diagnostics(s: &str) -> Option<&'static Message> {
    match s {
        "compilerOptions" => Some(&diagnostics::UNKNOWN_COMPILER_OPTION_0_DID_YOU_MEAN_1),
        "watchOptions" => Some(&diagnostics::UNKNOWN_WATCH_OPTION_0_DID_YOU_MEAN_1),
        "typeAcquisition" => Some(&diagnostics::UNKNOWN_TYPE_ACQUISITION_OPTION_0_DID_YOU_MEAN_1),
        "buildOptions" => Some(&diagnostics::UNKNOWN_BUILD_OPTION_0_DID_YOU_MEAN_1),
        _ => None,
    }
}

/// Validates a glob spec, returning the diagnostic message it violates (if any).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:specToDiagnostic
pub fn spec_to_diagnostic(
    spec: &str,
    disallow_trailing_recursion: bool,
) -> Option<&'static Message> {
    if disallow_trailing_recursion {
        if invalid_trailing_recursion(spec) {
            return Some(
                &diagnostics::FILE_SPECIFICATION_CANNOT_END_IN_A_RECURSIVE_DIRECTORY_WILDCARD_ASTERISK_ASTERISK_COLON_0,
            );
        }
    } else if invalid_dot_dot_after_recursive_wildcard(spec) {
        return Some(
            &diagnostics::FILE_SPECIFICATION_CANNOT_CONTAIN_A_PARENT_DIRECTORY_THAT_APPEARS_AFTER_A_RECURSIVE_DIRECTORY_WILDCARD_ASTERISK_ASTERISK_COLON_0,
        );
    }
    None
}

/// Reports whether a spec ends in a recursive directory wildcard (`**`).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:invalidTrailingRecursion
pub fn invalid_trailing_recursion(spec: &str) -> bool {
    let s = spec.strip_suffix('/').unwrap_or(spec);
    s == "**" || s.ends_with("/**")
}

/// Reports whether a `/../` segment appears after a `**/` recursive wildcard.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:invalidDotDotAfterRecursiveWildcard
pub fn invalid_dot_dot_after_recursive_wildcard(s: &str) -> bool {
    let wildcard_index: isize = if s.starts_with("**/") {
        0
    } else {
        match s.find("/**/") {
            Some(i) => i as isize,
            None => return false,
        }
    };
    let last_dot_index: isize = if s.ends_with("/..") {
        s.len() as isize
    } else {
        match s.rfind("/../") {
            Some(i) => i as isize,
            None => return false,
        }
    };
    last_dot_index > wildcard_index
}

/// Validates an option value against its extra-validation rule (`spec`/`locale`).
///
/// Returns `Some(value)` when valid (or no validation applies), and `None` plus
/// diagnostics when validation fails.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/tsconfigparsing.go:validateJsonOptionValue
pub fn validate_json_option_value(
    opt: &CommandLineOption,
    val: &OptionValue,
) -> (Option<OptionValue>, Vec<Diagnostic>) {
    use crate::commandlineoption::ExtraValidation;
    if val.is_null() {
        return (None, Vec::new());
    }
    let mut errors = Vec::new();
    match opt.extra_validation {
        ExtraValidation::Spec => {
            if let OptionValue::String(s) = val {
                if let Some(diag) = spec_to_diagnostic(s, false) {
                    errors.push(new_compiler_diagnostic(diag, vec![]));
                }
            }
        }
        ExtraValidation::Locale => {
            if let OptionValue::String(s) = val {
                if tsgo_locale::parse(s).is_none() {
                    errors.push(new_compiler_diagnostic(
                        &diagnostics::LOCALE_MUST_BE_AN_IETF_BCP_47_LANGUAGE_TAG_EXAMPLES_COLON_0_1,
                        vec!["en".to_string(), "ja-jp".to_string()],
                    ));
                }
            }
        }
        ExtraValidation::None => {}
    }
    if !errors.is_empty() {
        return (None, errors);
    }
    (Some(val.clone()), Vec::new())
}

#[cfg(test)]
#[path = "errors_test.rs"]
mod tests;
