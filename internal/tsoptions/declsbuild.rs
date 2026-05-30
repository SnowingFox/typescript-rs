//! The `tsc --build` option declarations (`TscBuildOption` + `OptionsForBuild`
//! = `BuildOpts`, with `commonOptionsWithBuild` prepended).
//!
//! 1:1 port of Go `internal/tsoptions/declsbuild.go`. See the divergence note
//! in `declscompiler.rs` about deferred help/showConfig message fields.

use std::sync::LazyLock;

use crate::commandlineoption::{CommandLineOption, CommandLineOptionKind as Kind, DefaultValue};
use crate::declscompiler::COMMON_OPTIONS_WITH_BUILD;

/// The `--build`/`-b` option declaration.
// Go: internal/tsoptions/declsbuild.go:TscBuildOption
pub fn tsc_build_option() -> CommandLineOption {
    CommandLineOption {
        name: "build",
        kind: Kind::Boolean,
        short_name: "b",
        show_in_simplified_help_view: true,
        default_value_description: DefaultValue::Bool(false),
        ..Default::default()
    }
}

fn options_for_build() -> Vec<CommandLineOption> {
    vec![
        tsc_build_option(),
        CommandLineOption {
            name: "verbose",
            short_name: "v",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "dry",
            short_name: "d",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "force",
            short_name: "f",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "clean",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "builders",
            kind: Kind::Number,
            min_value: 1,
            ..Default::default()
        },
        CommandLineOption {
            name: "stopBuildOnErrors",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
    ]
}

/// The build-only option declarations (not common with the compiler).
// Go: internal/tsoptions/declsbuild.go:OptionsForBuild
pub static OPTIONS_FOR_BUILD: LazyLock<Vec<CommandLineOption>> = LazyLock::new(options_for_build);

/// All `tsc --build` option declarations (`commonOptionsWithBuild` then
/// `OptionsForBuild`).
// Go: internal/tsoptions/declsbuild.go:BuildOpts
pub static BUILD_OPTS: LazyLock<Vec<CommandLineOption>> = LazyLock::new(|| {
    let mut v = COMMON_OPTIONS_WITH_BUILD.clone();
    v.extend(options_for_build());
    v
});

#[cfg(test)]
#[path = "declsbuild_test.rs"]
mod tests;
