use super::*;

use crate::commandlineoption::CommandLineOptionKind;

// Go: internal/tsoptions/commandlineparser_test.go:TestAffectsBuildInfo
// Every option that affects semantic diagnostics must also affect build info.
#[test]
fn affects_buildinfo_superset_of_semantic() {
    for decl in OPTIONS_DECLARATIONS.iter() {
        if decl.affects_semantic_diagnostics {
            assert!(
                decl.affects_build_info,
                "option {} affects semantic diagnostics but not build info",
                decl.name
            );
        }
    }
}

#[test]
fn options_declarations_contains_known_options() {
    let map = &*COMMAND_LINE_COMPILER_OPTIONS_MAP;
    let target = map.get("target").expect("target present");
    assert_eq!(target.kind, CommandLineOptionKind::Enum);
    assert!(target.affects_emit);
    let composite = map.get("composite").expect("composite present");
    assert!(composite.is_tsconfig_only);
    let builders = OPTIONS_DECLARATIONS.iter().find(|o| o.name == "checkers");
    assert_eq!(builders.unwrap().min_value, 1);
}
