//! Port of Go `internal/compiler/fileloader.go`: resolving and loading the
//! program's file graph.
//!
//! This round ports the reachable foundation: building a [`FileLoader`] from the
//! [`ProgramOptions`], turning the root file names into parsed files, and
//! resolving relative `import`/`export` module specifiers (via the
//! [`tsgo_module`] resolver) into additional files. The result is collected into
//! a deterministic [`ProcessedFiles`].
//!
//! Deferred (`// DEFER(P6)`): lib-file resolution, triple-slash references,
//! type-reference directives, automatic-type-directive tasks, project
//! references, and package deduplication.

use std::collections::HashMap;
use std::sync::Arc;

use tsgo_core::compileroptions::{
    CompilerOptions, ModuleResolutionKind, ResolutionMode, RESOLUTION_MODE_NONE,
};
use tsgo_module::{ResolutionHost, Resolver};
use tsgo_parser::SourceFileParseOptions;
use tsgo_tspath::{
    get_normalized_absolute_path, normalize_path, to_path, ComparePathsOptions, Path,
};
use tsgo_vfs::Fs;

use crate::filesparser::FilesParser;
use crate::host::{CompilerHost, ParsedFile};
use crate::program::ProgramOptions;

/// Bridges a [`CompilerHost`]'s file system + current directory to the
/// [`ResolutionHost`] the module resolver needs.
///
/// Side effects: exposes the shared file system to the resolver.
// Go: internal/compiler/host.go:compilerHost (as module.ResolutionHost)
struct LoaderResolutionHost {
    fs: Arc<dyn Fs + Send + Sync>,
    current_directory: String,
}

impl ResolutionHost for LoaderResolutionHost {
    fn fs(&self) -> &dyn Fs {
        &*self.fs
    }

    fn get_current_directory(&self) -> &str {
        &self.current_directory
    }
}

/// The program-shared products of loading the file graph.
///
/// This is the reachable subset of Go's `processedFiles`: the ordered file
/// list, the path index into it, and the names of root/referenced files that
/// could not be read.
///
/// Side effects: none (owns parse output).
// Go: internal/compiler/fileloader.go:processedFiles
#[derive(Debug, Default)]
pub struct ProcessedFiles {
    files: Vec<ParsedFile>,
    files_by_path: HashMap<Path, usize>,
    missing_files: Vec<String>,
}

impl ProcessedFiles {
    /// The loaded source files, in deterministic (depth-first) include order.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/fileloader.go:processedFiles.files
    pub fn files(&self) -> &[ParsedFile] {
        &self.files
    }

    /// The loaded source files, mutably (used to bind them in place).
    ///
    /// Side effects: none itself; callers may mutate the files.
    // Go: internal/compiler/fileloader.go:processedFiles.files
    pub fn files_mut(&mut self) -> &mut [ParsedFile] {
        &mut self.files
    }

    /// Looks up a loaded file by its canonical [`Path`].
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/fileloader.go:processedFiles.filesByPath
    pub fn file_by_path(&self, path: &Path) -> Option<&ParsedFile> {
        self.files_by_path.get(path).map(|&i| &self.files[i])
    }

    /// The normalized names of root/referenced files that could not be read.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/fileloader.go:processedFiles.missingFiles
    pub fn missing_files(&self) -> &[String] {
        &self.missing_files
    }

    pub(crate) fn from_parts(
        files: Vec<ParsedFile>,
        files_by_path: HashMap<Path, usize>,
        missing_files: Vec<String>,
    ) -> ProcessedFiles {
        ProcessedFiles {
            files,
            files_by_path,
            missing_files,
        }
    }
}

/// Loads and resolves the program's files: it owns the host, compiler options,
/// path-comparison settings, and the module resolver.
///
/// This is the reachable subset of Go's `fileLoader`.
///
/// Side effects: methods read the file system via the host.
// Go: internal/compiler/fileloader.go:fileLoader
pub struct FileLoader {
    host: Arc<dyn CompilerHost>,
    compare_paths_options: ComparePathsOptions,
    resolver: Resolver,
    options: Arc<CompilerOptions>,
}

impl FileLoader {
    /// The canonical [`Path`] for `file_name`, rooted at the host's current
    /// directory and folded per the file system's case sensitivity.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/fileloader.go:fileLoader.toPath
    pub fn to_path(&self, file_name: &str) -> Path {
        to_path(
            file_name,
            &self.compare_paths_options.current_directory,
            self.compare_paths_options.use_case_sensitive_file_names,
        )
    }

    /// Reads and parses the file named `normalized_file_path`, or returns `None`
    /// if the host cannot read it.
    ///
    /// Side effects: reads the file system through the host.
    // Go: internal/compiler/fileloader.go:fileLoader.parseSourceFile
    pub(crate) fn parse_source_file(&self, normalized_file_path: &str) -> Option<ParsedFile> {
        self.host.get_source_file(&SourceFileParseOptions {
            file_name: normalized_file_path.to_string(),
        })
    }

    /// Resolves `file`'s module specifiers to the normalized names of the files
    /// they reference, dropping specifiers that do not resolve.
    ///
    /// The per-import resolution mode is approximated by the file-level default
    /// ([`get_default_resolution_mode_for_file`]); the precise per-usage mode
    /// (`getModeForUsageLocation`: type-only `resolution-mode` attribute
    /// overrides and `import()`-call syntax) is deferred.
    ///
    /// DEFER(P6): `get_mode_for_usage_location` (per-import mode) needs the
    /// `usage` node's parent + `SourceFileMetaData`.
    /// blocked-by: ast `SourceFileMetaData` + `GetImpliedNodeFormatForEmitWorker`
    /// / `GetEmitModuleFormatOfFileWorker` (not ported in `tsgo_ast`).
    ///
    /// Side effects: reads the file system; populates the resolver caches.
    // Go: internal/compiler/fileloader.go:fileLoader.resolveImportsAndModuleAugmentations
    pub(crate) fn resolve_import_file_names(&self, file: &ParsedFile) -> Vec<String> {
        let containing_file = file.file_name();
        let mode = get_default_resolution_mode_for_file(&self.options);
        let mut resolved_names = Vec::new();
        for specifier in file.import_specifiers() {
            let (resolved, _trace) =
                self.resolver
                    .resolve_module_name(&specifier, containing_file, mode, None);
            if resolved.is_resolved() {
                resolved_names.push(normalize_path(&resolved.resolved_file_name));
            }
        }
        resolved_names
    }
}

/// Reports whether the way an import is written (ESM vs CJS syntax) affects how
/// its module specifier resolves.
///
/// True under node16/nodenext resolution, or when `package.json`
/// `exports`/`imports` are consulted (the default).
///
/// # Examples
/// ```
/// use tsgo_compiler::import_syntax_affects_module_resolution;
/// use tsgo_core::compileroptions::CompilerOptions;
/// // package.json exports are consulted by default.
/// assert!(import_syntax_affects_module_resolution(&CompilerOptions::default()));
/// ```
///
/// Side effects: none (pure).
// Go: internal/compiler/fileloader.go:importSyntaxAffectsModuleResolution
pub fn import_syntax_affects_module_resolution(options: &CompilerOptions) -> bool {
    let module_resolution = options.get_module_resolution_kind() as i32;
    (ModuleResolutionKind::Node16 as i32 <= module_resolution
        && module_resolution <= ModuleResolutionKind::NodeNext as i32)
        || options.get_resolve_package_json_exports()
        || options.get_resolve_package_json_imports()
}

/// The default module-resolution mode for a file.
///
/// When import syntax does not affect resolution the mode is `None`. When it
/// does, Go derives the file's implied node format from its extension and
/// nearest `package.json` `type`; that derivation is not yet portable, so this
/// also returns `None` for now (see the `blocked-by` note).
///
/// DEFER(P6): the import-syntax-affects branch should return
/// `ast::GetImpliedNodeFormatForEmitWorker(file, emit_module_kind, meta)`.
/// blocked-by: ast `SourceFileMetaData` + `GetImpliedNodeFormatForEmitWorker`
/// (not ported in `tsgo_ast`).
///
/// Side effects: none (pure).
// Go: internal/compiler/fileloader.go:getDefaultResolutionModeForFile
fn get_default_resolution_mode_for_file(options: &CompilerOptions) -> ResolutionMode {
    // When `_affects` is true Go derives the file's implied node format; that
    // derivation is blocked-by ast (see the doc note), so both cases currently
    // resolve in `None` mode.
    let _affects = import_syntax_affects_module_resolution(options);
    // DEFER(P6): when `_affects`, return GetImpliedNodeFormatForEmitWorker(...).
    // blocked-by: ast SourceFileMetaData + GetImpliedNodeFormatForEmitWorker.
    RESOLUTION_MODE_NONE
}

/// Loads every file reachable from the root file names in `opts`, returning the
/// deterministic [`ProcessedFiles`].
///
/// Side effects: reads the file system through `opts.host`.
// Go: internal/compiler/fileloader.go:processAllProgramFiles
pub fn process_all_program_files(opts: &ProgramOptions, single_threaded: bool) -> ProcessedFiles {
    // PERF(port): Go runs file discovery on `core.WorkGroup` for parallelism; the
    // deterministic order is produced by the serial `collect_files` post-pass, so
    // a sequential discovery yields the same result. Parallelizing is a later
    // round (`single_threaded` is accepted but currently always sequential).
    let _ = single_threaded;
    let current_directory = opts.host.get_current_directory().to_string();
    let use_case_sensitive_file_names = opts.host.fs().use_case_sensitive_file_names();
    let compiler_options = Arc::new(opts.config.compiler_options().clone());

    let resolution_host = Arc::new(LoaderResolutionHost {
        fs: opts.host.fs(),
        current_directory: current_directory.clone(),
    });
    let resolver = Resolver::new(resolution_host, compiler_options.clone(), "", "");

    let loader = FileLoader {
        host: opts.host.clone(),
        compare_paths_options: ComparePathsOptions {
            use_case_sensitive_file_names,
            current_directory: current_directory.clone(),
        },
        resolver,
        options: compiler_options,
    };

    let mut files_parser = FilesParser::new();
    for root in opts.config.file_names() {
        let absolute = get_normalized_absolute_path(root, &current_directory);
        // DEFER(P6): lib/automatic-type-directive/project-reference root tasks.
        // blocked-by: lib resolution + projectreferenceparser (P6-2+).
        files_parser.add_root_task(&loader, absolute);
    }
    files_parser.parse(&loader);
    files_parser.collect_files()
}

#[cfg(test)]
#[path = "fileloader_test.rs"]
mod tests;
