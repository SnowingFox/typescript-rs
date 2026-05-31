//! Test helper to build a [`ParsedCommandLine`] from raw `tsconfig.json` text.
//!
//! 1:1 port of Go `internal/tsoptions/tsoptionstest/parsedcommandline.go`.
//!
//! [`ParsedCommandLine`]: crate::ParsedCommandLine

use crate::tsoptionstest::VfsParseConfigHost;
use crate::{
    new_tsconfig_source_file_from_file_path, parse_json_source_file_config_file_content,
    ParsedCommandLine,
};

/// Builds a [`ParsedCommandLine`] by parsing `json_text` as the `tsconfig.json`
/// in `current_directory`, with `files` providing the in-memory file system.
///
/// Mirrors Go `GetParsedCommandLine`: it constructs a [`VfsParseConfigHost`],
/// parses the tsconfig source file, and runs the full config-content pipeline.
///
/// # Examples
/// ```
/// use tsgo_tsoptions::tsoptionstest::get_parsed_command_line;
/// let parsed = get_parsed_command_line(
///     r#"{ "files": ["a.ts"] }"#,
///     &[("/dev/a.ts", ""), ("/dev/b.ts", "")],
///     "/dev",
///     true,
/// );
/// assert_eq!(parsed.file_names(), &["/dev/a.ts".to_string()]);
/// ```
///
/// Side effects: enumerates directories through the in-memory host.
// Go: internal/tsoptions/tsoptionstest/parsedcommandline.go:GetParsedCommandLine
pub fn get_parsed_command_line(
    json_text: &str,
    files: &[(&str, &str)],
    current_directory: &str,
    use_case_sensitive_file_names: bool,
) -> ParsedCommandLine {
    let host = VfsParseConfigHost::new(files, current_directory, use_case_sensitive_file_names);
    let config_file_name = tsgo_tspath::combine_paths(current_directory, &["tsconfig.json"]);
    let source_file = new_tsconfig_source_file_from_file_path(&config_file_name, json_text);
    parse_json_source_file_config_file_content(
        &source_file,
        &host,
        current_directory,
        None,
        &config_file_name,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OptionValue;

    // Go: parsedcommandline.go:GetParsedCommandLine (behavior; the Go file has no
    // unit test, so this exercises the helper end-to-end).
    #[test]
    fn literal_files_are_resolved_relative_to_config() {
        let parsed = get_parsed_command_line(
            r#"{ "files": ["a.ts", "b.ts"] }"#,
            &[("/dev/a.ts", ""), ("/dev/b.ts", ""), ("/dev/c.ts", "")],
            "/dev",
            true,
        );
        assert_eq!(
            parsed.file_names(),
            &["/dev/a.ts".to_string(), "/dev/b.ts".to_string()]
        );
        assert!(parsed.errors.is_empty());
    }

    // Go: parsedcommandline_test.go:TestParsedCommandLine/.../"duplicates"
    // (LiteralFileNames dedups the `files` list).
    #[test]
    fn literal_file_names_are_deduplicated() {
        let parsed = get_parsed_command_line(
            r#"{ "files": ["a.ts", "a.ts", "b.ts"] }"#,
            &[("/dev/a.ts", ""), ("/dev/b.ts", "")],
            "/dev",
            true,
        );
        assert_eq!(
            parsed.literal_file_names(),
            &["/dev/a.ts".to_string(), "/dev/b.ts".to_string()]
        );
    }

    // Go: parsedcommandline.go:GetParsedCommandLine (behavior).
    #[test]
    fn default_include_expands_wildcards() {
        let parsed = get_parsed_command_line(
            "{}",
            &[("/dev/a.ts", ""), ("/dev/sub/b.ts", "")],
            "/dev",
            true,
        );
        assert!(parsed.file_names().contains(&"/dev/a.ts".to_string()));
        assert!(parsed.file_names().contains(&"/dev/sub/b.ts".to_string()));
        // The raw config round-trips as an (empty) object.
        assert!(matches!(parsed.raw, OptionValue::Map(_)));
    }
}
