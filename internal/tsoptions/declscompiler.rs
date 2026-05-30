//! The compiler option declaration tables (`commonOptionsWithBuild` +
//! `optionsForCompiler` = `OptionsDeclarations`) and the name->declaration map.
//!
//! 1:1 port of Go `internal/tsoptions/declscompiler.go`.
//!
//! DIVERGENCE(port): `Description`/`Category` messages (help/showConfig output)
//! are deferred and left unset; every parsing/validation-relevant field
//! (`kind`, `is_*`, `affects_*`, `min_value`, `transpile_option_value`,
//! `strict_flag`, `allow_js_flag`, `extra_validation`,
//! `allow_config_dir_template_substitution`, `list_preserve_falsy_values`) is
//! populated exactly. `default_value_description` carries the non-message
//! literal defaults.
//!
//! The reflection-based `optionsHaveChanges`/`ForEachCompilerOptionValue`
//! affect-comparison helpers live in [`crate::optionsfields`] (the field table).

use std::sync::LazyLock;

use tsgo_core::compileroptions::ScriptTarget;
use tsgo_core::tristate::Tristate;

use crate::commandlineoption::{
    CommandLineOption, CommandLineOptionKind as Kind, CommandLineOptionNameMap, DefaultValue,
    ExtraValidation,
};

fn common_options_with_build() -> Vec<CommandLineOption> {
    vec![
        CommandLineOption {
            name: "help",
            short_name: "h",
            kind: Kind::Boolean,
            show_in_simplified_help_view: true,
            is_command_line_only: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "help",
            short_name: "?",
            kind: Kind::Boolean,
            is_command_line_only: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "watch",
            short_name: "w",
            kind: Kind::Boolean,
            show_in_simplified_help_view: true,
            is_command_line_only: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "preserveWatchOutput",
            kind: Kind::Boolean,
            show_in_simplified_help_view: false,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "listFiles",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "explainFiles",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "listEmittedFiles",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "pretty",
            kind: Kind::Boolean,
            show_in_simplified_help_view: true,
            default_value_description: DefaultValue::Bool(true),
            ..Default::default()
        },
        CommandLineOption {
            name: "traceResolution",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "diagnostics",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "extendedDiagnostics",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "generateCpuProfile",
            kind: Kind::String,
            is_file_path: true,
            default_value_description: DefaultValue::Str("profile.cpuprofile"),
            ..Default::default()
        },
        CommandLineOption {
            name: "generateTrace",
            kind: Kind::String,
            is_file_path: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "incremental",
            short_name: "i",
            kind: Kind::Boolean,
            transpile_option_value: Tristate::Unknown,
            ..Default::default()
        },
        CommandLineOption {
            name: "declaration",
            short_name: "d",
            kind: Kind::Boolean,
            affects_build_info: true,
            show_in_simplified_help_view: true,
            transpile_option_value: Tristate::Unknown,
            ..Default::default()
        },
        CommandLineOption {
            name: "declarationMap",
            kind: Kind::Boolean,
            affects_build_info: true,
            show_in_simplified_help_view: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "emitDeclarationOnly",
            kind: Kind::Boolean,
            affects_build_info: true,
            show_in_simplified_help_view: true,
            transpile_option_value: Tristate::Unknown,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "sourceMap",
            kind: Kind::Boolean,
            affects_build_info: true,
            show_in_simplified_help_view: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "inlineSourceMap",
            kind: Kind::Boolean,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "noCheck",
            kind: Kind::Boolean,
            show_in_simplified_help_view: false,
            transpile_option_value: Tristate::True,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "deduplicatePackages",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(true),
            affects_program_structure: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "noEmit",
            kind: Kind::Boolean,
            show_in_simplified_help_view: true,
            transpile_option_value: Tristate::Unknown,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "assumeChangesOnlyAffectDirectDependencies",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_emit: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "locale",
            kind: Kind::String,
            is_command_line_only: true,
            extra_validation: ExtraValidation::Locale,
            ..Default::default()
        },
        CommandLineOption {
            name: "quiet",
            short_name: "q",
            kind: Kind::Boolean,
            ..Default::default()
        },
        CommandLineOption {
            name: "singleThreaded",
            kind: Kind::Boolean,
            ..Default::default()
        },
        CommandLineOption {
            name: "pprofDir",
            kind: Kind::String,
            is_file_path: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "checkers",
            kind: Kind::Number,
            min_value: 1,
            ..Default::default()
        },
    ]
}

fn options_for_compiler() -> Vec<CommandLineOption> {
    vec![
        // Command line only options
        CommandLineOption {
            name: "all",
            kind: Kind::Boolean,
            show_in_simplified_help_view: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "version",
            short_name: "v",
            kind: Kind::Boolean,
            show_in_simplified_help_view: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "init",
            kind: Kind::Boolean,
            show_in_simplified_help_view: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "project",
            short_name: "p",
            kind: Kind::String,
            is_file_path: true,
            show_in_simplified_help_view: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "showConfig",
            kind: Kind::Boolean,
            show_in_simplified_help_view: true,
            is_command_line_only: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "listFilesOnly",
            kind: Kind::Boolean,
            is_command_line_only: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "ignoreConfig",
            kind: Kind::Boolean,
            show_in_simplified_help_view: true,
            is_command_line_only: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        // Basic
        CommandLineOption {
            name: "target",
            short_name: "t",
            kind: Kind::Enum, // targetOptionMap
            affects_source_file: true,
            affects_module_resolution: true,
            affects_emit: true,
            affects_build_info: true,
            show_in_simplified_help_view: true,
            default_value_description: DefaultValue::Enum(ScriptTarget::Es2025 as i32),
            ..Default::default()
        },
        CommandLineOption {
            name: "module",
            short_name: "m",
            kind: Kind::Enum, // moduleOptionMap
            affects_module_resolution: true,
            affects_emit: true,
            affects_build_info: true,
            show_in_simplified_help_view: true,
            default_value_description: DefaultValue::Tristate(Tristate::Unknown),
            ..Default::default()
        },
        CommandLineOption {
            name: "lib",
            kind: Kind::List,
            affects_program_structure: true,
            show_in_simplified_help_view: true,
            transpile_option_value: Tristate::Unknown,
            ..Default::default()
        },
        CommandLineOption {
            name: "allowJs",
            kind: Kind::Boolean,
            allow_js_flag: true,
            affects_build_info: true,
            show_in_simplified_help_view: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "checkJs",
            kind: Kind::Boolean,
            affects_module_resolution: true,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            show_in_simplified_help_view: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "jsx",
            kind: Kind::Enum, // jsxOptionMap
            affects_source_file: true,
            affects_emit: true,
            affects_build_info: true,
            affects_module_resolution: true,
            affects_semantic_diagnostics: true,
            show_in_simplified_help_view: true,
            default_value_description: DefaultValue::Tristate(Tristate::Unknown),
            ..Default::default()
        },
        CommandLineOption {
            name: "outFile",
            kind: Kind::String,
            affects_emit: true,
            affects_build_info: true,
            affects_declaration_path: true,
            is_file_path: true,
            show_in_simplified_help_view: true,
            transpile_option_value: Tristate::Unknown,
            ..Default::default()
        },
        CommandLineOption {
            name: "outDir",
            kind: Kind::String,
            affects_emit: true,
            affects_build_info: true,
            affects_declaration_path: true,
            is_file_path: true,
            show_in_simplified_help_view: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "rootDir",
            kind: Kind::String,
            affects_emit: true,
            affects_build_info: true,
            affects_declaration_path: true,
            is_file_path: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "composite",
            kind: Kind::Boolean,
            affects_build_info: true,
            is_tsconfig_only: true,
            transpile_option_value: Tristate::Unknown,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "tsBuildInfoFile",
            kind: Kind::String,
            affects_emit: true,
            affects_build_info: true,
            is_file_path: true,
            transpile_option_value: Tristate::Unknown,
            default_value_description: DefaultValue::Str(".tsbuildinfo"),
            ..Default::default()
        },
        CommandLineOption {
            name: "removeComments",
            kind: Kind::Boolean,
            affects_emit: true,
            affects_build_info: true,
            show_in_simplified_help_view: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "importHelpers",
            kind: Kind::Boolean,
            affects_emit: true,
            affects_build_info: true,
            affects_source_file: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "downlevelIteration",
            kind: Kind::Boolean,
            affects_emit: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "isolatedModules",
            kind: Kind::Boolean,
            transpile_option_value: Tristate::True,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "verbatimModuleSyntax",
            kind: Kind::Boolean,
            affects_emit: true,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "isolatedDeclarations",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            affects_build_info: true,
            affects_semantic_diagnostics: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "erasableSyntaxOnly",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            affects_build_info: true,
            affects_semantic_diagnostics: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "libReplacement",
            kind: Kind::Boolean,
            affects_program_structure: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        // Strict Type Checks
        CommandLineOption {
            name: "strict",
            kind: Kind::Boolean,
            affects_build_info: true,
            show_in_simplified_help_view: true,
            default_value_description: DefaultValue::Bool(true),
            ..Default::default()
        },
        CommandLineOption {
            name: "noImplicitAny",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            strict_flag: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "strictNullChecks",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            strict_flag: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "strictFunctionTypes",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            strict_flag: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "strictBindCallApply",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            strict_flag: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "strictPropertyInitialization",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            strict_flag: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "strictBuiltinIteratorReturn",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            strict_flag: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "noImplicitThis",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            strict_flag: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "useUnknownInCatchVariables",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            strict_flag: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "alwaysStrict",
            kind: Kind::Boolean,
            affects_source_file: true,
            affects_emit: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(true),
            ..Default::default()
        },
        CommandLineOption {
            name: "stableTypeOrdering",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(true),
            ..Default::default()
        },
        // Additional Checks
        CommandLineOption {
            name: "noUnusedLocals",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "noUnusedParameters",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "exactOptionalPropertyTypes",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "noImplicitReturns",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "noFallthroughCasesInSwitch",
            kind: Kind::Boolean,
            affects_bind_diagnostics: true,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "noUncheckedIndexedAccess",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "noImplicitOverride",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "noPropertyAccessFromIndexSignature",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            show_in_simplified_help_view: false,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        // Module Resolution
        CommandLineOption {
            name: "moduleResolution",
            kind: Kind::Enum,
            affects_module_resolution: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "baseUrl",
            kind: Kind::String,
            affects_module_resolution: true,
            is_file_path: true,
            ..Default::default()
        },
        CommandLineOption {
            // tsconfig.json only; copied as-is.
            name: "paths",
            kind: Kind::Object,
            affects_module_resolution: true,
            allow_config_dir_template_substitution: true,
            is_tsconfig_only: true,
            transpile_option_value: Tristate::Unknown,
            ..Default::default()
        },
        CommandLineOption {
            name: "rootDirs",
            kind: Kind::List,
            is_tsconfig_only: true,
            affects_module_resolution: true,
            allow_config_dir_template_substitution: true,
            transpile_option_value: Tristate::Unknown,
            ..Default::default()
        },
        CommandLineOption {
            name: "typeRoots",
            kind: Kind::List,
            affects_module_resolution: true,
            allow_config_dir_template_substitution: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "types",
            kind: Kind::List,
            affects_program_structure: true,
            show_in_simplified_help_view: true,
            transpile_option_value: Tristate::Unknown,
            ..Default::default()
        },
        CommandLineOption {
            name: "allowSyntheticDefaultImports",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(true),
            ..Default::default()
        },
        CommandLineOption {
            name: "esModuleInterop",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_emit: true,
            affects_build_info: true,
            show_in_simplified_help_view: true,
            default_value_description: DefaultValue::Bool(true),
            ..Default::default()
        },
        CommandLineOption {
            name: "preserveSymlinks",
            kind: Kind::Boolean,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "allowUmdGlobalAccess",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "moduleSuffixes",
            kind: Kind::List,
            list_preserve_falsy_values: true,
            affects_module_resolution: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "allowImportingTsExtensions",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            transpile_option_value: Tristate::Unknown,
            ..Default::default()
        },
        CommandLineOption {
            name: "rewriteRelativeImportExtensions",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "resolvePackageJsonExports",
            kind: Kind::Boolean,
            affects_module_resolution: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "resolvePackageJsonImports",
            kind: Kind::Boolean,
            affects_module_resolution: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "customConditions",
            kind: Kind::List,
            affects_module_resolution: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "noUncheckedSideEffectImports",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(true),
            ..Default::default()
        },
        // Source Maps
        CommandLineOption {
            name: "sourceRoot",
            kind: Kind::String,
            affects_emit: true,
            affects_build_info: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "mapRoot",
            kind: Kind::String,
            affects_emit: true,
            affects_build_info: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "inlineSources",
            kind: Kind::Boolean,
            affects_emit: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        // Experimental
        CommandLineOption {
            name: "experimentalDecorators",
            kind: Kind::Boolean,
            affects_emit: true,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "emitDecoratorMetadata",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_emit: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        // Advanced
        CommandLineOption {
            name: "jsxFactory",
            kind: Kind::String,
            default_value_description: DefaultValue::Str("`React.createElement`"),
            ..Default::default()
        },
        CommandLineOption {
            name: "jsxFragmentFactory",
            kind: Kind::String,
            default_value_description: DefaultValue::Str("React.Fragment"),
            ..Default::default()
        },
        CommandLineOption {
            name: "jsxImportSource",
            kind: Kind::String,
            affects_semantic_diagnostics: true,
            affects_emit: true,
            affects_build_info: true,
            affects_module_resolution: true,
            affects_source_file: true,
            default_value_description: DefaultValue::Str("react"),
            ..Default::default()
        },
        CommandLineOption {
            name: "resolveJsonModule",
            kind: Kind::Boolean,
            affects_module_resolution: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "allowArbitraryExtensions",
            kind: Kind::Boolean,
            affects_program_structure: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "reactNamespace",
            kind: Kind::String,
            affects_emit: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Str("`React`"),
            ..Default::default()
        },
        CommandLineOption {
            name: "skipDefaultLibCheck",
            kind: Kind::Boolean,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "emitBOM",
            kind: Kind::Boolean,
            affects_emit: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "newLine",
            kind: Kind::Enum, // newLineOptionMap
            affects_emit: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Str("lf"),
            ..Default::default()
        },
        CommandLineOption {
            name: "noErrorTruncation",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "noLib",
            kind: Kind::Boolean,
            affects_program_structure: true,
            transpile_option_value: Tristate::True,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "noResolve",
            kind: Kind::Boolean,
            affects_module_resolution: true,
            transpile_option_value: Tristate::True,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "stripInternal",
            kind: Kind::Boolean,
            affects_emit: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "disableSizeLimit",
            kind: Kind::Boolean,
            affects_program_structure: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "disableSourceOfProjectReferenceRedirect",
            kind: Kind::Boolean,
            is_tsconfig_only: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "disableSolutionSearching",
            kind: Kind::Boolean,
            is_tsconfig_only: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "disableReferencedProjectLoad",
            kind: Kind::Boolean,
            is_tsconfig_only: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "noEmitHelpers",
            kind: Kind::Boolean,
            affects_emit: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "noEmitOnError",
            kind: Kind::Boolean,
            affects_emit: true,
            affects_build_info: true,
            transpile_option_value: Tristate::Unknown,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "preserveConstEnums",
            kind: Kind::Boolean,
            affects_emit: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "declarationDir",
            kind: Kind::String,
            affects_emit: true,
            affects_build_info: true,
            affects_declaration_path: true,
            is_file_path: true,
            transpile_option_value: Tristate::Unknown,
            ..Default::default()
        },
        CommandLineOption {
            name: "skipLibCheck",
            kind: Kind::Boolean,
            affects_build_info: true,
            default_value_description: DefaultValue::Bool(false),
            ..Default::default()
        },
        CommandLineOption {
            name: "allowUnusedLabels",
            kind: Kind::Boolean,
            affects_bind_diagnostics: true,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Tristate(Tristate::Unknown),
            ..Default::default()
        },
        CommandLineOption {
            name: "allowUnreachableCode",
            kind: Kind::Boolean,
            affects_bind_diagnostics: true,
            affects_semantic_diagnostics: true,
            affects_build_info: true,
            default_value_description: DefaultValue::Tristate(Tristate::Unknown),
            ..Default::default()
        },
        CommandLineOption {
            name: "forceConsistentCasingInFileNames",
            kind: Kind::Boolean,
            affects_module_resolution: true,
            default_value_description: DefaultValue::Bool(true),
            ..Default::default()
        },
        CommandLineOption {
            name: "maxNodeModuleJsDepth",
            kind: Kind::Number,
            affects_module_resolution: true,
            default_value_description: DefaultValue::Int(0),
            ..Default::default()
        },
        CommandLineOption {
            name: "useDefineForClassFields",
            kind: Kind::Boolean,
            affects_semantic_diagnostics: true,
            affects_emit: true,
            affects_build_info: true,
            ..Default::default()
        },
        CommandLineOption {
            // A list of language-service plugins (tsconfig.json only).
            name: "plugins",
            kind: Kind::List,
            is_tsconfig_only: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "moduleDetection",
            kind: Kind::Enum,
            affects_source_file: true,
            affects_module_resolution: true,
            ..Default::default()
        },
        CommandLineOption {
            name: "ignoreDeprecations",
            kind: Kind::String,
            default_value_description: DefaultValue::Tristate(Tristate::Unknown),
            ..Default::default()
        },
    ]
}

/// All compiler option declarations (`commonOptionsWithBuild` then
/// `optionsForCompiler`).
// Go: internal/tsoptions/declscompiler.go:OptionsDeclarations
pub static OPTIONS_DECLARATIONS: LazyLock<Vec<CommandLineOption>> = LazyLock::new(|| {
    let mut v = common_options_with_build();
    v.extend(options_for_compiler());
    v
});

/// The options declarations common to `tsc` and `tsc --build`.
// Go: internal/tsoptions/declscompiler.go:commonOptionsWithBuild
pub static COMMON_OPTIONS_WITH_BUILD: LazyLock<Vec<CommandLineOption>> =
    LazyLock::new(common_options_with_build);

/// The name->declaration map over all compiler options (lowercased keys).
// Go: internal/tsoptions/tsconfigparsing.go:CommandLineCompilerOptionsMap
pub static COMMAND_LINE_COMPILER_OPTIONS_MAP: LazyLock<CommandLineOptionNameMap> =
    LazyLock::new(|| CommandLineOptionNameMap::from_options(&OPTIONS_DECLARATIONS));

#[cfg(test)]
#[path = "declscompiler_test.rs"]
mod tests;
