use super::*;

use crate::commandlineoption::CommandLineOptionKind;

// Go: internal/tsoptions/declsbuild.go:BuildOpts (behavior-level)
#[test]
fn build_opts_includes_common_and_build_specific() {
    let names: Vec<&str> = BUILD_OPTS.iter().map(|o| o.name).collect();
    // common option
    assert!(names.contains(&"help"));
    // build-specific options
    assert!(names.contains(&"verbose"));
    assert!(names.contains(&"clean"));
    assert!(names.contains(&"builders"));
}

#[test]
fn tsc_build_option_shape() {
    let b = tsc_build_option();
    assert_eq!(b.name, "build");
    assert_eq!(b.short_name, "b");
    assert_eq!(b.kind, CommandLineOptionKind::Boolean);
}

#[test]
fn builders_has_min_value_one() {
    let builders = OPTIONS_FOR_BUILD.iter().find(|o| o.name == "builders");
    assert_eq!(builders.unwrap().min_value, 1);
}
