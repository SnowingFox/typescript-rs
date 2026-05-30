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
}

#[cfg(test)]
#[path = "parsedcommandline_test.rs"]
mod tests;
