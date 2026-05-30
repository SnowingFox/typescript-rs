use std::sync::Arc;

use tsgo_core::compileroptions::CompilerOptions;
use tsgo_tsoptions::{new_parsed_command_line, ParsedCommandLine};
use tsgo_tspath::ComparePathsOptions;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use super::*;
use crate::host::new_compiler_host;
use crate::program::ProgramOptions;

fn opts_for(files: &[(&str, &str)], cwd: &str, roots: &[&str]) -> ProgramOptions {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(files.iter().copied(), true));
    let host = Arc::new(new_compiler_host(cwd, fs, "/lib"));
    let config: ParsedCommandLine = new_parsed_command_line(
        CompilerOptions::default(),
        roots.iter().map(|s| s.to_string()).collect(),
        ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: cwd.to_string(),
        },
    );
    ProgramOptions {
        host,
        config: Arc::new(config),
        single_threaded: true,
    }
}

/// Tracer bullet: loading a program of one root file with no imports yields
/// exactly that file.
// Go: internal/compiler/fileloader.go:processAllProgramFiles (single root)
#[test]
fn loads_single_root_file() {
    let opts = opts_for(
        &[("/src/index.ts", "export const x = 1;")],
        "/src",
        &["/src/index.ts"],
    );
    let processed = process_all_program_files(&opts, true);
    let names: Vec<&str> = processed.files().iter().map(|f| f.file_name()).collect();
    assert_eq!(names, vec!["/src/index.ts"]);
    assert!(processed.missing_files().is_empty());
}

/// A root file the host cannot read is recorded in `missing_files`, not in the
/// loaded file set.
// Go: internal/compiler/fileloader.go:filesParser.getProcessedFiles (missingFiles)
#[test]
fn records_missing_root_file() {
    let opts = opts_for(&[("/src/index.ts", "")], "/src", &["/src/gone.ts"]);
    let processed = process_all_program_files(&opts, true);
    assert!(processed.files().is_empty());
    assert_eq!(processed.missing_files(), &["/src/gone.ts".to_string()]);
}

/// Multiple root files load in the order they are listed.
// Go: internal/compiler/fileloader.go:processAllProgramFiles (root order)
#[test]
fn loads_multiple_roots_in_order() {
    let opts = opts_for(
        &[("/src/a.ts", ""), ("/src/b.ts", "")],
        "/src",
        &["/src/b.ts", "/src/a.ts"],
    );
    let processed = process_all_program_files(&opts, true);
    let names: Vec<&str> = processed.files().iter().map(|f| f.file_name()).collect();
    assert_eq!(names, vec!["/src/b.ts", "/src/a.ts"]);
}

fn opts_with_options(
    files: &[(&str, &str)],
    cwd: &str,
    roots: &[&str],
    options: CompilerOptions,
) -> ProgramOptions {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(files.iter().copied(), true));
    let host = Arc::new(new_compiler_host(cwd, fs, "/lib"));
    let config = new_parsed_command_line(
        options,
        roots.iter().map(|s| s.to_string()).collect(),
        ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: cwd.to_string(),
        },
    );
    ProgramOptions {
        host,
        config: Arc::new(config),
        single_threaded: true,
    }
}

/// A root file importing a resolvable relative module pulls that module into the
/// program, ordered before its importer (imports are collected depth-first).
// Go: internal/compiler/fileloader.go:resolveImportsAndModuleAugmentations
#[test]
fn loads_resolved_relative_import() {
    let options = CompilerOptions {
        module_resolution: tsgo_core::compileroptions::ModuleResolutionKind::Bundler,
        ..Default::default()
    };
    let opts = opts_with_options(
        &[
            ("/src/index.ts", "import * as a from \"./a\";"),
            ("/src/a.ts", "export const a = 1;"),
        ],
        "/src",
        &["/src/index.ts"],
        options,
    );
    let processed = process_all_program_files(&opts, true);
    let names: Vec<&str> = processed.files().iter().map(|f| f.file_name()).collect();
    assert_eq!(names, vec!["/src/a.ts", "/src/index.ts"]);
}

/// `import_syntax_affects_module_resolution` is true under node16+ resolution or
/// when `package.json` exports/imports are consulted (the default), and false
/// only when both are disabled and resolution is not node16+.
// Go: internal/compiler/fileloader.go:importSyntaxAffectsModuleResolution
#[test]
fn import_syntax_affects_module_resolution_predicate() {
    use tsgo_core::compileroptions::ModuleResolutionKind;
    use tsgo_core::tristate::Tristate;

    // Default: package.json exports/imports are consulted => affects.
    assert!(import_syntax_affects_module_resolution(
        &CompilerOptions::default()
    ));

    // node16 resolution => affects, even with exports/imports disabled.
    let node16 = CompilerOptions {
        module_resolution: ModuleResolutionKind::Node16,
        resolve_package_json_exports: Tristate::False,
        resolve_package_json_imports: Tristate::False,
        ..Default::default()
    };
    assert!(import_syntax_affects_module_resolution(&node16));

    // Bundler + exports/imports disabled => does not affect.
    let bundler = CompilerOptions {
        module_resolution: ModuleResolutionKind::Bundler,
        resolve_package_json_exports: Tristate::False,
        resolve_package_json_imports: Tristate::False,
        ..Default::default()
    };
    assert!(!import_syntax_affects_module_resolution(&bundler));
}

/// An import cycle between two files is loaded once each and remains
/// deterministically ordered.
// Go: internal/compiler/fileloader.go (cycle through resolveImportsAndModuleAugmentations)
#[test]
fn loads_import_cycle_once() {
    let options = CompilerOptions {
        module_resolution: tsgo_core::compileroptions::ModuleResolutionKind::Bundler,
        ..Default::default()
    };
    let opts = opts_with_options(
        &[
            ("/src/index.ts", "import * as a from \"./a\";"),
            ("/src/a.ts", "import * as i from \"./index\";"),
        ],
        "/src",
        &["/src/index.ts"],
        options,
    );
    let processed = process_all_program_files(&opts, true);
    let names: Vec<&str> = processed.files().iter().map(|f| f.file_name()).collect();
    assert_eq!(names, vec!["/src/a.ts", "/src/index.ts"]);
    assert_eq!(processed.files().len(), 2);
}
