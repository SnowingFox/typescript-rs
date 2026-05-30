use super::*;

use crate::commandlineoption::CommandLineOptionKind;

// Go: internal/tsoptions/declswatch.go:OptionsForWatch (behavior-level)
#[test]
fn watch_options_shapes() {
    let by_name = |n: &str| OPTIONS_FOR_WATCH.iter().find(|o| o.name == n).unwrap();
    assert_eq!(by_name("watchFile").kind, CommandLineOptionKind::Enum);
    assert_eq!(by_name("watchInterval").kind, CommandLineOptionKind::Number);
    assert_eq!(
        by_name("synchronousWatchDirectory").kind,
        CommandLineOptionKind::Boolean
    );
    let exclude_dirs = by_name("excludeDirectories");
    assert_eq!(exclude_dirs.kind, CommandLineOptionKind::List);
    assert!(exclude_dirs.allow_config_dir_template_substitution);
}
