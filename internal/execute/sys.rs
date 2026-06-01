//! The [`System`] abstraction the orchestration reports and writes through, and
//! a VFS-backed implementation ([`VfsSystem`]).
//!
//! Ports the reachable subset of Go's `internal/execute/tsc/compile.go:System`
//! interface (the file system, current directory, default-library location,
//! output writer, TTY/colour environment). The watch/statistics-clock facets
//! (`Now`/`SinceStart`/`GetWidthOfTerminal`) are deferred — they back the watch
//! loop and `--diagnostics` statistics, which are later P9 chunks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tsgo_vfs::Fs;

/// The orchestration's window onto the outside world: the file system, the
/// current directory, the bundled default-library location, an output sink, and
/// the colour/TTY environment used to decide pretty formatting.
///
/// This is the reachable subset of Go's `tsc.System`. Output is modelled as a
/// `write(&self, ...)` sink (Go exposes an `io.Writer`); the
/// `Now`/`SinceStart`/`GetWidthOfTerminal` clock/terminal facets are deferred
/// with the watch loop and `--diagnostics` statistics.
///
/// Side effects: implementations perform I/O (file system, output writer).
// Go: internal/execute/tsc/compile.go:System
pub trait System {
    /// The file system used for all reads and emitted-file writes.
    ///
    /// Side effects: none (returns a shared handle).
    // Go: internal/execute/tsc/compile.go:System.FS
    fn fs(&self) -> Arc<dyn Fs + Send + Sync>;

    /// The directory holding the bundled default `lib.*.d.ts` files.
    ///
    /// Side effects: none (pure).
    // Go: internal/execute/tsc/compile.go:System.DefaultLibraryPath
    fn default_library_path(&self) -> &str;

    /// The current working directory, used to root relative paths and
    /// relativize diagnostic file names.
    ///
    /// Side effects: none (pure).
    // Go: internal/execute/tsc/compile.go:System.GetCurrentDirectory
    fn get_current_directory(&self) -> &str;

    /// Appends `s` to the output sink (Go's `fmt.Fprint(sys.Writer(), ...)`).
    ///
    /// Side effects: writes to the output sink.
    // Go: internal/execute/tsc/compile.go:System.Writer
    fn write(&self, s: &str);

    /// Whether the output sink is an interactive terminal (drives the default
    /// pretty/colour decision).
    ///
    /// Side effects: none (pure).
    // Go: internal/execute/tsc/compile.go:System.WriteOutputIsTTY
    fn write_output_is_tty(&self) -> bool;

    /// The value of environment variable `name`, or the empty string if unset
    /// (mirrors Go, where a missing variable reads as `""`).
    ///
    /// Side effects: none (reads process/test environment state).
    // Go: internal/execute/tsc/compile.go:System.GetEnvironmentVariable
    fn get_environment_variable(&self, name: &str) -> String;

    /// The current wall-clock time, used by the `--build` status reporter to
    /// stamp its `HH:MM:SS PM - ...` verbose/dry lines (Go's `sys.Now()`).
    ///
    /// Side effects: reads the system clock (implementations may advance a
    /// deterministic test clock instead).
    // Go: internal/execute/tsc/compile.go:System.Now
    fn now(&self) -> SystemTime;

    /// Blocks until the watched files change, returning `true` to run another
    /// watch-loop build cycle or `false` to terminate the loop.
    ///
    /// This is the reachable, testable seam for Go's `vfswatch.FileWatcher`
    /// poll loop (`watcher.go:Watcher.start`'s `w.fileWatcher.Run`): a real
    /// production system would block on an OS file-watcher and rerun the build
    /// on each change, while a test system reports a finite, deterministic
    /// sequence of changes so the loop terminates.
    ///
    /// The default implementation returns `false`, so the watch loop runs the
    /// initial build once and then exits. This keeps the change ADDITIVE — every
    /// existing `System` implementation (including `cmd/tsgo`'s `OsSystem`)
    /// compiles unchanged — and stubs out the production OS file-watcher.
    ///
    /// DEFER(P9): the real OS file-watching backend (poll interval, watched
    /// file/wildcard-directory state, debounce) and Ctrl-C/signal handling.
    /// blocked-by: `vfswatch.FileWatcher` (the `internal/vfs/vfswatch` package
    /// is not yet ported) + signal handling.
    ///
    /// Side effects: a real implementation blocks on file-system events.
    // Go: internal/vfs/vfswatch/filewatcher.go:FileWatcher.Run
    fn wait_for_change(&self) -> bool {
        false
    }
}

/// A [`System`] backed by a shared [`Fs`] with an in-memory output buffer.
///
/// This is the reachable, deterministic host the library `execute` entry and
/// its tests drive: output accumulates in a buffer readable via
/// [`output`](VfsSystem::output), and the colour environment is configurable so
/// both the default plain path and an explicit pretty path can be exercised.
///
/// Side effects: none at construction (holds the shared file-system handle).
// Go: internal/execute/tsc/compile.go:System (osSys implementation, reachable subset)
pub struct VfsSystem {
    fs: Arc<dyn Fs + Send + Sync>,
    current_directory: String,
    default_library_path: String,
    output: RefCell<String>,
    env: HashMap<String, String>,
    write_output_is_tty: bool,
    // A deterministic monotonic test clock (Go's `TestClock`): each `now()`
    // call advances one second so successive `--build` status lines get
    // distinct, reproducible timestamps in baselines.
    clock: RefCell<SystemTime>,
}

impl VfsSystem {
    /// Builds a [`VfsSystem`] over `fs`, rooting relative paths at
    /// `current_directory` and reading the default library from
    /// `default_library_path`. Output starts empty, the environment is empty,
    /// and the output is treated as non-TTY (so the default reporting is plain).
    ///
    /// # Examples
    /// ```
    /// use std::sync::Arc;
    /// use tsgo_execute::{System, VfsSystem};
    /// use tsgo_vfs::vfstest::MapFs;
    /// use tsgo_vfs::Fs;
    ///
    /// let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map([("/p/a.ts", "")], true));
    /// let sys = VfsSystem::new(fs, "/p", "/lib");
    /// assert_eq!(sys.get_current_directory(), "/p");
    /// assert!(!sys.write_output_is_tty());
    /// ```
    ///
    /// Side effects: none (stores the handles).
    pub fn new(
        fs: Arc<dyn Fs + Send + Sync>,
        current_directory: impl Into<String>,
        default_library_path: impl Into<String>,
    ) -> VfsSystem {
        VfsSystem {
            fs,
            current_directory: current_directory.into(),
            default_library_path: default_library_path.into(),
            output: RefCell::new(String::new()),
            env: HashMap::new(),
            write_output_is_tty: false,
            clock: RefCell::new(SystemTime::UNIX_EPOCH),
        }
    }

    /// Returns a copy of everything written to the output sink so far.
    ///
    /// Side effects: none (pure read of the buffer).
    pub fn output(&self) -> String {
        self.output.borrow().clone()
    }

    /// Sets environment variable `name` to `value` (builder-style), letting tests
    /// drive the `NO_COLOR`/`FORCE_COLOR` pretty decision.
    ///
    /// Side effects: mutates this system's environment map.
    pub fn set_environment_variable(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.env.insert(name.into(), value.into());
    }

    /// Marks the output sink as an interactive terminal (builder-style), so the
    /// default reporting becomes pretty.
    ///
    /// Side effects: mutates this system's TTY flag.
    pub fn set_write_output_is_tty(&mut self, is_tty: bool) {
        self.write_output_is_tty = is_tty;
    }
}

impl System for VfsSystem {
    fn fs(&self) -> Arc<dyn Fs + Send + Sync> {
        self.fs.clone()
    }

    fn default_library_path(&self) -> &str {
        &self.default_library_path
    }

    fn get_current_directory(&self) -> &str {
        &self.current_directory
    }

    fn write(&self, s: &str) {
        self.output.borrow_mut().push_str(s);
    }

    fn write_output_is_tty(&self) -> bool {
        self.write_output_is_tty
    }

    fn get_environment_variable(&self, name: &str) -> String {
        self.env.get(name).cloned().unwrap_or_default()
    }

    fn now(&self) -> SystemTime {
        // Mirror Go's `TestClock.Now`: advance one second, then return.
        let mut clock = self.clock.borrow_mut();
        *clock += Duration::from_secs(1);
        *clock
    }
}

#[cfg(test)]
#[path = "sys_test.rs"]
mod tests;
