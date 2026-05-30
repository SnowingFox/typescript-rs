use super::*;

use tsgo_core::compileroptions::{CompilerOptions, ScriptTarget};
use tsgo_tspath::ComparePathsOptions;

// Go: internal/tsoptions/parsedcommandline.go:NewParsedCommandLine
#[test]
fn new_parsed_command_line_exposes_options_and_files() {
    let co = CompilerOptions {
        target: ScriptTarget::Es2020,
        ..Default::default()
    };
    let pcl = new_parsed_command_line(
        co,
        vec!["a.ts".to_string(), "b.ts".to_string()],
        ComparePathsOptions {
            use_case_sensitive_file_names: true,
            current_directory: "/p".to_string(),
        },
    );
    assert_eq!(pcl.compiler_options().target, ScriptTarget::Es2020);
    assert_eq!(pcl.file_names(), &["a.ts".to_string(), "b.ts".to_string()]);
    assert!(pcl.errors().is_empty());
}
