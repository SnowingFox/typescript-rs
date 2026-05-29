//! Build options (`BuildOptions`) for `tsc --build`.
//!
//! 1:1 port of Go internal/core/buildoptions.go. This is a pure data struct
//! with no behavior; parsing/validation lands in `tsoptions` (P6).

use crate::tristate::Tristate;

/// Options for the `--build` (project references) mode.
#[derive(Clone, Debug, Default)]
pub struct BuildOptions {
    /// Dry run (report without building).
    pub dry: Tristate,
    /// Force a full rebuild.
    pub force: Tristate,
    /// Verbose output.
    pub verbose: Tristate,
    /// Number of concurrent builders.
    pub builders: Option<i32>,
    /// Stop the build on the first error.
    pub stop_build_on_errors: Tristate,
    /// Clean build outputs.
    pub clean: Tristate,
}
