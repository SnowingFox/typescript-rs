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

use crate::fileloader::{FileLoader, ProcessedFiles};
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
        self.ensure_task(path.clone(), normalized_file_path);
        self.root_paths.push(path);
    }

    /// Inserts an unloaded task for `path` if one does not exist yet.
    ///
    /// Side effects: none.
    fn ensure_task(&mut self, path: Path, normalized_file_path: String) {
        self.tasks_by_path.entry(path).or_insert(ParseTask {
            normalized_file_path,
            sub_tasks: Vec::new(),
            file: None,
            loaded: false,
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

            let normalized = match self.tasks_by_path.get(&path) {
                Some(task) if !task.loaded => task.normalized_file_path.clone(),
                _ => continue,
            };

            let file = loader.parse_source_file(&normalized);
            let mut sub_tasks = Vec::new();
            if let Some(parsed) = &file {
                for resolved_name in loader.resolve_import_file_names(parsed) {
                    let sub_path = loader.to_path(&resolved_name);
                    self.ensure_task(sub_path.clone(), resolved_name);
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
    /// (so referenced files precede their referrers) and visits each file once.
    ///
    /// Side effects: none (consumes the worklist).
    // Go: internal/compiler/filesparser.go:filesParser.getProcessedFiles
    pub fn collect_files(mut self) -> ProcessedFiles {
        let mut order: Vec<Path> = Vec::new();
        let mut seen: HashSet<Path> = HashSet::new();
        let roots = self.root_paths.clone();
        for root in &roots {
            self.collect_post_order(root, &mut seen, &mut order);
        }

        let mut files: Vec<ParsedFile> = Vec::new();
        let mut files_by_path: HashMap<Path, usize> = HashMap::new();
        let mut missing_files: Vec<String> = Vec::new();
        for path in order {
            let Some(task) = self.tasks_by_path.remove(&path) else {
                continue;
            };
            match task.file {
                Some(file) => {
                    files_by_path.insert(path, files.len());
                    files.push(file);
                }
                None => missing_files.push(task.normalized_file_path),
            }
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
