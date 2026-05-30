use super::*;

use tsgo_core::tristate::Tristate;

// Go: internal/tsoptions/parsedbuildcommandline.go:ParsedBuildCommandLine
#[test]
fn parsed_build_command_line_defaults() {
    let p = ParsedBuildCommandLine::default();
    assert!(p.projects.is_empty());
    assert!(p.errors.is_empty());
    assert_eq!(p.build_options.force, Tristate::Unknown);
    assert_eq!(p.build_options.builders, None);
}
