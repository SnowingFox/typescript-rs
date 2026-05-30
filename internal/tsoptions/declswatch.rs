//! The watch (`watchOptions`) declarations (`OptionsForWatch`).
//!
//! 1:1 port of Go `internal/tsoptions/declswatch.go`. See the divergence note
//! in `declscompiler.rs` about deferred help/showConfig message fields.

use std::sync::LazyLock;

use crate::commandlineoption::{CommandLineOption, CommandLineOptionKind as Kind};

/// The `watchOptions` declarations.
// Go: internal/tsoptions/declswatch.go:OptionsForWatch
pub static OPTIONS_FOR_WATCH: LazyLock<Vec<CommandLineOption>> = LazyLock::new(|| {
    vec![
        CommandLineOption {
            name: "watchInterval",
            kind: Kind::Number,
            ..Default::default()
        },
        CommandLineOption {
            name: "watchFile",
            kind: Kind::Enum, // watchFileEnumMap
            ..Default::default()
        },
        CommandLineOption {
            name: "watchDirectory",
            kind: Kind::Enum, // watchDirectoryEnumMap
            ..Default::default()
        },
        CommandLineOption {
            name: "fallbackPolling",
            kind: Kind::Enum, // fallbackEnumMap
            ..Default::default()
        },
        CommandLineOption {
            name: "synchronousWatchDirectory",
            kind: Kind::Boolean,
            ..Default::default()
        },
        CommandLineOption {
            name: "excludeDirectories",
            kind: Kind::List,
            allow_config_dir_template_substitution: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "excludeFiles",
            kind: Kind::List,
            allow_config_dir_template_substitution: true,
            ..Default::default()
        },
    ]
});

#[cfg(test)]
#[path = "declswatch_test.rs"]
mod tests;
