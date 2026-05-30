use super::*;

fn opt(name: &'static str, kind: CommandLineOptionKind) -> CommandLineOption {
    CommandLineOption {
        name,
        kind,
        ..Default::default()
    }
}

// Go: internal/tsoptions/commandlineoption.go:CommandLineOption.EnumMap
#[test]
fn enum_map_for_enum_options() {
    let target = opt("target", CommandLineOptionKind::Enum);
    let m = target.enum_map().expect("target has an enum map");
    assert_eq!(m.get(&"es5"), Some(&EnumValue::Int(1)));
    // The `lib` enum map uses string values.
    let lib = opt("lib", CommandLineOptionKind::Enum);
    assert_eq!(
        lib.enum_map().unwrap().get(&"es6"),
        Some(&EnumValue::Str("lib.es2015.d.ts"))
    );
}

#[test]
fn enum_map_none_for_non_enum() {
    assert!(opt("outDir", CommandLineOptionKind::String)
        .enum_map()
        .is_none());
}

// Go: internal/tsoptions/commandlineoption.go:CommandLineOption.Elements
#[test]
fn elements_for_list_options() {
    let lib = opt("lib", CommandLineOptionKind::List);
    let el = lib.elements().expect("lib has element declaration");
    assert_eq!(el.name, "lib");
    assert_eq!(el.kind, CommandLineOptionKind::Enum);

    let root_dirs = opt("rootDirs", CommandLineOptionKind::List);
    let el = root_dirs.elements().unwrap();
    assert_eq!(el.kind, CommandLineOptionKind::String);
    assert!(el.is_file_path);
}

#[test]
fn elements_none_for_non_list() {
    assert!(opt("outDir", CommandLineOptionKind::String)
        .elements()
        .is_none());
}

// Go: internal/tsoptions/commandlineoption.go:CommandLineOption.DeprecatedKeys
#[test]
fn deprecated_keys_for_enum_options() {
    let module = opt("module", CommandLineOptionKind::Enum);
    let keys = module
        .deprecated_keys()
        .expect("module has deprecated keys");
    assert!(keys.has(&"amd"));
    assert!(keys.has(&"system"));
    assert!(!keys.has(&"commonjs"));

    let target = opt("target", CommandLineOptionKind::Enum);
    assert!(target.deprecated_keys().unwrap().has(&"es5"));
}

#[test]
fn deprecated_keys_none_when_not_listed() {
    // jsx is an enum but has no deprecated keys.
    assert!(opt("jsx", CommandLineOptionKind::Enum)
        .deprecated_keys()
        .is_none());
    // A non-enum option never has deprecated keys.
    assert!(opt("module", CommandLineOptionKind::String)
        .deprecated_keys()
        .is_none());
}

// Go: internal/tsoptions/tsconfigparsing.go:CommandLineOptionNameMap.Get
#[test]
fn name_map_get_is_case_insensitive() {
    let m = CommandLineOptionNameMap::from_options(&[
        opt("outDir", CommandLineOptionKind::String),
        opt("strict", CommandLineOptionKind::Boolean),
    ]);
    assert_eq!(m.get("OUTDIR").map(|o| o.name), Some("outDir"));
    assert_eq!(m.get("strict").map(|o| o.name), Some("strict"));
    assert!(m.get("missing").is_none());
}
