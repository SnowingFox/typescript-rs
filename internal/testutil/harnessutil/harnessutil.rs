//! Port of Go `internal/testutil/harnessutil/harnessutil.go` (reachable subset).
//!
//! The compiler test harness: given a set of in-memory test files and compiler
//! options, build a [`tsgo_compiler::Program`] over a `MapFs` (wrapped with the
//! embedded `bundled:///` default-lib file system and an
//! [`OutputRecorderFs`](crate::OutputRecorderFs)), collect its diagnostics,
//! emit, and return a baseline-comparable [`CompilationResult`].
//!
//! # Divergence from Go
//!
//! Go's `*ast.Diagnostic` carries its own source file. The ported
//! [`tsgo_checker::Diagnostic`] does not (the checker reports a `start`/`length`
//! into the file it is checking), so the harness re-associates diagnostics with
//! a [`HarnessFile`] here: syntactic diagnostics keep their owning file, and
//! semantic diagnostics are attributed to the (single) user source file.
//!
//! DEFER(P10): multi-user-file semantic-diagnostic attribution (every semantic
//! diagnostic is attributed to the first non-library source file).
//! blocked-by: a per-file semantic-diagnostics API on `tsgo_compiler::Program`
//! (this crate must not edit the compiler); the reachable harness round proves
//! the pipeline on single-file inline cases.

use std::rc::Rc;
use std::sync::Arc;

use indexmap::IndexMap;

use tsgo_compiler::{
    check_source_files_belong_to_root_dir, new_compiler_host, new_program, CompilerHost,
    EmitOptions, ProgramOptions,
};
use tsgo_core::compileroptions::{CompilerOptions, NewLineKind, ResolutionMode};
use tsgo_core::text::TextPos;
use tsgo_core::tristate::Tristate;
use tsgo_diagnostics::{self as diagnostics, Category};
use tsgo_diagnosticwriter::{Diagnostic as DiagnosticTrait, FileLike};
use tsgo_locale::Locale;
use tsgo_module::{
    get_automatic_type_directive_names, ResolutionHost, Resolver, INFERRED_TYPES_CONTAINING_FILE,
};
use tsgo_tsoptions::{
    new_parsed_command_line, parse_compiler_options, CommandLineOptionKind, EnumValue, OptionValue,
    ParsedCommandLine, COMMAND_LINE_COMPILER_OPTIONS_MAP,
};
use tsgo_tspath::get_directory_path;
use tsgo_tspath::{
    file_extension_is, get_base_file_name, get_normalized_absolute_path, ComparePathsOptions,
    EXTENSION_JSON, EXTENSION_TS_BUILD_INFO,
};
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use crate::OutputRecorderFs;

/// One in-memory test file: its name (unit) and its source text.
///
/// Side effects: none (plain data).
// Go: internal/testutil/harnessutil/harnessutil.go:TestFile
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFile {
    /// The file's name (an absolute or `currentDirectory`-relative path).
    pub unit_name: String,
    /// The file's source text.
    pub content: String,
}

/// A map of harness/compiler setting name to its raw string value (Go's
/// `TestConfiguration`).
///
/// Side effects: none (plain data).
// Go: internal/testutil/harnessutil/harnessutil.go:TestConfiguration
pub type TestConfiguration = IndexMap<String, String>;

/// The harness-specific options (a reachable subset of Go's `HarnessOptions`).
///
/// Side effects: none (plain data).
// Go: internal/testutil/harnessutil/harnessutil.go:HarnessOptions
#[derive(Debug, Clone)]
pub struct HarnessOptions {
    /// Whether file names are matched case-sensitively.
    pub use_case_sensitive_file_names: bool,
    /// The working directory used to root relative file names.
    pub current_directory: String,
    /// Suppress the `.types`/`.symbols` baselines (deferred; see crate docs).
    pub no_types_and_symbols: bool,
    /// Add suggestion diagnostics to the error baseline (deferred).
    pub capture_suggestions: bool,
}

impl Default for HarnessOptions {
    fn default() -> Self {
        HarnessOptions {
            use_case_sensitive_file_names: true,
            current_directory: String::new(),
            no_types_and_symbols: false,
            capture_suggestions: false,
        }
    }
}

/// A source file as seen by the diagnostic renderers: name, text, and the ECMA
/// line map (computed once).
///
/// Side effects: none (plain data).
// Go: internal/diagnosticwriter/diagnosticwriter.go:FileLike (harness backing)
#[derive(Debug)]
pub struct HarnessFile {
    file_name: String,
    text: String,
    line_map: Vec<TextPos>,
}

impl HarnessFile {
    /// Bundles a file name and text, computing the ECMA line map.
    ///
    /// Side effects: none (pure).
    pub fn new(file_name: String, text: String) -> HarnessFile {
        let line_map = tsgo_core::compute_ecma_line_starts(&text);
        HarnessFile {
            file_name,
            text,
            line_map,
        }
    }
}

impl FileLike for HarnessFile {
    fn file_name(&self) -> &str {
        &self.file_name
    }
    fn text(&self) -> &str {
        &self.text
    }
    fn ecma_line_map(&self) -> &[TextPos] {
        &self.line_map
    }
}

/// A diagnostic plus the file it refers to, rendered into the harness baselines.
///
/// Implements [`tsgo_diagnosticwriter::Diagnostic`] so it can be formatted by
/// the shared diagnostic writer; the [`message`](Self::message) is already
/// localized/argument-substituted (the checker pre-localizes; syntactic and
/// option diagnostics are localized at construction).
///
/// Side effects: none (plain data).
// Go: internal/ast/diagnostic.go:Diagnostic (harness view)
#[derive(Debug, Clone)]
pub struct HarnessDiagnostic {
    file: Option<Rc<HarnessFile>>,
    code: i32,
    category: Category,
    message: String,
    start: i32,
    length: i32,
    message_chain: Vec<HarnessDiagnostic>,
    related_information: Vec<HarnessDiagnostic>,
}

impl HarnessDiagnostic {
    /// Builds a harness diagnostic from its parts (an already-localized
    /// message), with no message chain or related information.
    ///
    /// Side effects: none (pure).
    pub fn new(
        file: Option<Rc<HarnessFile>>,
        code: i32,
        category: Category,
        message: String,
        start: i32,
        length: i32,
    ) -> HarnessDiagnostic {
        HarnessDiagnostic {
            file,
            code,
            category,
            message,
            start,
            length,
            message_chain: Vec::new(),
            related_information: Vec::new(),
        }
    }

    /// The file this diagnostic refers to, or `None` for a global diagnostic.
    ///
    /// Side effects: none (pure).
    pub fn file_name(&self) -> Option<&str> {
        self.file.as_deref().map(FileLike::file_name)
    }

    /// The numeric diagnostic code (the `xxxx` in `TSxxxx`).
    ///
    /// Side effects: none (pure).
    pub fn code(&self) -> i32 {
        self.code
    }

    /// The diagnostic category.
    ///
    /// Side effects: none (pure).
    pub fn category(&self) -> Category {
        self.category
    }

    /// The localized primary message text.
    ///
    /// Side effects: none (pure).
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The start byte offset of the diagnostic span.
    ///
    /// Side effects: none (pure).
    pub fn start(&self) -> i32 {
        self.start
    }

    /// The byte length of the diagnostic span.
    ///
    /// Side effects: none (pure).
    pub fn length(&self) -> i32 {
        self.length
    }

    fn from_message(
        message: &'static tsgo_diagnostics::Message,
        args: &[String],
        file: Option<Rc<HarnessFile>>,
        start: i32,
        length: i32,
        locale: &Locale,
    ) -> HarnessDiagnostic {
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        HarnessDiagnostic {
            file,
            code: message.code(),
            category: message.category(),
            message: message.localize(locale, &arg_refs),
            start,
            length,
            message_chain: Vec::new(),
            related_information: Vec::new(),
        }
    }

    fn from_message_with_chain(
        message: &'static tsgo_diagnostics::Message,
        args: &[String],
        chain: Vec<HarnessDiagnostic>,
        locale: &Locale,
    ) -> HarnessDiagnostic {
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        HarnessDiagnostic {
            file: None,
            code: message.code(),
            category: message.category(),
            message: message.localize(locale, &arg_refs),
            start: 0,
            length: 0,
            message_chain: chain,
            related_information: Vec::new(),
        }
    }

    fn message_only(
        message: &'static tsgo_diagnostics::Message,
        args: &[String],
        locale: &Locale,
    ) -> HarnessDiagnostic {
        HarnessDiagnostic::from_message(message, args, None, 0, 0, locale)
    }

    fn from_checker(
        diag: &tsgo_checker::Diagnostic,
        file: Option<Rc<HarnessFile>>,
    ) -> HarnessDiagnostic {
        HarnessDiagnostic {
            file: file.clone(),
            code: diag.code,
            category: diag.category,
            message: diag.message.clone(),
            start: diag.start,
            length: diag.length,
            message_chain: diag
                .message_chain
                .iter()
                .map(harness_chain_from_checker)
                .collect(),
            related_information: diag
                .related_information
                .iter()
                .map(|r| HarnessDiagnostic::from_checker(r, file.clone()))
                .collect(),
        }
    }
}

// Converts a checker message-chain entry into a file-less harness diagnostic
// (the renderer only flattens its message + nested chain).
// Go: internal/ast/diagnostic.go:Diagnostic (messageChain entry)
fn harness_chain_from_checker(chain: &tsgo_checker::DiagnosticMessageChain) -> HarnessDiagnostic {
    HarnessDiagnostic {
        file: None,
        code: chain.code,
        category: chain.category,
        message: chain.message.clone(),
        start: 0,
        length: 0,
        message_chain: chain.next.iter().map(harness_chain_from_checker).collect(),
        related_information: Vec::new(),
    }
}

impl DiagnosticTrait for HarnessDiagnostic {
    fn file(&self) -> Option<&dyn FileLike> {
        self.file.as_deref().map(|f| f as &dyn FileLike)
    }
    fn pos(&self) -> i32 {
        self.start
    }
    fn end(&self) -> i32 {
        self.start + self.length
    }
    fn len(&self) -> i32 {
        self.length
    }
    fn code(&self) -> i32 {
        self.code
    }
    fn category(&self) -> Category {
        self.category
    }
    fn localize(&self, _locale: &Locale) -> String {
        self.message.clone()
    }
    fn message_chain(&self) -> Vec<&dyn DiagnosticTrait> {
        self.message_chain
            .iter()
            .map(|c| c as &dyn DiagnosticTrait)
            .collect()
    }
    fn related_information(&self) -> Vec<&dyn DiagnosticTrait> {
        self.related_information
            .iter()
            .map(|r| r as &dyn DiagnosticTrait)
            .collect()
    }
}

/// The baseline-comparable result of compiling a set of test files: the
/// collected diagnostics (with file association), the recorded outputs, and the
/// effective options.
///
/// Side effects: none (owns the compile output).
// Go: internal/testutil/harnessutil/harnessutil.go:CompilationResult
pub struct CompilationResult {
    diagnostics: Vec<HarnessDiagnostic>,
    options: CompilerOptions,
    harness_options: HarnessOptions,
    outputs: Vec<TestFile>,
    emit_skipped: bool,
}

impl CompilationResult {
    /// The collected diagnostics (option + syntactic + semantic), in collection
    /// order.
    ///
    /// Side effects: none (pure).
    // Go: internal/testutil/harnessutil/harnessutil.go:CompilationResult.Diagnostics
    pub fn diagnostics(&self) -> &[HarnessDiagnostic] {
        &self.diagnostics
    }

    /// The effective compiler options the program was built with.
    ///
    /// Side effects: none (pure).
    // Go: internal/testutil/harnessutil/harnessutil.go:CompilationResult.Options
    pub fn options(&self) -> &CompilerOptions {
        &self.options
    }

    /// The harness options the program was built with.
    ///
    /// Side effects: none (pure).
    // Go: internal/testutil/harnessutil/harnessutil.go:CompilationResult.HarnessOptions
    pub fn harness_options(&self) -> &HarnessOptions {
        &self.harness_options
    }

    /// Every emitted file, in write order.
    ///
    /// Side effects: none (pure).
    // Go: internal/testutil/harnessutil/harnessutil.go:CompilationResult.Outputs
    pub fn outputs(&self) -> &[TestFile] {
        &self.outputs
    }

    /// The emitted file written to `unit_name`, if any.
    ///
    /// Side effects: none (pure).
    // Go: internal/testutil/harnessutil/harnessutil.go:CompilationResult.GetOutput
    pub fn get_output(&self, unit_name: &str) -> Option<&TestFile> {
        self.outputs.iter().find(|f| f.unit_name == unit_name)
    }

    /// Whether emit was skipped (e.g. `--noEmit`).
    ///
    /// Side effects: none (pure).
    pub fn emit_skipped(&self) -> bool {
        self.emit_skipped
    }
}

/// Compiles `input_files` (plus any `other_files` on the file system) with the
/// given already-defaulted options, returning the baseline-comparable result.
///
/// Builds a `MapFs` over the files, wraps it with the embedded `bundled:///`
/// default-lib file system and an [`OutputRecorderFs`], constructs a program,
/// collects option + syntactic + semantic diagnostics, and emits.
///
/// # Examples
/// ```
/// use tsgo_testutil_harnessutil::{compile_files_ex, HarnessOptions, TestFile};
/// use tsgo_core::compileroptions::CompilerOptions;
///
/// let result = compile_files_ex(
///     vec![TestFile { unit_name: "/.src/a.ts".into(), content: "const x: number = 1;".into() }],
///     vec![],
///     HarnessOptions::default(),
///     CompilerOptions::default(),
///     "/.src",
/// );
/// assert!(result.diagnostics().is_empty());
/// ```
///
/// DEFER(P10): symlinks, an in-test `tsconfig.json`, `@libFiles`, declaration /
/// suggestion diagnostics, and the source-map record.
/// blocked-by: VFS symlink/config-host wiring + the language-service type
/// writer; the reachable round compiles the units against the bundled lib.
///
/// Side effects: reads the (in-memory) file system and writes emitted files
/// into it (recorded for the result).
/// Minimal [`ResolutionHost`] for type-reference resolution during harness compiles.
struct HarnessResolutionHost {
    fs: Arc<dyn Fs + Send + Sync>,
    current_directory: String,
}

impl ResolutionHost for HarnessResolutionHost {
    fn fs(&self) -> &dyn Fs {
        self.fs.as_ref()
    }

    fn get_current_directory(&self) -> &str {
        &self.current_directory
    }
}

/// Collects config-file and early-program diagnostics that Go surfaces through
/// `GetConfigFileParsingDiagnostics` / `GetProgramDiagnostics` before checking.
///
/// Side effects: reads the file system through `host`.
// Go: internal/compiler/fileloader.go:resolveAutomaticTypeDirectives
//     + internal/compiler/program.go:checkSourceFilesBelongToPath
fn collect_config_and_early_program_diagnostics(
    config: &ParsedCommandLine,
    host: &dyn CompilerHost,
    locale: &Locale,
) -> Vec<HarnessDiagnostic> {
    let mut diags = Vec::new();

    for err in config.errors() {
        diags.push(HarnessDiagnostic::from_message(
            err.message,
            &err.args,
            None,
            0,
            0,
            locale,
        ));
    }

    let resolution_host: Arc<dyn ResolutionHost> = Arc::new(HarnessResolutionHost {
        fs: host.fs(),
        current_directory: host.get_current_directory().to_string(),
    });
    let resolver = Resolver::new(
        Arc::clone(&resolution_host),
        Arc::new(config.compiler_options().clone()),
        "",
        "",
    );
    let containing_directory = if !config.compiler_options().config_file_path.is_empty() {
        get_directory_path(&config.compiler_options().config_file_path)
    } else {
        host.get_current_directory().to_string()
    };
    let containing_file =
        tsgo_tspath::combine_paths(&containing_directory, &[INFERRED_TYPES_CONTAINING_FILE]);

    for name in
        get_automatic_type_directive_names(config.compiler_options(), resolution_host.as_ref())
    {
        let (resolved, _) = resolver.resolve_type_reference_directive(
            &name,
            &containing_file,
            ResolutionMode::None,
            None,
        );
        if !resolved.is_resolved() {
            let detail = HarnessDiagnostic::message_only(
                &diagnostics::ENTRY_POINT_OF_TYPE_LIBRARY_0_SPECIFIED_IN_COMPILEROPTIONS,
                std::slice::from_ref(&name),
                locale,
            );
            let mut because = HarnessDiagnostic::message_only(
                &diagnostics::THE_FILE_IS_IN_THE_PROGRAM_BECAUSE_COLON,
                &[],
                locale,
            );
            because.message_chain = vec![detail];
            diags.push(HarnessDiagnostic::from_message_with_chain(
                &diagnostics::CANNOT_FIND_TYPE_DEFINITION_FILE_FOR_0,
                &[name],
                vec![because],
                locale,
            ));
        }
    }

    for prd in check_source_files_belong_to_root_dir(config) {
        let detail = HarnessDiagnostic::message_only(
            &diagnostics::ROOT_FILE_SPECIFIED_FOR_COMPILATION,
            &[],
            locale,
        );
        let mut because = HarnessDiagnostic::message_only(
            &diagnostics::THE_FILE_IS_IN_THE_PROGRAM_BECAUSE_COLON,
            &[],
            locale,
        );
        because.message_chain = vec![detail];
        diags.push(HarnessDiagnostic::from_message_with_chain(
            prd.message,
            &prd.args,
            vec![because],
            locale,
        ));
    }

    diags
}

// Go: internal/testutil/harnessutil/harnessutil.go:CompileFilesEx
pub fn compile_files_ex(
    input_files: Vec<TestFile>,
    other_files: Vec<TestFile>,
    harness_options: HarnessOptions,
    mut compiler_options: CompilerOptions,
    current_directory: &str,
    ts_config: Option<ParsedCommandLine>,
) -> CompilationResult {
    // Root the path-typed compiler options to the current directory, mirroring
    // Go's `CompileFilesEx` (`ts.convertToOptionsWithAbsolutePaths`). The
    // emitter combines `outDir` (etc.) into output paths verbatim; if it stays
    // relative (e.g. `outDir: dist`), the in-memory VFS — which rejects
    // non-absolute paths — panics when the emit is written. Rooting keeps the
    // output path absolute. `type_roots` is the only `Vec` whose Go counterpart
    // is a slice of file paths; the rest are single paths.
    // Go: internal/testutil/harnessutil/harnessutil.go:CompileFilesEx
    let root = |value: &str| get_normalized_absolute_path(value, current_directory);
    if !compiler_options.out_dir.is_empty() {
        compiler_options.out_dir = root(&compiler_options.out_dir);
    }
    if !compiler_options.project.is_empty() {
        compiler_options.project = root(&compiler_options.project);
    }
    if !compiler_options.root_dir.is_empty() {
        compiler_options.root_dir = root(&compiler_options.root_dir);
    }
    if !compiler_options.ts_build_info_file.is_empty() {
        compiler_options.ts_build_info_file = root(&compiler_options.ts_build_info_file);
    }
    if !compiler_options.base_url.is_empty() {
        compiler_options.base_url = root(&compiler_options.base_url);
    }
    if !compiler_options.declaration_dir.is_empty() {
        compiler_options.declaration_dir = root(&compiler_options.declaration_dir);
    }
    for dir in &mut compiler_options.root_dirs {
        *dir = root(dir);
    }
    if let Some(type_roots) = compiler_options.type_roots.as_mut() {
        for type_root in type_roots {
            *type_root = root(type_root);
        }
    }

    // Root file names (skip JSON / build-info inputs), normalized absolute.
    let mut program_file_names: Vec<String> = Vec::new();
    for file in &input_files {
        let file_name = get_normalized_absolute_path(&file.unit_name, current_directory);
        if !file_extension_is(&file_name, EXTENSION_JSON)
            && !file_extension_is(&file_name, EXTENSION_TS_BUILD_INFO)
        {
            program_file_names.push(file_name);
        }
    }

    // In-memory file system over the input + other files.
    let mut entries: Vec<(String, String)> = Vec::new();
    for file in input_files.iter().chain(other_files.iter()) {
        let file_name = get_normalized_absolute_path(&file.unit_name, current_directory);
        entries.push((file_name, file.content.clone()));
    }

    let use_case_sensitive = harness_options.use_case_sensitive_file_names;
    let inner = MapFs::from_map(entries, use_case_sensitive);
    let wrapped = tsgo_bundled::wrap_fs(inner);
    let recorder = Arc::new(OutputRecorderFs::new(wrapped));
    let fs: Arc<dyn Fs + Send + Sync> = recorder.clone();

    let host: Arc<dyn CompilerHost> = Arc::new(new_compiler_host(
        current_directory.to_string(),
        fs,
        tsgo_bundled::lib_path(),
    ));

    let compare_paths_options = ComparePathsOptions {
        use_case_sensitive_file_names: use_case_sensitive,
        current_directory: current_directory.to_string(),
    };

    let config = if let Some(mut ts_config) = ts_config {
        if let Some(co) = ts_config.parsed_config.compiler_options.as_mut() {
            **co = compiler_options.clone();
        }
        ts_config.parsed_config.file_names = program_file_names;
        ts_config.compare_paths_options = compare_paths_options;
        ts_config
    } else {
        new_parsed_command_line(
            compiler_options.clone(),
            program_file_names,
            compare_paths_options,
        )
    };

    let locale = default_locale();
    let mut diagnostics =
        collect_config_and_early_program_diagnostics(&config, host.as_ref(), &locale);

    let mut program = new_program(ProgramOptions {
        host: host.clone(),
        config: Arc::new(config),
        single_threaded: true,
    });

    // Option-consistency diagnostics are global (no file).
    for od in program.options_diagnostics() {
        diagnostics.push(HarnessDiagnostic::from_message(
            od.message, &od.args, None, 0, 0, &locale,
        ));
    }

    // Per-file syntactic diagnostics, and the user source files (non-library)
    // used to attribute semantic diagnostics.
    let mut user_files: Vec<Rc<HarnessFile>> = Vec::new();
    for parsed in program.source_files() {
        if program.is_default_library_file(parsed) {
            continue;
        }
        let file = Rc::new(HarnessFile::new(
            parsed.file_name().to_string(),
            parsed.text().to_string(),
        ));
        for diag in parsed.diagnostics() {
            diagnostics.push(HarnessDiagnostic::from_message(
                diag.message,
                &diag.args,
                Some(Rc::clone(&file)),
                diag.loc.pos(),
                diag.loc.len(),
                &locale,
            ));
        }
        user_files.push(file);
    }

    // Semantic diagnostics, attributed to the file that actually owns each one.
    // A checker diagnostic's `start`/`length` are byte offsets into its own
    // file's text, so a multi-file program must render each diagnostic against
    // its declaring file (blanket-attributing every diagnostic to the first user
    // file slices out of bounds when a later file's diagnostic has a byte offset
    // past the first file's length).
    let semantic_by_file = program.semantic_diagnostics_by_file();
    for (file_name, diags) in &semantic_by_file {
        let file = user_files
            .iter()
            .find(|f| f.file_name() == file_name)
            .cloned();
        for diag in diags {
            diagnostics.push(HarnessDiagnostic::from_checker(diag, file.clone()));
        }
    }

    let emit_result = program.emit(EmitOptions::default());
    let outputs = recorder.outputs();

    CompilationResult {
        diagnostics,
        options: program.options().clone(),
        harness_options,
        outputs,
        emit_skipped: emit_result.emit_skipped,
    }
}

/// Compiles `input_files` after applying the harness defaults (CRLF newline,
/// `skipDefaultLibCheck`, `noErrorTruncation`) and the `test_config` settings.
///
/// Mirrors Go's `CompileFiles`, which sets test defaults then delegates to
/// `CompileFilesEx`.
///
/// # Examples
/// ```
/// use tsgo_testutil_harnessutil::{compile_files, TestConfiguration, TestFile};
///
/// let result = compile_files(
///     vec![TestFile { unit_name: "/.src/a.ts".into(), content: "var x: number = 1;".into() }],
///     vec![],
///     &TestConfiguration::new(),
///     "/.src",
///     None,
/// );
/// assert!(result.diagnostics().is_empty());
/// ```
///
/// Side effects: as [`compile_files_ex`].
// Go: internal/testutil/harnessutil/harnessutil.go:CompileFiles
pub fn compile_files(
    input_files: Vec<TestFile>,
    other_files: Vec<TestFile>,
    test_config: &TestConfiguration,
    current_directory: &str,
    ts_config: Option<ParsedCommandLine>,
) -> CompilationResult {
    let mut compiler_options = ts_config
        .as_ref()
        .map(|c| c.compiler_options().clone())
        .unwrap_or_default();
    if compiler_options.new_line == NewLineKind::None {
        compiler_options.new_line = NewLineKind::Crlf;
    }
    if compiler_options.skip_default_lib_check.is_unknown() {
        compiler_options.skip_default_lib_check = Tristate::True;
    }
    compiler_options.no_error_truncation = Tristate::True;

    let mut harness_options = HarnessOptions {
        use_case_sensitive_file_names: true,
        current_directory: current_directory.to_string(),
        ..Default::default()
    };

    set_options_from_test_config(
        test_config,
        &mut compiler_options,
        &mut harness_options,
        current_directory,
    );

    compile_files_ex(
        input_files,
        other_files,
        harness_options,
        compiler_options,
        current_directory,
        ts_config,
    )
}

/// Applies the `test_config` directives to `compiler_options` / `harness_options`.
///
/// Recognized compiler options (boolean / number / string / enum) are parsed
/// through the `tsoptions` declaration table; a handful of harness options are
/// handled directly. Unknown or list/object options are skipped.
///
/// DIVERGENCE(port): Go fails the test on an unknown or malformed option; this
/// reachable subset is lenient (skips them) so an inline case with directives
/// the partial option table does not yet model still compiles. List/object
/// options and file-path normalization are deferred.
///
/// Side effects: mutates `compiler_options` and `harness_options`.
// Go: internal/testutil/harnessutil/harnessutil.go:SetOptionsFromTestConfig
pub fn set_options_from_test_config(
    test_config: &TestConfiguration,
    compiler_options: &mut CompilerOptions,
    harness_options: &mut HarnessOptions,
    _current_directory: &str,
) {
    for (name, value) in test_config {
        if name == "typescriptversion" {
            continue;
        }
        if let Some(option) = COMMAND_LINE_COMPILER_OPTIONS_MAP.get(name) {
            if let Some(parsed) = option_value_for(option, value) {
                let _ = parse_compiler_options(option.name, &parsed, compiler_options);
            }
            continue;
        }
        set_harness_option(name, value, harness_options);
    }
}

/// Builds an [`OptionValue`] from a raw string for `option`, or `None` when the
/// value cannot be represented (deferred kinds / malformed input).
///
/// DEFER(P10): list / object options and the file-path normalization Go applies
/// for `is_file_path` options.
/// blocked-by: `tsoptions::parse_list_type_option` wiring; the reachable round
/// handles boolean / number / string / enum scalars.
fn option_value_for(
    option: &tsgo_tsoptions::CommandLineOption,
    value: &str,
) -> Option<OptionValue> {
    match option.kind {
        CommandLineOptionKind::Boolean => match value.to_ascii_lowercase().as_str() {
            "true" => Some(OptionValue::Bool(true)),
            "false" => Some(OptionValue::Bool(false)),
            _ => None,
        },
        CommandLineOptionKind::Number => value.parse::<i32>().ok().map(OptionValue::Int),
        CommandLineOptionKind::String => Some(OptionValue::String(value.to_string())),
        CommandLineOptionKind::Enum => {
            let enum_map = option.enum_map()?;
            let key = value.to_lowercase();
            match enum_map.get(&key.as_str())? {
                EnumValue::Int(v) => Some(OptionValue::Int(*v)),
                EnumValue::Str(s) => Some(OptionValue::String(s.to_string())),
            }
        }
        _ => None,
    }
}

/// Sets a recognized harness option from its raw string value (case-insensitive
/// name); unrecognized harness options are skipped.
fn set_harness_option(name: &str, value: &str, harness_options: &mut HarnessOptions) {
    let parse_bool = |v: &str| matches!(v.to_ascii_lowercase().as_str(), "true");
    match name.to_ascii_lowercase().as_str() {
        "usecasesensitivefilenames" => {
            harness_options.use_case_sensitive_file_names = parse_bool(value);
        }
        "currentdirectory" => harness_options.current_directory = value.to_string(),
        "notypesandsymbols" => harness_options.no_types_and_symbols = parse_bool(value),
        "capturesuggestions" => harness_options.capture_suggestions = parse_bool(value),
        _ => {}
    }
}

/// Returns the base name lowercased when `filename` is a `tsconfig.json` /
/// `jsconfig.json`, else the empty string.
///
/// # Examples
/// ```
/// use tsgo_testutil_harnessutil::get_config_name_from_file_name;
/// assert_eq!(get_config_name_from_file_name("/x/tsconfig.json"), "tsconfig.json");
/// assert_eq!(get_config_name_from_file_name("/x/a.ts"), "");
/// ```
///
/// Side effects: none (pure).
// Go: internal/testutil/harnessutil/harnessutil.go:GetConfigNameFromFileName
pub fn get_config_name_from_file_name(filename: &str) -> String {
    let basename_lower = get_base_file_name(filename).to_ascii_lowercase();
    if basename_lower == "tsconfig.json" || basename_lower == "jsconfig.json" {
        basename_lower
    } else {
        String::new()
    }
}

/// The harness UI locale (always `en`).
///
/// Side effects: none (pure).
fn default_locale() -> Locale {
    tsgo_locale::parse("en").expect("en locale is always available")
}

#[cfg(test)]
#[path = "harnessutil_test.rs"]
mod tests;
