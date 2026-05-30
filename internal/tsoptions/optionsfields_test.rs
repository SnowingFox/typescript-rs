use super::*;

use std::collections::HashMap;

use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::tristate::Tristate;

use crate::declscompiler::OPTIONS_DECLARATIONS;

// Go: internal/tsoptions/decls_test.go:TestCompilerOptionsDeclaration
// Every CompilerOptions field maps to a declaration (or a known internal/skipped
// option), and every declaration maps back to a field. The JSON name must match
// the declaration name.
#[test]
fn compiler_options_declaration_bijection() {
    let mut decls: HashMap<String, &CommandLineOption> = HashMap::new();
    for decl in OPTIONS_DECLARATIONS.iter() {
        decls.insert(decl.name.to_lowercase(), decl);
    }

    // Internal options have a CompilerOptions field but no declaration.
    let internal_options = [
        "allowNonTsExtensions",
        "build",
        "configFilePath",
        "noDtsResolution",
        "noEmitForJsFiles",
        "pathsBasePath",
        "suppressOutputPathCheck",
    ];
    let internal_map: HashMap<String, &str> = internal_options
        .iter()
        .map(|o| (o.to_lowercase(), *o))
        .collect();

    for (go_name, json_name) in compiler_option_field_names() {
        let lower = go_name.to_lowercase();
        match decls.remove(&lower) {
            Some(decl) => {
                assert_eq!(
                    json_name, decl.name,
                    "field {go_name} json name {json_name} != decl name {}",
                    decl.name
                );
            }
            None => {
                if let Some(internal_name) = internal_map.get(&lower) {
                    assert_eq!(json_name, *internal_name, "internal field {go_name}");
                } else {
                    panic!("CompilerOptions.{go_name} has no options declaration");
                }
            }
        }
    }

    // `plugins` is a declaration with no CompilerOptions field.
    decls.remove("plugins");

    let leftover: Vec<&str> = decls.values().map(|d| d.name).collect();
    assert!(
        leftover.is_empty(),
        "declarations not present in CompilerOptions: {leftover:?}"
    );
}

// Go: internal/tsoptions/declscompiler.go:CompilerOptionsAffectSemanticDiagnostics
#[test]
fn affects_semantic_diagnostics_detects_strict_change() {
    let old = CompilerOptions::default();
    // `noUnusedLocals` affects semantic diagnostics and is not a strict flag,
    // so toggling it is a direct change.
    let new = CompilerOptions {
        no_unused_locals: Tristate::True,
        ..Default::default()
    };
    assert!(crate::compiler_options_affect_semantic_diagnostics(
        &old, &new
    ));

    // A strict flag is compared by effective value: with `strict` unset,
    // `strictNullChecks` already defaults to true, so setting it true is a no-op.
    let new_strict = CompilerOptions {
        strict_null_checks: Tristate::True,
        ..Default::default()
    };
    assert!(!crate::compiler_options_affect_semantic_diagnostics(
        &old,
        &new_strict
    ));

    // Changing outDir affects emit but not semantic diagnostics.
    let new_emit = CompilerOptions {
        out_dir: "bin".into(),
        ..Default::default()
    };
    assert!(!crate::compiler_options_affect_semantic_diagnostics(
        &old, &new_emit
    ));
    assert!(crate::compiler_options_affect_emit(&old, &new_emit));
}

// Go: internal/tsoptions/declscompiler.go:CompilerOptionsAffectDeclarationPath
#[test]
fn affects_declaration_path_detects_outdir() {
    let old = CompilerOptions::default();
    let new = CompilerOptions {
        declaration_dir: "types".into(),
        ..Default::default()
    };
    assert!(crate::compiler_options_affect_declaration_path(&old, &new));
}

#[test]
fn for_each_returns_false_when_no_change() {
    let a = CompilerOptions {
        out_dir: "bin".into(),
        ..Default::default()
    };
    let b = CompilerOptions {
        out_dir: "bin".into(),
        ..Default::default()
    };
    assert!(!crate::compiler_options_affect_emit(&a, &b));
}
