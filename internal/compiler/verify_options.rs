//! Port of the reachable, pure subset of Go
//! `internal/compiler/program.go:verifyCompilerOptions`.
//!
//! `verifyCompilerOptions` reports option-consistency diagnostics. The Go
//! routine threads each diagnostic onto the tsconfig source node so it can point
//! at the offending option key/value; that source-location machinery (and the
//! rules that need whole-program state such as the common-source-directory and
//! project references) is deferred. This module ports the rules that depend only
//! on [`CompilerOptions`], returning a flat list of [`OptionsDiagnostic`].
//!
//! DEFER(P6): source-located diagnostics (point at the tsconfig node) and the
//! program-state rules (`outDir`/`rootDir` layout, `paths` `*` checks, project
//! references). blocked-by: tsconfig option-syntax (`tsoptions` config-file AST)
//! + `Program` common-source-directory/emit wiring.

use tsgo_core::compileroptions::{CompilerOptions, ModuleKind, ModuleResolutionKind, ScriptTarget};
use tsgo_diagnostics::{self as diagnostics, Message};

/// A single option-consistency diagnostic: the message and its (already
/// stringified) arguments. The source location is deferred (see module docs).
///
/// Side effects: none (pure value type).
// Go: internal/ast/diagnostic.go:Diagnostic (the option-diagnostic subset)
#[derive(Debug, Clone)]
pub struct OptionsDiagnostic {
    /// The diagnostic message.
    pub message: &'static Message,
    /// The stringified message arguments (typically the option names).
    pub args: Vec<String>,
}

impl OptionsDiagnostic {
    fn new(message: &'static Message, args: &[&str]) -> OptionsDiagnostic {
        OptionsDiagnostic {
            message,
            args: args.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// Verifies option consistency, returning the reachable subset of Go's
/// `verifyCompilerOptions` diagnostics (those that depend only on the options).
///
/// # Examples
/// ```
/// use tsgo_compiler::verify_compiler_options;
/// use tsgo_core::compileroptions::CompilerOptions;
/// assert!(verify_compiler_options(&CompilerOptions::default()).is_empty());
/// ```
///
/// Side effects: none (pure).
// Go: internal/compiler/program.go:verifyCompilerOptions
pub fn verify_compiler_options(options: &CompilerOptions) -> Vec<OptionsDiagnostic> {
    let _ = (
        ScriptTarget::Es5,
        ModuleKind::Amd,
        ModuleResolutionKind::Classic,
    );
    let mut diags: Vec<OptionsDiagnostic> = Vec::new();

    // ── Removed options (TS7) ────────────────────────────────────────────────
    if !options.out_file.is_empty() {
        push_removed(&mut diags, "outFile", "");
    }
    if options.target == ScriptTarget::Es5 {
        push_removed(&mut diags, "target", "ES5");
    }
    match options.module {
        ModuleKind::Amd => push_removed(&mut diags, "module", "AMD"),
        ModuleKind::System => push_removed(&mut diags, "module", "System"),
        ModuleKind::Umd => push_removed(&mut diags, "module", "UMD"),
        _ => {}
    }
    match options.module_resolution {
        ModuleResolutionKind::Classic => push_removed(&mut diags, "moduleResolution", "Classic"),
        ModuleResolutionKind::Node10 => push_removed(&mut diags, "moduleResolution", "node10"),
        _ => {}
    }

    // ── Option pair / dependency rules ───────────────────────────────────────
    let strict_null_checks = options.get_strict_option_value(options.strict_null_checks);
    if options.strict_property_initialization.is_true() && !strict_null_checks {
        push_without(
            &mut diags,
            "strictPropertyInitialization",
            "strictNullChecks",
        );
    }
    if options.exact_optional_property_types.is_true() && !strict_null_checks {
        push_without(&mut diags, "exactOptionalPropertyTypes", "strictNullChecks");
    }
    if !options.lib.is_empty() && options.no_lib.is_true() {
        push_with(&mut diags, "lib", "noLib");
    }
    if options.check_js.is_true() && !options.get_allow_js() {
        push_without(&mut diags, "checkJs", "allowJs");
    }
    if options.emit_decorator_metadata.is_true()
        && options.experimental_decorators.is_false_or_unknown()
    {
        push_without(
            &mut diags,
            "emitDecoratorMetadata",
            "experimentalDecorators",
        );
    }

    diags
}

/// Appends `Option_0_cannot_be_specified_without_specifying_option_1`.
///
/// Side effects: pushes onto `diags`.
fn push_without(diags: &mut Vec<OptionsDiagnostic>, option1: &str, option2: &str) {
    diags.push(OptionsDiagnostic::new(
        &diagnostics::OPTION_0_CANNOT_BE_SPECIFIED_WITHOUT_SPECIFYING_OPTION_1,
        &[option1, option2],
    ));
}

/// Appends `Option_0_cannot_be_specified_with_option_1`.
///
/// Side effects: pushes onto `diags`.
fn push_with(diags: &mut Vec<OptionsDiagnostic>, option1: &str, option2: &str) {
    diags.push(OptionsDiagnostic::new(
        &diagnostics::OPTION_0_CANNOT_BE_SPECIFIED_WITH_OPTION_1,
        &[option1, option2],
    ));
}

/// Appends a "removed option" diagnostic: `Option_0_has_been_removed` when
/// `value` is empty, otherwise `Option_0_1_has_been_removed`.
///
/// Side effects: pushes onto `diags`.
// Go: internal/compiler/program.go:createRemovedOptionDiagnostic
fn push_removed(diags: &mut Vec<OptionsDiagnostic>, name: &str, value: &str) {
    if value.is_empty() {
        diags.push(OptionsDiagnostic::new(
            &diagnostics::OPTION_0_HAS_BEEN_REMOVED_PLEASE_REMOVE_IT_FROM_YOUR_CONFIGURATION,
            &[name],
        ));
    } else {
        diags.push(OptionsDiagnostic::new(
            &diagnostics::OPTION_0_1_HAS_BEEN_REMOVED_PLEASE_REMOVE_IT_FROM_YOUR_CONFIGURATION,
            &[name, value],
        ));
    }
}

#[cfg(test)]
#[path = "verify_options_test.rs"]
mod tests;
