use tsgo_core::compileroptions::CompilerOptions;
use tsgo_diagnostics::Message;

use super::*;

/// Returns the first diagnostic carrying `message`, if any.
fn find<'a>(diags: &'a [OptionsDiagnostic], message: &Message) -> Option<&'a OptionsDiagnostic> {
    diags.iter().find(|d| std::ptr::eq(d.message, message))
}

/// Default options are consistent: no diagnostics.
// Go: internal/compiler/program.go:verifyCompilerOptions (no rule fires)
#[test]
fn default_options_are_clean() {
    assert!(verify_compiler_options(&CompilerOptions::default()).is_empty());
}

/// `outFile` is a removed option.
// Go: internal/compiler/program.go:verifyCompilerOptions (outFile removed)
#[test]
fn out_file_is_removed() {
    let options = CompilerOptions {
        out_file: "/dist/bundle.js".to_string(),
        ..Default::default()
    };
    let diags = verify_compiler_options(&options);
    let diag = find(
        &diags,
        &diagnostics::OPTION_0_HAS_BEEN_REMOVED_PLEASE_REMOVE_IT_FROM_YOUR_CONFIGURATION,
    )
    .expect("outFile removed diagnostic");
    assert_eq!(diag.args, vec!["outFile".to_string()]);
}

/// `target: ES5` is a removed option (valued form).
// Go: internal/compiler/program.go:verifyCompilerOptions (target ES5 removed)
#[test]
fn target_es5_is_removed() {
    let options = CompilerOptions {
        target: tsgo_core::compileroptions::ScriptTarget::Es5,
        ..Default::default()
    };
    let diags = verify_compiler_options(&options);
    let diag = find(
        &diags,
        &diagnostics::OPTION_0_1_HAS_BEEN_REMOVED_PLEASE_REMOVE_IT_FROM_YOUR_CONFIGURATION,
    )
    .expect("target ES5 removed diagnostic");
    assert_eq!(diag.args, vec!["target".to_string(), "ES5".to_string()]);
}

/// `module: AMD`, `System`, and `UMD` are removed options (valued form).
// Go: internal/compiler/program.go:verifyCompilerOptions (module AMD/System/UMD removed)
#[test]
fn removed_module_kinds() {
    for (kind, name) in [
        (tsgo_core::compileroptions::ModuleKind::Amd, "AMD"),
        (tsgo_core::compileroptions::ModuleKind::System, "System"),
        (tsgo_core::compileroptions::ModuleKind::Umd, "UMD"),
    ] {
        let options = CompilerOptions {
            module: kind,
            ..Default::default()
        };
        let diags = verify_compiler_options(&options);
        let diag = find(
            &diags,
            &diagnostics::OPTION_0_1_HAS_BEEN_REMOVED_PLEASE_REMOVE_IT_FROM_YOUR_CONFIGURATION,
        )
        .unwrap_or_else(|| panic!("module {name} removed diagnostic"));
        assert_eq!(diag.args, vec!["module".to_string(), name.to_string()]);
    }
}

/// `strictPropertyInitialization` requires `strictNullChecks`.
// Go: internal/compiler/program.go:verifyCompilerOptions (strictPropertyInitialization)
#[test]
fn strict_property_initialization_requires_strict_null_checks() {
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        strict_property_initialization: Tristate::True,
        strict_null_checks: Tristate::False,
        ..Default::default()
    };
    let diags = verify_compiler_options(&options);
    let diag = find(
        &diags,
        &diagnostics::OPTION_0_CANNOT_BE_SPECIFIED_WITHOUT_SPECIFYING_OPTION_1,
    )
    .expect("strictPropertyInitialization diagnostic");
    assert_eq!(
        diag.args,
        vec![
            "strictPropertyInitialization".to_string(),
            "strictNullChecks".to_string()
        ]
    );

    // With `strict: true`, strictNullChecks is implied, so no diagnostic.
    let ok = CompilerOptions {
        strict_property_initialization: Tristate::True,
        strict: Tristate::True,
        ..Default::default()
    };
    assert!(find(
        &verify_compiler_options(&ok),
        &diagnostics::OPTION_0_CANNOT_BE_SPECIFIED_WITHOUT_SPECIFYING_OPTION_1,
    )
    .is_none());
}

/// `lib` cannot be combined with `noLib`.
// Go: internal/compiler/program.go:verifyCompilerOptions (lib + noLib)
#[test]
fn lib_cannot_be_used_with_no_lib() {
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        lib: vec!["lib.es2015.d.ts".to_string()],
        no_lib: Tristate::True,
        ..Default::default()
    };
    let diags = verify_compiler_options(&options);
    let diag = find(
        &diags,
        &diagnostics::OPTION_0_CANNOT_BE_SPECIFIED_WITH_OPTION_1,
    )
    .expect("lib + noLib diagnostic");
    assert_eq!(diag.args, vec!["lib".to_string(), "noLib".to_string()]);
}

/// `checkJs` requires `allowJs`: `checkJs` normally *implies* `allowJs`, so the
/// diagnostic only fires when `allowJs` is explicitly `false`.
// Go: internal/compiler/program.go:verifyCompilerOptions (checkJs + allowJs)
#[test]
fn check_js_requires_allow_js() {
    use tsgo_core::tristate::Tristate;
    // checkJs alone implies allowJs => no diagnostic.
    let implied = CompilerOptions {
        check_js: Tristate::True,
        ..Default::default()
    };
    assert!(find(
        &verify_compiler_options(&implied),
        &diagnostics::OPTION_0_CANNOT_BE_SPECIFIED_WITHOUT_SPECIFYING_OPTION_1,
    )
    .is_none());

    let options = CompilerOptions {
        check_js: Tristate::True,
        allow_js: Tristate::False,
        ..Default::default()
    };
    let diags = verify_compiler_options(&options);
    let diag = find(
        &diags,
        &diagnostics::OPTION_0_CANNOT_BE_SPECIFIED_WITHOUT_SPECIFYING_OPTION_1,
    )
    .expect("checkJs diagnostic");
    assert_eq!(
        diag.args,
        vec!["checkJs".to_string(), "allowJs".to_string()]
    );
}

/// `emitDecoratorMetadata` requires `experimentalDecorators`.
// Go: internal/compiler/program.go:verifyCompilerOptions (emitDecoratorMetadata)
#[test]
fn emit_decorator_metadata_requires_experimental_decorators() {
    use tsgo_core::tristate::Tristate;
    let options = CompilerOptions {
        emit_decorator_metadata: Tristate::True,
        ..Default::default()
    };
    let diags = verify_compiler_options(&options);
    let diag = find(
        &diags,
        &diagnostics::OPTION_0_CANNOT_BE_SPECIFIED_WITHOUT_SPECIFYING_OPTION_1,
    )
    .expect("emitDecoratorMetadata diagnostic");
    assert_eq!(
        diag.args,
        vec![
            "emitDecoratorMetadata".to_string(),
            "experimentalDecorators".to_string()
        ]
    );
}
