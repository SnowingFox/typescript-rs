//! `tsgo_pprof` — 1:1 Rust port of Go `internal/pprof`.
//!
//! Start/stop and persistence helpers for CPU, heap, and allocation profiling
//! (used by `--pprofDir` and on-demand LSP profiling). The public API shape,
//! profiling state machine, file naming, directory creation, and error text
//! mirror the Go package.
//!
//! # Divergence from Go
//! - Rust has no standard-library profiler and no garbage collector. The actual
//!   profile *payload* generation (Go's `runtime/pprof`) is not yet wired up:
//!   profile files are created with the same names and extensions, but writing
//!   real pprof protobuf data is deferred to P10 (backend selection). The
//!   state machine, naming, error messages, and directory handling are faithful.
//! - `runtime.GC()` has no analogue; [`run_gc`] is a no-op.
//! - `BeginProfiling`/`Stop` panic on I/O failure exactly like Go; the on-demand
//!   `CpuProfiler` and `save_*` helpers return `Result`.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Errors returned by the on-demand profiling APIs.
#[derive(Debug)]
pub enum PprofError {
    /// `start_cpu_profile` was called while a session was already running.
    AlreadyInProgress,
    /// `stop_cpu_profile` was called with no session running.
    NotInProgress,
    /// A wrapped I/O failure with Go-style context (e.g. "failed to create
    /// profile directory").
    Io {
        /// Human-readable context prefix, matching Go's `fmt.Errorf` text.
        context: &'static str,
        /// The underlying I/O error.
        source: io::Error,
    },
}

impl PprofError {
    fn io(context: &'static str, source: io::Error) -> Self {
        PprofError::Io { context, source }
    }
}

impl fmt::Display for PprofError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PprofError::AlreadyInProgress => f.write_str("CPU profiling already in progress"),
            PprofError::NotInProgress => f.write_str("CPU profiling not in progress"),
            PprofError::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl Error for PprofError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            PprofError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// A single profiling session: an open CPU profile file plus the paths it will
/// write to and a log sink for completion messages.
pub struct ProfileSession {
    cpu_file_path: PathBuf,
    mem_file_path: PathBuf,
    cpu_file: Option<fs::File>,
    log_writer: Box<dyn Write + Send>,
}

/// Starts CPU and memory profiling, writing the profiles into `profile_dir`.
///
/// File names match Go: `<pid>-cpuprofile.pb.gz` and `<pid>-memprofile.pb.gz`.
///
/// # Examples
/// ```no_run
/// use std::path::Path;
/// let session = tsgo_pprof::begin_profiling(Path::new("/tmp/prof"), Box::new(std::io::stdout()));
/// session.stop();
/// ```
///
/// Side effects: creates `profile_dir` (recursively) and the CPU profile file;
/// panics on I/O failure (mirroring Go). Worktree/state otherwise unchanged.
// Go: internal/pprof/pprof.go:BeginProfiling
pub fn begin_profiling(profile_dir: &Path, log_writer: Box<dyn Write + Send>) -> ProfileSession {
    fs::create_dir_all(profile_dir).expect("pprof: failed to create profile directory");
    let pid = std::process::id();
    let cpu_file_path = profile_dir.join(format!("{pid}-cpuprofile.pb.gz"));
    let mem_file_path = profile_dir.join(format!("{pid}-memprofile.pb.gz"));
    let cpu_file =
        fs::File::create(&cpu_file_path).expect("pprof: failed to create CPU profile file");
    // TODO(port): start real CPU profiling once a pprof backend is selected.
    ProfileSession {
        cpu_file_path,
        mem_file_path,
        cpu_file: Some(cpu_file),
        log_writer,
    }
}

impl ProfileSession {
    /// Stops CPU profiling, writes the memory profile (when this session has a
    /// memory path), and logs the resulting profile paths.
    ///
    /// Side effects: closes the CPU file; may create the memory profile file;
    /// writes completion lines to the session's log writer.
    // Go: internal/pprof/pprof.go:(*ProfileSession).Stop
    pub fn stop(mut self) {
        // TODO(port): stop real CPU profiling.
        self.cpu_file.take();

        if !self.mem_file_path.as_os_str().is_empty() {
            let _mem_file = fs::File::create(&self.mem_file_path)
                .expect("pprof: failed to create mem profile file");
            // TODO(port): write real allocs profile.
            let _ = writeln!(
                self.log_writer,
                "Memory profile: {}",
                self.mem_file_path.display()
            );
        }

        let _ = writeln!(
            self.log_writer,
            "CPU profile: {}",
            self.cpu_file_path.display()
        );
    }
}

/// Manages on-demand CPU profiling, guarding a single active session.
#[derive(Default)]
pub struct CpuProfiler {
    session: Mutex<Option<ProfileSession>>,
}

impl CpuProfiler {
    /// Creates a profiler with no active session.
    ///
    /// # Examples
    /// ```
    /// let _profiler = tsgo_pprof::CpuProfiler::new();
    /// ```
    pub fn new() -> Self {
        CpuProfiler::default()
    }

    /// Starts CPU profiling, writing to `profile_dir` when stopped.
    ///
    /// Returns [`PprofError::AlreadyInProgress`] if a session is already active.
    ///
    /// Side effects: creates `profile_dir` and a CPU profile file named
    /// `<pid>-<unix_millis>-cpuprofile.pb.gz`.
    // Go: internal/pprof/pprof.go:(*CPUProfiler).StartCPUProfile
    pub fn start_cpu_profile(&self, profile_dir: &Path) -> Result<(), PprofError> {
        let mut guard = self.session.lock().expect("pprof: poisoned session lock");
        if guard.is_some() {
            return Err(PprofError::AlreadyInProgress);
        }
        fs::create_dir_all(profile_dir)
            .map_err(|e| PprofError::io("failed to create profile directory", e))?;
        let cpu_file_path = profile_dir.join(format!(
            "{}-{}-cpuprofile.pb.gz",
            std::process::id(),
            now_millis()
        ));
        let cpu_file = fs::File::create(&cpu_file_path)
            .map_err(|e| PprofError::io("failed to create CPU profile file", e))?;
        // TODO(port): start real CPU profiling once a pprof backend is selected.
        *guard = Some(ProfileSession {
            cpu_file_path,
            mem_file_path: PathBuf::new(),
            cpu_file: Some(cpu_file),
            log_writer: Box::new(io::sink()),
        });
        Ok(())
    }

    /// Stops CPU profiling and returns the path to the profile file.
    ///
    /// Returns [`PprofError::NotInProgress`] if no session is active.
    ///
    /// Side effects: closes the CPU file and clears the active session.
    // Go: internal/pprof/pprof.go:(*CPUProfiler).StopCPUProfile
    pub fn stop_cpu_profile(&self) -> Result<String, PprofError> {
        let mut guard = self.session.lock().expect("pprof: poisoned session lock");
        let session = guard.take().ok_or(PprofError::NotInProgress)?;
        let file_path = session.cpu_file_path.to_string_lossy().into_owned();
        session.stop();
        Ok(file_path)
    }
}

/// Saves a heap profile to `profile_dir`, returning the written path.
///
/// Side effects: creates `profile_dir` and a file named
/// `<pid>-<unix_millis>-heapprofile.pb.gz`; runs [`run_gc`] first (a no-op).
// Go: internal/pprof/pprof.go:SaveHeapProfile
pub fn save_heap_profile(profile_dir: &Path) -> Result<String, PprofError> {
    fs::create_dir_all(profile_dir)
        .map_err(|e| PprofError::io("failed to create profile directory", e))?;
    let path = profile_dir.join(format!(
        "{}-{}-heapprofile.pb.gz",
        std::process::id(),
        now_millis()
    ));
    let mut file = fs::File::create(&path)
        .map_err(|e| PprofError::io("failed to create heap profile file", e))?;
    run_gc();
    // TODO(port): write real heap profile.
    file.flush()
        .map_err(|e| PprofError::io("failed to write heap profile", e))?;
    Ok(path.to_string_lossy().into_owned())
}

/// Saves an allocation profile to `profile_dir`, returning the written path.
///
/// Side effects: creates `profile_dir` and a file named
/// `<pid>-<unix_millis>-allocprofile.pb.gz`.
// Go: internal/pprof/pprof.go:SaveAllocProfile
pub fn save_alloc_profile(profile_dir: &Path) -> Result<String, PprofError> {
    fs::create_dir_all(profile_dir)
        .map_err(|e| PprofError::io("failed to create profile directory", e))?;
    let path = profile_dir.join(format!(
        "{}-{}-allocprofile.pb.gz",
        std::process::id(),
        now_millis()
    ));
    let mut file = fs::File::create(&path)
        .map_err(|e| PprofError::io("failed to create alloc profile file", e))?;
    // TODO(port): write real allocs profile.
    file.flush()
        .map_err(|e| PprofError::io("failed to write alloc profile", e))?;
    Ok(path.to_string_lossy().into_owned())
}

/// Triggers garbage collection.
///
/// Side effects: none. Rust has no garbage collector, so this is a no-op (see
/// the crate-level divergence note).
///
/// # Examples
/// ```
/// tsgo_pprof::run_gc();
/// ```
// Go: internal/pprof/pprof.go:RunGC
pub fn run_gc() {}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
