//! `tsgo_outputpaths` — 1:1 Rust port of Go `internal/outputpaths`.
//!
//! Computes the emit output paths for a source file (`.js`, source map,
//! `.d.ts`, declaration map, and `.tsbuildinfo`) plus the common source
//! directory used to re-root outputs under `outDir`.
//!
//! Everything here is pure path arithmetic: no file-system access happens in
//! this crate. Whether a file exists, the current directory, and case
//! sensitivity are all supplied by the caller through [`OutputPathsHost`].

mod commonsourcedirectory;

pub use commonsourcedirectory::{
    get_common_source_directory, get_computed_common_source_directory,
};

use std::cmp::Ordering;

use tsgo_core::compileroptions::{CompilerOptions, JsxEmit};
use tsgo_core::scriptkind::ScriptKind;
use tsgo_core::tristate::Tristate;
use tsgo_tspath::{
    change_extension, combine_paths, compare_paths, contains_path,
    ensure_trailing_directory_separator, file_extension_is, file_extension_is_one_of,
    get_base_file_name, get_canonical_file_name, get_declaration_emit_extension_for_path,
    get_normalized_absolute_path, get_relative_path_from_directory, remove_file_extension,
    resolve_path, ComparePathsOptions, EXTENSION_CJS, EXTENSION_CTS, EXTENSION_JS, EXTENSION_JSON,
    EXTENSION_JSX, EXTENSION_MJS, EXTENSION_MTS, EXTENSION_TSX, EXTENSION_TS_BUILD_INFO,
};

/// Read-only environment an emit needs to resolve output paths.
///
/// Mirrors Go's `OutputPathsHost` interface: the compiler supplies the common
/// source directory, the current working directory, and whether file names are
/// compared case-sensitively. None of the methods perform I/O from this crate's
/// point of view; they are simple accessors implemented by the host.
///
/// # Examples
/// ```
/// use tsgo_outputpaths::OutputPathsHost;
/// struct H;
/// impl OutputPathsHost for H {
///     fn common_source_directory(&self) -> String { "/src/".into() }
///     fn get_current_directory(&self) -> String { "/".into() }
///     fn use_case_sensitive_file_names(&self) -> bool { true }
/// }
/// assert_eq!(H.common_source_directory(), "/src/");
/// ```
///
/// Side effects: none (accessors).
// Go: internal/outputpaths/outputpaths.go:OutputPathsHost
pub trait OutputPathsHost {
    /// Returns the longest common directory of the program's input files,
    /// including a trailing directory separator.
    ///
    /// Side effects: none (accessor).
    fn common_source_directory(&self) -> String;

    /// Returns the current working directory used to root relative paths.
    ///
    /// Side effects: none (accessor).
    fn get_current_directory(&self) -> String;

    /// Reports whether file names are compared case-sensitively.
    ///
    /// Side effects: none (accessor).
    fn use_case_sensitive_file_names(&self) -> bool;
}

/// Read-only view of the `ast.SourceFile` fields this crate consumes.
///
/// DEFER(phase-4): the real `tsgo_ast::SourceFile` node is not ported yet
/// (`tsgo_ast` currently exposes only a representative spread of node
/// variants). Output-path computation only needs a source file's name and
/// script kind, so we abstract over those two reads here. Once `tsgo_ast`
/// lands `SourceFile`, implement this trait for it (or replace the bound) with
/// no change to the call sites.
/// blocked-by: tsgo_ast does not yet port `ast.SourceFile` / `ast.IsJsonSourceFile`.
///
/// # Examples
/// ```
/// use tsgo_outputpaths::SourceFileLike;
/// use tsgo_core::scriptkind::ScriptKind;
/// struct F;
/// impl SourceFileLike for F {
///     fn file_name(&self) -> &str { "/a/b.ts" }
///     fn script_kind(&self) -> ScriptKind { ScriptKind::Ts }
/// }
/// assert_eq!(F.file_name(), "/a/b.ts");
/// ```
///
/// Side effects: none (accessors).
// Go: internal/ast/ast.go:SourceFile.FileName
pub trait SourceFileLike {
    /// Returns the source file's name (its `parseOptions.FileName`).
    ///
    /// Side effects: none (accessor).
    fn file_name(&self) -> &str;

    /// Returns the source file's [`ScriptKind`].
    ///
    /// Side effects: none (accessor).
    fn script_kind(&self) -> ScriptKind;
}

/// The set of output file paths emitted for a single source file.
///
/// Each field is empty (`""`) when that artifact is not produced for the file.
/// Fields are private and read through the getters, mirroring Go's
/// `OutputPaths` struct + accessor methods.
///
/// Side effects: none (plain data).
// Go: internal/outputpaths/outputpaths.go:OutputPaths
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputPaths {
    js_file_path: String,
    source_map_file_path: String,
    declaration_file_path: String,
    declaration_map_path: String,
}

impl OutputPaths {
    /// Returns the JavaScript output path, or `""` if no `.js` is emitted.
    ///
    /// Side effects: none (accessor).
    // Go: internal/outputpaths/outputpaths.go:OutputPaths.JsFilePath
    pub fn js_file_path(&self) -> &str {
        &self.js_file_path
    }

    /// Returns the source map output path, or `""` if no map is emitted.
    ///
    /// Side effects: none (accessor).
    // Go: internal/outputpaths/outputpaths.go:OutputPaths.SourceMapFilePath
    pub fn source_map_file_path(&self) -> &str {
        &self.source_map_file_path
    }

    /// Returns the declaration (`.d.ts`) output path, or `""` if none.
    ///
    /// Side effects: none (accessor).
    // Go: internal/outputpaths/outputpaths.go:OutputPaths.DeclarationFilePath
    pub fn declaration_file_path(&self) -> &str {
        &self.declaration_file_path
    }

    /// Returns the declaration map output path, or `""` if none.
    ///
    /// Side effects: none (accessor).
    // Go: internal/outputpaths/outputpaths.go:OutputPaths.DeclarationMapPath
    pub fn declaration_map_path(&self) -> &str {
        &self.declaration_map_path
    }
}

/// Reports whether `file` is a JSON source file.
///
/// Ported locally because `tsgo_ast` does not yet expose
/// `ast.IsJsonSourceFile`; the check is identical (`ScriptKind == JSON`).
///
/// Side effects: none (pure).
// Go: internal/ast/utilities.go:IsJsonSourceFile
fn is_json_source_file(file: &impl SourceFileLike) -> bool {
    file.script_kind() == ScriptKind::Json
}

/// Returns the JavaScript output extension implied by `file_name` and the JSX
/// emit mode.
///
/// JSON keeps `.json`; `.tsx`/`.jsx` become `.jsx` only under
/// [`JsxEmit::Preserve`]; `.mts`/`.mjs` become `.mjs`; `.cts`/`.cjs` become
/// `.cjs`; everything else becomes `.js`.
///
/// # Examples
/// ```
/// use tsgo_outputpaths::get_output_extension;
/// use tsgo_core::compileroptions::JsxEmit;
/// assert_eq!(get_output_extension("/a/b.ts", JsxEmit::None), ".js");
/// assert_eq!(get_output_extension("/a/b.tsx", JsxEmit::Preserve), ".jsx");
/// assert_eq!(get_output_extension("/a/b.mts", JsxEmit::None), ".mjs");
/// ```
///
/// Side effects: none (pure).
// Go: internal/outputpaths/outputpaths.go:GetOutputExtension
pub fn get_output_extension(file_name: &str, jsx: JsxEmit) -> &'static str {
    if file_extension_is(file_name, EXTENSION_JSON) {
        EXTENSION_JSON
    } else if jsx == JsxEmit::Preserve
        && file_extension_is_one_of(file_name, &[EXTENSION_JSX, EXTENSION_TSX])
    {
        EXTENSION_JSX
    } else if file_extension_is_one_of(file_name, &[EXTENSION_MTS, EXTENSION_MJS]) {
        EXTENSION_MJS
    } else if file_extension_is_one_of(file_name, &[EXTENSION_CTS, EXTENSION_CJS]) {
        EXTENSION_CJS
    } else {
        EXTENSION_JS
    }
}

/// Returns the source map path for `js_file_path`, or `""` when source maps are
/// off or inlined.
///
/// A separate `.map` file is emitted only when `sourceMap` is enabled and
/// `inlineSourceMap` is not.
///
/// # Examples
/// ```
/// use tsgo_outputpaths::get_source_map_file_path;
/// use tsgo_core::compileroptions::CompilerOptions;
/// use tsgo_core::tristate::Tristate;
/// let opts = CompilerOptions { source_map: Tristate::True, ..Default::default() };
/// assert_eq!(get_source_map_file_path("/out/a.js", &opts), "/out/a.js.map");
/// ```
///
/// Side effects: none (pure).
// Go: internal/outputpaths/outputpaths.go:GetSourceMapFilePath
pub fn get_source_map_file_path(js_file_path: &str, options: &CompilerOptions) -> String {
    if options.source_map.is_true() && !options.inline_source_map.is_true() {
        format!("{js_file_path}.map")
    } else {
        String::new()
    }
}

/// Re-roots `file_name` under `new_dir_path`, stripping the common source
/// directory prefix when the file lives inside it.
///
/// Containment is tested with `tspath::contains_path`, i.e. a component-wise
/// comparison of the path against the (trailing-separator-normalized) common
/// source directory.
///
/// DIVERGENCE(port): this function and
/// [`get_source_file_path_in_new_dir_worker`] use two *different* prefix tests
/// (`ContainsPath` vs `strings.HasPrefix`). This is a historical quirk of the
/// Go upstream and both must be preserved 1:1; do not unify them.
///
/// Side effects: none (pure).
// Go: internal/outputpaths/outputpaths.go:GetSourceFilePathInNewDir
pub fn get_source_file_path_in_new_dir(
    file_name: &str,
    new_dir_path: &str,
    current_directory: &str,
    common_source_directory: &str,
    use_case_sensitive_file_names: bool,
) -> String {
    let mut source_file_path = get_normalized_absolute_path(file_name, current_directory);
    let common_source_directory = ensure_trailing_directory_separator(common_source_directory);
    let is_source_file_in_common_source_directory = contains_path(
        &common_source_directory,
        &source_file_path,
        &ComparePathsOptions {
            use_case_sensitive_file_names,
            current_directory: current_directory.to_string(),
        },
    );
    if is_source_file_in_common_source_directory {
        source_file_path = source_file_path[common_source_directory.len()..].to_string();
    }
    combine_paths(new_dir_path, &[&source_file_path])
}

/// Re-roots `file_name` under `new_dir_path`, stripping the common source
/// directory prefix when the file lives inside it.
///
/// Containment is tested with a raw `starts_with` on the canonicalized paths
/// (no trailing separator is appended to the common directory first).
///
/// DIVERGENCE(port): see [`get_source_file_path_in_new_dir`]; the two prefix
/// tests intentionally differ and are both kept 1:1 with Go.
///
/// Side effects: none (pure).
// Go: internal/outputpaths/outputpaths.go:GetSourceFilePathInNewDirWorker
pub fn get_source_file_path_in_new_dir_worker(
    file_name: &str,
    new_dir_path: &str,
    current_directory: &str,
    common_source_directory: &str,
    use_case_sensitive_file_names: bool,
) -> String {
    let mut source_file_path = get_normalized_absolute_path(file_name, current_directory);
    let common_dir =
        get_canonical_file_name(common_source_directory, use_case_sensitive_file_names);
    let canon_file = get_canonical_file_name(&source_file_path, use_case_sensitive_file_names);
    let is_source_file_in_common_source_directory = canon_file.starts_with(&common_dir);
    if is_source_file_in_common_source_directory {
        source_file_path = source_file_path[common_source_directory.len()..].to_string();
    }
    combine_paths(new_dir_path, &[&source_file_path])
}

/// Returns the `.js` output file name for `input_file_name`, or `""` when
/// `emitDeclarationOnly` is set or when a `.json` input would be written back to
/// its own location.
///
/// # Examples
/// ```
/// use tsgo_outputpaths::{get_output_js_file_name, OutputPathsHost};
/// use tsgo_core::compileroptions::CompilerOptions;
/// struct H;
/// impl OutputPathsHost for H {
///     fn common_source_directory(&self) -> String { "/src/".into() }
///     fn get_current_directory(&self) -> String { "/".into() }
///     fn use_case_sensitive_file_names(&self) -> bool { true }
/// }
/// let opts = CompilerOptions { out_dir: "/out".into(), ..Default::default() };
/// assert_eq!(get_output_js_file_name("/src/a.ts", &opts, &H), "/out/a.js");
/// ```
///
/// Side effects: none (pure).
// Go: internal/outputpaths/outputpaths.go:GetOutputJSFileName
pub fn get_output_js_file_name(
    input_file_name: &str,
    options: &CompilerOptions,
    host: &impl OutputPathsHost,
) -> String {
    if options.emit_declaration_only.is_true() {
        return String::new();
    }
    let output_file_name = get_output_js_file_name_worker(input_file_name, options, host);
    if !file_extension_is(&output_file_name, EXTENSION_JSON)
        || compare_paths(
            input_file_name,
            &output_file_name,
            &ComparePathsOptions {
                current_directory: host.get_current_directory(),
                use_case_sensitive_file_names: host.use_case_sensitive_file_names(),
            },
        ) != Ordering::Equal
    {
        return output_file_name;
    }
    String::new()
}

/// Computes the `.js` output path for `input_file_name` without the
/// JSON/same-location guard applied by [`get_output_js_file_name`].
///
/// Side effects: none (pure).
// Go: internal/outputpaths/outputpaths.go:GetOutputJSFileNameWorker
pub fn get_output_js_file_name_worker(
    input_file_name: &str,
    options: &CompilerOptions,
    host: &impl OutputPathsHost,
) -> String {
    change_extension(
        &get_output_path_without_changing_extension(input_file_name, &options.out_dir, host),
        get_output_extension(input_file_name, options.jsx),
    )
}

/// Computes the declaration output path for `input_file_name`, choosing
/// `declarationDir` (else `outDir`) as the output directory.
///
/// Unlike [`get_declaration_emit_output_file_path`], the declaration extension
/// is derived from the *input* file name and applied with `ChangeExtension`.
///
/// Side effects: none (pure).
// Go: internal/outputpaths/outputpaths.go:GetOutputDeclarationFileNameWorker
pub fn get_output_declaration_file_name_worker(
    input_file_name: &str,
    options: &CompilerOptions,
    host: &impl OutputPathsHost,
) -> String {
    let dir = if options.declaration_dir.is_empty() {
        &options.out_dir
    } else {
        &options.declaration_dir
    };
    change_extension(
        &get_output_path_without_changing_extension(input_file_name, dir, host),
        &get_declaration_emit_extension_for_path(input_file_name),
    )
}

/// Re-roots `input_file_name` under `output_directory` (relative to the common
/// source directory) without changing its extension; returns the input
/// unchanged when `output_directory` is empty.
///
/// Side effects: none (pure).
// Go: internal/outputpaths/outputpaths.go:getOutputPathWithoutChangingExtension
fn get_output_path_without_changing_extension(
    input_file_name: &str,
    output_directory: &str,
    host: &impl OutputPathsHost,
) -> String {
    if !output_directory.is_empty() {
        let relative = get_relative_path_from_directory(
            &host.common_source_directory(),
            input_file_name,
            &ComparePathsOptions {
                use_case_sensitive_file_names: host.use_case_sensitive_file_names(),
                current_directory: host.get_current_directory(),
            },
        );
        resolve_path(output_directory, &[&relative])
    } else {
        input_file_name.to_string()
    }
}

/// Computes this file's own `.js` (or `.json`/`.mjs`/`.cjs`) output path,
/// re-rooting it under `outDir` when set.
///
/// Side effects: none (pure).
// Go: internal/outputpaths/outputpaths.go:getOwnEmitOutputFilePath
fn get_own_emit_output_file_path(
    file_name: &str,
    options: &CompilerOptions,
    host: &impl OutputPathsHost,
    extension: &str,
) -> String {
    let emit_output_file_path_without_extension = if !options.out_dir.is_empty() {
        let current_directory = host.get_current_directory();
        remove_file_extension(&get_source_file_path_in_new_dir(
            file_name,
            &options.out_dir,
            &current_directory,
            &host.common_source_directory(),
            host.use_case_sensitive_file_names(),
        ))
        .to_string()
    } else {
        remove_file_extension(file_name).to_string()
    };
    format!("{emit_output_file_path_without_extension}{extension}")
}

/// Computes the declaration (`.d.ts`/`.d.mts`/`.d.cts`) output path for `file`.
///
/// The output directory is `declarationDir` if set, otherwise `outDir`, and the
/// file is re-rooted there (via [`get_source_file_path_in_new_dir_worker`]);
/// with neither set the file keeps its own directory.
///
/// # Examples
/// ```
/// use tsgo_outputpaths::{get_declaration_emit_output_file_path, OutputPathsHost};
/// use tsgo_core::compileroptions::CompilerOptions;
/// struct H;
/// impl OutputPathsHost for H {
///     fn common_source_directory(&self) -> String { "/src/".into() }
///     fn get_current_directory(&self) -> String { "/".into() }
///     fn use_case_sensitive_file_names(&self) -> bool { true }
/// }
/// let opts = CompilerOptions { declaration_dir: "/types".into(), ..Default::default() };
/// assert_eq!(
///     get_declaration_emit_output_file_path("/src/a/b.ts", &opts, &H),
///     "/types/a/b.d.ts"
/// );
/// ```
///
/// Side effects: none (pure).
// Go: internal/outputpaths/outputpaths.go:GetDeclarationEmitOutputFilePath
pub fn get_declaration_emit_output_file_path(
    file: &str,
    options: &CompilerOptions,
    host: &impl OutputPathsHost,
) -> String {
    // `outputDir` distinguishes "unset" (None) from an explicit empty string,
    // mirroring Go's `*string` and its `len(...) > 0` checks.
    let output_dir: Option<&str> = if !options.declaration_dir.is_empty() {
        Some(&options.declaration_dir)
    } else if !options.out_dir.is_empty() {
        Some(&options.out_dir)
    } else {
        None
    };

    let path = if let Some(output_dir) = output_dir {
        get_source_file_path_in_new_dir_worker(
            file,
            output_dir,
            &host.get_current_directory(),
            &host.common_source_directory(),
            host.use_case_sensitive_file_names(),
        )
    } else {
        file.to_string()
    };
    let declaration_extension = get_declaration_emit_extension_for_path(&path);
    format!("{}{declaration_extension}", remove_file_extension(&path))
}

/// Invokes `action` with the [`OutputPaths`] and source file for each entry of
/// `source_files`, stopping (and returning `true`) as soon as `action` returns
/// `true`; returns `false` if no call did.
///
/// Side effects: invokes `action` for each visited source file.
// Go: internal/outputpaths/outputpaths.go:ForEachEmittedFile
pub fn for_each_emitted_file<S, A>(
    host: &impl OutputPathsHost,
    options: &CompilerOptions,
    mut action: A,
    source_files: &[S],
    force_dts_emit: bool,
) -> bool
where
    S: SourceFileLike,
    A: FnMut(&OutputPaths, &S) -> bool,
{
    for source_file in source_files {
        let paths = get_output_paths_for(source_file, options, host, force_dts_emit);
        if action(&paths, source_file) {
            return true;
        }
    }
    false
}

/// Computes the full set of [`OutputPaths`] for `source_file`.
///
/// `force_dts_emit` forces a declaration path even when declarations are not
/// otherwise emitted (used by isolated declaration emit). JSON files that would
/// be written back to their own location, and `emitDeclarationOnly`, suppress
/// the `.js`/source-map outputs.
///
/// Side effects: none (pure).
// Go: internal/outputpaths/outputpaths.go:GetOutputPathsFor
pub fn get_output_paths_for(
    source_file: &impl SourceFileLike,
    options: &CompilerOptions,
    host: &impl OutputPathsHost,
    force_dts_emit: bool,
) -> OutputPaths {
    let file_name = source_file.file_name();
    let own_output_file_path = get_own_emit_output_file_path(
        file_name,
        options,
        host,
        get_output_extension(file_name, options.jsx),
    );
    let is_json_file = is_json_source_file(source_file);
    // If json file emits to the same location skip writing it, if
    // emitDeclarationOnly skip writing it.
    let is_json_emitted_to_same_location = is_json_file
        && compare_paths(
            file_name,
            &own_output_file_path,
            &ComparePathsOptions {
                current_directory: host.get_current_directory(),
                use_case_sensitive_file_names: host.use_case_sensitive_file_names(),
            },
        ) == Ordering::Equal;
    let mut paths = OutputPaths::default();
    if options.emit_declaration_only != Tristate::True && !is_json_emitted_to_same_location {
        paths.js_file_path = own_output_file_path;
        if !is_json_source_file(source_file) {
            paths.source_map_file_path = get_source_map_file_path(&paths.js_file_path, options);
        }
    }
    if force_dts_emit || (options.get_emit_declarations() && !is_json_file) {
        paths.declaration_file_path =
            get_declaration_emit_output_file_path(file_name, options, host);
        if options.get_are_declaration_maps_enabled() {
            paths.declaration_map_path = format!("{}.map", paths.declaration_file_path);
        }
    }
    paths
}

/// Computes the `.tsbuildinfo` output path, or `""` when incremental/build
/// output is not produced or there is no config file to derive a name from.
///
/// An explicit `tsBuildInfoFile` wins; otherwise the config file's name is
/// re-rooted using `outDir` (and `rootDir`, when set) and given the
/// `.tsbuildinfo` extension.
///
/// # Examples
/// ```
/// use tsgo_outputpaths::get_build_info_file_name;
/// use tsgo_core::compileroptions::CompilerOptions;
/// use tsgo_core::tristate::Tristate;
/// use tsgo_tspath::ComparePathsOptions;
/// let opts = ComparePathsOptions {
///     use_case_sensitive_file_names: true,
///     current_directory: "/".into(),
/// };
/// let options = CompilerOptions {
///     incremental: Tristate::True,
///     config_file_path: "/p/tsconfig.json".into(),
///     out_dir: "/out".into(),
///     root_dir: "/p".into(),
///     ..Default::default()
/// };
/// assert_eq!(get_build_info_file_name(&options, &opts), "/out/tsconfig.tsbuildinfo");
/// ```
///
/// Side effects: none (pure).
// Go: internal/outputpaths/outputpaths.go:GetBuildInfoFileName
pub fn get_build_info_file_name(options: &CompilerOptions, opts: &ComparePathsOptions) -> String {
    if !options.is_incremental() && !options.build.is_true() {
        return String::new();
    }
    if !options.ts_build_info_file.is_empty() {
        return options.ts_build_info_file.clone();
    }
    if options.config_file_path.is_empty() {
        return String::new();
    }
    let config_file_extension_less = remove_file_extension(&options.config_file_path);
    let build_info_extension_less = if !options.out_dir.is_empty() {
        if !options.root_dir.is_empty() {
            let relative = get_relative_path_from_directory(
                &options.root_dir,
                config_file_extension_less,
                opts,
            );
            resolve_path(&options.out_dir, &[relative.as_str()])
        } else {
            let base = get_base_file_name(config_file_extension_less);
            combine_paths(&options.out_dir, &[base.as_str()])
        }
    } else {
        config_file_extension_less.to_string()
    };
    format!("{build_info_extension_less}{EXTENSION_TS_BUILD_INFO}")
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
