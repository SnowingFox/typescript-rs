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
    combine_paths, get_normalized_absolute_path, normalize_path, to_path, ComparePathsOptions, Path,
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
    /// The canonical path of the program's default library file (the automatic
    /// target lib, or the first `--lib` entry), if one was included.
    default_lib_path: Option<Path>,
    /// Per-import module resolutions as `(containing file name, specifier text,
    /// resolved file name)`, captured while loading the file graph. The basis of
    /// the checker's specifier → module-symbol bridge (see
    /// [`crate::MultiFileBoundProgram`]).
    // Go: internal/compiler/program.go:Program.resolvedModules
    resolved_modules: Vec<(String, String, String)>,
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

    /// The program's default library file, if one was included and read (it is
    /// not present when `--noLib` is set or the host could not read the lib).
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/fileloader.go:processedFiles (default lib lookup)
    pub fn default_lib_file(&self) -> Option<&ParsedFile> {
        self.default_lib_path
            .as_ref()
            .and_then(|path| self.file_by_path(path))
    }

    /// Records the canonical path of the program's default library file.
    ///
    /// Side effects: none.
    pub(crate) fn set_default_lib_path(&mut self, path: Option<Path>) {
        self.default_lib_path = path;
    }

    pub(crate) fn from_parts(
        files: Vec<ParsedFile>,
        files_by_path: HashMap<Path, usize>,
        missing_files: Vec<String>,
        resolved_modules: Vec<(String, String, String)>,
    ) -> ProcessedFiles {
        ProcessedFiles {
            files,
            files_by_path,
            missing_files,
            default_lib_path: None,
            resolved_modules,
        }
    }

    /// The per-import module resolutions `(containing file name, specifier text,
    /// resolved file name)` captured while loading the file graph — the basis of
    /// the checker's specifier → module-symbol bridge.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:Program.resolvedModules
    pub(crate) fn resolved_modules(&self) -> &[(String, String, String)] {
        &self.resolved_modules
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
    /// The normalized absolute directory holding the bundled `lib.*.d.ts` files,
    /// used to resolve `/// <reference lib>` directives and to rank lib files by
    /// priority ([`get_default_lib_file_priority`]).
    // Go: internal/compiler/fileloader.go:fileLoader.defaultLibraryPath
    default_library_path: String,
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
    /// Resolves `file`'s module specifiers, returning each `(specifier text,
    /// normalized resolved file name)` pair (dropping specifiers that do not
    /// resolve).
    ///
    /// This is the data behind the checker's specifier → module-symbol bridge
    /// (`BoundProgram::resolve_module_symbol`): the program records, per
    /// importing file, which loaded file each `import`/`export` specifier
    /// resolved to, so the checker can map an import's specifier back to the
    /// target module's symbol (and its `exports`) without re-running resolution.
    /// The resolved-name projection of this also seeds the load worklist (see
    /// [`crate::FilesParser`]).
    ///
    /// Side effects: reads the file system; populates the resolver caches.
    // Go: internal/compiler/fileloader.go:fileLoader.resolveImportsAndModuleAugmentations
    //     (per-import resolutions stored on the program for resolveExternalModule)
    pub(crate) fn resolve_import_specifiers(&self, file: &ParsedFile) -> Vec<(String, String)> {
        let containing_file = file.file_name();
        let mode = get_default_resolution_mode_for_file(&self.options);
        let mut resolved = Vec::new();
        for specifier in file.import_specifiers() {
            let (result, _trace) =
                self.resolver
                    .resolve_module_name(&specifier, containing_file, mode, None);
            if result.is_resolved() {
                resolved.push((specifier, normalize_path(&result.resolved_file_name)));
            }
        }
        resolved
    }

    /// Resolves a lib file's `/// <reference lib="X" />` directives into the
    /// normalized lib file names they pull in (e.g. the `lib.d.ts` aggregator
    /// references `es5`/`dom`/... -> `lib.es5.d.ts`/`lib.dom.d.ts`/...).
    ///
    /// Each directive's lib name is mapped to its declaration-file name with
    /// [`tsgo_tsoptions::get_lib_file_name`] and rooted at the default-library
    /// directory. Unknown lib names are dropped (Go emits an "unknown reference"
    /// diagnostic for them; that diagnostic is deferred).
    ///
    /// # Divergence
    /// Go reads `file.LibReferenceDirectives` from the parser's pragma table.
    /// The Rust parser does not expose lib reference directives yet (pragma
    /// scanning is deferred in `tsgo_parser`, which this crate must not edit), so
    /// this scans the lib file's leading comment trivia for the directives
    /// instead — the reachable substitute for the parser's pragma table.
    /// blocked-by: parser lib-reference directives (pragma scanning in
    /// `tsgo_parser`).
    ///
    /// DEFER(P6): an "unknown reference" diagnostic for an unrecognized lib name.
    /// blocked-by: the program's processing-diagnostic surface.
    ///
    /// Side effects: none (pure; reads the already-parsed text).
    // Go: internal/compiler/filesparser.go:load (file.LibReferenceDirectives -> pathForLibFile)
    pub(crate) fn resolve_lib_references(&self, file: &ParsedFile) -> Vec<String> {
        let mut resolved = Vec::new();
        for lib in extract_lib_reference_names(file.text()) {
            if let Some(name) = tsgo_tsoptions::get_lib_file_name(&lib) {
                resolved.push(combine_paths(&self.default_library_path, &[&name]));
            }
            // DEFER(P6): error on an unknown lib name.
        }
        resolved
    }
}

/// Ranks a default-library file for the deterministic lib ordering (Go's
/// `sortLibs`): the base `lib.d.ts`/`lib.es6.d.ts` rank first (0), then each lib
/// ranks by its short name's position in [`tsgo_tsoptions::LIBS`] (+1); a file
/// outside the lib directory or with an unknown name ranks last.
///
/// # Examples
/// ```
/// use tsgo_compiler::get_default_lib_file_priority;
/// // `es5` is the first entry in `LIBS`, so it ranks 1 (after the base 0).
/// assert_eq!(get_default_lib_file_priority("/lib/lib.es5.d.ts", "/lib"), 1);
/// assert_eq!(get_default_lib_file_priority("/lib/lib.d.ts", "/lib"), 0);
/// ```
///
/// Side effects: none (pure).
// Go: internal/compiler/fileloader.go:fileLoader.getDefaultLibFilePriority
pub fn get_default_lib_file_priority(file_name: &str, default_library_path: &str) -> i32 {
    let libs = &*tsgo_tsoptions::LIBS;
    let lib_dir = default_library_path
        .strip_suffix('/')
        .unwrap_or(default_library_path);
    let in_lib_dir = file_name.len() > lib_dir.len()
        && file_name.starts_with(lib_dir)
        && file_name.as_bytes()[lib_dir.len()] == b'/';
    if in_lib_dir {
        let basename = match file_name.rfind('/') {
            Some(i) => &file_name[i + 1..],
            None => file_name,
        };
        if basename == "lib.d.ts" || basename == "lib.es6.d.ts" {
            return 0;
        }
        if let Some(name) = basename
            .strip_prefix("lib.")
            .and_then(|s| s.strip_suffix(".d.ts"))
        {
            if let Some(index) = libs.iter().position(|&l| l == name) {
                return index as i32 + 1;
            }
        }
    }
    libs.len() as i32 + 2
}

/// Scans `text`'s leading comment trivia for `/// <reference lib="X" />`
/// directives, returning the `X` names in source order.
///
/// Only the file's leading trivia (whitespace, `//` line comments, `/* */`
/// block comments) is scanned, stopping at the first real token — matching
/// where TypeScript recognizes triple-slash directives. This is the reachable
/// substitute for the parser's pragma table (see
/// [`FileLoader::resolve_lib_references`]).
///
/// Side effects: none (pure).
// Go: internal/parser/parser.go (lib reference directive pragma scanning)
fn extract_lib_reference_names(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    let mut names = Vec::new();
    loop {
        while i < n && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i + 1 < n && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            let start = i;
            while i < n && bytes[i] != b'\n' {
                i += 1;
            }
            if let Some(name) = parse_reference_lib_directive(&text[start..i]) {
                names.push(name.to_string());
            }
        } else if i + 1 < n && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < n && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else {
            // First non-trivia token: directives only precede real statements.
            break;
        }
    }
    names
}

/// Parses one comment line as a `/// <reference lib="X" />` directive, returning
/// `X` if it matches (and only for the `lib` attribute, not `path`/`types`).
///
/// Side effects: none (pure).
// Go: internal/parser/parser.go (triple-slash reference pragma)
fn parse_reference_lib_directive(comment: &str) -> Option<&str> {
    let rest = comment.trim_start().strip_prefix("///")?.trim_start();
    let rest = rest.strip_prefix("<reference")?.trim_start();
    // The attribute name must be exactly `lib` (so `path=`/`types=`/
    // `no-default-lib=` do not match).
    let rest = rest.strip_prefix("lib")?.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(&rest[..end])
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

    let default_library_path =
        get_normalized_absolute_path(opts.host.default_library_path(), &current_directory);

    let loader = FileLoader {
        host: opts.host.clone(),
        compare_paths_options: ComparePathsOptions {
            use_case_sensitive_file_names,
            current_directory: current_directory.clone(),
        },
        resolver,
        options: compiler_options,
        default_library_path: default_library_path.clone(),
    };

    let mut files_parser = FilesParser::new();
    for root in opts.config.file_names() {
        let absolute = get_normalized_absolute_path(root, &current_directory);
        // DEFER(P6): automatic-type-directive/project-reference root tasks.
        // blocked-by: ATA + projectreferenceparser (P6-2+).
        files_parser.add_root_task(&loader, absolute);
    }

    // Go: NewProgram includes the default library file when there are root
    // files, `--noLib` is off, and no explicit `--lib` list was given. The lib
    // file name is resolved from the emit target (e.g. ES5 -> `lib.es5.d.ts`)
    // and its path is rooted at the host's default-library directory (the
    // `bundled:///libs` embed when the host serves it).
    // Track the program's default-lib path (the automatic target lib, or the
    // first `--lib` entry) so the program can surface its globals.
    let mut default_lib_path: Option<Path> = None;
    let options_ref = opts.config.compiler_options();
    if !opts.config.file_names().is_empty() && options_ref.no_lib.is_false_or_unknown() {
        if options_ref.lib.is_empty() {
            // No explicit `--lib`: include the target's default lib file. When
            // it is a reference-only aggregator (`lib.d.ts`/`lib.es*.full.d.ts`),
            // its `/// <reference lib>` directives are expanded transitively by
            // the worklist (see `FilesParser::parse`).
            let name = tsgo_tsoptions::get_default_lib_file_name(options_ref);
            let lib_file_name = combine_paths(&default_library_path, &[&name]);
            default_lib_path = Some(loader.to_path(&lib_file_name));
            files_parser.add_lib_root_task(&loader, lib_file_name);
        } else {
            // Explicit `--lib` list: include each named lib (short name resolved
            // to its file name, e.g. `es5` -> `lib.es5.d.ts`).
            for lib in &options_ref.lib {
                if let Some(name) = tsgo_tsoptions::get_lib_file_name(lib) {
                    let lib_file_name = combine_paths(&default_library_path, &[&name]);
                    if default_lib_path.is_none() {
                        default_lib_path = Some(loader.to_path(&lib_file_name));
                    }
                    files_parser.add_lib_root_task(&loader, lib_file_name);
                }
                // DEFER(P6): report an error on an unknown lib name.
                // blocked-by: program option-syntax diagnostics surface.
            }
        }
    }

    files_parser.parse(&loader);
    let mut processed = files_parser.collect_files(&default_library_path);
    processed.set_default_lib_path(default_lib_path);
    processed
}

#[cfg(test)]
#[path = "fileloader_test.rs"]
mod tests;
