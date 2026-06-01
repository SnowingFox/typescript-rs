//! Port of the reachable subset of Go's project-reference machinery:
//! resolving a tsconfig's `references[]` into parsed referenced projects,
//! computing the topological build order (with cycle detection), and the
//! `composite`/`outDir`/`rootDir` resolution that project references rely on.
//!
//! This stitches together several Go sources, all anchored individually:
//! - `internal/core/projectreference.go` (`ResolveProjectReferencePath`)
//! - `internal/compiler/host.go` (`compilerHost.GetResolvedProjectReference`)
//! - `internal/compiler/projectreferenceparser.go` (the resolved reference
//!   graph + `initMapperWorker`)
//! - `internal/compiler/projectreferencefilemapper.go`
//!   (`rangeResolvedProjectReference`, `getResolvedProjectReferences`)
//! - `internal/compiler/program.go` (`verifyProjectReferences`)
//! - `internal/execute/build/orchestrator.go` (`setupBuildTask`: the build
//!   order + `TS6202` cycle detection)
//! - `internal/tsoptions/parsedcommandline.go`
//!   (`ResolvedProjectReferencePaths`, `getOutputDeclarationAndSourceFileNames`)
//!
//! # Reachable subset / divergence
//!
//! The full `--build` orchestration (up-to-date checking, `.tsbuildinfo`,
//! incremental affected-files, watch) is P9; this round ports only the
//! resolution + ordering + output-path machinery that P9 will drive. Diagnostics
//! are produced as compiler diagnostics (not anchored at the offending tsconfig
//! `reference` node) — the message text/code/args match Go; only the location
//! differs. See the per-item `DEFER`/`// blocked-by` notes.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use tsgo_core::projectreference::resolve_project_reference_path;
use tsgo_diagnostics::{localize, Message};
use tsgo_tsoptions::{
    new_tsconfig_source_file_from_file_path, parse_json_source_file_config_file_content,
    ParseConfigHost, ParsedCommandLine,
};
use tsgo_tspath::{get_directory_path, get_normalized_absolute_path, to_path, Path};
use tsgo_vfs::Fs;

use crate::host::CompilerHost;

/// A project-reference diagnostic: the message plus its (already stringified)
/// arguments. Like [`OptionsDiagnostic`](crate::verify_options::OptionsDiagnostic),
/// the source location is deferred — Go anchors these at the offending
/// `reference` node in the tsconfig AST.
///
/// DEFER(P9): anchor the diagnostic at the tsconfig `reference` syntax node
/// (Go's `CreateDiagnosticAtReferenceSyntax`).
/// blocked-by: tsconfig option-syntax AST diagnostics surface.
///
/// Side effects: none (pure value type).
// Go: internal/ast/diagnostic.go:Diagnostic (project-reference subset)
#[derive(Debug, Clone)]
pub struct ProjectReferenceDiagnostic {
    /// The diagnostic message.
    pub message: &'static Message,
    /// The stringified message arguments (e.g. the reference path).
    pub args: Vec<String>,
}

impl ProjectReferenceDiagnostic {
    fn new(message: &'static Message, args: Vec<String>) -> ProjectReferenceDiagnostic {
        ProjectReferenceDiagnostic { message, args }
    }

    /// The numeric diagnostic code (e.g. `6202` for the circular-graph error).
    ///
    /// Side effects: none (pure).
    pub fn code(&self) -> i32 {
        self.message.code()
    }

    /// The fully formatted (English) diagnostic text, with the arguments
    /// substituted into the message placeholders.
    ///
    /// Side effects: may lazily load a locale table on first use (none here, as
    /// English uses the built-in text).
    pub fn text(&self) -> String {
        let locale = tsgo_locale::parse("en").expect("en locale");
        let args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        localize(&locale, Some(self.message), "", &args)
    }
}

/// The topological build order of a project-reference graph plus any
/// circular-graph (`TS6202`) diagnostics found while computing it.
///
/// Side effects: none (plain data).
// Go: internal/execute/build/orchestrator.go:Orchestrator (order + errors)
#[derive(Debug, Default)]
pub struct BuildOrder {
    /// The config file names in dependency order (deepest reference first).
    pub order: Vec<String>,
    /// The `TS6202` "circular graph" diagnostics, one per detected cycle edge.
    pub circular_diagnostics: Vec<ProjectReferenceDiagnostic>,
}

/// Bridges a [`CompilerHost`]'s file system + current directory to the
/// [`ParseConfigHost`] the tsconfig parser needs (so a referenced config can be
/// read and parsed).
///
/// Side effects: exposes the shared file system to the parser.
// Go: internal/compiler/host.go:compilerHost (as tsoptions.ParseConfigHost)
struct CompilerParseConfigHost {
    fs: Arc<dyn Fs + Send + Sync>,
    current_directory: String,
}

impl ParseConfigHost for CompilerParseConfigHost {
    fn fs(&self) -> &dyn Fs {
        &*self.fs
    }

    fn get_current_directory(&self) -> &str {
        &self.current_directory
    }
}

/// Resolves a referenced project's config file into a [`ParsedCommandLine`], or
/// returns `None` if the config file cannot be read.
///
/// Mirrors Go's `compilerHost.GetResolvedProjectReference`, which delegates to
/// `tsoptions.GetParsedCommandLineOfConfigFilePath`: the name is rooted at the
/// host's current directory, the config text is read through the host file
/// system, and it is parsed (with the config file's directory as the base path).
///
/// Side effects: reads the config file through the host file system.
// Go: internal/compiler/host.go:compilerHost.GetResolvedProjectReference
pub fn get_resolved_project_reference(
    host: &dyn CompilerHost,
    config_file_name: &str,
) -> Option<ParsedCommandLine> {
    let current_directory = host.get_current_directory().to_string();
    let config_file_name = get_normalized_absolute_path(config_file_name, &current_directory);
    let text = host.fs().read_file(&config_file_name)?;
    let source_file = new_tsconfig_source_file_from_file_path(&config_file_name, &text);
    let parse_host = CompilerParseConfigHost {
        fs: host.fs(),
        current_directory,
    };
    let base_path = get_directory_path(&config_file_name);
    Some(parse_json_source_file_config_file_content(
        &source_file,
        &parse_host,
        &base_path,
        None,
        &config_file_name,
    ))
}

/// One project in the resolved reference graph: its config file name, the
/// parsed config (`None` when the config file could not be read), and the
/// canonical paths of the projects it references (in declaration order, 1:1
/// with the config's `references[]`).
///
/// Side effects: none (owns the resolved config).
// Go: internal/compiler/projectreferenceparser.go:projectReferenceParseTask
struct ProjectReferenceNode {
    config_name: String,
    resolved: Option<ParsedCommandLine>,
    reference_paths: Vec<Path>,
}

/// The resolved project-reference graph rooted at a program's config: every
/// reachable referenced project keyed by its canonical config [`Path`], plus
/// the root's path. Cycles are represented as back-edges in
/// [`ProjectReferenceNode::reference_paths`] (each project is resolved once,
/// keyed by path), so the graph never loops.
///
/// This is the reachable subset of Go's `projectReferenceFileMapper`
/// (`configToProjectReference` + `referencesInConfigFile`).
///
/// Side effects: none (owns the resolved configs).
// Go: internal/compiler/projectreferencefilemapper.go:projectReferenceFileMapper
pub struct ResolvedProjectReferences {
    nodes: HashMap<Path, ProjectReferenceNode>,
    root_path: Path,
    current_directory: String,
    use_case_sensitive_file_names: bool,
}

/// Resolves the project-reference graph reachable from `root_config` (whose own
/// config file is named `root_config_name`).
///
/// Each `references[]` entry is resolved to its config file name
/// ([`resolve_project_reference_path`]), read + parsed
/// ([`get_resolved_project_reference`]), and recursed into; every project is
/// resolved once (keyed by canonical path), so reference cycles terminate.
///
/// Mirrors Go's `projectReferenceParser` (the parallel parse + dedup) followed
/// by `initMapperWorker` (building `referencesInConfigFile`), run sequentially
/// here.
///
/// # Examples
/// ```
/// use std::sync::Arc;
/// use tsgo_compiler::{
///     get_resolved_project_reference, new_compiler_host, resolve_project_references,
/// };
/// use tsgo_vfs::vfstest::MapFs;
/// use tsgo_vfs::Fs;
///
/// let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map(
///     [
///         (
///             "/app/tsconfig.json",
///             r#"{ "compilerOptions": { "composite": true }, "files": ["a.ts"], "references": [{ "path": "../lib" }] }"#,
///         ),
///         ("/app/a.ts", "export const a = 1;"),
///         (
///             "/lib/tsconfig.json",
///             r#"{ "compilerOptions": { "composite": true }, "files": ["index.ts"] }"#,
///         ),
///         ("/lib/index.ts", "export const x = 1;"),
///     ],
///     true,
/// ));
/// let host = new_compiler_host("/app", fs, "/bundled/libs");
/// let root = get_resolved_project_reference(&host, "/app/tsconfig.json").unwrap();
/// let graph = resolve_project_references(&host, "/app/tsconfig.json", &root);
/// // The build order lists the deepest reference (lib) before the root (app).
/// assert_eq!(
///     graph.get_build_order().order,
///     vec!["/lib/tsconfig.json".to_string(), "/app/tsconfig.json".to_string()],
/// );
/// ```
///
/// DEFER(P9): parallel resolution (Go runs it on a `core.WorkGroup`); the result
/// is order-independent, so a sequential walk is equivalent.
///
/// Side effects: reads referenced config files through the host file system.
// Go: internal/compiler/projectreferenceparser.go:projectReferenceParser.parse
pub fn resolve_project_references(
    host: &dyn CompilerHost,
    root_config_name: &str,
    root_config: &ParsedCommandLine,
) -> ResolvedProjectReferences {
    let current_directory = host.get_current_directory().to_string();
    let use_case_sensitive_file_names = host.fs().use_case_sensitive_file_names();
    let to_path = |name: &str| to_path(name, &current_directory, use_case_sensitive_file_names);

    let root_path = to_path(root_config_name);
    let mut nodes: HashMap<Path, ProjectReferenceNode> = HashMap::new();
    nodes.insert(
        root_path.clone(),
        ProjectReferenceNode {
            config_name: root_config_name.to_string(),
            resolved: Some(root_config.clone()),
            reference_paths: Vec::new(),
        },
    );

    // Worklist of config paths whose `references[]` still need resolving. A path
    // is enqueued exactly once (guarded by membership in `nodes`), so reference
    // cycles terminate (Go dedups by path in `projectReferenceParser.start`).
    let mut queue: VecDeque<Path> = VecDeque::new();
    queue.push_back(root_path.clone());
    while let Some(path) = queue.pop_front() {
        let references = match nodes.get(&path).and_then(|node| node.resolved.as_ref()) {
            Some(config) => config.parsed_config.project_references.clone(),
            None => Vec::new(),
        };
        let mut reference_paths = Vec::with_capacity(references.len());
        for reference in &references {
            let child_name = resolve_project_reference_path(reference);
            let child_path = to_path(&child_name);
            reference_paths.push(child_path.clone());
            if !nodes.contains_key(&child_path) {
                let resolved = get_resolved_project_reference(host, &child_name);
                nodes.insert(
                    child_path.clone(),
                    ProjectReferenceNode {
                        config_name: child_name,
                        resolved,
                        reference_paths: Vec::new(),
                    },
                );
                queue.push_back(child_path);
            }
        }
        if let Some(node) = nodes.get_mut(&path) {
            node.reference_paths = reference_paths;
        }
    }

    ResolvedProjectReferences {
        nodes,
        root_path,
        current_directory,
        use_case_sensitive_file_names,
    }
}

impl ResolvedProjectReferences {
    /// The root config's directly-resolved references, in declaration order
    /// (an entry is `None` when that referenced config could not be read).
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/projectreferencefilemapper.go:getResolvedProjectReferences
    pub fn get_resolved_project_references(&self) -> Vec<Option<&ParsedCommandLine>> {
        self.references_of(&self.root_path)
    }

    /// The resolved config for the project at `config_path`, if it was reached
    /// and could be read.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/projectreferencefilemapper.go:getResolvedReferenceFor
    pub fn get_resolved_reference_for(&self, config_path: &Path) -> Option<&ParsedCommandLine> {
        self.nodes
            .get(config_path)
            .and_then(|node| node.resolved.as_ref())
    }

    /// Computes the topological build order of the reference graph (deepest
    /// reference first, the root last) and the `TS6202` circular-graph
    /// diagnostics found along the way.
    ///
    /// Mirrors the reachable subset of Go's `Orchestrator.setupBuildTask`: a
    /// depth-first post-order over the references, with `analyzing`/`completed`
    /// sets that both terminate at cycles and detect them.
    ///
    /// DEFER(P9): the full `--build` order (watch downstream tracking,
    /// up-to-date checking). blocked-by: `--build` orchestration.
    ///
    /// Side effects: none (pure).
    // Go: internal/execute/build/orchestrator.go:Orchestrator.setupBuildTask
    pub fn get_build_order(&self) -> BuildOrder {
        let mut state = BuildOrderState::default();
        self.setup_build_task(&self.root_path, false, &mut state);
        BuildOrder {
            order: state.order,
            circular_diagnostics: state.errors,
        }
    }

    /// Verifies the resolved references, returning the diagnostics Go's
    /// `verifyProjectReferences` produces over the reachable subset:
    /// `TS6306` when a referenced project is not `composite`, `TS6310` when it
    /// disables emit (`noEmit`), and `TS6053` (`File '{0}' not found`) when the
    /// reference could not be resolved. The composite/noEmit checks only fire
    /// when the *referencing* project has files of its own.
    ///
    /// Each reference is visited once (deduped by path) across the whole graph,
    /// mirroring `rangeResolvedProjectReference`'s `seenRef` walk.
    ///
    /// DEFER(P9): the `Cannot_write_file_..._tsbuildinfo` overwrite check (needs
    /// the `.tsbuildinfo` file-name machinery). blocked-by: P6-9b tsbuildinfo.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/program.go:verifyProjectReferences
    pub fn verify_project_references(&self) -> Vec<ProjectReferenceDiagnostic> {
        let mut diags = Vec::new();
        let mut seen: HashSet<Path> = HashSet::new();
        // Go seeds `seenRef` with the root config's path before walking.
        seen.insert(self.root_path.clone());
        self.verify_worker(&self.root_path, &mut seen, &mut diags);
        diags
    }

    /// Walks the references of the project at `parent_path`, checking each once
    /// (deduped via `seen`) and recursing depth-first.
    ///
    /// Side effects: appends to `diags`; mutates `seen`.
    // Go: internal/compiler/projectreferencefilemapper.go:rangeResolvedReferenceWorker
    fn verify_worker(
        &self,
        parent_path: &Path,
        seen: &mut HashSet<Path>,
        diags: &mut Vec<ProjectReferenceDiagnostic>,
    ) {
        let Some(parent_node) = self.nodes.get(parent_path) else {
            return;
        };
        let Some(parent_config) = &parent_node.resolved else {
            return;
        };
        let parent_refs = &parent_config.parsed_config.project_references;
        let parent_has_files = !parent_config.file_names().is_empty();
        for (index, ref_path) in parent_node.reference_paths.iter().enumerate() {
            if !seen.insert(ref_path.clone()) {
                continue;
            }
            // `ref.Path`: the normalized absolute reference path (the directory).
            let reference_path = parent_refs
                .get(index)
                .map(|r| r.path.clone())
                .unwrap_or_default();
            match self.nodes.get(ref_path).and_then(|n| n.resolved.as_ref()) {
                None => {
                    diags.push(ProjectReferenceDiagnostic::new(
                        &tsgo_diagnostics::FILE_0_NOT_FOUND,
                        vec![reference_path],
                    ));
                }
                Some(config) => {
                    let ref_options = config.compiler_options();
                    // Go: `if (!composite || noEmit) && len(parent.FileNames()) > 0`.
                    if (!ref_options.composite.is_true() || ref_options.no_emit.is_true())
                        && parent_has_files
                    {
                        if !ref_options.composite.is_true() {
                            diags.push(ProjectReferenceDiagnostic::new(
                                &tsgo_diagnostics::REFERENCED_PROJECT_0_MUST_HAVE_SETTING_COMPOSITE_COLON_TRUE,
                                vec![reference_path.clone()],
                            ));
                        }
                        if ref_options.no_emit.is_true() {
                            diags.push(ProjectReferenceDiagnostic::new(
                                &tsgo_diagnostics::REFERENCED_PROJECT_0_MAY_NOT_DISABLE_EMIT,
                                vec![reference_path],
                            ));
                        }
                    }
                }
            }
            self.verify_worker(ref_path, seen, diags);
        }
    }

    /// Visits `path` for the build order: recurses into its references in
    /// declaration order, then appends `path` (post-order). Detects cycles via
    /// the `analyzing` set.
    ///
    /// Side effects: mutates `state` (order, completed/analyzing sets, errors).
    // Go: internal/execute/build/orchestrator.go:Orchestrator.setupBuildTask
    fn setup_build_task(
        &self,
        path: &Path,
        in_circular_context: bool,
        state: &mut BuildOrderState,
    ) {
        let Some(node) = self.nodes.get(path) else {
            return;
        };
        if state.completed.contains(path) {
            return;
        }
        if state.analyzing.contains(path) {
            // A re-visit of a node still on the DFS stack is a cycle. The
            // message lists the current analyzing chain (the stack), joined by
            // newlines, unless this edge was explicitly marked circular.
            if !in_circular_context {
                state.errors.push(ProjectReferenceDiagnostic::new(
                    &tsgo_diagnostics::PROJECT_REFERENCES_MAY_NOT_FORM_A_CIRCULAR_GRAPH_CYCLE_DETECTED_COLON_0,
                    vec![state.stack.join("\n")],
                ));
            }
            return;
        }
        state.analyzing.insert(path.clone());
        state.stack.push(node.config_name.clone());
        if let Some(config) = &node.resolved {
            let project_refs = &config.parsed_config.project_references;
            for (index, sub_path) in node.reference_paths.iter().enumerate() {
                let circular =
                    in_circular_context || project_refs.get(index).is_some_and(|r| r.circular);
                self.setup_build_task(sub_path, circular, state);
            }
        }
        state.stack.pop();
        // Go keeps `path` in `analyzing` (never removes); `completed` is checked
        // first on re-entry, so a completed node never re-triggers cycle
        // detection.
        state.completed.insert(path.clone());
        state.order.push(node.config_name.clone());
    }

    /// The directly-resolved references of the config at `config_path`.
    ///
    /// Side effects: none (pure).
    fn references_of(&self, config_path: &Path) -> Vec<Option<&ParsedCommandLine>> {
        let Some(node) = self.nodes.get(config_path) else {
            return Vec::new();
        };
        node.reference_paths
            .iter()
            .map(|p| self.nodes.get(p).and_then(|n| n.resolved.as_ref()))
            .collect()
    }

    /// The canonical [`Path`] for `file_name`, rooted at the graph's current
    /// directory (use it to key [`Self::get_resolved_reference_for`]).
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/fileloader.go:fileLoader.toPath
    pub fn to_path(&self, file_name: &str) -> Path {
        to_path(
            file_name,
            &self.current_directory,
            self.use_case_sensitive_file_names,
        )
    }
}

/// Read-only [`OutputPathsHost`](tsgo_outputpaths::OutputPathsHost) backed by a
/// referenced project's parsed config (the reachable subset of Go, where
/// `ParsedCommandLine` itself implements `OutputPathsHost`).
///
/// Side effects: none (plain data).
// Go: internal/tsoptions/parsedcommandline.go:ParsedCommandLine (as outputpaths.OutputPathsHost)
struct ConfigOutputPathsHost {
    common_source_directory: String,
    current_directory: String,
    use_case_sensitive_file_names: bool,
}

impl tsgo_outputpaths::OutputPathsHost for ConfigOutputPathsHost {
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

/// Builds the [`OutputPathsHost`](tsgo_outputpaths::OutputPathsHost) for
/// `config`, computing its common source directory (Go's
/// `ParsedCommandLine.CommonSourceDirectory`: `rootDir` if set, else the config
/// file's directory, else the longest common directory of the input files —
/// `.d.ts` inputs and, under `noEmitForJsFiles`, `.js` inputs are excluded).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/parsedcommandline.go:ParsedCommandLine.CommonSourceDirectory
/// The source files contributing to a config's common source directory: `.d.ts`
/// inputs are dropped, and (under `noEmitForJsFiles`) `.js` inputs too — Go's
/// filter inside `ParsedCommandLine.CommonSourceDirectory`.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/parsedcommandline.go:CommonSourceDirectory (files filter)
fn common_source_files(config: &ParsedCommandLine) -> Vec<String> {
    let options = config.compiler_options();
    config
        .file_names()
        .iter()
        .filter(|file| {
            let excluded_js =
                options.no_emit_for_js_files.is_true() && tsgo_tspath::has_js_file_extension(file);
            !excluded_js && !tsgo_tspath::is_declaration_file_name(file)
        })
        .cloned()
        .collect()
}

fn config_output_paths_host(config: &ParsedCommandLine) -> ConfigOutputPathsHost {
    let options = config.compiler_options();
    let current_directory = config.compare_paths_options.current_directory.clone();
    let use_case_sensitive_file_names = config.compare_paths_options.use_case_sensitive_file_names;
    let file_names = common_source_files(config);
    let common_source_directory = tsgo_outputpaths::get_common_source_directory(
        options,
        || file_names.clone(),
        &current_directory,
        use_case_sensitive_file_names,
        None,
    );
    ConfigOutputPathsHost {
        common_source_directory,
        current_directory,
        use_case_sensitive_file_names,
    }
}

/// Computes the declaration (`.d.ts`/`.d.mts`/`.d.cts`) output file name for
/// `input_file_name` under `config`'s `outDir`/`declarationDir`/`rootDir`.
///
/// Mirrors `outputpaths.GetOutputDeclarationFileNameWorker` with `config` as the
/// host; this is the output a *referencing* project consults for a referenced
/// project's `.d.ts` (Go's `getOutputDeclarationAndSourceFileNames`).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/parsedcommandline.go:getOutputDeclarationAndSourceFileNames
pub fn get_output_declaration_file_name(
    config: &ParsedCommandLine,
    input_file_name: &str,
) -> String {
    let host = config_output_paths_host(config);
    tsgo_outputpaths::get_output_declaration_file_name_worker(
        input_file_name,
        config.compiler_options(),
        &host,
    )
}

/// Computes the JavaScript (`.js`/`.mjs`/`.cjs`) output file name for
/// `input_file_name` under `config`'s `outDir`/`rootDir`.
///
/// Mirrors `outputpaths.GetOutputJSFileName` with `config` as the host.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/parsedcommandline.go:GetOutputFileNames (js)
pub fn get_output_js_file_name(config: &ParsedCommandLine, input_file_name: &str) -> String {
    let host = config_output_paths_host(config);
    tsgo_outputpaths::get_output_js_file_name(input_file_name, config.compiler_options(), &host)
}

/// Reports the `TS6059` diagnostics for any of `config`'s source files that lie
/// outside its `rootDir` (or, when `rootDir` is unset, the config file's
/// directory) — the check Go runs while deriving the common source directory.
///
/// # Note on the diagnostic code
/// The task brief labels this "TS6307 file-not-in-rootDir", but Go's rootDir
/// containment check actually produces **`TS6059`** ("File '{0}' is not under
/// 'rootDir' '{1}'. ..."). `TS6307` is a *different* error ("File '{0}' is not
/// listed within the file list of project '{1}'."), raised elsewhere when a
/// referencing project imports an output of a referenced project. This follows
/// Go (`checkSourceFilesBelongToPath`).
///
/// DEFER(P9): the `TS6307` "not listed in file list" check (needs the program's
/// source-of-project-reference redirect). blocked-by: P9 build / redirect.
///
/// Side effects: none (returns the diagnostics rather than mutating, unlike Go
/// which appends to `ParsedCommandLine.Errors`).
// Go: internal/tsoptions/parsedcommandline.go:ParsedCommandLine.checkSourceFilesBelongToPath
pub fn check_source_files_belong_to_root_dir(
    config: &ParsedCommandLine,
) -> Vec<ProjectReferenceDiagnostic> {
    let options = config.compiler_options();
    let current_directory = config.compare_paths_options.current_directory.clone();
    let use_case_sensitive_file_names = config.compare_paths_options.use_case_sensitive_file_names;
    let compare = tsgo_tspath::ComparePathsOptions {
        use_case_sensitive_file_names,
        current_directory: current_directory.clone(),
    };
    let file_names = common_source_files(config);

    // Go appends to `ParsedCommandLine.Errors` from inside the
    // `checkSourceFilesBelongToPath` callback that `GetCommonSourceDirectory`
    // invokes (in the `rootDir`/config-file branches). Collect via interior
    // mutability so the callback stays `Fn`.
    let diags = RefCell::new(Vec::new());
    let check = |files: &[String], root_directory: &str| -> bool {
        let mut all_files_belong = true;
        for file in files {
            let absolute = tsgo_tspath::get_canonical_file_name(
                &get_normalized_absolute_path(file, &current_directory),
                use_case_sensitive_file_names,
            );
            if !tsgo_tspath::contains_path(root_directory, file, &compare) {
                diags.borrow_mut().push(ProjectReferenceDiagnostic::new(
                    &tsgo_diagnostics::FILE_0_IS_NOT_UNDER_ROOTDIR_1_ROOTDIR_IS_EXPECTED_TO_CONTAIN_ALL_SOURCE_FILES,
                    vec![absolute, root_directory.to_string()],
                ));
                all_files_belong = false;
            }
        }
        all_files_belong
    };
    let _ = tsgo_outputpaths::get_common_source_directory(
        options,
        || file_names.clone(),
        &current_directory,
        use_case_sensitive_file_names,
        Some(&check),
    );
    diags.into_inner()
}

/// Mutable accumulators threaded through [`ResolvedProjectReferences::setup_build_task`].
///
/// Side effects: none (plain data).
// Go: internal/execute/build/orchestrator.go (completed/analyzing/circularityStack/order/errors)
#[derive(Default)]
struct BuildOrderState {
    /// Config paths fully ordered (their references already emitted).
    completed: HashSet<Path>,
    /// Config paths currently on the DFS stack (a re-visit is a cycle).
    analyzing: HashSet<Path>,
    /// The config *names* on the DFS stack (the `TS6202` cycle message body).
    stack: Vec<String>,
    /// The build order being built (config names, deepest first).
    order: Vec<String>,
    /// The circular-graph diagnostics found.
    errors: Vec<ProjectReferenceDiagnostic>,
}

#[cfg(test)]
#[path = "projectreference_test.rs"]
mod tests;
