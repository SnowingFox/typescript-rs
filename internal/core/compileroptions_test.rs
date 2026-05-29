use super::*;

// Go: internal/core/modulekind_stringer_generated.go:String
#[test]
fn module_kind_display() {
    assert_eq!(ModuleKind::None.to_string(), "None");
    assert_eq!(ModuleKind::CommonJs.to_string(), "CommonJS");
    assert_eq!(ModuleKind::Amd.to_string(), "AMD");
    assert_eq!(ModuleKind::Umd.to_string(), "UMD");
    assert_eq!(ModuleKind::System.to_string(), "System");
    assert_eq!(ModuleKind::Es2015.to_string(), "ES2015");
    assert_eq!(ModuleKind::EsNext.to_string(), "ESNext");
    assert_eq!(ModuleKind::Node16.to_string(), "Node16");
    assert_eq!(ModuleKind::NodeNext.to_string(), "NodeNext");
    assert_eq!(ModuleKind::Preserve.to_string(), "Preserve");
}

// Go: internal/core/compileroptions.go:ModuleKind.IsNonNodeESM
#[test]
fn module_kind_is_non_node_esm() {
    assert!(ModuleKind::Es2015.is_non_node_esm());
    assert!(ModuleKind::Es2022.is_non_node_esm());
    assert!(ModuleKind::EsNext.is_non_node_esm());
    assert!(!ModuleKind::CommonJs.is_non_node_esm());
    assert!(!ModuleKind::Node16.is_non_node_esm());
}

// Go: internal/core/compileroptions.go:ModuleKind.SupportsImportAttributes
#[test]
fn module_kind_supports_import_attributes() {
    assert!(ModuleKind::Node18.supports_import_attributes());
    assert!(ModuleKind::NodeNext.supports_import_attributes());
    assert!(ModuleKind::Preserve.supports_import_attributes());
    assert!(ModuleKind::EsNext.supports_import_attributes());
    assert!(!ModuleKind::Node16.supports_import_attributes());
    assert!(!ModuleKind::CommonJs.supports_import_attributes());
}

// Go: internal/core/scripttarget_stringer_generated.go:String
#[test]
fn script_target_display() {
    assert_eq!(ScriptTarget::None.to_string(), "None");
    assert_eq!(ScriptTarget::Es5.to_string(), "ES5");
    assert_eq!(ScriptTarget::Es2025.to_string(), "ES2025");
    assert_eq!(ScriptTarget::EsNext.to_string(), "ESNext");
    assert_eq!(ScriptTarget::Json.to_string(), "JSON");
    assert_eq!(ScriptTarget::LATEST_STANDARD.to_string(), "ES2025");
    assert_eq!(ScriptTarget::LATEST.to_string(), "ESNext");
}

// Go: internal/core/compileroptions.go:ModuleResolutionKind.String
#[test]
fn module_resolution_kind_display() {
    assert_eq!(ModuleResolutionKind::Classic.to_string(), "Classic");
    assert_eq!(ModuleResolutionKind::Node10.to_string(), "Node10");
    assert_eq!(ModuleResolutionKind::Node16.to_string(), "Node16");
    assert_eq!(ModuleResolutionKind::NodeNext.to_string(), "NodeNext");
    assert_eq!(ModuleResolutionKind::Bundler.to_string(), "Bundler");
}

// Go: internal/core/compileroptions.go:ModuleResolutionKind.String
#[test]
#[should_panic(expected = "should not use zero value")]
fn module_resolution_kind_display_panics_on_unknown() {
    let _ = ModuleResolutionKind::Unknown.to_string();
}

// Go: internal/core/compileroptions.go:JsxEmit.String
#[test]
fn jsx_emit_display() {
    assert_eq!(JsxEmit::Preserve.to_string(), "preserve");
    assert_eq!(JsxEmit::ReactNative.to_string(), "react-native");
    assert_eq!(JsxEmit::React.to_string(), "react");
    assert_eq!(JsxEmit::ReactJsx.to_string(), "react-jsx");
    assert_eq!(JsxEmit::ReactJsxDev.to_string(), "react-jsxdev");
}

// Go: internal/core/compileroptions.go:JsxEmit.String
#[test]
#[should_panic(expected = "should not use zero value of JsxEmit")]
fn jsx_emit_display_panics_on_none() {
    let _ = JsxEmit::None.to_string();
}

// Go: internal/core/compileroptions.go:GetNewLineKind
// Go: internal/core/compileroptions.go:GetNewLineCharacter
#[test]
fn new_line_kind_roundtrip() {
    assert_eq!(get_new_line_kind("\r\n"), NewLineKind::Crlf);
    assert_eq!(get_new_line_kind("\n"), NewLineKind::Lf);
    assert_eq!(get_new_line_kind("x"), NewLineKind::None);
    assert_eq!(NewLineKind::Crlf.get_new_line_character(), "\r\n");
    assert_eq!(NewLineKind::Lf.get_new_line_character(), "\n");
    assert_eq!(NewLineKind::None.get_new_line_character(), "\n");
}

// Go: internal/core/compileroptions.go:GetEmitScriptTarget
#[test]
fn get_emit_script_target_default() {
    // Unset target falls back to ScriptTargetLatestStandard (ES2025).
    let opts = CompilerOptions::default();
    assert_eq!(opts.get_emit_script_target(), ScriptTarget::Es2025);
    // An explicit target is returned unchanged.
    let opts = CompilerOptions {
        target: ScriptTarget::Es2017,
        ..Default::default()
    };
    assert_eq!(opts.get_emit_script_target(), ScriptTarget::Es2017);
}

// Go: internal/core/compileroptions.go:GetEmitModuleKind
#[test]
fn get_emit_module_kind_by_target() {
    // An explicit module wins outright.
    let opts = CompilerOptions {
        module: ModuleKind::CommonJs,
        target: ScriptTarget::EsNext,
        ..Default::default()
    };
    assert_eq!(opts.get_emit_module_kind(), ModuleKind::CommonJs);

    let by_target = |target: ScriptTarget| {
        CompilerOptions {
            target,
            ..Default::default()
        }
        .get_emit_module_kind()
    };
    assert_eq!(by_target(ScriptTarget::EsNext), ModuleKind::EsNext);
    assert_eq!(by_target(ScriptTarget::Es2022), ModuleKind::Es2022);
    assert_eq!(by_target(ScriptTarget::Es2023), ModuleKind::Es2022);
    assert_eq!(by_target(ScriptTarget::Es2020), ModuleKind::Es2020);
    assert_eq!(by_target(ScriptTarget::Es2015), ModuleKind::Es2015);
    assert_eq!(by_target(ScriptTarget::Es5), ModuleKind::CommonJs);
    // Unset target -> ES2025 (latest standard) -> ES2022 module.
    assert_eq!(by_target(ScriptTarget::None), ModuleKind::Es2022);
}

// Go: internal/core/compileroptions.go:GetModuleResolutionKind
#[test]
fn get_module_resolution_kind() {
    // When ModuleResolution is Unknown/Classic/Node10, it is derived from the
    // emit module kind.
    let derived = |resolution: ModuleResolutionKind, module: ModuleKind| {
        CompilerOptions {
            module_resolution: resolution,
            module,
            ..Default::default()
        }
        .get_module_resolution_kind()
    };
    assert_eq!(
        derived(ModuleResolutionKind::Unknown, ModuleKind::Node16),
        ModuleResolutionKind::Node16
    );
    assert_eq!(
        derived(ModuleResolutionKind::Unknown, ModuleKind::Node18),
        ModuleResolutionKind::Node16
    );
    assert_eq!(
        derived(ModuleResolutionKind::Classic, ModuleKind::Node20),
        ModuleResolutionKind::Node16
    );
    assert_eq!(
        derived(ModuleResolutionKind::Node10, ModuleKind::NodeNext),
        ModuleResolutionKind::NodeNext
    );
    // Any other emit module kind derives Bundler.
    assert_eq!(
        derived(ModuleResolutionKind::Unknown, ModuleKind::CommonJs),
        ModuleResolutionKind::Bundler
    );
    // Default (module None -> ES2022 emit) also derives Bundler.
    assert_eq!(
        CompilerOptions::default().get_module_resolution_kind(),
        ModuleResolutionKind::Bundler
    );
    // An explicit non-derivable resolution is returned unchanged.
    assert_eq!(
        derived(ModuleResolutionKind::Bundler, ModuleKind::Node16),
        ModuleResolutionKind::Bundler
    );
    assert_eq!(
        derived(ModuleResolutionKind::NodeNext, ModuleKind::CommonJs),
        ModuleResolutionKind::NodeNext
    );
}

// Go: internal/core/compileroptions.go:GetEmitModuleDetectionKind
#[test]
fn get_emit_module_detection_kind() {
    // An explicit detection kind wins.
    let opts = CompilerOptions {
        module_detection: ModuleDetectionKind::Legacy,
        ..Default::default()
    };
    assert_eq!(
        opts.get_emit_module_detection_kind(),
        ModuleDetectionKind::Legacy
    );

    let by_module = |module: ModuleKind| {
        CompilerOptions {
            module,
            ..Default::default()
        }
        .get_emit_module_detection_kind()
    };
    // Node16..=NodeNext force module detection.
    assert_eq!(by_module(ModuleKind::Node16), ModuleDetectionKind::Force);
    assert_eq!(by_module(ModuleKind::Node20), ModuleDetectionKind::Force);
    assert_eq!(by_module(ModuleKind::NodeNext), ModuleDetectionKind::Force);
    // Outside that range falls back to Auto.
    assert_eq!(by_module(ModuleKind::EsNext), ModuleDetectionKind::Auto);
    assert_eq!(by_module(ModuleKind::Preserve), ModuleDetectionKind::Auto);
    assert_eq!(by_module(ModuleKind::CommonJs), ModuleDetectionKind::Auto);
    // Default (emit module ES2022) -> Auto.
    assert_eq!(
        CompilerOptions::default().get_emit_module_detection_kind(),
        ModuleDetectionKind::Auto
    );
}

// Go: internal/core/compileroptions.go:GetResolveJsonModule
#[test]
fn get_resolve_json_module() {
    use crate::tristate::Tristate;
    // An explicit tri-state overrides the derived behavior.
    assert!(CompilerOptions {
        resolve_json_module: Tristate::True,
        ..Default::default()
    }
    .get_resolve_json_module());
    assert!(!CompilerOptions {
        resolve_json_module: Tristate::False,
        module: ModuleKind::NodeNext,
        ..Default::default()
    }
    .get_resolve_json_module());

    // When unset, Node20/NodeNext emit module kinds enable it.
    assert!(CompilerOptions {
        module: ModuleKind::Node20,
        ..Default::default()
    }
    .get_resolve_json_module());
    assert!(CompilerOptions {
        module: ModuleKind::NodeNext,
        ..Default::default()
    }
    .get_resolve_json_module());

    // When unset and not Node20/NodeNext, it is enabled iff resolution is Bundler.
    // Default options derive Bundler resolution -> true.
    assert!(CompilerOptions::default().get_resolve_json_module());
    // Node16 emit module derives Node16 resolution (not Bundler) -> false.
    assert!(!CompilerOptions {
        module: ModuleKind::Node16,
        ..Default::default()
    }
    .get_resolve_json_module());
}

// Go: internal/core/compileroptions.go:GetStrictOptionValue
#[test]
fn get_strict_option_value() {
    use crate::tristate::Tristate;
    // An explicit per-option tri-state wins regardless of `strict`.
    assert!(CompilerOptions::default().get_strict_option_value(Tristate::True));
    assert!(!CompilerOptions {
        strict: Tristate::True,
        ..Default::default()
    }
    .get_strict_option_value(Tristate::False));

    // When the per-option value is unset, it follows `strict != false`.
    let strict_true = CompilerOptions {
        strict: Tristate::True,
        ..Default::default()
    };
    assert!(strict_true.get_strict_option_value(Tristate::Unknown));
    let strict_false = CompilerOptions {
        strict: Tristate::False,
        ..Default::default()
    };
    assert!(!strict_false.get_strict_option_value(Tristate::Unknown));
    // Unset `strict` is not false, so an unset option resolves to true.
    assert!(CompilerOptions::default().get_strict_option_value(Tristate::Unknown));
}

// Go: internal/core/compileroptions.go:GetIsolatedModules
#[test]
fn get_isolated_modules() {
    use crate::tristate::Tristate;
    assert!(CompilerOptions {
        isolated_modules: Tristate::True,
        ..Default::default()
    }
    .get_isolated_modules());
    // verbatimModuleSyntax also implies isolated modules.
    assert!(CompilerOptions {
        verbatim_module_syntax: Tristate::True,
        ..Default::default()
    }
    .get_isolated_modules());
    assert!(!CompilerOptions::default().get_isolated_modules());
}

// Go: internal/core/compileroptions.go:ShouldPreserveConstEnums
#[test]
fn should_preserve_const_enums() {
    use crate::tristate::Tristate;
    assert!(CompilerOptions {
        preserve_const_enums: Tristate::True,
        ..Default::default()
    }
    .should_preserve_const_enums());
    // Isolated modules force const enums to be preserved.
    assert!(CompilerOptions {
        isolated_modules: Tristate::True,
        ..Default::default()
    }
    .should_preserve_const_enums());
    assert!(!CompilerOptions::default().should_preserve_const_enums());
}

// Go: internal/core/compileroptions.go:GetUseDefineForClassFields
#[test]
fn get_use_define_for_class_fields() {
    use crate::tristate::Tristate;
    // When unset, it follows target >= ES2022.
    assert!(CompilerOptions {
        target: ScriptTarget::Es2022,
        ..Default::default()
    }
    .get_use_define_for_class_fields());
    assert!(!CompilerOptions {
        target: ScriptTarget::Es2021,
        ..Default::default()
    }
    .get_use_define_for_class_fields());
    // Default target (ES2025) is >= ES2022.
    assert!(CompilerOptions::default().get_use_define_for_class_fields());
    // An explicit value overrides the target heuristic.
    assert!(!CompilerOptions {
        use_define_for_class_fields: Tristate::False,
        target: ScriptTarget::EsNext,
        ..Default::default()
    }
    .get_use_define_for_class_fields());
    assert!(CompilerOptions {
        use_define_for_class_fields: Tristate::True,
        target: ScriptTarget::Es5,
        ..Default::default()
    }
    .get_use_define_for_class_fields());
}

// Go: internal/core/compileroptions.go:GetEmitStandardClassFields
#[test]
fn get_emit_standard_class_fields() {
    use crate::tristate::Tristate;
    // Requires useDefineForClassFields != false AND target >= ES2022.
    assert!(CompilerOptions::default().get_emit_standard_class_fields());
    assert!(!CompilerOptions {
        target: ScriptTarget::Es2021,
        ..Default::default()
    }
    .get_emit_standard_class_fields());
    assert!(!CompilerOptions {
        use_define_for_class_fields: Tristate::False,
        target: ScriptTarget::Es2022,
        ..Default::default()
    }
    .get_emit_standard_class_fields());
    assert!(CompilerOptions {
        use_define_for_class_fields: Tristate::True,
        target: ScriptTarget::Es2022,
        ..Default::default()
    }
    .get_emit_standard_class_fields());
}

// Go: internal/core/compileroptions.go:GetEmitDeclarations
#[test]
fn get_emit_declarations() {
    use crate::tristate::Tristate;
    assert!(CompilerOptions {
        declaration: Tristate::True,
        ..Default::default()
    }
    .get_emit_declarations());
    // Composite implies declaration emit.
    assert!(CompilerOptions {
        composite: Tristate::True,
        ..Default::default()
    }
    .get_emit_declarations());
    assert!(!CompilerOptions::default().get_emit_declarations());
}

// Go: internal/core/compileroptions.go:GetAreDeclarationMapsEnabled
#[test]
fn get_are_declaration_maps_enabled() {
    use crate::tristate::Tristate;
    // Requires declarationMap == true AND declaration emit.
    assert!(CompilerOptions {
        declaration_map: Tristate::True,
        declaration: Tristate::True,
        ..Default::default()
    }
    .get_are_declaration_maps_enabled());
    // declarationMap alone (no declaration emit) is not enough.
    assert!(!CompilerOptions {
        declaration_map: Tristate::True,
        ..Default::default()
    }
    .get_are_declaration_maps_enabled());
    assert!(!CompilerOptions {
        declaration: Tristate::True,
        ..Default::default()
    }
    .get_are_declaration_maps_enabled());
}

// Go: internal/core/compileroptions.go:HasJsonModuleEmitEnabled
#[test]
fn has_json_module_emit_enabled() {
    let by_module = |module: ModuleKind| {
        CompilerOptions {
            module,
            ..Default::default()
        }
        .has_json_module_emit_enabled()
    };
    // System and UMD emit module kinds disable JSON module emit.
    assert!(!by_module(ModuleKind::System));
    assert!(!by_module(ModuleKind::Umd));
    assert!(by_module(ModuleKind::CommonJs));
    assert!(CompilerOptions::default().has_json_module_emit_enabled());
}

// Go: internal/core/compileroptions.go:IsIncremental
#[test]
fn is_incremental() {
    use crate::tristate::Tristate;
    assert!(CompilerOptions {
        incremental: Tristate::True,
        ..Default::default()
    }
    .is_incremental());
    assert!(CompilerOptions {
        composite: Tristate::True,
        ..Default::default()
    }
    .is_incremental());
    assert!(!CompilerOptions::default().is_incremental());
}

// Go: internal/core/compileroptions.go:GetResolvePackageJsonExports
// Go: internal/core/compileroptions.go:GetResolvePackageJsonImports
#[test]
fn get_resolve_package_json_exports_imports() {
    use crate::tristate::Tristate;
    // Default (Unknown) resolves to true (IsTrueOrUnknown).
    assert!(CompilerOptions::default().get_resolve_package_json_exports());
    assert!(CompilerOptions::default().get_resolve_package_json_imports());
    assert!(!CompilerOptions {
        resolve_package_json_exports: Tristate::False,
        ..Default::default()
    }
    .get_resolve_package_json_exports());
    assert!(!CompilerOptions {
        resolve_package_json_imports: Tristate::False,
        ..Default::default()
    }
    .get_resolve_package_json_imports());
}

// Go: internal/core/compileroptions.go:GetAllowImportingTsExtensions
// Go: internal/core/compileroptions.go:AllowImportingTsExtensionsFrom
#[test]
fn get_allow_importing_ts_extensions() {
    use crate::tristate::Tristate;
    assert!(CompilerOptions {
        allow_importing_ts_extensions: Tristate::True,
        ..Default::default()
    }
    .get_allow_importing_ts_extensions());
    assert!(CompilerOptions {
        rewrite_relative_import_extensions: Tristate::True,
        ..Default::default()
    }
    .get_allow_importing_ts_extensions());
    assert!(!CompilerOptions::default().get_allow_importing_ts_extensions());

    // `AllowImportingTsExtensionsFrom` is also true for declaration files.
    assert!(CompilerOptions::default().allow_importing_ts_extensions_from("a.d.ts"));
    assert!(!CompilerOptions::default().allow_importing_ts_extensions_from("a.ts"));
    assert!(CompilerOptions {
        allow_importing_ts_extensions: Tristate::True,
        ..Default::default()
    }
    .allow_importing_ts_extensions_from("a.ts"));
}

// Go: internal/core/compileroptions.go:GetAllowJS
#[test]
fn get_allow_js() {
    use crate::tristate::Tristate;
    assert!(CompilerOptions {
        allow_js: Tristate::True,
        ..Default::default()
    }
    .get_allow_js());
    assert!(!CompilerOptions {
        allow_js: Tristate::False,
        check_js: Tristate::True,
        ..Default::default()
    }
    .get_allow_js());
    // When allowJs is unset, checkJs enables it.
    assert!(CompilerOptions {
        check_js: Tristate::True,
        ..Default::default()
    }
    .get_allow_js());
    assert!(!CompilerOptions::default().get_allow_js());
}

// Go: internal/core/compileroptions.go:GetJSXTransformEnabled
#[test]
fn get_jsx_transform_enabled() {
    let with_jsx = |jsx: JsxEmit| {
        CompilerOptions {
            jsx,
            ..Default::default()
        }
        .get_jsx_transform_enabled()
    };
    assert!(with_jsx(JsxEmit::React));
    assert!(with_jsx(JsxEmit::ReactJsx));
    assert!(with_jsx(JsxEmit::ReactJsxDev));
    assert!(!with_jsx(JsxEmit::Preserve));
    assert!(!with_jsx(JsxEmit::ReactNative));
    assert!(!with_jsx(JsxEmit::None));
}

// Go: internal/core/compileroptions.go:UsesWildcardTypes
#[test]
fn uses_wildcard_types() {
    assert!(CompilerOptions {
        types: vec!["node".to_string(), "*".to_string()],
        ..Default::default()
    }
    .uses_wildcard_types());
    assert!(!CompilerOptions {
        types: vec!["node".to_string()],
        ..Default::default()
    }
    .uses_wildcard_types());
    assert!(!CompilerOptions::default().uses_wildcard_types());
}

// Go: internal/core/compileroptions.go:GetPathsBasePath
#[test]
fn get_paths_base_path() {
    use tsgo_collections::OrderedMap;
    // No paths -> empty base path regardless of current directory.
    assert_eq!(CompilerOptions::default().get_paths_base_path("/cwd"), "");

    let mut paths: OrderedMap<String, Vec<String>> = OrderedMap::default();
    paths.set("@/*".to_string(), vec!["src/*".to_string()]);
    // Paths present, no explicit base -> current directory.
    let opts = CompilerOptions {
        paths: Some(paths.clone()),
        ..Default::default()
    };
    assert_eq!(opts.get_paths_base_path("/cwd"), "/cwd");
    // Explicit pathsBasePath wins.
    let opts = CompilerOptions {
        paths: Some(paths),
        paths_base_path: "/base".to_string(),
        ..Default::default()
    };
    assert_eq!(opts.get_paths_base_path("/cwd"), "/base");
}

// Go: internal/core/compileroptions.go:GetEffectiveTypeRoots
#[test]
fn get_effective_type_roots() {
    // Explicit typeRoots are returned with fromConfig = true.
    let opts = CompilerOptions {
        type_roots: Some(vec!["/custom/types".to_string()]),
        ..Default::default()
    };
    assert_eq!(
        opts.get_effective_type_roots("/ignored"),
        (vec!["/custom/types".to_string()], true)
    );

    // From a config file path: ancestor `node_modules/@types` directories.
    let opts = CompilerOptions {
        config_file_path: "/home/user/project/tsconfig.json".to_string(),
        ..Default::default()
    };
    let (roots, from_config) = opts.get_effective_type_roots("/ignored");
    assert!(!from_config);
    assert_eq!(roots[0], "/home/user/project/node_modules/@types");
    assert_eq!(roots[1], "/home/user/node_modules/@types");
    assert_eq!(roots.last().unwrap(), "/node_modules/@types");

    // No config: falls back to the current directory.
    let opts = CompilerOptions::default();
    let (roots, from_config) = opts.get_effective_type_roots("/home/user/project");
    assert!(!from_config);
    assert_eq!(roots[0], "/home/user/project/node_modules/@types");
}

// Go: internal/core/compileroptions.go:GetEffectiveTypeRoots
#[test]
#[should_panic(expected = "cannot get effective type roots")]
fn get_effective_type_roots_panics_without_base() {
    let _ = CompilerOptions::default().get_effective_type_roots("");
}

// Go: internal/core/compileroptions.go:Clone
#[test]
fn clone_independent() {
    let mut original = CompilerOptions {
        lib: vec!["es2020".to_string()],
        target: ScriptTarget::Es2020,
        ..Default::default()
    };
    let cloned = original.clone();
    // Mutating the original's owned fields must not affect the clone.
    original.lib.push("dom".to_string());
    original.target = ScriptTarget::EsNext;
    assert_eq!(cloned.lib, vec!["es2020".to_string()]);
    assert_eq!(cloned.target, ScriptTarget::Es2020);
}

// Go: internal/core/compileroptions.go:ModuleKindToModuleResolutionKind
#[test]
fn module_kind_to_module_resolution_kind_map() {
    assert_eq!(
        module_kind_to_module_resolution_kind(ModuleKind::Node16),
        Some(ModuleResolutionKind::Node16)
    );
    assert_eq!(
        module_kind_to_module_resolution_kind(ModuleKind::NodeNext),
        Some(ModuleResolutionKind::NodeNext)
    );
    assert_eq!(
        module_kind_to_module_resolution_kind(ModuleKind::CommonJs),
        None
    );
}
