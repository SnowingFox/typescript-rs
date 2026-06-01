//! Test-only helpers shared by the language-service feature tests: a minimal
//! in-memory [`LanguageServiceHost`] and a [`LanguageService`] builder over a
//! single-threaded program loaded from an in-memory file system (no default
//! lib), the same way the compiler tests build a program.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use tsgo_compiler::{new_compiler_host, new_program, ProgramOptions};
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_tsoptions::new_parsed_command_line;
use tsgo_tspath::{to_path, ComparePathsOptions};
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use crate::host::LanguageServiceHost;
use crate::LanguageService;

/// A minimal in-memory [`LanguageServiceHost`]: it answers
/// `read_file`/`use_case_sensitive_file_names` from an owned snapshot, standing
/// in for the project layer (P8).
pub(crate) struct MockHost {
    files: HashMap<String, String>,
    case_sensitive: bool,
}

impl LanguageServiceHost for MockHost {
    fn use_case_sensitive_file_names(&self) -> bool {
        self.case_sensitive
    }

    fn read_file(&self, file_name: &str) -> Option<String> {
        self.files.get(file_name).cloned()
    }
}

/// Builds a [`LanguageService`] over a single-threaded program loaded from the
/// `files` map (rooted at `cwd`, with `roots` as the program's root files).
pub(crate) fn build_service(files: &[(&str, &str)], cwd: &str, roots: &[&str]) -> LanguageService {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(files.iter().copied(), true));
    let host = Arc::new(new_compiler_host(cwd, fs, "/lib"));
    let config = new_parsed_command_line(
        CompilerOptions::default(),
        roots.iter().map(|s| s.to_string()).collect(),
        ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: cwd.to_string(),
        },
    );
    let program = new_program(ProgramOptions {
        host,
        config: Arc::new(config),
        single_threaded: true,
    });
    let ls_host: Rc<dyn LanguageServiceHost> = Rc::new(MockHost {
        files: files
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        case_sensitive: true,
    });
    LanguageService::new(to_path(cwd, cwd, true), program, ls_host)
}
