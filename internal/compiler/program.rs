//! Port of Go `internal/compiler/program.go` (the foundation skeleton).
//!
//! This round ports the construction skeleton only: [`ProgramOptions`],
//! [`Program`], [`new_program`], and the read-back accessors the checker pool
//! and language service will consume. Diagnostics, emit, project references, and
//! the full `verifyCompilerOptions` pass are deferred to later compiler rounds
//! (see `docs/rust-rewrite/phase-6-compiler/compiler/impl.md`).

use std::sync::Arc;

use tsgo_binder::BinderDiagnostic;
use tsgo_core::compileroptions::{CompilerOptions, NewLineKind};
use tsgo_core::scriptkind::ScriptKind;
use tsgo_core::tristate::Tristate;
use tsgo_outputpaths::{get_output_paths_for, get_source_map_file_path, OutputPathsHost};
use tsgo_parser::Diagnostic;
use tsgo_sourcemap::Generator;
use tsgo_tsoptions::ParsedCommandLine;
use tsgo_tspath::{
    get_base_file_name, get_directory_path, is_declaration_file_name, normalize_path,
    normalize_slashes, to_path, ComparePathsOptions, Path,
};

use crate::checkerpool::CompilerCheckerPool;
use crate::emitter::{emit_js_text_with_source_map, EmitOnly};
use crate::fileloader::{process_all_program_files, ProcessedFiles};
use crate::host::{effective_script_kind, CompilerHost, ParsedFile};
use crate::projectreference::{
    resolve_project_references, BuildOrder, ProjectReferenceDiagnostic, ResolvedProjectReferences,
};
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
    /// The resolved project-reference graph (`None` when the program was not
    /// built from a config file, i.e. has no `references[]` to resolve).
    resolved_project_references: Option<ResolvedProjectReferences>,
    /// The project-reference consistency diagnostics (`TS6306`/`TS6310`/`TS6053`)
    /// produced by `verify_project_references` during construction.
    reference_diagnostics: Vec<ProjectReferenceDiagnostic>,
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

    // Resolve the project-reference graph and run the composite/noEmit checks
    // (Go's `verifyProjectReferences`, part of `NewProgram`). Only programs built
    // from a config file have references to resolve, gated on a non-empty
    // `configFilePath` (Go gates on `Config.ConfigFile != nil`).
    let config_file_path = opts.config.compiler_options().config_file_path.clone();
    let (resolved_project_references, reference_diagnostics) = if config_file_path.is_empty() {
        (None, Vec::new())
    } else {
        let graph =
            resolve_project_references(opts.host.as_ref(), &config_file_path, opts.config.as_ref());
        let diagnostics = graph.verify_project_references();
        (Some(graph), diagnostics)
    };

    Program {
        opts,
        processed,
        checker_pool,
        compare_paths_options,
        program_diagnostics,
        resolved_project_references,
        reference_diagnostics,
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
        let resolved_modules = self.processed.resolved_modules();
        self.checker_pool
            .create_checkers_with_modules(files, options, resolved_modules);
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
    /// DEFER(P6): suggestion/declaration diagnostics, the `isCheckJS`
    /// `JSDocDiagnostics` append, and `@ts-expect-error`/`@ts-ignore` directive
    /// handling.
    /// blocked-by: the checker's suggestion/JSDoc-diagnostic + directive surface.
    ///
    /// Side effects: binds every file (mutating per-file arenas) and allocates
    /// the pool's checkers.
    // Go: internal/compiler/program.go:GetSemanticDiagnostics
    pub fn semantic_diagnostics(&mut self) -> Vec<tsgo_checker::Diagnostic> {
        // Flatten the per-file bind-and-check groups in source-file order: each
        // file contributes its binder diagnostics followed by its checker
        // diagnostics (Go's per-file `BindDiagnostics() ++ checker.GetDiagnostics()`).
        self.bind_and_check_diagnostics_grouped()
            .into_iter()
            .flatten()
            .collect()
    }

    /// Like [`Self::semantic_diagnostics`], but returns the diagnostics grouped
    /// by the source file that owns them, as `(file_name, diagnostics)` pairs in
    /// source-file order (default-library files excluded, files with no
    /// diagnostics omitted).
    ///
    /// A [`Diagnostic`](tsgo_checker::Diagnostic)'s `start`/`length` are byte
    /// offsets into *its own* file's text. Callers (the harness baseline
    /// renderer) must attribute each diagnostic to that file; the flat
    /// [`Self::semantic_diagnostics`] drops the association, which let a later
    /// file's diagnostic be rendered against an earlier (shorter) file and slice
    /// out of bounds. This preserves the per-file partition.
    ///
    /// Side effects: binds every file and builds the pool's checkers (same as
    /// [`Self::semantic_diagnostics`]).
    // Go: internal/compiler/program.go:GetSemanticDiagnostics (per-file)
    pub fn semantic_diagnostics_by_file(&mut self) -> Vec<(String, Vec<tsgo_checker::Diagnostic>)> {
        let groups = self.bind_and_check_diagnostics_grouped();
        // Bound files in `source_files()` order, parallel to the grouped output.
        let bound_file_names: Vec<String> = self
            .processed
            .files()
            .iter()
            .filter(|f| f.is_bound())
            .map(|f| f.file_name().to_string())
            .collect();
        bound_file_names
            .into_iter()
            .zip(groups)
            .filter(|(_, diags)| !diags.is_empty())
            .collect()
    }

    /// The per-file semantic (bind-and-check) diagnostics for every bound file,
    /// in source-file order: each file's list is its binder diagnostics followed
    /// by its checker diagnostics, the reachable subset of Go's
    /// `getBindAndCheckDiagnosticsWithChecker`
    /// (`slices.Clip(sourceFile.BindDiagnostics())` then
    /// `append(..., fileChecker.GetDiagnostics(...))`).
    ///
    /// Gating mirrors Go exactly:
    /// - a file the bind-and-check gate skips (`SkipTypeChecking`: a default-lib
    ///   file, or a JS file the JS-skip rule drops) yields an EMPTY list, so
    ///   neither its bind nor its check diagnostics surface
    ///   ([`Self::is_excluded_from_semantic_diagnostics`]);
    /// - for a *plain* JS file (`checkJs` unset) Go keeps only the
    ///   `plainJSErrors` codes of the combined list. This port applies that
    ///   filter to the BINDER diagnostics (whose codes are a small, known set),
    ///   so a plain-JS duplicate-class does not over-report TS2300 while a
    ///   block-scoped redeclare (TS2451, in `plainJSErrors`) still surfaces. The
    ///   checker-diagnostic half of the plain-JS filter is a pre-existing
    ///   divergence left unchanged here.
    ///
    /// The result is parallel to the pool's grouped output (one entry per bound
    /// file, including excluded files as an empty list), so the flat and
    /// per-file collectors can flatten / zip it directly. Ordering within a file
    /// is bind-then-check; cross-file ordering is source-file order. The harness
    /// baseline writer sorts by (file, position) before rendering, matching Go's
    /// final diagnostic sort.
    ///
    /// DEFER(P6): the `isCheckJS` `JSDocDiagnostics` append and the
    /// `@ts-expect-error`/`@ts-ignore` directive filtering of the combined list.
    /// blocked-by: the checker's JSDoc-diagnostic + comment-directive surface.
    ///
    /// Side effects: binds every file (mutating per-file arenas) and allocates
    /// the pool's checkers (via [`Self::create_checkers`]).
    // Go: internal/compiler/program.go:getBindAndCheckDiagnosticsWithChecker
    fn bind_and_check_diagnostics_grouped(&mut self) -> Vec<Vec<tsgo_checker::Diagnostic>> {
        // Bind every file and build the pool's checkers (idempotent).
        self.create_checkers();
        // Exclude the auto-included default-library files and JS files the
        // bind-and-check gate skips: `tsc` does not report semantic diagnostics
        // located in `lib.*.d.ts`, and a JS file with `checkJs: false` (or a
        // `@ts-nocheck` directive, deferred) is not bind-and-checked. A
        // lib-positioned diagnostic rendered against a user file also panics the
        // diagnostic writer (out-of-bounds slice). Parallel to the bound-program's
        // source-file order (`MultiFileBoundProgram::new` keeps the
        // `processed.files()` order, filtered to bound files).
        let lib_dir = self.default_library_directory();
        // Convert each bound file's binder diagnostics up front (an empty list
        // for an excluded file), parallel to the pool's grouped checker output.
        let mut bind_groups: Vec<Vec<tsgo_checker::Diagnostic>> = Vec::new();
        let mut exclude: Vec<bool> = Vec::new();
        // Each bound file's source text, parallel to `bind_groups`, so the
        // combined per-file list can be filtered by its `@ts-ignore` /
        // `@ts-expect-error` directives (Go's `getDiagnosticsWithPrecedingDirectives`).
        let mut file_texts: Vec<String> = Vec::new();
        for file in self.processed.files().iter().filter(|f| f.is_bound()) {
            file_texts.push(file.text().to_string());
            let excluded = self.is_excluded_from_semantic_diagnostics(file, &lib_dir);
            exclude.push(excluded);
            if excluded {
                bind_groups.push(Vec::new());
                continue;
            }
            let plain_js = self.is_plain_js_file(file);
            let text = file.text();
            let mut diags: Vec<tsgo_checker::Diagnostic> = Vec::new();
            if let Some(bind) = file.bind_result() {
                for diagnostic in &bind.diagnostics {
                    let converted = binder_diagnostic_to_checker(diagnostic, text);
                    // Plain JS keeps only the `plainJSErrors` codes (Go filters
                    // the combined list); for the binder contribution this drops
                    // e.g. TS2300/TS2567 while keeping TS2451/TS2528.
                    if plain_js && !binder_code_allowed_in_plain_js(converted.code) {
                        continue;
                    }
                    diags.push(converted);
                }
            }
            bind_groups.push(diags);
        }
        let checker_groups = self
            .checker_pool
            .collect_diagnostics_grouped_excluding(&exclude);
        // For each bound file, the bind diagnostics precede the checker
        // diagnostics (Go's `BindDiagnostics() ++ checker.GetDiagnostics()`),
        // then the combined per-file list is sorted and deduplicated exactly as
        // Go's `GetSemanticDiagnosticsWithoutNoEmitFiltering` applies
        // `SortAndDeduplicateDiagnostics` per file. The dedup collapses the
        // IDENTICAL diagnostic a binder merge conflict re-emits on the same prior
        // declaration across two separate conflicts (a `get x` flagged once when
        // a method collides and again when the trailing `set x` collides).
        // Go: internal/compiler/program.go:GetSemanticDiagnosticsWithoutNoEmitFiltering
        bind_groups
            .into_iter()
            .zip(checker_groups)
            .zip(file_texts)
            .map(|((mut bind, check), text)| {
                bind.extend(check);
                let combined = sort_and_deduplicate_diagnostics(bind);
                // Drop diagnostics suppressed by a preceding `@ts-ignore` /
                // `@ts-expect-error` directive (Go filters the combined list per
                // file before returning it).
                filter_diagnostics_with_preceding_directives(&text, combined)
            })
            .collect()
    }

    /// Reports whether `file` is one of the auto-included default-library files
    /// (`lib.*.d.ts`) — i.e. it lives under the default-library directory (the
    /// directory of the resolved default lib). `tsc` does not report syntactic or
    /// semantic diagnostics located in default-library files; this is the reachable
    /// proxy for Go's `Program.isSourceFileDefaultLibrary` (a per-file flag set at
    /// load time, which this port does not yet thread through `ParsedFile`).
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.isSourceFileDefaultLibrary
    pub fn is_default_library_file(&self, file: &ParsedFile) -> bool {
        let Some(lib_dir) = self.default_library_directory() else {
            return false;
        };
        !lib_dir.is_empty() && file.file_name().starts_with(lib_dir.as_str())
    }

    /// Reports whether the bind-and-check (semantic) diagnostics of `file`
    /// should be skipped, the reachable subset of Go's `Program.SkipTypeChecking`.
    ///
    /// This round ports the JS bind-and-check gate
    /// ([`Self::can_include_bind_and_check_diagnostics`]); the `NoCheck` /
    /// `SkipLibCheck` / `SkipDefaultLibCheck` / project-reference arms of Go's
    /// `SkipTypeChecking` are handled elsewhere (the default-library exclusion in
    /// the diagnostics collectors) or deferred.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.SkipTypeChecking
    fn skip_type_checking(&self, file: &ParsedFile) -> bool {
        !self.can_include_bind_and_check_diagnostics(file)
    }

    /// Reports whether `file` is eligible for bind-and-check (semantic)
    /// diagnostics, a 1:1 port of Go's `canIncludeBindAndCheckDiagnostics`.
    ///
    /// `.ts`/`.tsx`/external files are always checked. A `.js`/`.jsx` file is
    /// checked only when it is *plain JS* (`checkJs` unset) or *check-JS*
    /// (`checkJs: true`); with `checkJs: false` it is neither, so it is skipped.
    /// A `Deferred` script kind is always checked. This is why a plain `.js`
    /// file is still type-checked by default (matching `tsc`/Go) — only an
    /// explicit `checkJs: false` (or, once parsed, a `// @ts-nocheck` directive)
    /// suppresses it.
    ///
    /// DEFER(P10): the `// @ts-check` / `// @ts-nocheck` directive
    /// (`SourceFile.CheckJsDirective`) is not parsed yet, so the directive arms
    /// of Go's predicate — `@ts-nocheck` forces a skip, `@ts-check` forces a
    /// check even when `checkJs` is off — are not modeled. With no directive,
    /// this matches Go exactly (`IsCheckJSEnabledForFile` collapses to
    /// `checkJs == true`, `IsPlainJSFile` to `checkJs` unset).
    /// blocked-by: the parser's check-js directive scan + `CheckJsDirective` on
    /// the source file.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.canIncludeBindAndCheckDiagnostics
    fn can_include_bind_and_check_diagnostics(&self, file: &ParsedFile) -> bool {
        let kind = effective_script_kind(file.file_name());
        if matches!(
            kind,
            ScriptKind::Ts | ScriptKind::Tsx | ScriptKind::External
        ) {
            return true;
        }
        let is_js = matches!(kind, ScriptKind::Js | ScriptKind::Jsx);
        let check_js = self.options().check_js;
        // `IsCheckJSEnabledForFile` with no `CheckJsDirective`: `checkJs == true`.
        let is_check_js = is_js && check_js == Tristate::True;
        // `IsPlainJSFile` with no `CheckJsDirective`: a `.js`/`.jsx` file whose
        // `checkJs` is unset (`Tristate::Unknown`).
        let is_plain_js = is_js && check_js == Tristate::Unknown;
        is_plain_js || is_check_js || kind == ScriptKind::Deferred
    }

    /// Reports whether `file` is a *plain* JS file (a `.js`/`.jsx` script kind
    /// whose `checkJs` is unset and that has no `// @ts-check` directive), a
    /// 1:1 port of Go's `ast.IsPlainJSFile`.
    ///
    /// Go keeps only the `plainJSErrors` codes of a plain JS file's combined
    /// bind-and-check diagnostics; this is the predicate that selects it.
    ///
    /// DEFER(P10): the `// @ts-check` directive arm (`CheckJsDirective`), which
    /// is not parsed yet (so this collapses to `is_js && checkJs unset`, exactly
    /// matching Go when no directive is present).
    /// blocked-by: the parser's check-js directive scan.
    ///
    /// Side effects: none (pure).
    // Go: internal/ast/utilities.go:IsPlainJSFile
    fn is_plain_js_file(&self, file: &ParsedFile) -> bool {
        let kind = effective_script_kind(file.file_name());
        let is_js = matches!(kind, ScriptKind::Js | ScriptKind::Jsx);
        is_js && self.options().check_js == Tristate::Unknown
    }

    /// Reports whether `file`'s semantic diagnostics are excluded from the
    /// program's bind-and-check pass: it is a default-library file (`tsc` does
    /// not report diagnostics located in `lib.*.d.ts`, given by `lib_dir`), or
    /// the bind-and-check gate ([`Self::skip_type_checking`]) skips it (a JS file
    /// without `checkJs`). The two diagnostics collectors share this mask.
    ///
    /// Side effects: none (pure).
    fn is_excluded_from_semantic_diagnostics(
        &self,
        file: &ParsedFile,
        lib_dir: &Option<String>,
    ) -> bool {
        let is_lib = match lib_dir {
            Some(dir) if !dir.is_empty() => file.file_name().starts_with(dir.as_str()),
            _ => false,
        };
        is_lib || self.skip_type_checking(file)
    }

    /// The directory (with trailing `/`) that the default-library files live in,
    /// derived from the resolved default lib's path, or `None` when no default lib
    /// was loaded.
    ///
    /// Side effects: none (pure).
    fn default_library_directory(&self) -> Option<String> {
        self.processed.default_lib_file().map(|f| {
            let name = f.file_name();
            match name.rfind('/') {
                Some(slash) => name[..=slash].to_string(),
                None => String::new(),
            }
        })
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

    /// The root config's directly-resolved project references, in declaration
    /// order (an entry is `None` when that referenced config could not be read).
    ///
    /// Empty when the program was not built from a config file.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.GetResolvedProjectReferences
    pub fn get_resolved_project_references(&self) -> Vec<Option<&ParsedCommandLine>> {
        match &self.resolved_project_references {
            Some(graph) => graph.get_resolved_project_references(),
            None => Vec::new(),
        }
    }

    /// The project-reference consistency diagnostics (`TS6306` non-composite,
    /// `TS6310` noEmit, `TS6053` not-found) found while constructing the program.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:verifyProjectReferences (programDiagnostics)
    pub fn project_reference_diagnostics(&self) -> &[ProjectReferenceDiagnostic] {
        &self.reference_diagnostics
    }

    /// The topological build order of the project-reference graph plus any
    /// `TS6202` circular-graph diagnostics (empty/no-op when the program has no
    /// references).
    ///
    /// This is the reachable input a `--build` orchestration (P9) drives; the
    /// program itself does not build referenced projects.
    ///
    /// Side effects: none (pure).
    // Go: internal/execute/build/orchestrator.go:Orchestrator.setupBuildTask
    pub fn build_order(&self) -> BuildOrder {
        match &self.resolved_project_references {
            Some(graph) => graph.get_build_order(),
            None => BuildOrder::default(),
        }
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

    /// Emits the JavaScript output for `file` to `js_file_path` (with its source
    /// map when enabled), writing through `options.write_file` (or the host file
    /// system).
    ///
    /// When `--sourceMap`/`--inlineSourceMap` is set this drives a
    /// `sourcemap::Generator` while printing, appends the
    /// `//# sourceMappingURL=` comment (a relative `.map` path or an inlined
    /// `data:` URL), and writes the separate `.js.map` in file mode.
    ///
    /// Side effects: writes the emitted `.js` (and `.js.map`) file.
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

        let opts = self.options();
        let emit_source_maps = should_emit_source_maps(opts, file);
        let source_map_file_path = get_source_map_file_path(js_file_path, opts);

        // Construct the generator with the generated file's base name, source
        // root, and the directory sources are relativized against.
        let generator = if emit_source_maps {
            Some(Generator::new(
                &get_base_file_name(&normalize_slashes(js_file_path)),
                &get_source_root(opts),
                &self.get_source_map_directory(opts, js_file_path),
                ComparePathsOptions {
                    use_case_sensitive_file_names: self
                        .compare_paths_options
                        .use_case_sensitive_file_names,
                    current_directory: self.compare_paths_options.current_directory.clone(),
                },
            ))
        } else {
            None
        };

        let (mut text, generator) =
            emit_js_text_with_source_map(file.file_name(), file.text(), opts, generator);

        let mut source_map_url_pos = -1;
        if let Some(mut generator) = generator {
            // Record the source-map emit result (Go appends for source map /
            // inline source map / declaration maps).
            if opts.source_map.is_true() || opts.inline_source_map.is_true() {
                result.source_maps.push(SourceMapEmitResult {
                    input_source_file_names: generator.sources().to_vec(),
                    generated_file: js_file_path.to_string(),
                });
            }

            let source_mapping_url =
                self.get_source_mapping_url(opts, &mut generator, &source_map_file_path);
            if !source_mapping_url.is_empty() {
                // Mirror Go's writer: only insert a newline when not already at
                // the start of a line (the printer output ends with one).
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push_str(if opts.new_line == NewLineKind::Crlf {
                        "\r\n"
                    } else {
                        "\n"
                    });
                }
                source_map_url_pos = text.len() as i32;
                text.push_str("//# sourceMappingURL=");
                text.push_str(&source_mapping_url);
            }

            // Write the separate `.map` file (file mode only).
            if !source_map_file_path.is_empty() {
                let source_map = generator.to_string();
                let map_data = WriteFileData {
                    source_map_url_pos: -1,
                    skipped_dts_write: false,
                };
                if self
                    .write_text(&source_map_file_path, &source_map, &map_data, options)
                    .is_ok()
                {
                    result.emitted_files.push(source_map_file_path);
                }
            }
        }

        if self.options().emit_bom.is_true() {
            text = tsgo_stringutil::add_utf8_byte_order_mark(&text);
        }
        let data = WriteFileData {
            source_map_url_pos,
            skipped_dts_write: false,
        };
        if self.write_text(js_file_path, &text, &data, options).is_ok() {
            result.emitted_files.push(js_file_path.to_string());
        }
        // DEFER(P6): on write error, add a `Could_not_write_file` diagnostic
        // (needs the `ast.Diagnostic` compiler-diagnostic constructor surface).
    }

    /// Returns the directory that source-map `sources` are relativized against
    /// (the reachable subset of Go's `getSourceMapDirectory`).
    ///
    /// `--sourceRoot`/`--mapRoot` (which re-root the map directory) are deferred;
    /// without them the directory is that of the generated `.js`.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/emitter.go:emitter.getSourceMapDirectory
    fn get_source_map_directory(&self, _options: &CompilerOptions, js_file_path: &str) -> String {
        // DEFER(P6): `--sourceRoot` -> CommonSourceDirectory; `--mapRoot` ->
        // re-rooted per-file directory. blocked-by: common-source-directory +
        // mapRoot layout (reachable subset uses the `.js` directory).
        get_directory_path(&normalize_path(js_file_path))
    }

    /// Computes the `//# sourceMappingURL=` value: an inlined base64 `data:` URL
    /// for `--inlineSourceMap`, otherwise the URI-encoded base name of the
    /// `.map` file.
    ///
    /// Side effects: flushes the generator's pending mapping (when inlining).
    // Go: internal/compiler/emitter.go:emitter.getSourceMappingURL
    fn get_source_mapping_url(
        &self,
        options: &CompilerOptions,
        generator: &mut Generator,
        source_map_file_path: &str,
    ) -> String {
        if options.inline_source_map.is_true() {
            return generator.base64_data_url();
        }
        // DEFER(P6): `--mapRoot` re-rooting. blocked-by: common-source-directory.
        let source_map_file = get_base_file_name(&normalize_slashes(source_map_file_path));
        tsgo_stringutil::encode_uri(&source_map_file)
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

/// Reports whether source maps should be emitted for `file` (the reachable
/// subset of Go's `shouldEmitSourceMaps`).
///
/// Source maps are produced when `--sourceMap` or `--inlineSourceMap` is set and
/// the file is not JSON.
///
/// Side effects: none (pure).
// Go: internal/compiler/emitter.go:shouldEmitSourceMaps
fn should_emit_source_maps(options: &CompilerOptions, file: &ParsedFile) -> bool {
    (options.source_map.is_true() || options.inline_source_map.is_true())
        && !tsgo_tspath::file_extension_is(file.file_name(), tsgo_tspath::EXTENSION_JSON)
}

/// Returns the source-map `sourceRoot` value: the normalized `--sourceRoot`
/// option with a trailing separator, or empty when unset.
///
/// Side effects: none (pure).
// Go: internal/compiler/emitter.go:getSourceRoot
fn get_source_root(options: &CompilerOptions) -> String {
    let source_root = normalize_slashes(&options.source_root);
    if source_root.is_empty() {
        source_root
    } else {
        tsgo_tspath::ensure_trailing_directory_separator(&source_root)
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
/// Produced for each `.js` when `--sourceMap`/`--inlineSourceMap` is set (P6-7).
/// The full `RawSourceMap` payload field is still omitted from this struct (the
/// reachable subset records only the input source names + generated file);
/// callers needing the serialized map read it from the emitted `.js.map`.
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
    /// The emitted source maps (one per `.js` when source maps are enabled).
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

/// Converts a binder [`BinderDiagnostic`] into the [`tsgo_checker::Diagnostic`]
/// shape the program returns, the bridge that lets a binder
/// `bindDiagnostic` flow through the program's bind-and-check pass alongside
/// checker diagnostics (Go: both are `*ast.Diagnostic`, so no conversion is
/// needed there; this port keeps two diagnostic types and reconciles them
/// here).
///
/// The span is refined to skip leading trivia against the OWNING file's `text`,
/// matching Go's `createDiagnosticForNode` → `scanner.GetErrorRangeForNode`
/// (the default case for the name nodes the binder reports merge conflicts on:
/// `SkipTrivia(text, node.Pos())..node.End()`). The binder stores the raw node
/// loc, so the start is trivia-skipped here to byte-match `tsc`'s baseline. The
/// message text is localized exactly as the checker does
/// (`tsgo_diagnostics::format(&message.to_string(), args)`), and the binder's
/// `related` list is converted recursively into `related_information` (Go's
/// `Diagnostic.relatedInformation`).
///
/// Sorts and deduplicates a single file's semantic (bind-and-check) diagnostics,
/// the per-file reachable subset of Go's `SortAndDeduplicateDiagnostics`
/// (`compactAndMergeRelatedInfos` after a `CompareDiagnostics` sort). Within one
/// file there is no path key, so diagnostics order by `(start, end, code,
/// message)`; a run of consecutive entries that are equal IGNORING related
/// information is compacted into one, merging (then sorting + deduplicating)
/// their related-information lists.
///
/// This removes the IDENTICAL duplicate a binder merge conflict re-emits on the
/// SAME prior declaration across two separate conflicts — e.g. a `get x` flagged
/// once when a method of the same name collides and again when the trailing
/// `set x` collides (the surviving symbol having been marked a full accessor) —
/// exactly as `tsc`'s `GetSemanticDiagnostics` collapses them.
///
/// Side effects: none (pure).
// Go: internal/compiler/program.go:SortAndDeduplicateDiagnostics / compactAndMergeRelatedInfos
fn sort_and_deduplicate_diagnostics(
    mut diagnostics: Vec<tsgo_checker::Diagnostic>,
) -> Vec<tsgo_checker::Diagnostic> {
    if diagnostics.len() < 2 {
        return diagnostics;
    }
    diagnostics.sort_by(compare_checker_diagnostics);
    let mut result: Vec<tsgo_checker::Diagnostic> = Vec::with_capacity(diagnostics.len());
    let mut i = 0;
    while i < diagnostics.len() {
        // Count the run of entries equal to `diagnostics[i]` ignoring related info.
        let mut n = 1;
        while i + n < diagnostics.len()
            && equal_diagnostics_no_related_info(&diagnostics[i], &diagnostics[i + n])
        {
            n += 1;
        }
        let mut kept = diagnostics[i].clone();
        if n > 1 {
            // Merge the run's related-information lists (sorted + deduplicated).
            let mut related: Vec<tsgo_checker::Diagnostic> = Vec::new();
            for entry in diagnostics.iter().skip(i).take(n) {
                related.extend(entry.related_information.iter().cloned());
            }
            if !related.is_empty() {
                related.sort_by(compare_checker_diagnostics);
                related.dedup();
                kept.related_information = related;
            }
        }
        result.push(kept);
        i += n;
    }
    result
}

/// Removes the diagnostics suppressed by a preceding `@ts-ignore` /
/// `@ts-expect-error` comment directive (Go's
/// `Program.getDiagnosticsWithPrecedingDirectives`).
///
/// A diagnostic is dropped when, scanning upward from the line above it, the
/// first non-blank / non-comment line is preceded by a directive line (blank
/// and comment lines in between are skipped). This is the reachable subset of
/// Go's filter: the directive lines are recovered by a text scan of the
/// line-leading `//` / `/*` comment forms rather than from the scanner's
/// captured `SourceFile.CommentDirectives` (which the parser does not yet
/// thread through).
///
/// DEFER(P10): the scanner-captured directive list (so a TRAILING directive
/// comment such as `code(); // @ts-ignore` is recognized), and the
/// `Unused_ts_expect_error_directive` (TS2578) emitted for an `@ts-expect-error`
/// that suppressed nothing. Only the suppression half is ported; it can only
/// remove diagnostics, never add one, so it is parity-safe. blocked-by: the
/// scanner's comment-directive capture on the source file.
///
/// Side effects: none (pure).
// Go: internal/compiler/program.go:Program.getDiagnosticsWithPrecedingDirectives(1333)
fn filter_diagnostics_with_preceding_directives(
    text: &str,
    diagnostics: Vec<tsgo_checker::Diagnostic>,
) -> Vec<tsgo_checker::Diagnostic> {
    let line_starts = tsgo_core::compute_ecma_line_starts(text);
    // Mark every line that bears a `@ts-ignore` / `@ts-expect-error` directive.
    let mut directive_line: Vec<bool> = vec![false; line_starts.len()];
    let mut any_directive = false;
    for (line, start) in line_starts.iter().enumerate() {
        let line_end = line_starts
            .get(line + 1)
            .map(|p| p.0 as usize)
            .unwrap_or(text.len());
        if line_is_comment_directive(&text[start.0 as usize..line_end]) {
            directive_line[line] = true;
            any_directive = true;
        }
    }
    if !any_directive {
        return diagnostics;
    }
    diagnostics
        .into_iter()
        .filter(|diag| {
            let (diag_line, _) =
                tsgo_core::position_to_line_and_byte_offset(diag.start, &line_starts);
            let mut line = diag_line - 1;
            while line >= 0 {
                let l = line as usize;
                if directive_line[l] {
                    return false;
                }
                if !is_comment_or_blank_line(text, line_starts[l].0 as usize) {
                    break;
                }
                line -= 1;
            }
            true
        })
        .collect()
}

/// Reports whether a (single) source line begins with a `//` or `/*` comment
/// whose first token is the `@ts-ignore` / `@ts-expect-error` directive (Go's
/// scanner `processCommentDirective`, restricted here to the line-leading form).
///
/// Side effects: none (pure).
// Go: internal/scanner/scanner.go:Scanner.processCommentDirective(955)
fn line_is_comment_directive(line: &str) -> bool {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let after_open = if let Some(rest) = trimmed.strip_prefix("//") {
        // Skip any extra leading slashes (`///`).
        rest.trim_start_matches('/')
    } else if let Some(rest) = trimmed.strip_prefix("/*") {
        // Skip extra `*` then any `/`, mirroring Go's `/`+`*` skip.
        rest.trim_start_matches(['*', '/'])
    } else {
        return false;
    };
    let directive = after_open.trim_start_matches([' ', '\t']);
    let Some(rest) = directive.strip_prefix('@') else {
        return false;
    };
    rest.starts_with("ts-expect-error") || rest.starts_with("ts-ignore")
}

/// Reports whether the line starting at byte `pos` is blank or a `//` line
/// comment (Go's `isCommentOrBlankLine`): skip spaces / tabs, then the line is
/// blank/comment when it is at end of text, at a line break, or at `//`.
///
/// Side effects: none (pure).
// Go: internal/compiler/program.go:isCommentOrBlankLine(1396)
fn is_comment_or_blank_line(text: &str, pos: usize) -> bool {
    let bytes = text.as_bytes();
    let mut pos = pos;
    while pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
        pos += 1;
    }
    pos == bytes.len()
        || (pos < bytes.len() && (bytes[pos] == b'\r' || bytes[pos] == b'\n'))
        || (pos + 1 < bytes.len() && bytes[pos] == b'/' && bytes[pos + 1] == b'/')
}

/// Orders two checker diagnostics within one file: by start, then end
/// (`start + length`), then code, then message — the path-less reachable subset
/// of Go's `ast.CompareDiagnostics`.
// Go: internal/ast/diagnostic.go:CompareDiagnostics
fn compare_checker_diagnostics(
    a: &tsgo_checker::Diagnostic,
    b: &tsgo_checker::Diagnostic,
) -> std::cmp::Ordering {
    a.start
        .cmp(&b.start)
        .then_with(|| (a.start + a.length).cmp(&(b.start + b.length)))
        .then_with(|| a.code.cmp(&b.code))
        .then_with(|| a.message.cmp(&b.message))
        .then_with(|| a.message_chain.len().cmp(&b.message_chain.len()))
}

/// Reports whether two checker diagnostics are equal IGNORING their related
/// information (Go's `ast.EqualDiagnosticsNoRelatedInfo`): same span, code,
/// localized message (which bakes in the message args), and message chain.
// Go: internal/ast/diagnostic.go:EqualDiagnosticsNoRelatedInfo
fn equal_diagnostics_no_related_info(
    a: &tsgo_checker::Diagnostic,
    b: &tsgo_checker::Diagnostic,
) -> bool {
    a.start == b.start
        && a.length == b.length
        && a.code == b.code
        && a.message == b.message
        && a.message_chain == b.message_chain
}

/// Side effects: none (pure).
// Go: internal/ast/diagnostic.go:NewDiagnostic + internal/scanner/scanner.go:GetErrorRangeForNode
fn binder_diagnostic_to_checker(
    diagnostic: &BinderDiagnostic,
    text: &str,
) -> tsgo_checker::Diagnostic {
    let start = tsgo_scanner::skip_trivia(text, diagnostic.loc.pos());
    let args: Vec<&str> = diagnostic.args.iter().map(String::as_str).collect();
    tsgo_checker::Diagnostic {
        code: diagnostic.message.code(),
        category: diagnostic.message.category(),
        message: tsgo_diagnostics::format(&diagnostic.message.to_string(), &args),
        start,
        length: diagnostic.loc.end() - start,
        related_information: diagnostic
            .related
            .iter()
            .map(|related| binder_diagnostic_to_checker(related, text))
            .collect(),
        message_chain: Vec::new(),
    }
}

/// Reports whether a BINDER diagnostic `code` is in Go's `plainJSErrors` set, so
/// it survives the plain-JS filter (Go keeps only `plainJSErrors` codes of a
/// plain JS file's combined diagnostics).
///
/// The binder emits only a small set of codes (`report_merge_conflict` +
/// `add_class_prototype`): `Duplicate_identifier` (TS2300) and the enum-merge
/// TS2567 are NOT in `plainJSErrors` (dropped in plain JS), while
/// `Cannot_redeclare_block_scoped_variable` (TS2451),
/// `A_module_cannot_have_multiple_default_exports` (TS2528), and the two
/// export-default-location notes (TS2752/TS2753) ARE. This is the binder slice
/// of Go's `plainJSErrors`; the full set (grammar/strict-mode codes) is not
/// reached by this port's binder.
///
/// Side effects: none (pure).
// Go: internal/compiler/program.go:plainJSErrors (binder-error subset)
fn binder_code_allowed_in_plain_js(code: i32) -> bool {
    code == tsgo_diagnostics::CANNOT_REDECLARE_BLOCK_SCOPED_VARIABLE_0.code()
        || code == tsgo_diagnostics::A_MODULE_CANNOT_HAVE_MULTIPLE_DEFAULT_EXPORTS.code()
        || code == tsgo_diagnostics::ANOTHER_EXPORT_DEFAULT_IS_HERE.code()
        || code == tsgo_diagnostics::THE_FIRST_EXPORT_DEFAULT_IS_HERE.code()
}

#[cfg(test)]
#[path = "program_test.rs"]
mod tests;
