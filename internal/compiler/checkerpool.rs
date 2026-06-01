//! Port of Go `internal/compiler/checkerpool.go`: the [`CheckerPool`] abstraction
//! and the built-in compiler pool.
//!
//! This round ports the pool's sizing policy ([`checker_count`]), the pool shell
//! ([`CompilerCheckerPool`]), real checker creation through the checker's public
//! `Checker::new_checker(Rc<dyn BoundProgram>)` retain seam (round 4l), the
//! file→checker association (`i % K`) plus the grouped-iteration shape, and —
//! since P6-6 — *driving* per-file diagnostics over a real multi-file
//! [`BoundProgram`] ([`CompilerCheckerPool::collect_diagnostics`]).
//!
//! # How the pool drives checking (P6-6)
//!
//! Go builds K checkers over one shared `*Program` and each checker collects
//! diagnostics for its `i % K` file subset. The pool now builds one
//! [`MultiFileBoundProgram`] over *all* bound files (lib + sources, with a
//! merged global table) and `Rc::clone`s it into each of the K checkers (Go's
//! "one program per pool"). [`Self::collect_diagnostics`] then iterates the
//! program's [`source_files`](BoundProgram::source_files) handles in input
//! order, driving the file's associated checker (`i % K`) via
//! [`Checker::get_diagnostics`], which returns *only that file's* diagnostics
//! (Go's `collection.GetDiagnosticsForFile(name)` partitioning).
//!
//! DEFER(P6): the grouped-*parallel* collection (across checkers) and
//! non-exclusive emit access.
//! blocked-by: parallel `Arc` checkers (PORTING §6) — the checker retains an
//! `Rc` (not `Arc + Send + Sync`) program today, so the pool drives the per-file
//! subsets sequentially.

use std::rc::Rc;

use tsgo_checker::{BoundProgram, Checker, Diagnostic};
use tsgo_core::compileroptions::CompilerOptions;

use crate::host::ParsedFile;
use crate::multifile::MultiFileBoundProgram;

/// Computes how many checkers the built-in pool should create.
///
/// Mirrors Go: the base count is 1 when single-threaded, else the configured
/// `--checkers` value (default 4); it is then clamped to `[1, min(file_count,
/// 256)]`.
///
/// # Examples
/// ```
/// use tsgo_compiler::checker_count;
/// assert_eq!(checker_count(false, None, 10), 4); // default
/// assert_eq!(checker_count(false, Some(8), 2), 2); // clamped to file count
/// assert_eq!(checker_count(true, Some(8), 10), 1); // single-threaded
/// assert_eq!(checker_count(false, None, 0), 1); // at least one
/// ```
///
/// Side effects: none (pure).
// Go: internal/compiler/checkerpool.go:newCheckerPoolWithTracing
pub fn checker_count(single_threaded: bool, configured: Option<i32>, file_count: usize) -> usize {
    let base: usize = if single_threaded {
        1
    } else {
        match configured {
            Some(c) if c > 0 => c as usize,
            _ => 4,
        }
    };
    base.min(file_count).clamp(1, 256)
}

/// The built-in compiler checker pool.
///
/// It is sized eagerly (see [`checker_count`]); [`Self::create_checkers`] then
/// builds the real [`Checker`] slots and the file→checker association.
///
/// Side effects: none at construction.
// Go: internal/compiler/checkerpool.go:checkerPool
pub struct CompilerCheckerPool {
    checker_count: usize,
    checkers: Vec<Checker>,
    /// File index → owning checker index, assigned `i % checker_count`.
    file_associations: Vec<usize>,
    /// The multi-file bound program the K checkers share (Go: one `*Program` per
    /// pool). `None` until [`Self::create_checkers`] builds it.
    program: Option<Rc<dyn BoundProgram>>,
}

impl CompilerCheckerPool {
    /// Creates a pool sized by [`checker_count`].
    ///
    /// Side effects: none.
    // Go: internal/compiler/checkerpool.go:newCheckerPool
    pub fn new(
        single_threaded: bool,
        configured: Option<i32>,
        file_count: usize,
    ) -> CompilerCheckerPool {
        CompilerCheckerPool {
            checker_count: checker_count(single_threaded, configured, file_count),
            checkers: Vec::new(),
            file_associations: Vec::new(),
            program: None,
        }
    }

    /// The number of checkers this pool will create (its configured size).
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/checkerpool.go:checkerPool.checkers (len)
    pub fn checker_count(&self) -> usize {
        self.checker_count
    }

    /// Creates the real checkers (idempotently) and assigns each file to a
    /// checker round-robin (`file_index % checker_count`).
    ///
    /// The K checkers all share one program: a [`MultiFileBoundProgram`] over
    /// *all* the bound files (lib + sources, with a merged global table) is
    /// placed in an `Rc<dyn BoundProgram>` and `Rc::clone`d into each
    /// [`Checker::new_checker`] call, mirroring Go where one `*Program` is
    /// shared by every checker in the pool. Files must be bound first (see
    /// [`Program::bind_source_files`](crate::Program::bind_source_files));
    /// unbound files do not contribute a source-file handle.
    ///
    /// Side effects: allocates `checker_count` checkers (each allocates its
    /// intrinsic-type arena) and retains the shared program.
    // Go: internal/compiler/checkerpool.go:createCheckers
    pub fn create_checkers(&mut self, files: &[ParsedFile]) {
        self.create_checkers_with_options(files, Rc::new(CompilerOptions::default()));
    }

    /// Creates the real checkers (idempotently) over a program carrying the
    /// real compiler `options`, associating each file to a checker round-robin
    /// (`file_index % checker_count`).
    ///
    /// Identical to [`Self::create_checkers`] except the shared
    /// [`MultiFileBoundProgram`] is built with `options`, so every checker's
    /// [`Checker::compiler_options`](tsgo_checker::Checker::compiler_options)
    /// reads the program's actual `--target`/`--downlevelIteration`/`--strict`
    /// (round 4al's option-gated diagnostics) end-to-end, rather than the
    /// all-defaults the options-free overload supplies.
    ///
    /// Side effects: allocates `checker_count` checkers (each allocates its
    /// intrinsic-type arena) and retains the shared program.
    // Go: internal/compiler/checkerpool.go:createCheckers (program.Options())
    pub fn create_checkers_with_options(
        &mut self,
        files: &[ParsedFile],
        options: Rc<CompilerOptions>,
    ) {
        if !self.checkers.is_empty() {
            return;
        }

        // Go constructs every checker over the same shared `*Program`; the
        // ported counterpart is one `MultiFileBoundProgram` over every bound
        // file, shared by `Rc::clone`, carrying the program's real options.
        let program: Rc<dyn BoundProgram> =
            Rc::new(MultiFileBoundProgram::new_with_options(files, options));
        // Associate by source-file handle index (the checker drives one file per
        // handle); `i % K` mirrors Go's `fileAssociations`.
        let file_count = program.source_files().len();
        self.file_associations = (0..file_count).map(|i| i % self.checker_count).collect();

        for _ in 0..self.checker_count {
            self.checkers
                .push(Checker::new_checker(Rc::clone(&program)));
        }
        self.program = Some(program);
    }

    /// The number of checkers actually created by [`Self::create_checkers`].
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/checkerpool.go:checkerPool.checkers (len)
    pub fn created_checker_count(&self) -> usize {
        self.checkers.len()
    }

    /// The checker index assigned to file `file_index`, or `None` if the index is
    /// out of range.
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/checkerpool.go:checkerPool.fileAssociations
    pub fn checker_index_for_file(&self, file_index: usize) -> Option<usize> {
        self.file_associations.get(file_index).copied()
    }

    /// The file indices (in `0..file_count`) assigned to `checker_index`.
    ///
    /// This is the grouped-iteration shape of Go's `forEachCheckerGroupDo`: each
    /// checker processes the files where `i % checker_count == checker_index`,
    /// preserving the original file order.
    ///
    /// # Examples
    /// ```
    /// use tsgo_compiler::CompilerCheckerPool;
    /// let pool = CompilerCheckerPool::new(false, Some(2), 3);
    /// assert_eq!(pool.files_for_checker(0, 3), vec![0, 2]);
    /// assert_eq!(pool.files_for_checker(1, 3), vec![1]);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/compiler/checkerpool.go:forEachCheckerGroupDo
    pub fn files_for_checker(&self, checker_index: usize, file_count: usize) -> Vec<usize> {
        if self.checker_count == 0 {
            return Vec::new();
        }
        (0..file_count)
            .filter(|i| i % self.checker_count == checker_index)
            .collect()
    }

    /// Drives type-checking of every file through the pool and returns the
    /// collected semantic diagnostics, in input file order.
    ///
    /// Each file's source-file handle (from
    /// [`BoundProgram::source_files`]) is checked by its associated checker
    /// (`i % K`) via [`Checker::get_diagnostics`], which runs `checkSourceFile`
    /// and returns *only that file's* diagnostics (Go's
    /// `collection.GetDiagnosticsForFile(name)` partitioning). The per-file
    /// results are concatenated in file order, so the output is deterministic
    /// and independent of the `i % K` assignment. Returns an empty list when no
    /// checkers were created (no bound files). Idempotent: the checker guards
    /// re-checking, so repeated calls do not double-report.
    ///
    /// DEFER(P6): grouped-*parallel* collection across checkers.
    /// blocked-by: parallel `Arc` checkers (PORTING §6) — the checker retains an
    /// `Rc` program, so the per-file subsets are driven sequentially.
    // Go: internal/compiler/checkerpool.go:forEachCheckerGroupDo + program.go:getDiagnostics
    pub fn collect_diagnostics(&mut self) -> Vec<Diagnostic> {
        self.collect_diagnostics_excluding(&[])
    }

    /// Like [`Self::collect_diagnostics`], but skips the bound files whose index
    /// is `true` in `exclude` (parallel to [`BoundProgram::source_files`] order).
    /// Used to omit the auto-included default-library files: `tsc` does not
    /// report semantic diagnostics located in `lib.*.d.ts`, and the partial
    /// checker would otherwise false-positive on their advanced constructs (and
    /// such a lib-positioned diagnostic, rendered against a user file, panics the
    /// diagnostic writer with an out-of-bounds slice). An empty `exclude`
    /// (or shorter than the file list) excludes nothing.
    // Go: internal/compiler/program.go:getDiagnostics (skips default-lib files)
    pub fn collect_diagnostics_excluding(&mut self, exclude: &[bool]) -> Vec<Diagnostic> {
        // The K checkers share one `Rc<dyn BoundProgram>`; clone the handle so it
        // does not borrow `self` while a checker is driven with `&mut`.
        let Some(program) = self.program.clone() else {
            return Vec::new();
        };
        let mut diagnostics = Vec::new();
        // Iterate handles in input file order for a deterministic result.
        for (file_index, handle) in program.source_files().into_iter().enumerate() {
            if exclude.get(file_index).copied().unwrap_or(false) {
                continue;
            }
            let checker_index = self.checker_index_for_file(file_index).unwrap_or(0);
            diagnostics.extend(
                self.checkers[checker_index]
                    .get_diagnostics(handle)
                    .iter()
                    .cloned(),
            );
        }
        diagnostics
    }
}

#[cfg(test)]
#[path = "checkerpool_test.rs"]
mod tests;
