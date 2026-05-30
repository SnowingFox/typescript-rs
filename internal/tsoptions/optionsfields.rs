//! Explicit `CompilerOptions` field table — the port's replacement for Go's
//! `reflect`-based field iteration.
//!
//! Go uses `reflect.VisibleFields` / `reflect.ValueOf(...).Field(i)` in
//! `mergeCompilerOptions`, `ForEachCompilerOptionValue`, and the two
//! consistency tests. Rust has no runtime reflection, so this module lists
//! every exported `CompilerOptions` field once — its Go name, JSON name, Rust
//! field identifier, and a kind marker — and a macro generates:
//!
//! * [`compiler_option_field_names`] — `(go_name, json_name)` pairs (consistency tests),
//! * [`merge_compiler_options`] — non-zero / explicit-null field merge,
//! * [`for_each_compiler_option_value`] — per-field change detection.
//!
//! PERF(port): a `derive` macro on `core::CompilerOptions` could generate this
//! table; the explicit list keeps the dependency surface minimal and is checked
//! against the option declarations by `TestCompilerOptionsDeclaration`.

use tsgo_collections::{OrderedMap, Set};
use tsgo_core::compileroptions::CompilerOptions;

use crate::commandlineoption::CommandLineOption;
use crate::declscompiler::COMMAND_LINE_COMPILER_OPTIONS_MAP;

fn paths_equal(
    a: &Option<OrderedMap<String, Vec<String>>>,
    b: &Option<OrderedMap<String, Vec<String>>>,
) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x.size() == y.size() && x.entries().all(|(k, v)| y.get(k) == Some(v)),
        _ => false,
    }
}

macro_rules! field_is_set {
    (map, $default:expr, $opts:expr, $field:ident) => {
        $opts.$field.is_some()
    };
    ($k:ident, $default:expr, $opts:expr, $field:ident) => {
        $opts.$field != $default.$field
    };
}

macro_rules! field_changed {
    (tri, $decl:expr, $old:expr, $new:expr, $field:ident) => {{
        if $decl.strict_flag {
            $old.get_strict_option_value($old.$field) != $new.get_strict_option_value($new.$field)
        } else if $decl.allow_js_flag {
            $old.get_allow_js() != $new.get_allow_js()
        } else {
            $old.$field != $new.$field
        }
    }};
    (map, $decl:expr, $old:expr, $new:expr, $field:ident) => {{
        let _ = $decl;
        !paths_equal(&$old.$field, &$new.$field)
    }};
    (val, $decl:expr, $old:expr, $new:expr, $field:ident) => {{
        let _ = $decl;
        $old.$field != $new.$field
    }};
}

macro_rules! compiler_option_fields {
    ($(($go:literal, $json:literal, $field:ident, $kind:ident)),* $(,)?) => {
        /// Returns `(go_field_name, json_name)` for every exported
        /// `CompilerOptions` field, in declaration order.
        ///
        /// Side effects: none (pure).
        pub fn compiler_option_field_names() -> Vec<(&'static str, &'static str)> {
            vec![ $(($go, $json)),* ]
        }

        /// Merges `source` into `target`: explicitly-null fields (by JSON name)
        /// are zeroed, otherwise non-zero source fields overwrite the target.
        ///
        /// Replaces Go's reflection walk in `mergeCompilerOptions`.
        ///
        /// Side effects: mutates `target`.
        // Go: internal/tsoptions/parsinghelpers.go:mergeCompilerOptions
        pub fn merge_compiler_options(
            target: &mut CompilerOptions,
            source: &CompilerOptions,
            explicit_null_fields: &Set<String>,
        ) {
            let zero = CompilerOptions::default();
            $(
                if explicit_null_fields.has(&$json.to_string()) {
                    target.$field = zero.$field.clone();
                } else if field_is_set!($kind, zero, source, $field) {
                    target.$field = source.$field.clone();
                }
            )*
        }

        /// Invokes change detection over every field whose declaration matches
        /// `filter`, returning `true` as soon as a change is found.
        ///
        /// Replaces Go's reflection walk in `ForEachCompilerOptionValue`.
        ///
        /// Side effects: none (pure).
        // Go: internal/tsoptions/declscompiler.go:ForEachCompilerOptionValue
        pub fn for_each_compiler_option_value(
            old_options: &CompilerOptions,
            new_options: &CompilerOptions,
            filter: impl Fn(&CommandLineOption) -> bool,
        ) -> bool {
            $(
                if let Some(decl) = COMMAND_LINE_COMPILER_OPTIONS_MAP.get($go) {
                    if filter(decl) && field_changed!($kind, decl, old_options, new_options, $field) {
                        return true;
                    }
                }
            )*
            false
        }
    };
}

compiler_option_fields![
    ("AllowJs", "allowJs", allow_js, tri),
    (
        "AllowArbitraryExtensions",
        "allowArbitraryExtensions",
        allow_arbitrary_extensions,
        tri
    ),
    (
        "AllowImportingTsExtensions",
        "allowImportingTsExtensions",
        allow_importing_ts_extensions,
        tri
    ),
    (
        "AllowNonTsExtensions",
        "allowNonTsExtensions",
        allow_non_ts_extensions,
        tri
    ),
    (
        "AllowUmdGlobalAccess",
        "allowUmdGlobalAccess",
        allow_umd_global_access,
        tri
    ),
    (
        "AllowUnreachableCode",
        "allowUnreachableCode",
        allow_unreachable_code,
        tri
    ),
    (
        "AllowUnusedLabels",
        "allowUnusedLabels",
        allow_unused_labels,
        tri
    ),
    (
        "AssumeChangesOnlyAffectDirectDependencies",
        "assumeChangesOnlyAffectDirectDependencies",
        assume_changes_only_affect_direct_dependencies,
        tri
    ),
    ("CheckJs", "checkJs", check_js, tri),
    (
        "CustomConditions",
        "customConditions",
        custom_conditions,
        val
    ),
    ("Composite", "composite", composite, tri),
    (
        "EmitDeclarationOnly",
        "emitDeclarationOnly",
        emit_declaration_only,
        tri
    ),
    ("EmitBOM", "emitBOM", emit_bom, tri),
    (
        "EmitDecoratorMetadata",
        "emitDecoratorMetadata",
        emit_decorator_metadata,
        tri
    ),
    ("Declaration", "declaration", declaration, tri),
    ("DeclarationDir", "declarationDir", declaration_dir, val),
    ("DeclarationMap", "declarationMap", declaration_map, tri),
    (
        "DeduplicatePackages",
        "deduplicatePackages",
        deduplicate_packages,
        tri
    ),
    (
        "DisableSizeLimit",
        "disableSizeLimit",
        disable_size_limit,
        tri
    ),
    (
        "DisableSourceOfProjectReferenceRedirect",
        "disableSourceOfProjectReferenceRedirect",
        disable_source_of_project_reference_redirect,
        tri
    ),
    (
        "DisableSolutionSearching",
        "disableSolutionSearching",
        disable_solution_searching,
        tri
    ),
    (
        "DisableReferencedProjectLoad",
        "disableReferencedProjectLoad",
        disable_referenced_project_load,
        tri
    ),
    (
        "ErasableSyntaxOnly",
        "erasableSyntaxOnly",
        erasable_syntax_only,
        tri
    ),
    (
        "ExactOptionalPropertyTypes",
        "exactOptionalPropertyTypes",
        exact_optional_property_types,
        tri
    ),
    (
        "ExperimentalDecorators",
        "experimentalDecorators",
        experimental_decorators,
        tri
    ),
    (
        "ForceConsistentCasingInFileNames",
        "forceConsistentCasingInFileNames",
        force_consistent_casing_in_file_names,
        tri
    ),
    ("IsolatedModules", "isolatedModules", isolated_modules, tri),
    (
        "IsolatedDeclarations",
        "isolatedDeclarations",
        isolated_declarations,
        tri
    ),
    ("IgnoreConfig", "ignoreConfig", ignore_config, tri),
    (
        "IgnoreDeprecations",
        "ignoreDeprecations",
        ignore_deprecations,
        val
    ),
    ("ImportHelpers", "importHelpers", import_helpers, tri),
    ("InlineSourceMap", "inlineSourceMap", inline_source_map, tri),
    ("InlineSources", "inlineSources", inline_sources, tri),
    ("Init", "init", init, tri),
    ("Incremental", "incremental", incremental, tri),
    ("Jsx", "jsx", jsx, val),
    ("JsxFactory", "jsxFactory", jsx_factory, val),
    (
        "JsxFragmentFactory",
        "jsxFragmentFactory",
        jsx_fragment_factory,
        val
    ),
    ("JsxImportSource", "jsxImportSource", jsx_import_source, val),
    ("Lib", "lib", lib, val),
    ("LibReplacement", "libReplacement", lib_replacement, tri),
    ("Locale", "locale", locale, val),
    ("MapRoot", "mapRoot", map_root, val),
    ("Module", "module", module, val),
    (
        "ModuleResolution",
        "moduleResolution",
        module_resolution,
        val
    ),
    ("ModuleSuffixes", "moduleSuffixes", module_suffixes, val),
    ("ModuleDetection", "moduleDetection", module_detection, val),
    ("NewLine", "newLine", new_line, val),
    ("NoEmit", "noEmit", no_emit, tri),
    ("NoCheck", "noCheck", no_check, tri),
    (
        "NoErrorTruncation",
        "noErrorTruncation",
        no_error_truncation,
        tri
    ),
    (
        "NoFallthroughCasesInSwitch",
        "noFallthroughCasesInSwitch",
        no_fallthrough_cases_in_switch,
        tri
    ),
    ("NoImplicitAny", "noImplicitAny", no_implicit_any, tri),
    ("NoImplicitThis", "noImplicitThis", no_implicit_this, tri),
    (
        "NoImplicitReturns",
        "noImplicitReturns",
        no_implicit_returns,
        tri
    ),
    ("NoEmitHelpers", "noEmitHelpers", no_emit_helpers, tri),
    ("NoLib", "noLib", no_lib, tri),
    (
        "NoPropertyAccessFromIndexSignature",
        "noPropertyAccessFromIndexSignature",
        no_property_access_from_index_signature,
        tri
    ),
    (
        "NoUncheckedIndexedAccess",
        "noUncheckedIndexedAccess",
        no_unchecked_indexed_access,
        tri
    ),
    ("NoEmitOnError", "noEmitOnError", no_emit_on_error, tri),
    ("NoUnusedLocals", "noUnusedLocals", no_unused_locals, tri),
    (
        "NoUnusedParameters",
        "noUnusedParameters",
        no_unused_parameters,
        tri
    ),
    ("NoResolve", "noResolve", no_resolve, tri),
    (
        "NoImplicitOverride",
        "noImplicitOverride",
        no_implicit_override,
        tri
    ),
    (
        "NoUncheckedSideEffectImports",
        "noUncheckedSideEffectImports",
        no_unchecked_side_effect_imports,
        tri
    ),
    ("OutDir", "outDir", out_dir, val),
    ("Paths", "paths", paths, map),
    (
        "PreserveConstEnums",
        "preserveConstEnums",
        preserve_const_enums,
        tri
    ),
    (
        "PreserveSymlinks",
        "preserveSymlinks",
        preserve_symlinks,
        tri
    ),
    ("Project", "project", project, val),
    (
        "ResolveJsonModule",
        "resolveJsonModule",
        resolve_json_module,
        tri
    ),
    (
        "ResolvePackageJsonExports",
        "resolvePackageJsonExports",
        resolve_package_json_exports,
        tri
    ),
    (
        "ResolvePackageJsonImports",
        "resolvePackageJsonImports",
        resolve_package_json_imports,
        tri
    ),
    ("RemoveComments", "removeComments", remove_comments, tri),
    (
        "RewriteRelativeImportExtensions",
        "rewriteRelativeImportExtensions",
        rewrite_relative_import_extensions,
        tri
    ),
    ("ReactNamespace", "reactNamespace", react_namespace, val),
    ("RootDir", "rootDir", root_dir, val),
    ("RootDirs", "rootDirs", root_dirs, val),
    ("SkipLibCheck", "skipLibCheck", skip_lib_check, tri),
    (
        "StableTypeOrdering",
        "stableTypeOrdering",
        stable_type_ordering,
        tri
    ),
    ("Strict", "strict", strict, tri),
    (
        "StrictBindCallApply",
        "strictBindCallApply",
        strict_bind_call_apply,
        tri
    ),
    (
        "StrictBuiltinIteratorReturn",
        "strictBuiltinIteratorReturn",
        strict_builtin_iterator_return,
        tri
    ),
    (
        "StrictFunctionTypes",
        "strictFunctionTypes",
        strict_function_types,
        tri
    ),
    (
        "StrictNullChecks",
        "strictNullChecks",
        strict_null_checks,
        tri
    ),
    (
        "StrictPropertyInitialization",
        "strictPropertyInitialization",
        strict_property_initialization,
        tri
    ),
    ("StripInternal", "stripInternal", strip_internal, tri),
    (
        "SkipDefaultLibCheck",
        "skipDefaultLibCheck",
        skip_default_lib_check,
        tri
    ),
    ("SourceMap", "sourceMap", source_map, tri),
    ("SourceRoot", "sourceRoot", source_root, val),
    (
        "SuppressOutputPathCheck",
        "suppressOutputPathCheck",
        suppress_output_path_check,
        tri
    ),
    ("Target", "target", target, val),
    ("TraceResolution", "traceResolution", trace_resolution, tri),
    (
        "TsBuildInfoFile",
        "tsBuildInfoFile",
        ts_build_info_file,
        val
    ),
    ("TypeRoots", "typeRoots", type_roots, val),
    ("Types", "types", types, val),
    (
        "UseDefineForClassFields",
        "useDefineForClassFields",
        use_define_for_class_fields,
        tri
    ),
    (
        "UseUnknownInCatchVariables",
        "useUnknownInCatchVariables",
        use_unknown_in_catch_variables,
        tri
    ),
    (
        "VerbatimModuleSyntax",
        "verbatimModuleSyntax",
        verbatim_module_syntax,
        tri
    ),
    (
        "MaxNodeModuleJsDepth",
        "maxNodeModuleJsDepth",
        max_node_module_js_depth,
        val
    ),
    (
        "AllowSyntheticDefaultImports",
        "allowSyntheticDefaultImports",
        allow_synthetic_default_imports,
        tri
    ),
    ("AlwaysStrict", "alwaysStrict", always_strict, tri),
    ("BaseUrl", "baseUrl", base_url, val),
    (
        "DownlevelIteration",
        "downlevelIteration",
        downlevel_iteration,
        tri
    ),
    ("ESModuleInterop", "esModuleInterop", es_module_interop, tri),
    ("OutFile", "outFile", out_file, val),
    ("ConfigFilePath", "configFilePath", config_file_path, val),
    ("NoDtsResolution", "noDtsResolution", no_dts_resolution, tri),
    ("PathsBasePath", "pathsBasePath", paths_base_path, val),
    ("Diagnostics", "diagnostics", diagnostics, tri),
    (
        "ExtendedDiagnostics",
        "extendedDiagnostics",
        extended_diagnostics,
        tri
    ),
    (
        "GenerateCpuProfile",
        "generateCpuProfile",
        generate_cpu_profile,
        val
    ),
    ("GenerateTrace", "generateTrace", generate_trace, val),
    (
        "ListEmittedFiles",
        "listEmittedFiles",
        list_emitted_files,
        tri
    ),
    ("ListFiles", "listFiles", list_files, tri),
    ("ExplainFiles", "explainFiles", explain_files, tri),
    ("ListFilesOnly", "listFilesOnly", list_files_only, tri),
    (
        "NoEmitForJsFiles",
        "noEmitForJsFiles",
        no_emit_for_js_files,
        tri
    ),
    (
        "PreserveWatchOutput",
        "preserveWatchOutput",
        preserve_watch_output,
        tri
    ),
    ("Pretty", "pretty", pretty, tri),
    ("Version", "version", version, tri),
    ("Watch", "watch", watch, tri),
    ("ShowConfig", "showConfig", show_config, tri),
    ("Build", "build", build, tri),
    ("Help", "help", help, tri),
    ("All", "all", all, tri),
    ("PprofDir", "pprofDir", pprof_dir, val),
    ("SingleThreaded", "singleThreaded", single_threaded, tri),
    ("Quiet", "quiet", quiet, tri),
    ("Checkers", "checkers", checkers, val),
];

/// Reports whether any option that affects semantic diagnostics changed.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/declscompiler.go:CompilerOptionsAffectSemanticDiagnostics
pub fn compiler_options_affect_semantic_diagnostics(
    old_options: &CompilerOptions,
    new_options: &CompilerOptions,
) -> bool {
    for_each_compiler_option_value(old_options, new_options, |o| o.affects_semantic_diagnostics)
}

/// Reports whether any option that affects the declaration output path changed.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/declscompiler.go:CompilerOptionsAffectDeclarationPath
pub fn compiler_options_affect_declaration_path(
    old_options: &CompilerOptions,
    new_options: &CompilerOptions,
) -> bool {
    for_each_compiler_option_value(old_options, new_options, |o| o.affects_declaration_path)
}

/// Reports whether any option that affects emit changed.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/declscompiler.go:CompilerOptionsAffectEmit
pub fn compiler_options_affect_emit(
    old_options: &CompilerOptions,
    new_options: &CompilerOptions,
) -> bool {
    for_each_compiler_option_value(old_options, new_options, |o| o.affects_emit)
}

#[cfg(test)]
#[path = "optionsfields_test.rs"]
mod tests;
