use super::*;

use crate::commandlineoption::CommandLineOptionKind;

// Go: internal/tsoptions/namemap.go:GetNameMapFromList / NameMap.Get
#[test]
fn compiler_name_map_lookups() {
    let nm = &*COMPILER_NAME_MAP;
    assert_eq!(nm.get("strict").map(|o| o.name), Some("strict"));
    assert_eq!(nm.get("STRICT").map(|o| o.name), Some("strict"));
    assert!(nm.get("notARealOption").is_none());
}

// Go: internal/tsoptions/namemap.go:NameMap.GetFromShort
#[test]
fn short_name_lookup() {
    let nm = &*COMPILER_NAME_MAP;
    assert_eq!(nm.get_from_short("t").map(|o| o.name), Some("target"));
    assert_eq!(nm.get_from_short("p").map(|o| o.name), Some("project"));
    // A non-short name is not resolved via get_from_short.
    assert!(nm.get_from_short("target").is_none());
}

// Go: internal/tsoptions/namemap.go:NameMap.GetOptionDeclarationFromName
#[test]
fn get_option_declaration_from_name_translates_short() {
    let nm = &*COMPILER_NAME_MAP;
    assert_eq!(
        nm.get_option_declaration_from_name("t", true)
            .map(|o| o.name),
        Some("target")
    );
    // allow_short=false does not translate the short alias.
    assert!(nm.get_option_declaration_from_name("t", false).is_none());
    assert_eq!(
        nm.get_option_declaration_from_name("target", false)
            .map(|o| o.kind),
        Some(CommandLineOptionKind::Enum)
    );
}

// Go: internal/tsoptions/namemap.go:BuildNameMap
#[test]
fn build_name_map_has_build_and_common() {
    let nm = &*BUILD_NAME_MAP;
    assert_eq!(nm.get("verbose").map(|o| o.name), Some("verbose"));
    assert_eq!(nm.get_from_short("b").map(|o| o.name), Some("build"));
}
