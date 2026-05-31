//! Port of Go `internal/compiler/filesparser.go`: the file-discovery worklist
//! and the deterministic depth-first collection of the parsed file set.
//!
//! This round ports the reachable foundation as a **sequential** worklist
//! (`// PERF(port)`: Go runs this on `core.WorkGroup` for parallelism; the
//! deterministic order comes from the serial `collect_files` post-pass either
//! way, so the sequential version produces the same order and is parallelized in
//! a later round). Lib/type-reference/automatic-type-directive/redirect tasks
//! are deferred.

use std::collections::{HashMap, HashSet};

use tsgo_tspath::Path;

use crate::fileloader::{get_default_lib_file_priority, FileLoader, ProcessedFiles};
use crate::host::ParsedFile;

/// One node in the file-discovery graph: a file to read, parse, and resolve the
/// imports of.
///
/// This is the reachable subset of Go's `parseTask`; lib/redirect/ATA fields are
/// deferred.
///
/// Side effects: none (owns parse output once loaded).
// Go: internal/compiler/filesparser.go:parseTask
pub struct ParseTask {
    normalized_file_path: String,
    sub_tasks: Vec<Path>,
    file: Option<ParsedFile>,
    loaded: bool,
    /// Whether this task loads a default-library file (`lib.*.d.ts`). Lib tasks
    /// have their `/// <reference lib>` directives expanded into more lib tasks,
    /// and are collected lib-first (sorted by priority) by [`FilesParser::collect_files`].
    // Go: internal/compiler/filesparser.go:parseTask.libFile
    is_lib: bool,
}

/// The file-discovery worklist: every reachable file keyed by its canonical
/// [`Path`], plus the roots in declaration order.
///
/// This is the reachable subset of Go's `filesParser`.
///
/// Side effects: none.
// Go: internal/compiler/filesparser.go:filesParser
pub struct FilesParser {
    tasks_by_path: HashMap<Path, ParseTask>,
    root_paths: Vec<Path>,
}

impl Default for FilesParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FilesParser {
    /// Creates an empty worklist.
    ///
    /// Side effects: none.
    pub fn new() -> FilesParser {
        FilesParser {
            tasks_by_path: HashMap::new(),
            root_paths: Vec::new(),
        }
    }

    /// Registers `normalized_file_path` as a root file to load.
    ///
    /// Side effects: none (records the task; no I/O until [`Self::parse`]).
    // Go: internal/compiler/fileloader.go:fileLoader.addRootTask
    pub fn add_root_task(&mut self, loader: &FileLoader, normalized_file_path: String) {
        let path = loader.to_path(&normalized_file_path);
        self.ensure_task(path.clone(), normalized_file_path, false);
        self.root_paths.push(path);
    }

    /// Registers `normalized_file_path` as a default-library root to load (its
    /// `/// <reference lib>` directives are expanded; see [`Self::parse`]).
    ///
    /// Side effects: none (records the task; no I/O until [`Self::parse`]).
    // Go: internal/compiler/fileloader.go:fileLoader.addRootTask (with libFile)
    pub fn add_lib_root_task(&mut self, loader: &FileLoader, normalized_file_path: String) {
        let path = loader.to_path(&normalized_file_path);
        self.ensure_task(path.clone(), normalized_file_path, true);
        self.root_paths.push(path);
    }

    /// Inserts an unloaded task for `path` if one does not exist yet.
    ///
    /// The `is_lib` flag is first-wins: an existing task keeps its flag (mirroring
    /// Go, where the first task to register a path fixes its `libFile`).
    ///
    /// Side effects: none.
    fn ensure_task(&mut self, path: Path, normalized_file_path: String, is_lib: bool) {
        self.tasks_by_path.entry(path).or_insert(ParseTask {
            normalized_file_path,
            sub_tasks: Vec::new(),
            file: None,
            loaded: false,
            is_lib,
        });
    }

    /// Loads every reachable file: parses each task and resolves its imports into
    /// sub-tasks, following them transitively until the worklist drains.
    ///
    /// Side effects: reads the file system through `loader`.
    // Go: internal/compiler/filesparser.go:filesParser.parse
    pub fn parse(&mut self, loader: &FileLoader) {
        let mut queue: Vec<Path> = self.root_paths.clone();
        let mut head = 0;
        while head < queue.len() {
            let path = queue[head].clone();
            head += 1;

            let (normalized, is_lib) = match self.tasks_by_path.get(&path) {
                Some(task) if !task.loaded => (task.normalized_file_path.clone(), task.is_lib),
                _ => continue,
            };

            let file = loader.parse_source_file(&normalized);
            let mut sub_tasks = Vec::new();
            if let Some(parsed) = &file {
                // A lib file's `/// <reference lib>` directives pull in more lib
                // files (e.g. the `lib.d.ts` aggregator references `lib.es5.d.ts`,
                // `lib.dom.d.ts`, ...). The parser does not expose these
                // directives yet, so the loader scans the lib's leading trivia.
                if is_lib {
                    for resolved_name in loader.resolve_lib_references(parsed) {
                        let sub_path = loader.to_path(&resolved_name);
                        self.ensure_task(sub_path.clone(), resolved_name, true);
                        queue.push(sub_path.clone());
                        sub_tasks.push(sub_path);
                    }
                }
                for resolved_name in loader.resolve_import_file_names(parsed) {
                    let sub_path = loader.to_path(&resolved_name);
                    self.ensure_task(sub_path.clone(), resolved_name, false);
                    queue.push(sub_path.clone());
                    sub_tasks.push(sub_path);
                }
            }

            let task = self
                .tasks_by_path
                .get_mut(&path)
                .expect("task registered before loading");
            task.file = file;
            task.sub_tasks = sub_tasks;
            task.loaded = true;
        }
    }

    /// Collects the loaded files into a deterministic [`ProcessedFiles`]: a
    /// depth-first walk of the roots that appends each file *after* its imports
    /// (so referenced files precede their referrers) and visits each file once,
    /// then orders the lib files *first* — sorted by
    /// [`get_default_lib_file_priority`] — ahead of the source files (Go's
    /// `sortLibs` + `append(libFiles, files...)`). `default_library_path` is the
    /// normalized lib directory the priority ranking is relative to.
    ///
    /// Side effects: none (consumes the worklist).
    // Go: internal/compiler/filesparser.go:filesParser.getProcessedFiles (sortLibs + libs first)
    pub fn collect_files(mut self, default_library_path: &str) -> ProcessedFiles {
        let mut order: Vec<Path> = Vec::new();
        let mut seen: HashSet<Path> = HashSet::new();
        let roots = self.root_paths.clone();
        for root in &roots {
            self.collect_post_order(root, &mut seen, &mut order);
        }

        // Partition into lib files and source files in traversal order, keeping
        // the (path, file) pair so the final list can be assembled libs-first.
        let mut lib_entries: Vec<(Path, ParsedFile)> = Vec::new();
        let mut file_entries: Vec<(Path, ParsedFile)> = Vec::new();
        let mut missing_files: Vec<String> = Vec::new();
        for path in order {
            let Some(task) = self.tasks_by_path.remove(&path) else {
                continue;
            };
            match task.file {
                Some(file) if task.is_lib => lib_entries.push((path, file)),
                Some(file) => file_entries.push((path, file)),
                None => missing_files.push(task.normalized_file_path),
            }
        }

        // Sort the libs by priority (stable, so equal priorities keep traversal
        // order); the worklist already deduplicated by path.
        lib_entries.sort_by_key(|(_, file)| {
            get_default_lib_file_priority(file.file_name(), default_library_path)
        });

        let mut files: Vec<ParsedFile> = Vec::with_capacity(lib_entries.len() + file_entries.len());
        let mut files_by_path: HashMap<Path, usize> = HashMap::new();
        for (path, file) in lib_entries.into_iter().chain(file_entries) {
            files_by_path.insert(path, files.len());
            files.push(file);
        }

        ProcessedFiles::from_parts(files, files_by_path, missing_files)
    }

    /// Depth-first post-order traversal: records `path` after recursing into its
    /// sub-tasks, skipping unloaded or already-seen tasks.
    ///
    /// Side effects: none (mutates the `seen`/`order` accumulators).
    // Go: internal/compiler/filesparser.go:collectFiles
    fn collect_post_order(&self, path: &Path, seen: &mut HashSet<Path>, order: &mut Vec<Path>) {
        let task = match self.tasks_by_path.get(path) {
            Some(task) if task.loaded => task,
            _ => return,
        };
        if !seen.insert(path.clone()) {
            return;
        }
        for sub in &task.sub_tasks {
            self.collect_post_order(sub, seen, order);
        }
        order.push(path.clone());
    }
}

#[cfg(test)]
#[path = "filesparser_test.rs"]
mod tests;
