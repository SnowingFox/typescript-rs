//! Port of Go `internal/compiler/program.go` (the foundation skeleton).
//!
//! This round ports the construction skeleton only: [`ProgramOptions`],
//! [`Program`], [`new_program`], and the read-back accessors the checker pool
//! and language service will consume. Diagnostics, emit, project references, and
//! the full `verifyCompilerOptions` pass are deferred to later compiler rounds
//! (see `docs/rust-rewrite/phase-6-compiler/compiler/impl.md`).

use std::sync::Arc;

use tsgo_core::compileroptions::CompilerOptions;
use tsgo_outputpaths::{get_output_paths_for, OutputPathsHost};
use tsgo_parser::Diagnostic;
use tsgo_tsoptions::ParsedCommandLine;
use tsgo_tspath::{is_declaration_file_name, to_path, ComparePathsOptions, Path};

use crate::checkerpool::CompilerCheckerPool;
use crate::emitter::{emit_js_text, EmitOnly};
use crate::fileloader::{process_all_program_files, ProcessedFiles};
use crate::host::{CompilerHost, ParsedFile};
use crate::verify_options::{verify_compiler_options, OptionsDiagnostic};

/// The inputs to [`new_program`]: the host, the parsed configuration, and the
/// threading mode.
///
/// This is the reachable subset of Go's `ProgramOptions`; project-reference,
/// typings, tracing, and custom-checker-pool fields are deferred.
///
/// Side effects: none (plain data, holds shared handles).
// Go: internal/compiler/program.go:ProgramOptions
#[derive(Clone)]
pub struct ProgramOptions {
    /// The host the program reads files through.
    pub host: Arc<dyn CompilerHost>,
    /// The parsed command line / tsconfig (compiler options + root file names).
    pub config: Arc<ParsedCommandLine>,
    /// Whether to load and check single-threaded (debugging / determinism).
    pub single_threaded: bool,
    // DEFER(P6): use_source_of_project_reference / create_checker_pool /
    // typings_location / project_name / tracing.
    // blocked-by: project references + tracing wiring (P6-2+).
}

/// The central compiler artifact: the loaded source files, the compiler options,
/// and the checker pool, built from a [`ProgramOptions`].
///
/// This round ports the construction skeleton and the read-back accessors the
/// checker pool and language service consume. Diagnostics, emit, and the host
/// trait implementations the checker calls back into are later rounds.
///
/// Side effects: none (owns loaded files).
// Go: internal/compiler/program.go:Program
pub struct Program {
    opts: ProgramOptions,
    processed: ProcessedFiles,
    checker_pool: CompilerCheckerPool,
    compare_paths_options: ComparePathsOptions,
    program_diagnostics: Vec<OptionsDiagnostic>,
}

/// Builds a [`Program`]: loads and parses all files reachable from the root
/// names in `opts.config`, then sizes the checker pool.
///
/// # Examples
/// ```
/// use std::sync::Arc;
/// use tsgo_compiler::{new_program, ProgramOptions, new_compiler_host, CompilerHost};
/// use tsgo_core::compileroptions::CompilerOptions;
/// use tsgo_tsoptions::new_parsed_command_line;
/// use tsgo_tspath::ComparePathsOptions;
/// use tsgo_vfs::vfstest::MapFs;
/// use tsgo_vfs::Fs;
///
/// let fs: Arc<dyn Fs + Send + Sync> =
///     Arc::new(MapFs::from_map([("/src/index.ts", "export const x = 1;")], true));
/// let host = Arc::new(new_compiler_host("/src", fs, "/lib"));
/// let config = new_parsed_command_line(
///     CompilerOptions::default(),
///     vec!["/src/index.ts".to_string()],
///     ComparePathsOptions { use_case_sensitive_file_names: true, current_directory: "/src".into() },
/// );
/// let program = new_program(ProgramOptions { host, config: Arc::new(config), single_threaded: true });
/// assert_eq!(program.source_files().len(), 1);
/// assert_eq!(program.source_files()[0].file_name(), "/src/index.ts");
/// ```
///
/// Side effects: reads the file system through `opts.host`.
// Go: internal/compiler/program.go:NewProgram
pub fn new_program(opts: ProgramOptions) -> Program {
    // Go: NewProgram = processAllProgramFiles -> initCheckerPool -> verifyCompilerOptions.
    let single_threaded = single_threaded_of(&opts);
    let current_directory = opts.host.get_current_directory().to_string();
    let use_case_sensitive_file_names = opts.host.fs().use_case_sensitive_file_names();
    let compare_paths_options = ComparePathsOptions {
        use_case_sensitive_file_names,
        current_directory,
    };

    let processed = process_all_program_files(&opts, single_threaded);
    let checker_pool = CompilerCheckerPool::new(
        single_threaded,
        opts.config.compiler_options().checkers,
        processed.files().len(),
    );
    // Go: initCheckerPool runs here; the built-in pool is sized now and its
    // checkers are created on demand by `create_checkers`.
    let program_diagnostics = verify_compiler_options(opts.config.compiler_options());

    Program {
        opts,
        processed,
        checker_pool,
        compare_paths_options,
        program_diagnostics,
    }
}

impl Program {
    /// The compiler options this program was built with.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.Options
    pub fn options(&self) -> &CompilerOptions {
        self.opts.config.compiler_options()
    }

    /// The parsed command line / tsconfig this program was built from.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.CommandLine
    pub fn command_line(&self) -> &ParsedCommandLine {
        &self.opts.config
    }

    /// The host the program reads files through.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.Host
    pub fn host(&self) -> &Arc<dyn CompilerHost> {
        &self.opts.host
    }

    /// Whether the program loads and checks single-threaded.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.SingleThreaded
    pub fn single_threaded(&self) -> bool {
        single_threaded_of(&self.opts)
    }

    /// The loaded source files, in deterministic include order.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.GetSourceFiles
    pub fn source_files(&self) -> &[ParsedFile] {
        self.processed.files()
    }

    /// Binds every loaded source file (idempotently), so the binder's symbol and
    /// flow graphs are available for type-checking.
    ///
    /// A file the *partial* binder cannot handle is skipped (left unbound) rather
    /// than aborting the whole program; the checker's multi-file view only
    /// includes bound files (see [`MultiFileBoundProgram::new`](crate::MultiFileBoundProgram)),
    /// so a skipped lib simply does not contribute globals. This matters once the
    /// default lib's `/// <reference lib>` graph is expanded: the ES5 aggregator
    /// pulls in `lib.dom.d.ts`, whose `[Symbol.x]` computed property names the
    /// binder cannot name yet — `lib.es5.d.ts` (the `String`/`Array` globals)
    /// still binds, so real globals stay resolvable.
    ///
    /// DEFER(P6): bind every lib (so the `dom`/`webworker` globals are
    /// checkable, not just parsed).
    /// blocked-by: the binder's computed-property-name handling
    /// (`internal/binder/symbols.rs:getDeclarationName` `panic!`s on a
    /// non-literal computed name), which this crate must not edit.
    ///
    /// Side effects: binds each bindable file via `tsgo_binder`, mutating
    /// per-file arenas; a file that panics mid-bind is left unbound (its arena
    /// may be partially mutated but is never read, as it is excluded from the
    /// checker view).
    // Go: internal/compiler/program.go:BindSourceFiles
    pub fn bind_source_files(&mut self) {
        // PERF(port): Go binds unbound files on a `core.WorkGroup`; this round
        // binds sequentially. Each file owns its own arena, so binding is
        // embarrassingly parallel and can be switched to `rayon` later.
        for file in self.processed.files_mut() {
            // The binder is a partial port that `panic!`s on some real lib
            // constructs; tolerate that here (see the doc note) instead of
            // aborting. `catch_unwind` is sound because each file owns its arena
            // and a skipped file is never read.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                file.bind();
            }));
        }
    }

    /// Looks up a loaded source file by file name.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.GetSourceFile
    pub fn get_source_file(&self, file_name: &str) -> Option<&ParsedFile> {
        self.get_source_file_by_path(&self.to_path(file_name))
    }

    /// Looks up a loaded source file by its canonical [`Path`].
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.GetSourceFileByPath
    pub fn get_source_file_by_path(&self, path: &Path) -> Option<&ParsedFile> {
        self.processed.file_by_path(path)
    }

    /// The canonical [`Path`] for `file_name`, rooted at the host's current
    /// directory.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.toPath
    pub fn to_path(&self, file_name: &str) -> Path {
        to_path(
            file_name,
            &self.compare_paths_options.current_directory,
            self.compare_paths_options.use_case_sensitive_file_names,
        )
    }

    /// Binds all files and creates the checker pool's checkers, associating each
    /// file to a checker round-robin.
    ///
    /// Side effects: binds each file (mutating per-file arenas) and allocates the
    /// pool's checkers.
    // Go: internal/compiler/program.go:initCheckerPool + checkerpool.go:createCheckers
    pub fn create_checkers(&mut self) {
        self.bind_source_files();
        // Share the program's REAL compiler options with the checker pool so the
        // checker's option-gated diagnostics (round 4al's `2802` for-of
        // downlevel-iteration / `--target` gating, `strictNullChecks`, ...) read
        // the program's actual config rather than all-defaults. Go: the pool's
        // checkers are built over the `*Program`, and `c.compilerOptions =
        // program.Options()`.
        let options = std::rc::Rc::new(self.options().clone());
        let files = self.processed.files();
        self.checker_pool
            .create_checkers_with_options(files, options);
    }

    /// The built-in checker pool (sized; call [`Self::create_checkers`] to build
    /// its checkers).
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.checkerPool
    pub fn checker_pool(&self) -> &CompilerCheckerPool {
        &self.checker_pool
    }

    /// Binds the program's files, builds the checker pool, and returns the
    /// semantic diagnostics it collects (Go's `getSemanticDiagnostics`, driven
    /// through the checker pool).
    ///
    /// # Examples
    /// ```
    /// use std::sync::Arc;
    /// use tsgo_compiler::{new_compiler_host, new_program, ProgramOptions};
    /// use tsgo_core::compileroptions::CompilerOptions;
    /// use tsgo_tsoptions::new_parsed_command_line;
    /// use tsgo_tspath::ComparePathsOptions;
    /// use tsgo_vfs::vfstest::MapFs;
    /// use tsgo_vfs::Fs;
    ///
    /// // `y` is never declared, so the checker reports 2304 "Cannot find name".
    /// let fs: Arc<dyn Fs + Send + Sync> =
    ///     Arc::new(MapFs::from_map([("/src/index.ts", "y;")], true));
    /// let host = Arc::new(new_compiler_host("/src", fs, "/lib"));
    /// let config = new_parsed_command_line(
    ///     CompilerOptions::default(),
    ///     vec!["/src/index.ts".to_string()],
    ///     ComparePathsOptions { use_case_sensitive_file_names: true, current_directory: "/src".into() },
    /// );
    /// let mut program = new_program(ProgramOptions { host, config: Arc::new(config), single_threaded: true });
    /// let diags = program.semantic_diagnostics();
    /// assert_eq!(diags[0].code, 2304);
    /// assert_eq!(diags[0].message, "Cannot find name 'y'.");
    /// ```
    ///
    /// DEFER(P6): per-file filtering, suggestion/declaration diagnostics, and
    /// `@ts-expect-error`/`@ts-ignore` directive handling.
    /// blocked-by: multi-file `BoundProgram` view + checker directive surface.
    ///
    /// Side effects: binds every file (mutating per-file arenas) and allocates
    /// the pool's checkers.
    // Go: internal/compiler/program.go:GetSemanticDiagnostics
    pub fn semantic_diagnostics(&mut self) -> Vec<tsgo_checker::Diagnostic> {
        // Bind every file and build the pool's checkers (idempotent), then drive
        // checking through the pool.
        self.create_checkers();
        self.checker_pool.collect_diagnostics()
    }

    /// The loaded default library file (the lib included automatically from the
    /// target, or the first `--lib` entry), if one was included and read.
    ///
    /// This is the file whose top-level declarations form the program's global
    /// scope (see [`BoundFile::globals`](crate::BoundFile)); a checker built over
    /// its bound view resolves the real `Array`/`String`/`Object` globals.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.GetDefaultLibFile
    pub fn default_lib_file(&self) -> Option<&ParsedFile> {
        self.processed.default_lib_file()
    }

    /// The normalized names of root/referenced files that could not be read.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.missingFiles
    pub fn missing_files(&self) -> &[String] {
        self.processed.missing_files()
    }

    /// The option-consistency diagnostics produced by `verify_compiler_options`
    /// during construction.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.programDiagnostics (verifyCompilerOptions subset)
    pub fn options_diagnostics(&self) -> &[OptionsDiagnostic] {
        &self.program_diagnostics
    }

    /// Emits the program's source files, running each through the script
    /// transformers and printer and writing the resulting `.js`, then combining
    /// the per-file results in input order.
    ///
    /// # Examples
    /// ```
    /// use std::sync::Arc;
    /// use tsgo_compiler::{
    ///     new_compiler_host, new_program, EmitOnly, EmitOptions, ProgramOptions,
    /// };
    /// use tsgo_core::compileroptions::CompilerOptions;
    /// use tsgo_tsoptions::new_parsed_command_line;
    /// use tsgo_tspath::ComparePathsOptions;
    /// use tsgo_vfs::vfstest::MapFs;
    /// use tsgo_vfs::Fs;
    ///
    /// let fs: Arc<dyn Fs + Send + Sync> =
    ///     Arc::new(MapFs::from_map([("/src/index.ts", "const x: number = 1;")], true));
    /// let host = Arc::new(new_compiler_host("/src", fs, "/lib"));
    /// let config = new_parsed_command_line(
    ///     CompilerOptions::default(),
    ///     vec!["/src/index.ts".to_string()],
    ///     ComparePathsOptions { use_case_sensitive_file_names: true, current_directory: "/src".into() },
    /// );
    /// let program = new_program(ProgramOptions { host, config: Arc::new(config), single_threaded: true });
    /// let result = program.emit(EmitOptions::default());
    /// assert_eq!(result.emitted_files, vec!["/src/index.js".to_string()]);
    /// // The type annotation is erased and the file prints as plain JavaScript.
    /// assert_eq!(program.host().fs().read_file("/src/index.js").as_deref(), Some("const x = 1;\n"));
    /// ```
    ///
    /// Side effects: writes emitted files (through `options.write_file` if set,
    /// else the host file system).
    // Go: internal/compiler/program.go:Program.Emit
    pub fn emit(&self, options: EmitOptions) -> EmitResult {
        // DEFER(P6) blocked-by: checker public API. Go's `Emit` first runs
        // `HandleNoEmitOnError` (needs semantic diagnostics) and emits `.d.ts`
        // via the declarations transformer with a checker `EmitResolver`. The
        // reachable subset is transform + print of JS only, in input order.
        let mut options = options;
        let force_dts = options.emit_only == EmitOnly::ForcedDts;
        let files = self.get_source_files_to_emit(options.target_source_file.as_deref(), force_dts);
        let results: Vec<EmitResult> = files
            .into_iter()
            .map(|file| self.emit_one(file, &mut options))
            .collect();
        combine_emit_results(results)
    }

    /// The source files that may be emitted, honoring an optional single-file
    /// target and preserving input order.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/emitter.go:getSourceFilesToEmit
    fn get_source_files_to_emit(&self, target: Option<&str>, force_dts: bool) -> Vec<&ParsedFile> {
        let target_path = target.map(|t| self.to_path(t));
        self.source_files()
            .iter()
            .filter(|file| {
                if let Some(target_path) = &target_path {
                    if &self.to_path(file.file_name()) != target_path {
                        return false;
                    }
                }
                source_file_may_be_emitted(file, force_dts)
            })
            .collect()
    }

    /// Emits one source file: computes its output paths and emits the `.js`.
    ///
    /// Side effects: writes the emitted file.
    // Go: internal/compiler/program.go:Emit (per-file body) + emitter.go:emitter.emit
    fn emit_one(&self, file: &ParsedFile, options: &mut EmitOptions) -> EmitResult {
        let mut result = EmitResult::default();
        let host = self.output_paths_host();
        let force_dts = options.emit_only == EmitOnly::ForcedDts;
        let paths = get_output_paths_for(file, self.options(), &host, force_dts);
        self.emit_js_file(file, paths.js_file_path(), options, &mut result);
        // DEFER(P6) blocked-by: declarations transformer + checker EmitResolver:
        // `emit_declaration_file` (`.d.ts` + declaration map).
        result
    }

    /// Emits the JavaScript output for `file` to `js_file_path`, writing it
    /// through `options.write_file` (or the host file system).
    ///
    /// Side effects: writes the emitted `.js` file.
    // Go: internal/compiler/emitter.go:emitter.emitJSFile + printSourceFile
    fn emit_js_file(
        &self,
        file: &ParsedFile,
        js_file_path: &str,
        options: &mut EmitOptions,
        result: &mut EmitResult,
    ) {
        let emit_only = options.emit_only;
        if js_file_path.is_empty() || (emit_only != EmitOnly::All && emit_only != EmitOnly::Js) {
            return;
        }
        // DEFER(P6) blocked-by: `IsEmitBlocked` (project-reference output guard).
        if self.options().no_emit.is_true() {
            result.emit_skipped = true;
            return;
        }
        let mut text = emit_js_text(file.file_name(), file.text(), self.options());
        if self.options().emit_bom.is_true() {
            text = tsgo_stringutil::add_utf8_byte_order_mark(&text);
        }
        let data = WriteFileData {
            source_map_url_pos: -1,
            skipped_dts_write: false,
        };
        if self.write_text(js_file_path, &text, &data, options).is_ok() {
            result.emitted_files.push(js_file_path.to_string());
        }
        // DEFER(P6): on write error, add a `Could_not_write_file` diagnostic
        // (needs the `ast.Diagnostic` compiler-diagnostic constructor surface).
    }

    /// Writes emitted `text` for `file_name`, preferring `options.write_file`
    /// and otherwise writing through the host file system.
    ///
    /// Side effects: writes a file.
    // Go: internal/compiler/emitter.go:emitter.writeText
    fn write_text(
        &self,
        file_name: &str,
        text: &str,
        data: &WriteFileData,
        options: &mut EmitOptions,
    ) -> Result<(), String> {
        if let Some(write_file) = options.write_file.as_mut() {
            write_file(file_name, text, data)
        } else {
            self.host()
                .fs()
                .write_file(file_name, text)
                .map_err(|err| format!("{err:?}"))
        }
    }

    /// Builds the [`OutputPathsHost`] the emitter consults.
    ///
    /// The common source directory (Go's lazy, diagnostic-driven
    /// `CommonSourceDirectory`) is deferred; it is not consulted in the
    /// reachable subset (no `outDir`/declarations), so this supplies the current
    /// directory as a placeholder.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/emitHost.go:emitHost (OutputPathsHost facet)
    fn output_paths_host(&self) -> ProgramOutputPathsHost {
        ProgramOutputPathsHost {
            // DEFER(P6) blocked-by: includeProcessor diagnostics + program state:
            // the real `CommonSourceDirectory` (lazy `OnceLock`).
            common_source_directory: self.compare_paths_options.current_directory.clone(),
            current_directory: self.compare_paths_options.current_directory.clone(),
            use_case_sensitive_file_names: self.compare_paths_options.use_case_sensitive_file_names,
        }
    }
}

/// Reports whether `file` may be emitted (the reachable subset of Go's
/// `sourceFileMayBeEmitted`).
///
/// Declaration files are never emitted; forced declaration emit always emits.
/// The remaining Go conditions (`noEmitForJsFiles`, external-library files,
/// project-reference sources, and JSON-without-`outDir`) are deferred.
///
/// Side effects: none (pure).
// Go: internal/compiler/emitter.go:sourceFileMayBeEmitted
fn source_file_may_be_emitted(file: &ParsedFile, force_dts: bool) -> bool {
    if is_declaration_file_name(file.file_name()) {
        return false;
    }
    if force_dts {
        return true;
    }
    // DEFER(P6) blocked-by: checker/program state — `noEmitForJsFiles`,
    // external-library files, project-reference sources, json-without-`outDir`.
    true
}

/// Per-file metadata passed to a [`WriteFileCallback`] alongside the text.
///
/// This is the reachable subset of Go's `WriteFileData`; `BuildInfo` and the
/// declaration-emit diagnostics are deferred.
///
/// Side effects: none (plain data).
// Go: internal/compiler/program.go:WriteFileData
#[derive(Debug, Default)]
pub struct WriteFileData {
    /// The position of the `//# sourceMappingURL=` comment, or `-1` if none.
    pub source_map_url_pos: i32,
    /// Whether a `.d.ts` write was skipped (unchanged output).
    pub skipped_dts_write: bool,
}

/// A sink for emitted file contents. When [`EmitOptions::write_file`] is set,
/// the emitter calls it instead of writing through the host file system.
///
/// Side effects: implementations typically perform I/O.
// Go: internal/compiler/program.go:WriteFile
pub type WriteFileCallback = Box<dyn FnMut(&str, &str, &WriteFileData) -> Result<(), String>>;

/// The inputs to [`Program::emit`].
///
/// Side effects: none (holds the optional write-file sink).
// Go: internal/compiler/program.go:EmitOptions
#[derive(Default)]
pub struct EmitOptions {
    /// A single file to emit (by name); `None` emits all files.
    pub target_source_file: Option<String>,
    /// Which artifacts to emit.
    pub emit_only: EmitOnly,
    /// An optional sink replacing the host's `write_file`.
    pub write_file: Option<WriteFileCallback>,
}

/// One emitted source map (shape parity with Go).
///
/// The `source_map` payload itself is deferred (see [`Program::emit`]'s DEFER
/// note), so the reachable emitter never produces one.
///
/// Side effects: none (plain data).
// Go: internal/compiler/program.go:SourceMapEmitResult
#[derive(Debug, Default, Clone)]
pub struct SourceMapEmitResult {
    /// The input source file names, 1:1 with the source map's `sources`.
    pub input_source_file_names: Vec<String>,
    /// The generated file the source map describes.
    pub generated_file: String,
}

/// The result of an emit: whether it was skipped, the diagnostics, the files
/// written, and any source maps.
///
/// Side effects: none (plain data).
// Go: internal/compiler/program.go:EmitResult
#[derive(Debug, Default)]
pub struct EmitResult {
    /// Whether emit was skipped (e.g. `noEmit`).
    pub emit_skipped: bool,
    /// Emit diagnostics (declaration-transform / write errors). Empty in the
    /// reachable subset; see [`Program::emit`].
    pub diagnostics: Vec<Diagnostic>,
    /// The files the emitter wrote, in input order.
    pub emitted_files: Vec<String>,
    /// The emitted source maps (always empty in the reachable subset).
    pub source_maps: Vec<SourceMapEmitResult>,
}

/// Merges per-file [`EmitResult`]s into one, preserving input order.
///
/// `emit_skipped` becomes the logical OR of the inputs; emitted files, source
/// maps, and diagnostics are concatenated in order.
///
/// # Examples
/// ```
/// use tsgo_compiler::{combine_emit_results, EmitResult};
///
/// let a = EmitResult { emitted_files: vec!["a.js".into()], ..Default::default() };
/// let b = EmitResult { emit_skipped: true, emitted_files: vec!["b.js".into()], ..Default::default() };
/// let combined = combine_emit_results(vec![a, b]);
/// assert!(combined.emit_skipped);
/// assert_eq!(combined.emitted_files, vec!["a.js".to_string(), "b.js".to_string()]);
/// ```
///
/// Side effects: none (pure).
// Go: internal/compiler/program.go:CombineEmitResults
pub fn combine_emit_results(results: Vec<EmitResult>) -> EmitResult {
    let mut result = EmitResult::default();
    for emit_result in results {
        if emit_result.emit_skipped {
            result.emit_skipped = true;
        }
        result.diagnostics.extend(emit_result.diagnostics);
        result.emitted_files.extend(emit_result.emitted_files);
        result.source_maps.extend(emit_result.source_maps);
    }
    result
}

/// The read-only environment [`get_output_paths_for`] needs, backed by a
/// program.
struct ProgramOutputPathsHost {
    common_source_directory: String,
    current_directory: String,
    use_case_sensitive_file_names: bool,
}

impl OutputPathsHost for ProgramOutputPathsHost {
    fn common_source_directory(&self) -> String {
        self.common_source_directory.clone()
    }

    fn get_current_directory(&self) -> String {
        self.current_directory.clone()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.use_case_sensitive_file_names
    }
}

/// Resolves the effective single-threaded flag: the explicit option falls back
/// to the compiler option.
///
/// Side effects: none (pure).
// Go: internal/compiler/program.go:Program.SingleThreaded
fn single_threaded_of(opts: &ProgramOptions) -> bool {
    opts.single_threaded || opts.config.compiler_options().single_threaded.is_true()
}

#[cfg(test)]
#[path = "program_test.rs"]
mod tests;
