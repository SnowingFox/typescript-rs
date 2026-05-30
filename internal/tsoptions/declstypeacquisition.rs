//! The `typeAcquisition` declarations.
//!
//! 1:1 port of Go `internal/tsoptions/declstypeacquisition.go`.

use std::sync::LazyLock;

use crate::commandlineoption::{
    CommandLineOption, CommandLineOptionKind as Kind, CommandLineOptionNameMap, DefaultValue,
};

/// The child option declarations of `typeAcquisition`.
// Go: internal/tsoptions/declstypeacquisition.go:typeAcquisitionDecls
pub fn type_acquisition_decls() -> Vec<CommandLineOption> {
    vec![
        CommandLineOption {
            name: "enable",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "include",
            kind: Kind::List,
            ..Default::default()
        },
        CommandLineOption {
            name: "exclude",
            kind: Kind::List,
            ..Default::default()
        },
        CommandLineOption {
            name: "disableFilenameBasedTypeAcquisition",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
    ]
}

/// The `typeAcquisition` object declaration (its child options form
/// `ElementOptions`).
// Go: internal/tsoptions/declstypeacquisition.go:typeAcquisitionDeclaration
pub static TYPE_ACQUISITION_DECLARATION: LazyLock<CommandLineOption> =
    LazyLock::new(|| CommandLineOption {
        name: "typeAcquisition",
        kind: Kind::Object,
        element_options: Some(Box::new(CommandLineOptionNameMap::from_options(
            &type_acquisition_decls(),
        ))),
        ..Default::default()
    });

#[cfg(test)]
#[path = "declstypeacquisition_test.rs"]
mod tests;
