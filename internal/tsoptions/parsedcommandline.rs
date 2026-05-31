//! The result of parsing a command line or tsconfig: `ParsedCommandLine`.
//!
//! Partial 1:1 port of Go `internal/tsoptions/parsedcommandline.go`. The core
//! data (parsed options, file names, errors, raw) and the constructor are
//! ported here; the lazy-cached derivations (`wildcardDirectories`, output file
//! names, project-reference resolution, `PossiblyMatchesFileName`, ...) depend
//! on the downstream `module`/`outputpaths`/`glob` integration and the tsconfig
//! `ConfigFileSpecs`, and are deferred.

use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::parsedoptions::ParsedOptions;
use tsgo_core::watchoptions::WatchOptions;
use tsgo_parser::Diagnostic;
use tsgo_tspath::ComparePathsOptions;

use crate::OptionValue;

/// File glob pattern for a non-recursive directory.
// Go: internal/tsoptions/parsedcommandline.go:fileGlobPattern
pub const FILE_GLOB_PATTERN: &str = "*.{js,jsx,mjs,cjs,ts,tsx,mts,cts,json}";
/// File glob pattern for a recursive directory.
// Go: internal/tsoptions/parsedcommandline.go:recursiveFileGlobPattern
pub const RECURSIVE_FILE_GLOB_PATTERN: &str = "**/*.{js,jsx,mjs,cjs,ts,tsx,mts,cts,json}";

/// The fully parsed configuration: compiler/watch options, file names, project
/// references, raw JSON, and diagnostics.
///
/// Side effects: none (pure value type).
// Go: internal/tsoptions/parsedcommandline.go:ParsedCommandLine
#[derive(Clone, Debug, Default)]
pub struct ParsedCommandLine {
    /// The parsed options bundle (compiler/watch/typeAcquisition + file names).
    pub parsed_config: ParsedOptions,
    /// Diagnostics produced during parsing.
    pub errors: Vec<Diagnostic>,
    /// The raw JSON / options map.
    pub raw: OptionValue,
    /// The `compileOnSave` value, if present.
    pub compile_on_save: Option<bool>,
    /// Path comparison options (case sensitivity + current directory).
    pub compare_paths_options: ComparePathsOptions,
    /// The validated `files`/`include`/`exclude` specs from a config file
    /// (`None` for command-line results). Populated by the tsconfig worker.
    pub config_file_specs: Option<ConfigFileSpecs>,
    /// The number of leading entries in [`ParsedOptions::file_names`] that came
    /// from the literal `files` list (the rest are wildcard matches).
    pub literal_file_names_len: usize,
}

/// The validated `files`/`include`/`exclude` specifications of a parsed config.
///
/// 1:1 port of Go `internal/tsoptions/tsconfigparsing.go:configFileSpecs`. The
/// `validated*` lists are used for file-name matching; the unvalidated
/// `*_specs` lists are retained for error reporting.
///
/// Side effects: none (pure value type).
// Go: internal/tsoptions/tsconfigparsing.go:configFileSpecs
#[derive(Clone, Debug, Default)]
pub struct ConfigFileSpecs {
    /// The raw `files` specs (for error reporting).
    pub files_specs: Option<Vec<OptionValue>>,
    /// The raw `include` specs (for error reporting).
    pub include_specs: Option<Vec<OptionValue>>,
    /// The raw `exclude` specs (for error reporting).
    pub exclude_specs: Option<Vec<OptionValue>>,
    /// The validated `files` specs used for matching.
    pub validated_files_spec: Vec<String>,
    /// The validated `include` specs used for matching.
    pub validated_include_specs: Vec<String>,
    /// The validated `exclude` specs used for matching.
    pub validated_exclude_specs: Vec<String>,
    /// The validated `files` specs before `${configDir}` substitution.
    pub validated_files_spec_before_substitution: Vec<String>,
    /// The validated `include` specs before `${configDir}` substitution.
    pub validated_include_specs_before_substitution: Vec<String>,
    /// Whether `include` defaulted to `["**/*"]`.
    pub is_default_include_spec: bool,
}

/// Constructs a [`ParsedCommandLine`] from compiler options and root file names.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/parsedcommandline.go:NewParsedCommandLine
pub fn new_parsed_command_line(
    compiler_options: CompilerOptions,
    root_file_names: Vec<String>,
    compare_paths_options: ComparePathsOptions,
) -> ParsedCommandLine {
    ParsedCommandLine {
        parsed_config: ParsedOptions {
            compiler_options: Some(Box::new(compiler_options)),
            file_names: root_file_names,
            ..Default::default()
        },
        compare_paths_options,
        ..Default::default()
    }
}

impl ParsedCommandLine {
    /// Returns the parsed compiler options.
    ///
    /// Side effects: none (pure).
    // Go: internal/tsoptions/parsedcommandline.go:ParsedCommandLine.CompilerOptions
    pub fn compiler_options(&self) -> &CompilerOptions {
        self.parsed_config
            .compiler_options
            .as_deref()
            .expect("compiler options always set by the parser")
    }

    /// Returns the parsed watch options, if any.
    ///
    /// Side effects: none (pure).
    pub fn watch_options(&self) -> Option<&WatchOptions> {
        self.parsed_config.watch_options.as_deref()
    }

    /// Returns the resolved input file names.
    ///
    /// Side effects: none (pure).
    pub fn file_names(&self) -> &[String] {
        &self.parsed_config.file_names
    }

    /// Returns the parsing diagnostics.
    ///
    /// Side effects: none (pure).
    pub fn errors(&self) -> &[Diagnostic] {
        &self.errors
    }

    /// Returns the literal file names (the leading `files`-listed entries),
    /// excluding wildcard matches. Empty for command-line results.
    ///
    /// Side effects: none (pure).
    // Go: internal/tsoptions/parsedcommandline.go:ParsedCommandLine.LiteralFileNames
    pub fn literal_file_names(&self) -> &[String] {
        if self.config_file_specs.is_some() {
            &self.parsed_config.file_names[..self.literal_file_names_len]
        } else {
            &[]
        }
    }
}

#[cfg(test)]
#[path = "parsedcommandline_test.rs"]
mod tests;
