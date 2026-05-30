//! Value parsers (`Tristate`/string/number/array/map), the `parseCompilerOptions`
//! mega-switch, compiler-option merging, and absolute-path conversion.
//!
//! 1:1 port of Go `internal/tsoptions/parsinghelpers.go`. Go reads values out of
//! `any`; here they come from [`OptionValue`]. Go's `reflect`-based
//! `mergeCompilerOptions` is replaced by an explicit field walk (see
//! [`crate::optionsfields`]).

use tsgo_collections::OrderedMap;
use tsgo_core::buildoptions::BuildOptions;
use tsgo_core::compileroptions::{
    CompilerOptions, JsxEmit, ModuleDetectionKind, ModuleKind, ModuleResolutionKind, NewLineKind,
    ScriptTarget,
};
use tsgo_core::projectreference::ProjectReference;
use tsgo_core::tristate::Tristate;
use tsgo_core::typeacquisition::TypeAcquisition;
use tsgo_core::watchoptions::{PollingKind, WatchDirectoryKind, WatchFileKind, WatchOptions};

use crate::commandlineoption::{CommandLineOptionKind, CommandLineOptionNameMap};
use crate::declscompiler::COMMAND_LINE_COMPILER_OPTIONS_MAP;
use crate::namemap::BUILD_NAME_MAP;
use crate::OptionValue;

/// Parses a value into a [`Tristate`]: `null` -> unknown, `true` -> true,
/// everything else (including `false`) -> false.
///
/// # Examples
/// ```
/// use tsgo_tsoptions::{parse_tristate, OptionValue};
/// use tsgo_core::tristate::Tristate;
/// assert_eq!(parse_tristate(&OptionValue::Null), Tristate::Unknown);
/// assert_eq!(parse_tristate(&OptionValue::Bool(true)), Tristate::True);
/// assert_eq!(parse_tristate(&OptionValue::Bool(false)), Tristate::False);
/// ```
///
/// Side effects: none (pure).
// Go: internal/tsoptions/parsinghelpers.go:ParseTristate
pub fn parse_tristate(value: &OptionValue) -> Tristate {
    match value {
        OptionValue::Null => Tristate::Unknown,
        OptionValue::Bool(true) => Tristate::True,
        _ => Tristate::False,
    }
}

/// Parses an array value into a `Vec<String>`, keeping only the string
/// elements. Returns `None` when the value is not an array (mirrors Go's
/// `nil`).
///
/// # Examples
/// ```
/// use tsgo_tsoptions::{parse_string_array, OptionValue};
/// let v = OptionValue::Array(vec![
///     OptionValue::String("a".into()),
///     OptionValue::Bool(true),
///     OptionValue::String("b".into()),
/// ]);
/// assert_eq!(parse_string_array(&v), Some(vec!["a".to_string(), "b".to_string()]));
/// assert_eq!(parse_string_array(&OptionValue::Null), None);
/// ```
///
/// Side effects: none (pure).
// Go: internal/tsoptions/parsinghelpers.go:ParseStringArray
pub fn parse_string_array(value: &OptionValue) -> Option<Vec<String>> {
    match value {
        OptionValue::Array(arr) => Some(
            arr.iter()
                .filter_map(|v| match v {
                    OptionValue::String(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    }
}

/// Parses a string value, returning `""` for any non-string value.
///
/// # Examples
/// ```
/// use tsgo_tsoptions::{parse_string, OptionValue};
/// assert_eq!(parse_string(&OptionValue::String("x".into())), "x");
/// assert_eq!(parse_string(&OptionValue::Null), "");
/// ```
///
/// Side effects: none (pure).
// Go: internal/tsoptions/parsinghelpers.go:ParseString
pub fn parse_string(value: &OptionValue) -> String {
    match value {
        OptionValue::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// Parses a number value into an `i32` (truncating a float), returning `None`
/// for any non-number value.
///
/// # Examples
/// ```
/// use tsgo_tsoptions::{parse_number, OptionValue};
/// assert_eq!(parse_number(&OptionValue::Number(2.0)), Some(2));
/// assert_eq!(parse_number(&OptionValue::Int(5)), Some(5));
/// assert_eq!(parse_number(&OptionValue::Null), None);
/// ```
///
/// Side effects: none (pure).
// Go: internal/tsoptions/parsinghelpers.go:parseNumber
pub fn parse_number(value: &OptionValue) -> Option<i32> {
    match value {
        OptionValue::Int(n) => Some(*n),
        // Go truncates toward zero via `int(num)`.
        OptionValue::Number(n) => Some(*n as i32),
        _ => None,
    }
}

/// Parses an array value into a `Vec<String>` keyed map of include lists.
///
/// Mirrors Go's `parseStringMap`: a map value becomes an ordered map of
/// string -> string-array; any other value yields `None`.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/parsinghelpers.go:parseStringMap
pub fn parse_string_map(value: &OptionValue) -> Option<OrderedMap<String, Vec<String>>> {
    match value {
        OptionValue::Map(m) => {
            let mut result = OrderedMap::with_size_hint(m.size());
            for (k, v) in m.entries() {
                result.set(k.clone(), parse_string_array(v).unwrap_or_default());
            }
            Some(result)
        }
        _ => None,
    }
}

/// Parses a project-reference object value into a list of [`ProjectReference`].
///
/// Side effects: none (pure).
// Go: internal/tsoptions/parsinghelpers.go:parseProjectReference
pub fn parse_project_reference(json: &OptionValue) -> Vec<ProjectReference> {
    let mut result = Vec::new();
    if let OptionValue::Map(m) = json {
        let mut reference = ProjectReference::default();
        if let Some(OptionValue::String(p)) = m.get(&"path".to_string()) {
            reference.path = p.clone();
        }
        if let Some(OptionValue::Bool(c)) = m.get(&"circular".to_string()) {
            reference.circular = *c;
        }
        result.push(reference);
    }
    result
}

/// Extracts the recognized top-level tsconfig keys (`include`, `exclude`,
/// `files`, `references`, `extends`, `compilerOptions`, `excludes`,
/// `typeAcquisition`) from a JSON object, preserving order.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/parsinghelpers.go:parseJsonToStringKey
pub fn parse_json_to_string_key(json: &OptionValue) -> OrderedMap<String, OptionValue> {
    let mut result = OrderedMap::with_size_hint(6);
    if let OptionValue::Map(m) = json {
        for key in [
            "include",
            "exclude",
            "files",
            "references",
            "extends",
            "compilerOptions",
            "excludes",
            "typeAcquisition",
        ] {
            if let Some(v) = m.get(&key.to_string()) {
                result.set(key.to_string(), v.clone());
            }
        }
    }
    result
}

fn option_value_to_i32(value: &OptionValue) -> i32 {
    match value {
        OptionValue::Int(n) => *n,
        // Go truncates a float discriminant via `T(value.(float64))`.
        OptionValue::Number(n) => *n as i32,
        _ => 0,
    }
}

fn script_target_from_i32(v: i32) -> ScriptTarget {
    match v {
        1 => ScriptTarget::Es5,
        2 => ScriptTarget::Es2015,
        3 => ScriptTarget::Es2016,
        4 => ScriptTarget::Es2017,
        5 => ScriptTarget::Es2018,
        6 => ScriptTarget::Es2019,
        7 => ScriptTarget::Es2020,
        8 => ScriptTarget::Es2021,
        9 => ScriptTarget::Es2022,
        10 => ScriptTarget::Es2023,
        11 => ScriptTarget::Es2024,
        12 => ScriptTarget::Es2025,
        99 => ScriptTarget::EsNext,
        100 => ScriptTarget::Json,
        _ => ScriptTarget::None,
    }
}

fn module_kind_from_i32(v: i32) -> ModuleKind {
    match v {
        1 => ModuleKind::CommonJs,
        2 => ModuleKind::Amd,
        3 => ModuleKind::Umd,
        4 => ModuleKind::System,
        5 => ModuleKind::Es2015,
        6 => ModuleKind::Es2020,
        7 => ModuleKind::Es2022,
        99 => ModuleKind::EsNext,
        100 => ModuleKind::Node16,
        101 => ModuleKind::Node18,
        102 => ModuleKind::Node20,
        199 => ModuleKind::NodeNext,
        200 => ModuleKind::Preserve,
        _ => ModuleKind::None,
    }
}

fn module_resolution_from_i32(v: i32) -> ModuleResolutionKind {
    match v {
        1 => ModuleResolutionKind::Classic,
        2 => ModuleResolutionKind::Node10,
        3 => ModuleResolutionKind::Node16,
        99 => ModuleResolutionKind::NodeNext,
        100 => ModuleResolutionKind::Bundler,
        _ => ModuleResolutionKind::Unknown,
    }
}

fn module_detection_from_i32(v: i32) -> ModuleDetectionKind {
    match v {
        1 => ModuleDetectionKind::Auto,
        2 => ModuleDetectionKind::Legacy,
        3 => ModuleDetectionKind::Force,
        _ => ModuleDetectionKind::None,
    }
}

fn jsx_emit_from_i32(v: i32) -> JsxEmit {
    match v {
        1 => JsxEmit::Preserve,
        2 => JsxEmit::ReactNative,
        3 => JsxEmit::React,
        4 => JsxEmit::ReactJsx,
        5 => JsxEmit::ReactJsxDev,
        _ => JsxEmit::None,
    }
}

fn new_line_from_i32(v: i32) -> NewLineKind {
    match v {
        1 => NewLineKind::Crlf,
        2 => NewLineKind::Lf,
        _ => NewLineKind::None,
    }
}

fn watch_file_from_i32(v: i32) -> WatchFileKind {
    match v {
        1 => WatchFileKind::FixedPollingInterval,
        2 => WatchFileKind::PriorityPollingInterval,
        3 => WatchFileKind::DynamicPriorityPolling,
        4 => WatchFileKind::FixedChunkSizePolling,
        5 => WatchFileKind::UseFsEvents,
        6 => WatchFileKind::UseFsEventsOnParentDirectory,
        _ => WatchFileKind::None,
    }
}

fn watch_directory_from_i32(v: i32) -> WatchDirectoryKind {
    match v {
        1 => WatchDirectoryKind::UseFsEvents,
        2 => WatchDirectoryKind::FixedPollingInterval,
        3 => WatchDirectoryKind::DynamicPriorityPolling,
        4 => WatchDirectoryKind::FixedChunkSizePolling,
        _ => WatchDirectoryKind::None,
    }
}

fn polling_from_i32(v: i32) -> PollingKind {
    match v {
        1 => PollingKind::FixedInterval,
        2 => PollingKind::PriorityInterval,
        3 => PollingKind::DynamicPriority,
        4 => PollingKind::FixedChunkSize,
        _ => PollingKind::None,
    }
}

/// Applies a single key/value to `all_options`, returning whether the key
/// matched a known compiler option.
///
/// This is the giant `parseCompilerOptions` switch; it is the parsing
/// counterpart to the option-declaration table and is exercised by
/// `TestParseCompilerOptionNoMissingFields`.
///
/// Side effects: mutates `all_options`.
// Go: internal/tsoptions/parsinghelpers.go:parseCompilerOptions
pub fn parse_compiler_options(
    key: &str,
    value: &OptionValue,
    all_options: &mut CompilerOptions,
) -> bool {
    // Normalize the key through the declaration map (handles aliasing).
    let key = COMMAND_LINE_COMPILER_OPTIONS_MAP
        .get(key)
        .map(|o| o.name)
        .unwrap_or(key);
    match key {
        "allowJs" => all_options.allow_js = parse_tristate(value),
        "allowImportingTsExtensions" => {
            all_options.allow_importing_ts_extensions = parse_tristate(value)
        }
        "allowSyntheticDefaultImports" => {
            all_options.allow_synthetic_default_imports = parse_tristate(value)
        }
        "allowNonTsExtensions" => all_options.allow_non_ts_extensions = parse_tristate(value),
        "allowUmdGlobalAccess" => all_options.allow_umd_global_access = parse_tristate(value),
        "allowUnreachableCode" => all_options.allow_unreachable_code = parse_tristate(value),
        "allowUnusedLabels" => all_options.allow_unused_labels = parse_tristate(value),
        "allowArbitraryExtensions" => {
            all_options.allow_arbitrary_extensions = parse_tristate(value)
        }
        "alwaysStrict" => all_options.always_strict = parse_tristate(value),
        "assumeChangesOnlyAffectDirectDependencies" => {
            all_options.assume_changes_only_affect_direct_dependencies = parse_tristate(value)
        }
        "baseUrl" => all_options.base_url = parse_string(value),
        "build" => all_options.build = parse_tristate(value),
        "checkJs" => all_options.check_js = parse_tristate(value),
        "customConditions" => {
            all_options.custom_conditions = parse_string_array(value).unwrap_or_default()
        }
        "composite" => all_options.composite = parse_tristate(value),
        "declarationDir" => all_options.declaration_dir = parse_string(value),
        "deduplicatePackages" => all_options.deduplicate_packages = parse_tristate(value),
        "diagnostics" => all_options.diagnostics = parse_tristate(value),
        "disableSizeLimit" => all_options.disable_size_limit = parse_tristate(value),
        "disableSourceOfProjectReferenceRedirect" => {
            all_options.disable_source_of_project_reference_redirect = parse_tristate(value)
        }
        "disableSolutionSearching" => {
            all_options.disable_solution_searching = parse_tristate(value)
        }
        "disableReferencedProjectLoad" => {
            all_options.disable_referenced_project_load = parse_tristate(value)
        }
        "declarationMap" => all_options.declaration_map = parse_tristate(value),
        "declaration" => all_options.declaration = parse_tristate(value),
        "downlevelIteration" => all_options.downlevel_iteration = parse_tristate(value),
        "erasableSyntaxOnly" => all_options.erasable_syntax_only = parse_tristate(value),
        "emitDeclarationOnly" => all_options.emit_declaration_only = parse_tristate(value),
        "extendedDiagnostics" => all_options.extended_diagnostics = parse_tristate(value),
        "emitDecoratorMetadata" => all_options.emit_decorator_metadata = parse_tristate(value),
        "emitBOM" => all_options.emit_bom = parse_tristate(value),
        "esModuleInterop" => all_options.es_module_interop = parse_tristate(value),
        "exactOptionalPropertyTypes" => {
            all_options.exact_optional_property_types = parse_tristate(value)
        }
        "explainFiles" => all_options.explain_files = parse_tristate(value),
        "experimentalDecorators" => all_options.experimental_decorators = parse_tristate(value),
        "forceConsistentCasingInFileNames" => {
            all_options.force_consistent_casing_in_file_names = parse_tristate(value)
        }
        "generateCpuProfile" => all_options.generate_cpu_profile = parse_string(value),
        "generateTrace" => all_options.generate_trace = parse_string(value),
        "isolatedModules" => all_options.isolated_modules = parse_tristate(value),
        "ignoreConfig" => all_options.ignore_config = parse_tristate(value),
        "ignoreDeprecations" => all_options.ignore_deprecations = parse_string(value),
        "importHelpers" => all_options.import_helpers = parse_tristate(value),
        "incremental" => all_options.incremental = parse_tristate(value),
        "init" => all_options.init = parse_tristate(value),
        "inlineSourceMap" => all_options.inline_source_map = parse_tristate(value),
        "inlineSources" => all_options.inline_sources = parse_tristate(value),
        "isolatedDeclarations" => all_options.isolated_declarations = parse_tristate(value),
        "jsx" => all_options.jsx = jsx_emit_from_i32(option_value_to_i32(value)),
        "jsxFactory" => all_options.jsx_factory = parse_string(value),
        "jsxFragmentFactory" => all_options.jsx_fragment_factory = parse_string(value),
        "jsxImportSource" => all_options.jsx_import_source = parse_string(value),
        "lib" => all_options.lib = parse_string_array(value).unwrap_or_default(),
        "libReplacement" => all_options.lib_replacement = parse_tristate(value),
        "listEmittedFiles" => all_options.list_emitted_files = parse_tristate(value),
        "listFiles" => all_options.list_files = parse_tristate(value),
        "listFilesOnly" => all_options.list_files_only = parse_tristate(value),
        "locale" => all_options.locale = parse_string(value),
        "mapRoot" => all_options.map_root = parse_string(value),
        "module" => all_options.module = module_kind_from_i32(option_value_to_i32(value)),
        // Alias case present in Go's switch.
        "moduleDetectionKind" => {
            all_options.module_detection = module_detection_from_i32(option_value_to_i32(value))
        }
        "moduleResolution" => {
            all_options.module_resolution = module_resolution_from_i32(option_value_to_i32(value))
        }
        "moduleSuffixes" => {
            all_options.module_suffixes = parse_string_array(value).unwrap_or_default()
        }
        "moduleDetection" => {
            all_options.module_detection = module_detection_from_i32(option_value_to_i32(value))
        }
        "noCheck" => all_options.no_check = parse_tristate(value),
        "noFallthroughCasesInSwitch" => {
            all_options.no_fallthrough_cases_in_switch = parse_tristate(value)
        }
        "noEmitForJsFiles" => all_options.no_emit_for_js_files = parse_tristate(value),
        "noErrorTruncation" => all_options.no_error_truncation = parse_tristate(value),
        "noImplicitAny" => all_options.no_implicit_any = parse_tristate(value),
        "noImplicitThis" => all_options.no_implicit_this = parse_tristate(value),
        "noLib" => all_options.no_lib = parse_tristate(value),
        "noPropertyAccessFromIndexSignature" => {
            all_options.no_property_access_from_index_signature = parse_tristate(value)
        }
        "noUncheckedIndexedAccess" => {
            all_options.no_unchecked_indexed_access = parse_tristate(value)
        }
        "noEmitHelpers" => all_options.no_emit_helpers = parse_tristate(value),
        "noEmitOnError" => all_options.no_emit_on_error = parse_tristate(value),
        "noImplicitReturns" => all_options.no_implicit_returns = parse_tristate(value),
        "noUnusedLocals" => all_options.no_unused_locals = parse_tristate(value),
        "noUnusedParameters" => all_options.no_unused_parameters = parse_tristate(value),
        "noImplicitOverride" => all_options.no_implicit_override = parse_tristate(value),
        "noUncheckedSideEffectImports" => {
            all_options.no_unchecked_side_effect_imports = parse_tristate(value)
        }
        "outFile" => all_options.out_file = parse_string(value),
        "noResolve" => all_options.no_resolve = parse_tristate(value),
        "paths" => all_options.paths = parse_string_map(value),
        "preserveWatchOutput" => all_options.preserve_watch_output = parse_tristate(value),
        "preserveConstEnums" => all_options.preserve_const_enums = parse_tristate(value),
        "preserveSymlinks" => all_options.preserve_symlinks = parse_tristate(value),
        "project" => all_options.project = parse_string(value),
        "pretty" => all_options.pretty = parse_tristate(value),
        "resolveJsonModule" => all_options.resolve_json_module = parse_tristate(value),
        "resolvePackageJsonExports" => {
            all_options.resolve_package_json_exports = parse_tristate(value)
        }
        "resolvePackageJsonImports" => {
            all_options.resolve_package_json_imports = parse_tristate(value)
        }
        "reactNamespace" => all_options.react_namespace = parse_string(value),
        "rewriteRelativeImportExtensions" => {
            all_options.rewrite_relative_import_extensions = parse_tristate(value)
        }
        "rootDir" => all_options.root_dir = parse_string(value),
        "rootDirs" => all_options.root_dirs = parse_string_array(value).unwrap_or_default(),
        "removeComments" => all_options.remove_comments = parse_tristate(value),
        "stableTypeOrdering" => all_options.stable_type_ordering = parse_tristate(value),
        "strict" => all_options.strict = parse_tristate(value),
        "strictBindCallApply" => all_options.strict_bind_call_apply = parse_tristate(value),
        "strictBuiltinIteratorReturn" => {
            all_options.strict_builtin_iterator_return = parse_tristate(value)
        }
        "strictFunctionTypes" => all_options.strict_function_types = parse_tristate(value),
        "strictNullChecks" => all_options.strict_null_checks = parse_tristate(value),
        "strictPropertyInitialization" => {
            all_options.strict_property_initialization = parse_tristate(value)
        }
        "skipDefaultLibCheck" => all_options.skip_default_lib_check = parse_tristate(value),
        "sourceMap" => all_options.source_map = parse_tristate(value),
        "sourceRoot" => all_options.source_root = parse_string(value),
        "stripInternal" => all_options.strip_internal = parse_tristate(value),
        "suppressOutputPathCheck" => all_options.suppress_output_path_check = parse_tristate(value),
        "target" => all_options.target = script_target_from_i32(option_value_to_i32(value)),
        "traceResolution" => all_options.trace_resolution = parse_tristate(value),
        "tsBuildInfoFile" => all_options.ts_build_info_file = parse_string(value),
        "typeRoots" => all_options.type_roots = parse_string_array(value),
        "types" => all_options.types = parse_string_array(value).unwrap_or_default(),
        "useDefineForClassFields" => {
            all_options.use_define_for_class_fields = parse_tristate(value)
        }
        "useUnknownInCatchVariables" => {
            all_options.use_unknown_in_catch_variables = parse_tristate(value)
        }
        "verbatimModuleSyntax" => all_options.verbatim_module_syntax = parse_tristate(value),
        "version" => all_options.version = parse_tristate(value),
        "help" => all_options.help = parse_tristate(value),
        "all" => all_options.all = parse_tristate(value),
        "maxNodeModuleJsDepth" => all_options.max_node_module_js_depth = parse_number(value),
        "skipLibCheck" => all_options.skip_lib_check = parse_tristate(value),
        "noEmit" => all_options.no_emit = parse_tristate(value),
        "showConfig" => all_options.show_config = parse_tristate(value),
        "configFilePath" => all_options.config_file_path = parse_string(value),
        "noDtsResolution" => all_options.no_dts_resolution = parse_tristate(value),
        "pathsBasePath" => all_options.paths_base_path = parse_string(value),
        "outDir" => all_options.out_dir = parse_string(value),
        "newLine" => all_options.new_line = new_line_from_i32(option_value_to_i32(value)),
        "watch" => all_options.watch = parse_tristate(value),
        "pprofDir" => all_options.pprof_dir = parse_string(value),
        "singleThreaded" => all_options.single_threaded = parse_tristate(value),
        "quiet" => all_options.quiet = parse_tristate(value),
        "checkers" => all_options.checkers = parse_number(value),
        _ => return false,
    }
    true
}

/// Public wrapper around [`parse_compiler_options`]: a `null` value or a key
/// that does not match is a no-op (Go returns no diagnostics here).
///
/// Side effects: mutates `all_options`.
// Go: internal/tsoptions/parsinghelpers.go:ParseCompilerOptions
pub fn parse_compiler_options_pub(
    key: &str,
    value: &OptionValue,
    all_options: &mut CompilerOptions,
) {
    if value.is_null() {
        return;
    }
    parse_compiler_options(key, value, all_options);
}

/// Applies a single key/value to watch options.
///
/// Side effects: mutates `all_options`.
// Go: internal/tsoptions/parsinghelpers.go:ParseWatchOptions
pub fn parse_watch_options(key: &str, value: &OptionValue, all_options: &mut WatchOptions) {
    match key {
        "watchInterval" => all_options.interval = parse_number(value),
        "watchFile" => {
            if !value.is_null() {
                all_options.file_kind = watch_file_from_i32(option_value_to_i32(value));
            }
        }
        "watchDirectory" => {
            if !value.is_null() {
                all_options.directory_kind = watch_directory_from_i32(option_value_to_i32(value));
            }
        }
        "fallbackPolling" => {
            if !value.is_null() {
                all_options.fallback_polling = polling_from_i32(option_value_to_i32(value));
            }
        }
        "synchronousWatchDirectory" => all_options.sync_watch_dir = parse_tristate(value),
        "excludeDirectories" => {
            all_options.exclude_dir = parse_string_array(value).unwrap_or_default()
        }
        "excludeFiles" => all_options.exclude_files = parse_string_array(value).unwrap_or_default(),
        _ => {}
    }
}

/// Applies a single key/value to type-acquisition options.
///
/// Side effects: mutates `all_options`.
// Go: internal/tsoptions/parsinghelpers.go:ParseTypeAcquisition
pub fn parse_type_acquisition(key: &str, value: &OptionValue, all_options: &mut TypeAcquisition) {
    if value.is_null() {
        return;
    }
    match key {
        "enable" => all_options.enable = parse_tristate(value),
        "include" => all_options.include = parse_string_array(value).unwrap_or_default(),
        "exclude" => all_options.exclude = parse_string_array(value).unwrap_or_default(),
        "disableFilenameBasedTypeAcquisition" => {
            all_options.disable_filename_based_type_acquisition = parse_tristate(value)
        }
        _ => {}
    }
}

/// Applies a single key/value to build options.
///
/// Side effects: mutates `all_options`.
// Go: internal/tsoptions/parsinghelpers.go:ParseBuildOptions
pub fn parse_build_options(key: &str, value: &OptionValue, all_options: &mut BuildOptions) {
    if value.is_null() {
        return;
    }
    let key = BUILD_NAME_MAP.get(key).map(|o| o.name).unwrap_or(key);
    match key {
        "clean" => all_options.clean = parse_tristate(value),
        "dry" => all_options.dry = parse_tristate(value),
        "force" => all_options.force = parse_tristate(value),
        "builders" => all_options.builders = parse_number(value),
        "stopBuildOnErrors" => all_options.stop_build_on_errors = parse_tristate(value),
        "verbose" => all_options.verbose = parse_tristate(value),
        _ => {}
    }
}

/// Rewrites file-path option values in `options_base` to absolute paths.
///
/// Side effects: mutates and returns `options_base`.
// Go: internal/tsoptions/parsinghelpers.go:convertToOptionsWithAbsolutePaths
pub fn convert_to_options_with_absolute_paths(
    mut options_base: OrderedMap<String, OptionValue>,
    option_map: &CommandLineOptionNameMap,
    cwd: &str,
) -> OrderedMap<String, OptionValue> {
    let keys: Vec<String> = options_base.keys().cloned().collect();
    for o in keys {
        if let Some(v) = options_base.get(&o) {
            if let Some(result) = convert_option_to_absolute_path(&o, v, option_map, cwd) {
                options_base.set(o, result);
            }
        }
    }
    options_base
}

/// Converts a single file-path option value to an absolute path, returning
/// `None` when the option is unknown or not a file path.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/parsinghelpers.go:ConvertOptionToAbsolutePath
pub fn convert_option_to_absolute_path(
    o: &str,
    v: &OptionValue,
    option_map: &CommandLineOptionNameMap,
    cwd: &str,
) -> Option<OptionValue> {
    let option = option_map.get(o)?;
    if option.kind == CommandLineOptionKind::List {
        if option.elements().is_some_and(|e| e.is_file_path) {
            if let OptionValue::Array(arr) = v {
                let mapped = arr
                    .iter()
                    .map(|item| match item {
                        OptionValue::String(s) => {
                            OptionValue::String(tsgo_tspath::get_normalized_absolute_path(s, cwd))
                        }
                        other => other.clone(),
                    })
                    .collect();
                return Some(OptionValue::Array(mapped));
            }
        }
    } else if option.is_file_path {
        if let OptionValue::String(s) = v {
            return Some(OptionValue::String(
                tsgo_tspath::get_normalized_absolute_path(s, cwd),
            ));
        }
    }
    None
}

#[cfg(test)]
#[path = "parsinghelpers_test.rs"]
mod tests;
