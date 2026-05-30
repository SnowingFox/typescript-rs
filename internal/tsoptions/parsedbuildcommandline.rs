//! The result of parsing `tsc -b`: `ParsedBuildCommandLine`.
//!
//! 1:1 port of Go `internal/tsoptions/parsedbuildcommandline.go` (core data;
//! the `resolvedProjectPaths`/`locale` lazy caches are deferred with the rest
//! of the project-reference resolution).

use tsgo_core::buildoptions::BuildOptions;
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::watchoptions::WatchOptions;
use tsgo_parser::Diagnostic;
use tsgo_tspath::ComparePathsOptions;

use crate::OptionValue;

/// The parsed `tsc -b` command line: build/compiler/watch options, the projects
/// to build, raw options, and diagnostics.
///
/// Side effects: none (pure value type).
// Go: internal/tsoptions/parsedbuildcommandline.go:ParsedBuildCommandLine
#[derive(Clone, Debug, Default)]
pub struct ParsedBuildCommandLine {
    /// The build options.
    pub build_options: BuildOptions,
    /// The compiler options carried over from the command line.
    pub compiler_options: CompilerOptions,
    /// The watch options.
    pub watch_options: WatchOptions,
    /// The projects to build (defaults to `["."]`).
    pub projects: Vec<String>,
    /// Diagnostics produced during parsing.
    pub errors: Vec<Diagnostic>,
    /// The raw options map.
    pub raw: OptionValue,
    /// Path comparison options.
    pub compare_paths_options: ComparePathsOptions,
}

#[cfg(test)]
#[path = "parsedbuildcommandline_test.rs"]
mod tests;
