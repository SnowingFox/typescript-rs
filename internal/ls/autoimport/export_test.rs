use super::*;
use tsgo_ast::symbol::{INTERNAL_SYMBOL_NAME_DEFAULT, INTERNAL_SYMBOL_NAME_EXPORT_EQUALS};

// Go: internal/ls/autoimport/export_stringer_generated.go:ExportSyntax.String
#[test]
fn export_syntax_display_matches_stringer() {
    assert_eq!(ExportSyntax::None.to_string(), "ExportSyntaxNone");
    assert_eq!(ExportSyntax::Modifier.to_string(), "ExportSyntaxModifier");
    assert_eq!(ExportSyntax::Named.to_string(), "ExportSyntaxNamed");
    assert_eq!(
        ExportSyntax::DefaultModifier.to_string(),
        "ExportSyntaxDefaultModifier"
    );
    assert_eq!(
        ExportSyntax::DefaultDeclaration.to_string(),
        "ExportSyntaxDefaultDeclaration"
    );
    assert_eq!(ExportSyntax::Equals.to_string(), "ExportSyntaxEquals");
    assert_eq!(ExportSyntax::Umd.to_string(), "ExportSyntaxUMD");
    assert_eq!(ExportSyntax::Star.to_string(), "ExportSyntaxStar");
    assert_eq!(
        ExportSyntax::CommonJsModuleExports.to_string(),
        "ExportSyntaxCommonJSModuleExports"
    );
    assert_eq!(
        ExportSyntax::CommonJsExportsProperty.to_string(),
        "ExportSyntaxCommonJSExportsProperty"
    );
}

#[test]
fn export_syntax_discriminants() {
    assert_eq!(ExportSyntax::None as i32, 0);
    assert_eq!(ExportSyntax::Modifier as i32, 1);
    assert_eq!(ExportSyntax::Named as i32, 2);
    assert_eq!(ExportSyntax::CommonJsExportsProperty as i32, 9);
}

// Go: internal/ls/autoimport/export.go:Export.Name
#[test]
fn name_prefers_local_name() {
    let mut e = Export {
        id: ExportId {
            module_id: ModuleId::new("/m.ts"),
            export_name: INTERNAL_SYMBOL_NAME_DEFAULT.to_string(),
        },
        ..Default::default()
    };
    e.local_name = "MyComponent".to_string();
    assert_eq!(e.name(), "MyComponent");
}

#[test]
fn name_export_equals_uses_target() {
    let e = Export {
        id: ExportId {
            module_id: ModuleId::new("/m.ts"),
            export_name: INTERNAL_SYMBOL_NAME_EXPORT_EQUALS.to_string(),
        },
        target: ExportId {
            module_id: ModuleId::new("/m.ts"),
            export_name: "realName".to_string(),
        },
        ..Default::default()
    };
    assert_eq!(e.name(), "realName");
}

#[test]
fn name_plain_export() {
    let e = Export {
        id: ExportId {
            module_id: ModuleId::new("/m.ts"),
            export_name: "foo".to_string(),
        },
        ..Default::default()
    };
    assert_eq!(e.name(), "foo");
}

// Go: internal/ls/autoimport/export.go:Export.IsRenameable
#[test]
fn is_renameable() {
    let make = |n: &str| Export {
        id: ExportId {
            module_id: ModuleId::new("/m.ts"),
            export_name: n.to_string(),
        },
        ..Default::default()
    };
    assert!(make(INTERNAL_SYMBOL_NAME_DEFAULT).is_renameable());
    assert!(make(INTERNAL_SYMBOL_NAME_EXPORT_EQUALS).is_renameable());
    assert!(!make("foo").is_renameable());
}

// Go: internal/ls/autoimport/export.go:Export.AmbientModuleName
#[test]
fn ambient_module_name() {
    // Bare module name -> returned as the ambient module name.
    let bare = Export {
        id: ExportId {
            module_id: ModuleId::new("react"),
            export_name: "useState".to_string(),
        },
        ..Default::default()
    };
    assert_eq!(bare.ambient_module_name(), "react");

    // Relative file path -> empty.
    let relative = Export {
        id: ExportId {
            module_id: ModuleId::new("./lib/b.ts"),
            export_name: "b".to_string(),
        },
        ..Default::default()
    };
    assert_eq!(relative.ambient_module_name(), "");
}

// Go: internal/ls/autoimport/export.go:Export.IsUnresolvedAlias
#[test]
fn is_unresolved_alias() {
    use tsgo_ast::SymbolFlags;
    let alias = Export {
        flags: SymbolFlags::ALIAS,
        ..Default::default()
    };
    assert!(alias.is_unresolved_alias());

    let func = Export {
        flags: SymbolFlags::FUNCTION,
        ..Default::default()
    };
    assert!(!func.is_unresolved_alias());
}

// Go: internal/ls/autoimport/extract.go:isUnusableName
#[test]
fn unusable_names() {
    assert!(is_unusable_name(""));
    assert!(is_unusable_name("_default"));
    assert!(is_unusable_name(INTERNAL_SYMBOL_NAME_DEFAULT));
    assert!(is_unusable_name(INTERNAL_SYMBOL_NAME_EXPORT_EQUALS));
    assert!(!is_unusable_name("foo"));
    assert!(!is_unusable_name("default_export"));
}

// Go: internal/ls/autoimport/index.go:Named (Export implements Named)
#[test]
fn export_implements_named() {
    let e = Export {
        id: ExportId {
            module_id: ModuleId::new("/m.ts"),
            export_name: "widget".to_string(),
        },
        ..Default::default()
    };
    assert_eq!(Named::name(&e), "widget");
}
