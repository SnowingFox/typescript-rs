//! Compiler options (`CompilerOptions`) and the module/target/JSX enums it uses.
//!
//! 1:1 port of Go `internal/core/compileroptions.go`,
//! `modulekind_stringer_generated.go`, and `scripttarget_stringer_generated.go`.
//!
//! DIVERGENCE(port): `TypeRoots` is `Option<Vec<String>>` (not `Vec<String>`)
//! because `GetEffectiveTypeRoots` distinguishes an unset (`nil`) value from an
//! explicitly empty one. The other `[]string` fields do not make that
//! distinction, so they map to `Vec<String>`.

use std::fmt;

use tsgo_collections::OrderedMap;

use crate::tristate::Tristate;

/// Compiler options controlling parsing, type-checking, and emit.
///
/// A 1:1 data port of Go's `CompilerOptions`: each exported field maps to a
/// public field here, optional/pointer fields map to [`Option`], and Go's
/// tri-valued booleans map to [`Tristate`]. The derived getters reproduce the
/// fallback logic TypeScript uses to fill in unset options.
///
/// The derived [`Clone`] replaces Go's reflection-based `Clone`: it performs the
/// same shallow copy of value fields plus independent copies of the owned
/// `Vec`/`OrderedMap` fields.
///
/// Side effects: none (plain data).
// Go: internal/core/compileroptions.go:CompilerOptions
// Go: internal/core/compileroptions.go:Clone
#[derive(Clone, Debug, Default)]
pub struct CompilerOptions {
    /// Allow JavaScript files to be compiled.
    pub allow_js: Tristate,
    /// Allow importing files with arbitrary extensions.
    pub allow_arbitrary_extensions: Tristate,
    /// Allow imports to include TypeScript file extensions.
    pub allow_importing_ts_extensions: Tristate,
    /// Allow non-TypeScript extensions (internal).
    pub allow_non_ts_extensions: Tristate,
    /// Allow accessing UMD globals from modules.
    pub allow_umd_global_access: Tristate,
    /// Do not report errors on unreachable code.
    pub allow_unreachable_code: Tristate,
    /// Do not report errors on unused labels.
    pub allow_unused_labels: Tristate,
    /// Assume changes only affect direct dependencies (watch optimization).
    pub assume_changes_only_affect_direct_dependencies: Tristate,
    /// Report errors in `.js` files.
    pub check_js: Tristate,
    /// Conditions to apply when resolving package exports/imports.
    pub custom_conditions: Vec<String>,
    /// Enable project compilation (composite build).
    pub composite: Tristate,
    /// Only emit declaration files.
    pub emit_declaration_only: Tristate,
    /// Emit a UTF-8 BOM at the start of output files.
    pub emit_bom: Tristate,
    /// Emit design-type metadata for decorated declarations.
    pub emit_decorator_metadata: Tristate,
    /// Generate `.d.ts` declaration files.
    pub declaration: Tristate,
    /// Output directory for declaration files.
    pub declaration_dir: String,
    /// Generate sourcemaps for declaration files.
    pub declaration_map: Tristate,
    /// Deduplicate identical packages during resolution.
    pub deduplicate_packages: Tristate,
    /// Disable the program size limit for JavaScript.
    pub disable_size_limit: Tristate,
    /// Disable preferring source files over declaration files for references.
    pub disable_source_of_project_reference_redirect: Tristate,
    /// Disable searching upward for a solution `tsconfig`.
    pub disable_solution_searching: Tristate,
    /// Disable automatically loading referenced projects.
    pub disable_referenced_project_load: Tristate,
    /// Disallow runtime constructs that are not type-erasable.
    pub erasable_syntax_only: Tristate,
    /// Treat optional property types as written, including `undefined`.
    pub exact_optional_property_types: Tristate,
    /// Enable legacy experimental decorators.
    pub experimental_decorators: Tristate,
    /// Enforce consistent casing in file references.
    pub force_consistent_casing_in_file_names: Tristate,
    /// Treat each file as a separate module.
    pub isolated_modules: Tristate,
    /// Require declarations to be independently emittable.
    pub isolated_declarations: Tristate,
    /// Ignore the config file (internal).
    pub ignore_config: Tristate,
    /// Suppress deprecation warnings up to the given version.
    pub ignore_deprecations: String,
    /// Import emit helpers from `tslib`.
    pub import_helpers: Tristate,
    /// Emit a single inline sourcemap.
    pub inline_source_map: Tristate,
    /// Include source text in the emitted sourcemap.
    pub inline_sources: Tristate,
    /// Initialize a new `tsconfig` (CLI flag).
    pub init: Tristate,
    /// Enable incremental compilation.
    pub incremental: Tristate,
    /// JSX emit mode.
    pub jsx: JsxEmit,
    /// JSX factory function (classic transform).
    pub jsx_factory: String,
    /// JSX fragment factory function (classic transform).
    pub jsx_fragment_factory: String,
    /// Module specifier for the JSX runtime import.
    pub jsx_import_source: String,
    /// Library declaration files to include.
    pub lib: Vec<String>,
    /// Enable `lib` replacement lookups.
    pub lib_replacement: Tristate,
    /// Locale for diagnostic messages.
    pub locale: String,
    /// Root path for sourcemap locations.
    pub map_root: String,
    /// Module system to emit.
    pub module: ModuleKind,
    /// Module resolution strategy.
    pub module_resolution: ModuleResolutionKind,
    /// Suffixes to try when resolving module specifiers.
    pub module_suffixes: Vec<String>,
    /// How module format is detected.
    pub module_detection: ModuleDetectionKind,
    /// Newline style to emit.
    pub new_line: NewLineKind,
    /// Do not emit output files.
    pub no_emit: Tristate,
    /// Skip type checking (emit only).
    pub no_check: Tristate,
    /// Do not truncate error messages.
    pub no_error_truncation: Tristate,
    /// Report errors for fallthrough cases in switch statements.
    pub no_fallthrough_cases_in_switch: Tristate,
    /// Report errors for implicitly typed `any` values.
    pub no_implicit_any: Tristate,
    /// Report errors for implicitly typed `this`.
    pub no_implicit_this: Tristate,
    /// Report errors for code paths that do not return.
    pub no_implicit_returns: Tristate,
    /// Do not emit runtime helper functions.
    pub no_emit_helpers: Tristate,
    /// Do not include any default library files.
    pub no_lib: Tristate,
    /// Disallow property access via index signatures using dot notation.
    pub no_property_access_from_index_signature: Tristate,
    /// Add `undefined` to indexed access results.
    pub no_unchecked_indexed_access: Tristate,
    /// Do not emit if any errors are reported.
    pub no_emit_on_error: Tristate,
    /// Report errors for unused local variables.
    pub no_unused_locals: Tristate,
    /// Report errors for unused parameters.
    pub no_unused_parameters: Tristate,
    /// Do not resolve imports/references.
    pub no_resolve: Tristate,
    /// Require `override` on members that override a base.
    pub no_implicit_override: Tristate,
    /// Report errors for side-effect imports that cannot be checked.
    pub no_unchecked_side_effect_imports: Tristate,
    /// Output directory.
    pub out_dir: String,
    /// Path mapping table for module resolution, if set.
    pub paths: Option<OrderedMap<String, Vec<String>>>,
    /// Keep `const enum` declarations in emit.
    pub preserve_const_enums: Tristate,
    /// Do not resolve symlinks to their real paths.
    pub preserve_symlinks: Tristate,
    /// Project to compile (CLI flag).
    pub project: String,
    /// Allow importing `.json` modules.
    pub resolve_json_module: Tristate,
    /// Use `package.json` `exports` during resolution.
    pub resolve_package_json_exports: Tristate,
    /// Use `package.json` `imports` during resolution.
    pub resolve_package_json_imports: Tristate,
    /// Strip comments from emitted output.
    pub remove_comments: Tristate,
    /// Rewrite relative import extensions in emit.
    pub rewrite_relative_import_extensions: Tristate,
    /// Namespace used for non-imported JSX factories.
    pub react_namespace: String,
    /// Root directory of input files.
    pub root_dir: String,
    /// List of root directories treated as one virtual root.
    pub root_dirs: Vec<String>,
    /// Skip type checking of all declaration files.
    pub skip_lib_check: Tristate,
    /// Emit types in a stable order (internal).
    pub stable_type_ordering: Tristate,
    /// Enable all strict type-checking options.
    pub strict: Tristate,
    /// Strict `bind`/`call`/`apply` checking.
    pub strict_bind_call_apply: Tristate,
    /// Strict checking of built-in iterator return types.
    pub strict_builtin_iterator_return: Tristate,
    /// Strict function-type variance checking.
    pub strict_function_types: Tristate,
    /// Enable strict null checks.
    pub strict_null_checks: Tristate,
    /// Require class property initialization.
    pub strict_property_initialization: Tristate,
    /// Strip declarations marked `@internal`.
    pub strip_internal: Tristate,
    /// Skip type checking of default library declaration files.
    pub skip_default_lib_check: Tristate,
    /// Generate sourcemaps.
    pub source_map: Tristate,
    /// Root path for source files in sourcemaps.
    pub source_root: String,
    /// Suppress output path collision checks (internal).
    pub suppress_output_path_check: Tristate,
    /// ECMAScript language target.
    pub target: ScriptTarget,
    /// Trace module resolution for debugging.
    pub trace_resolution: Tristate,
    /// Path for the incremental build info file.
    pub ts_build_info_file: String,
    /// Type-declaration root directories, if explicitly set.
    pub type_roots: Option<Vec<String>>,
    /// Type packages to include automatically.
    pub types: Vec<String>,
    /// Emit class fields with `Object.defineProperty` semantics.
    pub use_define_for_class_fields: Tristate,
    /// Type caught values as `unknown` rather than `any`.
    pub use_unknown_in_catch_variables: Tristate,
    /// Enforce verbatim module syntax (no elision).
    pub verbatim_module_syntax: Tristate,
    /// Maximum dependency depth searched in `node_modules` JS, if set.
    pub max_node_module_js_depth: Option<i32>,

    /// Allow synthetic default imports (deprecated; parsing/validation only).
    pub allow_synthetic_default_imports: Tristate,
    /// Always emit `"use strict"` (deprecated; parsing/validation only).
    pub always_strict: Tristate,
    /// Base URL for non-relative module resolution (deprecated).
    pub base_url: String,
    /// Provide full iteration support for older targets (deprecated).
    pub downlevel_iteration: Tristate,
    /// Enable CommonJS/ES module interop (deprecated).
    pub es_module_interop: Tristate,
    /// Concatenate output into a single file (deprecated).
    pub out_file: String,

    /// Path of the originating config file (internal).
    pub config_file_path: String,
    /// Disable resolving `.d.ts` files (internal).
    pub no_dts_resolution: Tristate,
    /// Base path for `paths` mapping (internal).
    pub paths_base_path: String,
    /// Emit diagnostics (internal).
    pub diagnostics: Tristate,
    /// Emit extended diagnostics (internal).
    pub extended_diagnostics: Tristate,
    /// Path to write a CPU profile (internal).
    pub generate_cpu_profile: String,
    /// Path to write a trace (internal).
    pub generate_trace: String,
    /// List emitted files (internal).
    pub list_emitted_files: Tristate,
    /// List input files (internal).
    pub list_files: Tristate,
    /// Explain why files are included (internal).
    pub explain_files: Tristate,
    /// List files only, without compiling (internal).
    pub list_files_only: Tristate,
    /// Do not emit for `.js` files (internal).
    pub no_emit_for_js_files: Tristate,
    /// Preserve watch-mode output between runs (internal).
    pub preserve_watch_output: Tristate,
    /// Pretty-print diagnostics (internal).
    pub pretty: Tristate,
    /// Print the compiler version (internal).
    pub version: Tristate,
    /// Run in watch mode (internal).
    pub watch: Tristate,
    /// Print the resolved config (internal).
    pub show_config: Tristate,
    /// Run in `--build` mode (internal).
    pub build: Tristate,
    /// Print help (internal).
    pub help: Tristate,
    /// Show all options in help (internal).
    pub all: Tristate,

    /// Directory for pprof output (internal).
    pub pprof_dir: String,
    /// Force single-threaded execution (internal).
    pub single_threaded: Tristate,
    /// Suppress non-error output (internal).
    pub quiet: Tristate,
    /// Number of checker threads, if set (internal).
    pub checkers: Option<i32>,
}

impl CompilerOptions {
    /// Returns the effective emit target, falling back to the latest
    /// standardized target ([`ScriptTarget::LATEST_STANDARD`]) when unset.
    ///
    /// # Examples
    /// ```
    /// use tsgo_core::compileroptions::{CompilerOptions, ScriptTarget};
    /// assert_eq!(
    ///     CompilerOptions::default().get_emit_script_target(),
    ///     ScriptTarget::Es2025
    /// );
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetEmitScriptTarget
    pub fn get_emit_script_target(&self) -> ScriptTarget {
        if self.target != ScriptTarget::None {
            return self.target;
        }
        ScriptTarget::LATEST_STANDARD
    }

    /// Returns the effective emitted module kind. An explicit [`Self::module`]
    /// wins; otherwise the module kind is derived from the emit target.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetEmitModuleKind
    pub fn get_emit_module_kind(&self) -> ModuleKind {
        if self.module != ModuleKind::None {
            return self.module;
        }
        let target = self.get_emit_script_target() as i32;
        if target == ScriptTarget::EsNext as i32 {
            return ModuleKind::EsNext;
        }
        if target >= ScriptTarget::Es2022 as i32 {
            return ModuleKind::Es2022;
        }
        if target >= ScriptTarget::Es2020 as i32 {
            return ModuleKind::Es2020;
        }
        if target >= ScriptTarget::Es2015 as i32 {
            return ModuleKind::Es2015;
        }
        ModuleKind::CommonJs
    }

    /// Returns the effective module resolution kind. When the configured
    /// resolution is unset/`Classic`/`Node10`, it is derived from the emit
    /// module kind; otherwise it is returned unchanged.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetModuleResolutionKind
    pub fn get_module_resolution_kind(&self) -> ModuleResolutionKind {
        match self.module_resolution {
            ModuleResolutionKind::Unknown
            | ModuleResolutionKind::Classic
            | ModuleResolutionKind::Node10 => match self.get_emit_module_kind() {
                ModuleKind::Node16 | ModuleKind::Node18 | ModuleKind::Node20 => {
                    ModuleResolutionKind::Node16
                }
                ModuleKind::NodeNext => ModuleResolutionKind::NodeNext,
                _ => ModuleResolutionKind::Bundler,
            },
            other => other,
        }
    }

    /// Returns the effective module-detection kind. An explicit value wins;
    /// otherwise `Node16..=NodeNext` emit module kinds force detection and
    /// everything else uses `Auto`.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetEmitModuleDetectionKind
    pub fn get_emit_module_detection_kind(&self) -> ModuleDetectionKind {
        if self.module_detection != ModuleDetectionKind::None {
            return self.module_detection;
        }
        let module_kind = self.get_emit_module_kind() as i32;
        if (ModuleKind::Node16 as i32) <= module_kind
            && module_kind <= (ModuleKind::NodeNext as i32)
        {
            return ModuleDetectionKind::Force;
        }
        ModuleDetectionKind::Auto
    }

    /// Reports whether importing `.json` modules is enabled. An explicit
    /// tri-state wins; otherwise `Node20`/`NodeNext` emit module kinds enable it,
    /// falling back to whether resolution is `Bundler`.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetResolveJsonModule
    pub fn get_resolve_json_module(&self) -> bool {
        if self.resolve_json_module != Tristate::Unknown {
            return self.resolve_json_module == Tristate::True;
        }
        // TODO in 6.0: add Node16/Node18
        match self.get_emit_module_kind() {
            ModuleKind::Node20 | ModuleKind::NodeNext => return true,
            _ => {}
        }
        self.get_module_resolution_kind() == ModuleResolutionKind::Bundler
    }

    /// Resolves a `strict`-family option: an explicit per-option tri-state wins,
    /// otherwise the option is enabled iff `strict` is not explicitly false.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetStrictOptionValue
    pub fn get_strict_option_value(&self, value: Tristate) -> bool {
        if value != Tristate::Unknown {
            return value == Tristate::True;
        }
        self.strict != Tristate::False
    }

    /// Reports whether each file is treated as an isolated module. True when
    /// `isolatedModules` or `verbatimModuleSyntax` is explicitly enabled.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetIsolatedModules
    pub fn get_isolated_modules(&self) -> bool {
        self.isolated_modules == Tristate::True || self.verbatim_module_syntax == Tristate::True
    }

    /// Reports whether `const enum` declarations are preserved in emit. True
    /// when `preserveConstEnums` is enabled or isolated modules are in effect.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:ShouldPreserveConstEnums
    pub fn should_preserve_const_enums(&self) -> bool {
        self.preserve_const_enums == Tristate::True || self.get_isolated_modules()
    }

    /// Reports whether standard (spec-compliant) class fields are emitted. True
    /// when `useDefineForClassFields` is not explicitly false and the emit
    /// target is `ES2022` or later.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetEmitStandardClassFields
    pub fn get_emit_standard_class_fields(&self) -> bool {
        self.use_define_for_class_fields != Tristate::False
            && (self.get_emit_script_target() as i32) >= (ScriptTarget::Es2022 as i32)
    }

    /// Reports whether `useDefineForClassFields` is in effect. An explicit value
    /// wins; when unset it follows whether the emit target is `ES2022` or later.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetUseDefineForClassFields
    pub fn get_use_define_for_class_fields(&self) -> bool {
        if self.use_define_for_class_fields == Tristate::Unknown {
            return (self.get_emit_script_target() as i32) >= (ScriptTarget::Es2022 as i32);
        }
        self.use_define_for_class_fields == Tristate::True
    }

    /// Reports whether `.d.ts` declaration files are emitted. True when
    /// `declaration` or `composite` is enabled.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetEmitDeclarations
    pub fn get_emit_declarations(&self) -> bool {
        self.declaration.is_true() || self.composite.is_true()
    }

    /// Reports whether declaration sourcemaps are emitted. True when
    /// `declarationMap` is enabled and declarations are being emitted.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetAreDeclarationMapsEnabled
    pub fn get_are_declaration_maps_enabled(&self) -> bool {
        self.declaration_map == Tristate::True && self.get_emit_declarations()
    }

    /// Reports whether `.json` modules can be emitted. False for the `System`
    /// and `UMD` emit module kinds, true otherwise.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:HasJsonModuleEmitEnabled
    pub fn has_json_module_emit_enabled(&self) -> bool {
        !matches!(
            self.get_emit_module_kind(),
            ModuleKind::System | ModuleKind::Umd
        )
    }

    /// Reports whether incremental compilation is enabled. True when
    /// `incremental` or `composite` is enabled.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:IsIncremental
    pub fn is_incremental(&self) -> bool {
        self.incremental.is_true() || self.composite.is_true()
    }

    /// Reports whether `package.json` `exports` are consulted during resolution
    /// (enabled unless explicitly disabled).
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetResolvePackageJsonExports
    pub fn get_resolve_package_json_exports(&self) -> bool {
        self.resolve_package_json_exports.is_true_or_unknown()
    }

    /// Reports whether `package.json` `imports` are consulted during resolution
    /// (enabled unless explicitly disabled).
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetResolvePackageJsonImports
    pub fn get_resolve_package_json_imports(&self) -> bool {
        self.resolve_package_json_imports.is_true_or_unknown()
    }

    /// Reports whether imports may use TypeScript file extensions. True when
    /// `allowImportingTsExtensions` or `rewriteRelativeImportExtensions` is set.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetAllowImportingTsExtensions
    pub fn get_allow_importing_ts_extensions(&self) -> bool {
        self.allow_importing_ts_extensions.is_true()
            || self.rewrite_relative_import_extensions.is_true()
    }

    /// Like [`Self::get_allow_importing_ts_extensions`], but also true when
    /// `file_name` is a declaration file.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:AllowImportingTsExtensionsFrom
    pub fn allow_importing_ts_extensions_from(&self, file_name: &str) -> bool {
        self.get_allow_importing_ts_extensions() || tsgo_tspath::is_declaration_file_name(file_name)
    }

    /// Reports whether JavaScript files are compiled. An explicit `allowJs`
    /// wins; otherwise `checkJs` implies it.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetAllowJS
    pub fn get_allow_js(&self) -> bool {
        if self.allow_js != Tristate::Unknown {
            return self.allow_js == Tristate::True;
        }
        self.check_js == Tristate::True
    }

    /// Reports whether a JSX transform (rather than preserve/react-native) is in
    /// effect.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetJSXTransformEnabled
    pub fn get_jsx_transform_enabled(&self) -> bool {
        matches!(
            self.jsx,
            JsxEmit::React | JsxEmit::ReactJsx | JsxEmit::ReactJsxDev
        )
    }

    /// Reports whether the `types` list contains the wildcard `"*"`.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:UsesWildcardTypes
    pub fn uses_wildcard_types(&self) -> bool {
        self.types.iter().any(|t| t == "*")
    }

    /// Returns the base path used to resolve `paths` mappings. Empty when no
    /// `paths` are configured; otherwise the explicit `pathsBasePath` if set, or
    /// `current_directory`.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetPathsBasePath
    pub fn get_paths_base_path(&self, current_directory: &str) -> String {
        if self.paths.as_ref().map_or(0, |p| p.size()) == 0 {
            return String::new();
        }
        if !self.paths_base_path.is_empty() {
            return self.paths_base_path.clone();
        }
        current_directory.to_string()
    }

    /// Returns the effective `@types` roots and whether they came from explicit
    /// config. Explicit `typeRoots` are returned with `true`; otherwise the
    /// `node_modules/@types` directory of each ancestor of the config directory
    /// (or `current_directory`) is returned with `false`.
    ///
    /// # Panics
    /// Panics when neither a config file path nor a `current_directory` is
    /// available to anchor the search.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetEffectiveTypeRoots
    pub fn get_effective_type_roots(&self, current_directory: &str) -> (Vec<String>, bool) {
        if let Some(type_roots) = &self.type_roots {
            return (type_roots.clone(), true);
        }
        let base_dir = if !self.config_file_path.is_empty() {
            tsgo_tspath::get_directory_path(&self.config_file_path)
        } else {
            if current_directory.is_empty() {
                // This was accounted for in the TS codebase, but only for
                // third-party API usage where the module resolution host does
                // not provide a getCurrentDirectory().
                panic!("cannot get effective type roots without a config file path or current directory");
            }
            current_directory.to_string()
        };
        let mut type_roots: Vec<String> = Vec::new();
        tsgo_tspath::for_each_ancestor_directory::<()>(&base_dir, |dir| {
            type_roots.push(tsgo_tspath::combine_paths(dir, &["node_modules", "@types"]));
            ((), false)
        });
        (type_roots, false)
    }
}

/// How module-format detection is performed.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ModuleDetectionKind {
    /// Unset / no detection.
    #[default]
    None = 0,
    /// Automatic detection (the default).
    Auto = 1,
    /// Legacy detection.
    Legacy = 2,
    /// Always treat files as modules.
    Force = 3,
}

/// The module system emitted and used for resolution.
///
/// Discriminants are non-contiguous (e.g. `EsNext = 99`) and are relied upon by
/// range checks; comparisons use the underlying `i32` value.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ModuleKind {
    /// Unset.
    #[default]
    None = 0,
    /// CommonJS.
    CommonJs = 1,
    /// AMD (deprecated).
    Amd = 2,
    /// UMD (deprecated).
    Umd = 3,
    /// SystemJS (deprecated).
    System = 4,
    /// ES2015 modules.
    Es2015 = 5,
    /// ES2020 modules.
    Es2020 = 6,
    /// ES2022 modules.
    Es2022 = 7,
    /// Latest ES modules.
    EsNext = 99,
    /// Node 16 module system.
    Node16 = 100,
    /// Node 18 module system.
    Node18 = 101,
    /// Node 20 module system.
    Node20 = 102,
    /// Moving Node module target.
    NodeNext = 199,
    /// Emit modules as written.
    Preserve = 200,
}

impl ModuleKind {
    /// Reports whether this is a non-Node ES module kind (`ES2015..=ESNext`).
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:ModuleKind.IsNonNodeESM
    pub fn is_non_node_esm(self) -> bool {
        let v = self as i32;
        v >= ModuleKind::Es2015 as i32 && v <= ModuleKind::EsNext as i32
    }

    /// Reports whether this module kind supports import attributes.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:ModuleKind.SupportsImportAttributes
    pub fn supports_import_attributes(self) -> bool {
        let v = self as i32;
        (v >= ModuleKind::Node18 as i32 && v <= ModuleKind::NodeNext as i32)
            || self == ModuleKind::Preserve
            || self == ModuleKind::EsNext
    }
}

// Go: internal/core/modulekind_stringer_generated.go:String
impl fmt::Display for ModuleKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ModuleKind::None => "None",
            ModuleKind::CommonJs => "CommonJS",
            ModuleKind::Amd => "AMD",
            ModuleKind::Umd => "UMD",
            ModuleKind::System => "System",
            ModuleKind::Es2015 => "ES2015",
            ModuleKind::Es2020 => "ES2020",
            ModuleKind::Es2022 => "ES2022",
            ModuleKind::EsNext => "ESNext",
            ModuleKind::Node16 => "Node16",
            ModuleKind::Node18 => "Node18",
            ModuleKind::Node20 => "Node20",
            ModuleKind::NodeNext => "NodeNext",
            ModuleKind::Preserve => "Preserve",
        };
        f.write_str(s)
    }
}

/// The resolution mode of an import, an alias of [`ModuleKind`] restricted in
/// practice to `None`/`CommonJs`/`EsNext`.
// Go: internal/core/compileroptions.go:ResolutionMode
pub type ResolutionMode = ModuleKind;

/// Resolution mode: unset.
// Go: internal/core/compileroptions.go:ResolutionModeNone
pub const RESOLUTION_MODE_NONE: ModuleKind = ModuleKind::None;
/// Resolution mode: CommonJS.
// Go: internal/core/compileroptions.go:ResolutionModeCommonJS
pub const RESOLUTION_MODE_COMMON_JS: ModuleKind = ModuleKind::CommonJs;
/// Resolution mode: ES module.
// Go: internal/core/compileroptions.go:ResolutionModeESM
pub const RESOLUTION_MODE_ESM: ModuleKind = ModuleKind::EsNext;

/// The module resolution algorithm.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ModuleResolutionKind {
    /// Unset (invalid to render).
    #[default]
    Unknown = 0,
    /// Classic resolution (deprecated).
    Classic = 1,
    /// Node 10 (classic CJS) resolution (deprecated).
    Node10 = 2,
    /// Node 16 resolution.
    Node16 = 3,
    /// Moving Node resolution target.
    NodeNext = 99,
    /// Bundler resolution.
    Bundler = 100,
}

// We don't trim a common prefix here because these values are user-facing in
// `--traceResolution`, and there is no TS equivalent of the zero value, so we
// panic on it to surface porting mistakes.
// Go: internal/core/compileroptions.go:ModuleResolutionKind.String
impl fmt::Display for ModuleResolutionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ModuleResolutionKind::Unknown => {
                panic!("should not use zero value of ModuleResolutionKind")
            }
            ModuleResolutionKind::Classic => "Classic",
            ModuleResolutionKind::Node10 => "Node10",
            ModuleResolutionKind::Node16 => "Node16",
            ModuleResolutionKind::NodeNext => "NodeNext",
            ModuleResolutionKind::Bundler => "Bundler",
        };
        f.write_str(s)
    }
}

/// Maps a Node-family [`ModuleKind`] to its corresponding
/// [`ModuleResolutionKind`], or `None` for other module kinds.
///
/// Mirrors Go's `ModuleKindToModuleResolutionKind` map, where a lookup miss
/// yields the zero value with `ok == false`.
///
/// # Examples
/// ```
/// use tsgo_core::compileroptions::{
///     module_kind_to_module_resolution_kind, ModuleKind, ModuleResolutionKind,
/// };
/// assert_eq!(
///     module_kind_to_module_resolution_kind(ModuleKind::Node16),
///     Some(ModuleResolutionKind::Node16)
/// );
/// assert_eq!(module_kind_to_module_resolution_kind(ModuleKind::CommonJs), None);
/// ```
///
/// Side effects: none (pure).
// Go: internal/core/compileroptions.go:ModuleKindToModuleResolutionKind
pub fn module_kind_to_module_resolution_kind(
    module_kind: ModuleKind,
) -> Option<ModuleResolutionKind> {
    match module_kind {
        ModuleKind::Node16 => Some(ModuleResolutionKind::Node16),
        ModuleKind::NodeNext => Some(ModuleResolutionKind::NodeNext),
        _ => None,
    }
}

/// The newline style used when emitting.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum NewLineKind {
    /// Unset (defaults to LF when emitting).
    #[default]
    None = 0,
    /// `\r\n`.
    Crlf = 1,
    /// `\n`.
    Lf = 2,
}

impl NewLineKind {
    /// Returns the newline character sequence for this kind (defaults to `\n`).
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:NewLineKind.GetNewLineCharacter
    pub fn get_new_line_character(self) -> &'static str {
        match self {
            NewLineKind::Crlf => "\r\n",
            _ => "\n",
        }
    }
}

/// Parses a newline character sequence into a [`NewLineKind`].
///
/// Side effects: none (pure).
// Go: internal/core/compileroptions.go:GetNewLineKind
pub fn get_new_line_kind(s: &str) -> NewLineKind {
    match s {
        "\r\n" => NewLineKind::Crlf,
        "\n" => NewLineKind::Lf,
        _ => NewLineKind::None,
    }
}

/// The ECMAScript language target.
///
/// Discriminants are non-contiguous (e.g. `EsNext = 99`) and are relied upon by
/// range checks; comparisons use the underlying `i32` value.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ScriptTarget {
    /// Unset.
    #[default]
    None = 0,
    /// ES5 (deprecated).
    Es5 = 1,
    /// ES2015.
    Es2015 = 2,
    /// ES2016.
    Es2016 = 3,
    /// ES2017.
    Es2017 = 4,
    /// ES2018.
    Es2018 = 5,
    /// ES2019.
    Es2019 = 6,
    /// ES2020.
    Es2020 = 7,
    /// ES2021.
    Es2021 = 8,
    /// ES2022.
    Es2022 = 9,
    /// ES2023.
    Es2023 = 10,
    /// ES2024.
    Es2024 = 11,
    /// ES2025.
    Es2025 = 12,
    /// Latest ES features.
    EsNext = 99,
    /// JSON.
    Json = 100,
}

impl ScriptTarget {
    /// The latest target (`ESNext`).
    // Go: internal/core/compileroptions.go:ScriptTargetLatest
    pub const LATEST: ScriptTarget = ScriptTarget::EsNext;
    /// The latest standardized target (`ES2025`).
    // Go: internal/core/compileroptions.go:ScriptTargetLatestStandard
    pub const LATEST_STANDARD: ScriptTarget = ScriptTarget::Es2025;
}

// Go: internal/core/scripttarget_stringer_generated.go:String
impl fmt::Display for ScriptTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ScriptTarget::None => "None",
            ScriptTarget::Es5 => "ES5",
            ScriptTarget::Es2015 => "ES2015",
            ScriptTarget::Es2016 => "ES2016",
            ScriptTarget::Es2017 => "ES2017",
            ScriptTarget::Es2018 => "ES2018",
            ScriptTarget::Es2019 => "ES2019",
            ScriptTarget::Es2020 => "ES2020",
            ScriptTarget::Es2021 => "ES2021",
            ScriptTarget::Es2022 => "ES2022",
            ScriptTarget::Es2023 => "ES2023",
            ScriptTarget::Es2024 => "ES2024",
            ScriptTarget::Es2025 => "ES2025",
            ScriptTarget::EsNext => "ESNext",
            ScriptTarget::Json => "JSON",
        };
        f.write_str(s)
    }
}

/// The JSX emit mode.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum JsxEmit {
    /// Unset (invalid to render).
    #[default]
    None = 0,
    /// Preserve JSX as written.
    Preserve = 1,
    /// React Native (preserve, but `.js` extension).
    ReactNative = 2,
    /// Classic `React.createElement` transform.
    React = 3,
    /// Automatic runtime (`react-jsx`).
    ReactJsx = 4,
    /// Automatic runtime, dev build (`react-jsxdev`).
    ReactJsxDev = 5,
}

// Go: internal/core/compileroptions.go:JsxEmit.String
impl fmt::Display for JsxEmit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            JsxEmit::None => panic!("should not use zero value of JsxEmit"),
            JsxEmit::Preserve => "preserve",
            JsxEmit::ReactNative => "react-native",
            JsxEmit::React => "react",
            JsxEmit::ReactJsx => "react-jsx",
            JsxEmit::ReactJsxDev => "react-jsxdev",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
#[path = "compileroptions_test.rs"]
mod tests;
