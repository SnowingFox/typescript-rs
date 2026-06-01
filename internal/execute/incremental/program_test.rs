use super::*;
use std::sync::Arc;

use tsgo_compiler::{new_compiler_host, new_program, Program as CompilerProgram, ProgramOptions};
use tsgo_core::compileroptions::{CompilerOptions, ModuleKind, ScriptTarget};
use tsgo_core::tristate::Tristate;
use tsgo_tsoptions::new_parsed_command_line;
use tsgo_tspath::ComparePathsOptions;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use crate::get_files_affected_by;

const A_TS: &str = "import { b } from \"./b\";\nexport const a = b + 1;\n";
const B_TS: &str = "export const b = 1;\n";
const B_TS_VERSION: &str = "90312e1cbc42534115cfa9601aa41950";

// The reachable-subset `.tsbuildinfo` the incremental program emits for the
// two-file `--incremental --noLib` project. It matches `cmd/tsgo`'s output for
// this project except for the `errors`/`semanticDiagnosticsPerFile`-error state
// (DEFER: needs checker diagnostics + hasErrors tracking).
const EXPECTED_TSBUILDINFO: &str = concat!(
    r#"{"version":"7.0.0-dev","root":[[1,2]],"fileNames":["./b.ts","./a.ts"],"#,
    r#""fileInfos":["90312e1cbc42534115cfa9601aa41950","b6df5f2b27e276d9e3e67069347c11a5"],"#,
    r#""fileIdsList":[[1]],"options":{"module":99,"target":99},"#,
    r#""referencedMap":[[2,1]],"semanticDiagnosticsPerFile":[1,2]}"#,
);

fn incremental_program() -> CompilerProgram {
    let files = [("/proj/a.ts", A_TS), ("/proj/b.ts", B_TS)];
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(files, true));
    let host = Arc::new(new_compiler_host("/proj", fs, "/lib"));
    let options = CompilerOptions {
        module: ModuleKind::EsNext,
        target: ScriptTarget::EsNext,
        no_lib: Tristate::True,
        incremental: Tristate::True,
        ts_build_info_file: "/proj/tsconfig.tsbuildinfo".to_string(),
        ..CompilerOptions::default()
    };
    let config = new_parsed_command_line(
        options,
        vec!["/proj/a.ts".to_string(), "/proj/b.ts".to_string()],
        ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: "/proj".to_string(),
        },
    );
    new_program(ProgramOptions {
        host,
        config: Arc::new(config),
        single_threaded: true,
    })
}

// Go: internal/execute/incremental/program.go:emitBuildInfo + snapshotToBuildInfo
// A `--incremental` program writes its `.tsbuildinfo` to the expected path, in
// the compact Go shape (1-based ids, bare-string file infos, [2,1] referenced
// map).
#[test]
fn emits_tsbuildinfo_to_expected_path_in_go_shape() {
    let compiler_program = incremental_program();
    let inc = Program::new(&compiler_program);

    let result = inc
        .emit_build_info()
        .expect("incremental program emits a buildinfo");
    assert_eq!(result.file_name, "/proj/tsconfig.tsbuildinfo");
    assert_eq!(result.text, EXPECTED_TSBUILDINFO);

    // It was actually written to the host file system.
    let written = compiler_program
        .host()
        .fs()
        .read_file("/proj/tsconfig.tsbuildinfo")
        .expect("buildinfo file written to host fs");
    assert_eq!(written, EXPECTED_TSBUILDINFO);
}

// Go: internal/outputpaths/outputpaths.go:GetBuildInfoFileName (non-incremental -> "")
#[test]
fn non_incremental_program_emits_nothing() {
    let files = [("/proj/a.ts", A_TS), ("/proj/b.ts", B_TS)];
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(files, true));
    let host = Arc::new(new_compiler_host("/proj", fs, "/lib"));
    let options = CompilerOptions {
        no_lib: Tristate::True,
        ..CompilerOptions::default()
    };
    let config = new_parsed_command_line(
        options,
        vec!["/proj/a.ts".to_string(), "/proj/b.ts".to_string()],
        ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: "/proj".to_string(),
        },
    );
    let compiler_program = new_program(ProgramOptions {
        host,
        config: Arc::new(config),
        single_threaded: true,
    });
    let inc = Program::new(&compiler_program);

    assert_eq!(inc.build_info_file_name(), "");
    assert!(inc.build_info().is_none());
    assert!(inc.emit_build_info().is_none());
}

// Go: internal/execute/incremental/incremental.go:ReadBuildInfoProgram
// Reading the emitted `.tsbuildinfo` back seeds the next build's reuse: the
// prior file versions and import graph are restored, and the affected-files
// walk works off the restored referenced map.
#[test]
fn reads_back_tsbuildinfo_to_seed_reuse() {
    let compiler_program = incremental_program();
    let inc = Program::new(&compiler_program);
    inc.emit_build_info().expect("emits buildinfo");

    let snapshot = read_build_info_program(
        compiler_program.host().as_ref(),
        "/proj/tsconfig.tsbuildinfo",
    )
    .expect("reads the emitted buildinfo back");

    let b_path = compiler_program.to_path("/proj/b.ts");
    let a_path = compiler_program.to_path("/proj/a.ts");

    // Prior version is restored (seeds reuse / change detection).
    assert_eq!(
        snapshot.file_infos.get(&b_path).unwrap().version,
        B_TS_VERSION
    );

    // The import graph is restored: changing b.ts affects {b.ts, a.ts}.
    assert_eq!(
        get_files_affected_by(&snapshot.referenced_map, &b_path),
        vec![a_path.clone(), b_path.clone()]
    );
    // Changing a.ts only affects a.ts.
    assert_eq!(
        get_files_affected_by(&snapshot.referenced_map, &a_path),
        vec![a_path]
    );
}
