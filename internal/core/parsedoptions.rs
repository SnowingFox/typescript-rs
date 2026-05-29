//! Parsed configuration bundle (`ParsedOptions`).
//!
//! 1:1 port of Go `internal/core/parsedoptions.go`. A pure data aggregate; the
//! parser that fills it lands in `tsoptions` (P6). Go's pointer fields
//! (`*CompilerOptions`, ...) map to `Option<Box<...>>`.

use crate::compileroptions::CompilerOptions;
use crate::projectreference::ProjectReference;
use crate::typeacquisition::TypeAcquisition;
use crate::watchoptions::WatchOptions;

/// Aggregated parsed options: compiler/watch/type-acquisition settings plus the
/// resolved file list and project references.
#[derive(Clone, Debug, Default)]
pub struct ParsedOptions {
    /// Compiler options, if present.
    pub compiler_options: Option<Box<CompilerOptions>>,
    /// Watch options, if present.
    pub watch_options: Option<Box<WatchOptions>>,
    /// Type-acquisition options, if present.
    pub type_acquisition: Option<Box<TypeAcquisition>>,
    /// Resolved input file names.
    pub file_names: Vec<String>,
    /// Project references (composite build edges).
    pub project_references: Vec<ProjectReference>,
}
