//! Port of Go `internal/testrunner/test_case_parser.go`.
//!
//! Parses a conformance/compiler test file into its constituent in-memory
//! units. A test file uses `// @<option>: <value>` directives for compiler /
//! harness settings and `// @filename: <name>` directives to split the file
//! into multiple named units; `// @link: <a> -> <b>` declares a symlink. This
//! module turns that text into a [`TestCaseContent`] (the named units plus the
//! collected symlinks) and exposes [`extract_compiler_settings`] for the
//! `// @<option>: <value>` directives.
//!
//! DIVERGENCE(port): Go's [`parse_test_files_and_symlinks_with_options`] is
//! generic over a `parseFile` callback returning `(T, error)` so fourslash can
//! build its own unit type. The only compiler-test caller builds a plain
//! [`TestUnit`] and never errors, so this port specializes the callback to
//! produce [`TestUnit`] directly (fourslash is a separate, later crate).

use indexmap::IndexMap;

use tsgo_scanner::skip_trivia;
use tsgo_testutil_harnessutil::get_config_name_from_file_name;
use tsgo_tsoptions::{
    new_tsconfig_source_file_from_file_path, parse_json_source_file_config_file_content,
    tsoptionstest::VfsParseConfigHost, ParsedCommandLine,
};
use tsgo_tspath::{get_base_file_name, get_directory_path, get_normalized_absolute_path};

use crate::SRC_FOLDER;

/// A compiler setting mapped to its raw value as written in the test file.
///
/// For example, `// @target: esnext, es2015` maps `"target"` to the string
/// `"esnext, es2015"` (the harness later splits and resolves it). Keys are
/// lowercased; iteration order follows first-encounter order.
///
/// Side effects: none (plain data).
// Go: internal/testrunner/test_case_parser.go:rawCompilerSettings
pub type RawCompilerSettings = IndexMap<String, String>;

/// One named in-memory unit of a (possibly multi-file) test: the file name from
/// a `// @filename:` directive and the source text that follows it.
///
/// Side effects: none (plain data).
// Go: internal/testrunner/test_case_parser.go:testUnit
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestUnit {
    /// The unit's source text (the lines between this and the next directive).
    pub content: String,
    /// The unit's file name (from its `// @filename:` directive).
    pub name: String,
}

/// The parsed contents of a test file: its named units and the symlinks
/// declared via `// @link:` / `// @symlink:` directives.
///
/// Side effects: none (plain data).
// Go: internal/testrunner/test_case_parser.go:testCaseContent
#[derive(Debug, Clone, Default)]
pub struct TestCaseContent {
    /// The named units, in declaration order.
    pub test_unit_data: Vec<TestUnit>,
    /// When the test embeds a `tsconfig.json` / `jsconfig.json` unit, its parsed
    /// command line (the unit itself is removed from [`test_unit_data`]).
    pub ts_config: Option<ParsedCommandLine>,
    /// The original `tsconfig.json` unit (for baseline file ordering).
    pub ts_config_file_unit: Option<TestUnit>,
    /// Declared symlinks (`link target` -> `source`).
    pub symlinks: IndexMap<String, String>,
}

/// Options controlling how a test file is split into units.
///
/// Side effects: none (plain data).
// Go: internal/testrunner/test_case_parser.go:ParseTestFilesOptions
#[derive(Debug, Clone, Copy, Default)]
pub struct ParseTestFilesOptions {
    /// If `true`, content appearing before the first `// @filename:` directive
    /// is collected into an implicit first file named by the `file_name`
    /// argument (the fourslash harness behavior). If `false`, such content is a
    /// hard error.
    pub allow_implicit_first_file: bool,
}

/// The outcome of splitting a test file into units (mirrors Go's multi-value
/// return of `ParseTestFilesAndSymlinks*`).
///
/// Side effects: none (plain data).
// Go: internal/testrunner/test_case_parser.go:ParseTestFilesAndSymlinksWithOptions (returns)
#[derive(Debug, Clone, Default)]
pub struct ParsedTestFiles {
    /// The named units, in declaration order.
    pub units: Vec<TestUnit>,
    /// Declared symlinks (`link target` -> `source`).
    pub symlinks: IndexMap<String, String>,
    /// The `// @currentDirectory:` value, or empty when unset.
    pub current_directory: String,
    /// Global `// @<option>:` directives (everything but `@filename`).
    pub global_options: IndexMap<String, String>,
}

/// File-specific directives recognized inside a unit (vs. global options).
// Go: internal/testrunner/test_case_parser.go:fourslashDirectives
const FOURSLASH_DIRECTIVES: &[&str] = &["emitthisfile"];

/// Splits `code` into named units given `// @filename` and `// @link` /
/// `// @symlink` directives, using the default options (no implicit first
/// file).
///
/// # Examples
/// ```
/// use tsgo_testrunner::parse_test_files_and_symlinks;
/// let parsed = parse_test_files_and_symlinks(
///     "// @filename: a.ts\nconst x = 1;",
///     "test.ts",
/// );
/// assert_eq!(parsed.units.len(), 1);
/// assert_eq!(parsed.units[0].name, "a.ts");
/// assert_eq!(parsed.units[0].content, "const x = 1;");
/// ```
///
/// Side effects: none (pure).
// Go: internal/testrunner/test_case_parser.go:ParseTestFilesAndSymlinks
pub fn parse_test_files_and_symlinks(code: &str, file_name: &str) -> ParsedTestFiles {
    parse_test_files_and_symlinks_with_options(code, file_name, ParseTestFilesOptions::default())
}

/// Splits `code` into named units, honoring `options`.
///
/// Lines beginning `// @<name>: <value>` are directives: `@filename` starts a
/// new unit, `@link`/`@symlink` declare symlinks, `@currentDirectory` sets the
/// working directory, and any other name is a global option. All other lines
/// are unit content.
///
/// # Panics
/// Panics (mirroring Go) if non-comment content appears before the first
/// `// @filename:` directive and `options.allow_implicit_first_file` is `false`.
///
/// Side effects: none (pure).
// Go: internal/testrunner/test_case_parser.go:ParseTestFilesAndSymlinksWithOptions
pub fn parse_test_files_and_symlinks_with_options(
    code: &str,
    file_name: &str,
    options: ParseTestFilesOptions,
) -> ParsedTestFiles {
    let mut test_units: Vec<TestUnit> = Vec::new();

    let lines = split_lines(code);

    let mut current_file_content = String::new();
    let mut current_file_name = if options.allow_implicit_first_file {
        // For fourslash tests, content before the first @filename goes into an
        // implicit first file named by the `file_name` parameter.
        file_name.to_string()
    } else {
        String::new()
    };
    let mut seen_content_line = false;
    let mut has_seen_file = false;
    let mut current_directory = String::new();
    let mut current_file_options: IndexMap<String, String> = IndexMap::new();
    let mut symlinks: IndexMap<String, String> = IndexMap::new();
    let mut global_options: IndexMap<String, String> = IndexMap::new();

    for line in &lines {
        if parse_symlink_from_test(line, &mut symlinks) {
            continue;
        }
        if let Some((name_raw, value_raw)) = match_option(line) {
            // Comment line: a global/file `@option`.
            let meta_data_name = name_raw.to_ascii_lowercase();
            let meta_data_value = value_raw.trim().to_string();
            if meta_data_name == "currentdirectory" {
                current_directory = meta_data_value.clone();
            }
            if meta_data_name != "filename" {
                if meta_data_name == "symlink" && !current_file_name.is_empty() {
                    for link in meta_data_value.split(',') {
                        let link = link.trim();
                        if !link.is_empty() {
                            symlinks.insert(link.to_string(), current_file_name.clone());
                        }
                    }
                } else if FOURSLASH_DIRECTIVES.contains(&meta_data_name.as_str()) {
                    // File-specific option.
                    current_file_options.insert(meta_data_name, meta_data_value);
                } else {
                    // Global option. Go's duplicate check is a commented-out
                    // no-op (it would break existing submodule tests), so a
                    // later value simply overwrites an earlier one.
                    global_options.insert(meta_data_name, meta_data_value);
                }
                continue;
            }

            // `@filename` directive: flush the previous unit and start a new one.
            if !current_file_name.is_empty() {
                let should_save_file = !options.allow_implicit_first_file
                    || !current_file_content.is_empty()
                    || has_seen_file;
                if should_save_file {
                    has_seen_file = true;
                    test_units.push(TestUnit {
                        content: current_file_content.clone(),
                        name: current_file_name.clone(),
                    });
                }

                current_file_content.clear();
                seen_content_line = false;
                current_file_name = meta_data_value;
                current_file_options = IndexMap::new();
            } else {
                let has_content_before_first_filename = !current_file_content.is_empty()
                    && skip_trivia(&current_file_content, 0) as usize != current_file_content.len();
                if has_content_before_first_filename && !options.allow_implicit_first_file {
                    panic!(
                        "Non-comment test content appears before the first '// @Filename' directive"
                    );
                }

                if has_content_before_first_filename
                    && options.allow_implicit_first_file
                    && !current_file_name.is_empty()
                {
                    has_seen_file = true;
                    test_units.push(TestUnit {
                        content: current_file_content.clone(),
                        name: current_file_name.clone(),
                    });
                }

                current_file_content.clear();
                seen_content_line = false;
                current_file_name = value_raw.trim().to_string();
                current_file_options = IndexMap::new();
            }
        } else {
            // Subfile content line.
            if options.allow_implicit_first_file {
                if seen_content_line {
                    current_file_content.push('\n');
                }
                seen_content_line = true;
            } else if !current_file_content.is_empty() {
                current_file_content.push('\n');
            }
            current_file_content.push_str(line);
        }
    }

    // Normalize the file name for the single-file case.
    if test_units.is_empty() && current_file_name.is_empty() {
        current_file_name = get_base_file_name(file_name);
    }

    // EOF: push whatever remains.
    test_units.push(TestUnit {
        content: current_file_content,
        name: current_file_name,
    });

    let _ = current_file_options;

    ParsedTestFiles {
        units: test_units,
        symlinks,
        current_directory,
        global_options,
    }
}

/// Builds a [`TestCaseContent`] from a test file containing `// @filename`
/// directives.
///
/// # Examples
/// ```
/// use tsgo_testrunner::make_units_from_test;
/// let content = make_units_from_test(
///     "// @filename: a.ts\nconst x = 1;",
///     "test.ts",
/// );
/// assert_eq!(content.test_unit_data.len(), 1);
/// assert_eq!(content.test_unit_data[0].name, "a.ts");
/// ```
///
/// Side effects: none (pure).
// Go: internal/testrunner/test_case_parser.go:makeUnitsFromTest
pub fn make_units_from_test(code: &str, file_name: &str) -> TestCaseContent {
    let parsed = parse_test_files_and_symlinks(code, file_name);

    let current_directory = if parsed.current_directory.is_empty() {
        SRC_FOLDER.to_string()
    } else {
        get_normalized_absolute_path(&parsed.current_directory, SRC_FOLDER)
    };

    let mut test_unit_data = parsed.units;
    let mut ts_config = None;
    let mut ts_config_file_unit = None;

    // Mirror Go `makeUnitsFromTest`: if a unit is a ts/jsconfig file, parse it
    // and remove it from the compile unit list.
    let all_files: Vec<(String, String)> = test_unit_data
        .iter()
        .map(|u| {
            (
                get_normalized_absolute_path(&u.name, &current_directory),
                u.content.clone(),
            )
        })
        .collect();
    let file_refs: Vec<(&str, &str)> = all_files
        .iter()
        .map(|(name, content)| (name.as_str(), content.as_str()))
        .collect();
    let parse_config_host = VfsParseConfigHost::new(
        &file_refs,
        &current_directory,
        true, /*useCaseSensitiveFileNames*/
    );

    if let Some(index) = test_unit_data
        .iter()
        .position(|u| !get_config_name_from_file_name(&u.name).is_empty())
    {
        let unit = test_unit_data.remove(index);
        let config_file_name = get_normalized_absolute_path(&unit.name, &current_directory);
        let config_dir = get_directory_path(&config_file_name);
        let source_file = new_tsconfig_source_file_from_file_path(&config_file_name, &unit.content);
        ts_config = Some(parse_json_source_file_config_file_content(
            &source_file,
            &parse_config_host,
            &config_dir,
            None,
            &config_file_name,
        ));
        ts_config_file_unit = Some(unit);
    }

    TestCaseContent {
        test_unit_data,
        ts_config,
        ts_config_file_unit,
        symlinks: parsed.symlinks,
    }
}

/// Extracts every `// @<option>: <value>` directive in `content` into a
/// settings map, lowercasing the option name and trimming a trailing `;` from
/// the value.
///
/// # Examples
/// ```
/// use tsgo_testrunner::extract_compiler_settings;
/// let settings = extract_compiler_settings("// @strict: true\n// @target: ES2015;");
/// assert_eq!(settings.get("strict").map(String::as_str), Some("true"));
/// assert_eq!(settings.get("target").map(String::as_str), Some("ES2015"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/testrunner/test_case_parser.go:extractCompilerSettings
pub fn extract_compiler_settings(content: &str) -> RawCompilerSettings {
    let mut opts = RawCompilerSettings::new();
    for line in split_lines(content) {
        if let Some((name, value)) = match_option(&line) {
            let key = name.to_ascii_lowercase();
            let value = value.trim().trim_end_matches(';').to_string();
            opts.insert(key, value);
        }
    }
    opts
}

/// Parses a `// @link: <source> -> <target>` directive into `symlinks`,
/// returning whether `line` was a link directive.
///
/// The map is keyed by the trimmed target (right side) and stores the trimmed
/// source (left side), mirroring Go.
///
/// # Examples
/// ```
/// use indexmap::IndexMap;
/// use tsgo_testrunner::parse_symlink_from_test;
/// let mut symlinks: IndexMap<String, String> = IndexMap::new();
/// assert!(parse_symlink_from_test("// @link: /a -> /b", &mut symlinks));
/// assert_eq!(symlinks.get("/b").map(String::as_str), Some("/a"));
/// assert!(!parse_symlink_from_test("const x = 1;", &mut symlinks));
/// ```
///
/// Side effects: inserts into `symlinks` when `line` is a link directive.
// Go: internal/testrunner/test_case_parser.go:parseSymlinkFromTest
pub fn parse_symlink_from_test(line: &str, symlinks: &mut IndexMap<String, String>) -> bool {
    match match_link(line) {
        Some((source, target)) => {
            symlinks.insert(target.trim().to_string(), source.trim().to_string());
            true
        }
        None => false,
    }
}

/// Splits `code` on `\r?\n`, mirroring Go's `lineDelimiter` regex split.
// Go: internal/testrunner/test_case_parser.go:lineDelimiter
fn split_lines(code: &str) -> Vec<String> {
    code.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect()
}

/// Matches a single line against Go's `optionRegex`
/// (`^// *@(\w+) *: *(rest-of-line)`), returning the option name and raw value
/// when it matches.
// Go: internal/testrunner/test_case_parser.go:optionRegex
fn match_option(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("//")?;
    let rest = rest.trim_start_matches(is_ascii_ws);
    let rest = rest.strip_prefix('@')?;
    // Option name: one or more `\w` characters (ASCII letters, digits, `_`).
    let name_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    if name_end == 0 {
        return None;
    }
    let name = &rest[..name_end];
    let after_name = rest[name_end..].trim_start_matches(is_ascii_ws);
    let value = after_name.strip_prefix(':')?;
    let value = value.trim_start_matches(is_ascii_ws);
    Some((name, value))
}

/// Matches a single line against Go's `linkRegex`
/// (`^// *@link *: *(left) *-> *(right)`), returning the `(source, target)`
/// halves when it matches.
// Go: internal/testrunner/test_case_parser.go:linkRegex
fn match_link(line: &str) -> Option<(&str, &str)> {
    let (name, value) = match_option(line)?;
    if !name.eq_ignore_ascii_case("link") {
        return None;
    }
    let (source, target) = value.split_once("->")?;
    Some((source, target))
}

/// Reports whether `c` is an ASCII whitespace character matched by the regex
/// `\s` class (RE2: `[\t\n\f\r ]`) used in the directive patterns.
fn is_ascii_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{0c}')
}

#[cfg(test)]
#[path = "test_case_parser_test.rs"]
mod tests;
