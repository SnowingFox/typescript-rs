//! "Did you mean" / alternate-mode / type-mismatch diagnostic bundles used by
//! the command-line parser.
//!
//! 1:1 port of Go `internal/tsoptions/diagnostics.go`. Go's package-level `var`s
//! with a `sync.Once` become [`LazyLock`] statics; the alternate-mode name map
//! references the global [`NameMap`] statics by `&'static` borrow.

use std::sync::LazyLock;

use tsgo_diagnostics::{self as diagnostics, Message};

use crate::commandlineoption::CommandLineOption;
use crate::declsbuild::BUILD_OPTS;
use crate::declscompiler::OPTIONS_DECLARATIONS;
use crate::declswatch::OPTIONS_FOR_WATCH;
use crate::namemap::{NameMap, BUILD_NAME_MAP, COMPILER_NAME_MAP};

/// The alternate-mode hint: an unknown option may belong to the other mode
/// (`tsc` vs `tsc -b`).
///
/// Side effects: none (pure value type).
// Go: internal/tsoptions/diagnostics.go:AlternateModeDiagnostics
pub struct AlternateModeDiagnostics {
    /// The diagnostic to report when the option belongs to the other mode.
    pub diagnostic: &'static Message,
    /// The name map of the other mode's options.
    pub options_name_map: &'static NameMap,
}

/// The unknown-option / did-you-mean diagnostics plus the option declarations
/// and optional alternate mode.
///
/// Side effects: none (pure value type).
// Go: internal/tsoptions/diagnostics.go:DidYouMeanOptionsDiagnostics
pub struct DidYouMeanOptionsDiagnostics {
    /// Alternate-mode hint, if any.
    pub alternate_mode: Option<AlternateModeDiagnostics>,
    /// The declarations valid in this mode.
    pub option_declarations: Vec<CommandLineOption>,
    /// Message for an unknown option.
    pub unknown_option_diagnostic: &'static Message,
    /// Message for an unknown option with a spelling suggestion.
    pub unknown_did_you_mean_diagnostic: &'static Message,
}

/// The full diagnostic bundle a command-line parse uses.
///
/// Side effects: none (pure value type).
// Go: internal/tsoptions/diagnostics.go:ParseCommandLineWorkerDiagnostics
pub struct ParseCommandLineWorkerDiagnostics {
    /// The did-you-mean bundle.
    pub did_you_mean: DidYouMeanOptionsDiagnostics,
    /// Message for an option missing its argument.
    pub option_type_mismatch_diagnostic: &'static Message,
}

/// Builds the worker diagnostics for compiler mode over the given declarations.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/diagnostics.go:getParseCommandLineWorkerDiagnostics
pub fn get_parse_command_line_worker_diagnostics(
    decls: Vec<CommandLineOption>,
) -> ParseCommandLineWorkerDiagnostics {
    ParseCommandLineWorkerDiagnostics {
        did_you_mean: DidYouMeanOptionsDiagnostics {
            alternate_mode: Some(AlternateModeDiagnostics {
                diagnostic: &diagnostics::COMPILER_OPTION_0_MAY_ONLY_BE_USED_WITH_BUILD,
                options_name_map: &BUILD_NAME_MAP,
            }),
            option_declarations: decls,
            unknown_option_diagnostic: &diagnostics::UNKNOWN_COMPILER_OPTION_0,
            unknown_did_you_mean_diagnostic: &diagnostics::UNKNOWN_COMPILER_OPTION_0_DID_YOU_MEAN_1,
        },
        option_type_mismatch_diagnostic: &diagnostics::COMPILER_OPTION_0_EXPECTS_AN_ARGUMENT,
    }
}

/// Compiler-mode command-line diagnostics.
// Go: internal/tsoptions/diagnostics.go:CompilerOptionsDidYouMeanDiagnostics
pub static COMPILER_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS: LazyLock<ParseCommandLineWorkerDiagnostics> =
    LazyLock::new(|| get_parse_command_line_worker_diagnostics(OPTIONS_DECLARATIONS.clone()));

/// Watch-mode command-line diagnostics (no alternate mode).
// Go: internal/tsoptions/diagnostics.go:watchOptionsDidYouMeanDiagnostics
pub static WATCH_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS: LazyLock<ParseCommandLineWorkerDiagnostics> =
    LazyLock::new(|| ParseCommandLineWorkerDiagnostics {
        did_you_mean: DidYouMeanOptionsDiagnostics {
            alternate_mode: None,
            option_declarations: OPTIONS_FOR_WATCH.clone(),
            unknown_option_diagnostic: &diagnostics::UNKNOWN_WATCH_OPTION_0,
            unknown_did_you_mean_diagnostic: &diagnostics::UNKNOWN_WATCH_OPTION_0_DID_YOU_MEAN_1,
        },
        option_type_mismatch_diagnostic: &diagnostics::WATCH_OPTION_0_REQUIRES_A_VALUE_OF_TYPE_1,
    });

/// Build-mode (`tsc -b`) command-line diagnostics.
// Go: internal/tsoptions/diagnostics.go:buildOptionsDidYouMeanDiagnostics
pub static BUILD_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS: LazyLock<ParseCommandLineWorkerDiagnostics> =
    LazyLock::new(|| ParseCommandLineWorkerDiagnostics {
        did_you_mean: DidYouMeanOptionsDiagnostics {
            alternate_mode: Some(AlternateModeDiagnostics {
                diagnostic: &diagnostics::COMPILER_OPTION_0_MAY_NOT_BE_USED_WITH_BUILD,
                options_name_map: &COMPILER_NAME_MAP,
            }),
            option_declarations: BUILD_OPTS.clone(),
            unknown_option_diagnostic: &diagnostics::UNKNOWN_BUILD_OPTION_0,
            unknown_did_you_mean_diagnostic: &diagnostics::UNKNOWN_BUILD_OPTION_0_DID_YOU_MEAN_1,
        },
        option_type_mismatch_diagnostic: &diagnostics::BUILD_OPTION_0_REQUIRES_A_VALUE_OF_TYPE_1,
    });

#[cfg(test)]
#[path = "diagnostics_test.rs"]
mod tests;
