//! `tsgo`: the command-line entry point (ports Go's `cmd/tsgo/main.go`).
//!
//! Thin argv dispatcher: it reads `std::env::args`, builds a real-filesystem
//! [`System`] (the os-backed implementation of [`tsgo_execute::System`]), and
//! routes to the `tsc` build/check/emit path in [`tsgo_execute`]. The `--lsp`
//! and `--api` subcommands are recognized by arg0 and routed to "not yet
//! implemented" stubs (those servers are P8). `--version`/`-v` print the
//! compiler version; `--help`/`-h` are bridged to a deferred stub.
//!
//! The logic lives in [`run`] (`args`, `sys`) so it can be unit-tested against a
//! VFS-backed [`tsgo_execute::VfsSystem`] without spawning a process; [`main`]
//! only builds the real [`System`] and calls [`std::process::exit`].

use std::io::{IsTerminal, Write};
use std::sync::Arc;

use tsgo_execute::{ExitStatus, System};
use tsgo_tsoptions::{parse_command_line, ParseConfigHost};
use tsgo_vfs::Fs;

/// A real-filesystem [`System`]: reads/writes the actual disk through
/// [`tsgo_vfs::osvfs`] (wrapped by [`tsgo_bundled`] so `bundled:///` lib files
/// resolve), and writes output to the process stdout.
// Go: cmd/tsgo/sys.go:osSys
struct OsSystem {
    fs: Arc<dyn Fs + Send + Sync>,
    current_directory: String,
    default_library_path: String,
}

impl System for OsSystem {
    // Go: cmd/tsgo/sys.go:osSys.FS
    fn fs(&self) -> Arc<dyn Fs + Send + Sync> {
        self.fs.clone()
    }

    // Go: cmd/tsgo/sys.go:osSys.DefaultLibraryPath
    fn default_library_path(&self) -> &str {
        &self.default_library_path
    }

    // Go: cmd/tsgo/sys.go:osSys.GetCurrentDirectory
    fn get_current_directory(&self) -> &str {
        &self.current_directory
    }

    // Go: cmd/tsgo/sys.go:osSys.Now (the real wall clock)
    fn now(&self) -> std::time::SystemTime {
        std::time::SystemTime::now()
    }

    // Go: cmd/tsgo/sys.go:osSys.Writer (os.Stdout)
    fn write(&self, s: &str) {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(s.as_bytes());
    }

    // Go: cmd/tsgo/sys.go:osSys.WriteOutputIsTTY
    fn write_output_is_tty(&self) -> bool {
        std::io::stdout().is_terminal()
    }

    // Go: cmd/tsgo/sys.go:osSys.GetEnvironmentVariable
    fn get_environment_variable(&self, name: &str) -> String {
        std::env::var(name).unwrap_or_default()
    }
}

/// Dispatches an already-argv0-stripped command line, mirroring Go's `runMain`.
// Go: cmd/tsgo/main.go:runMain
fn run(args: &[String], sys: &dyn System) -> ExitStatus {
    tsgo_core::apply_debug_stack_limit();
    if let Some(first) = args.first() {
        match first.as_str() {
            "--lsp" => return run_lsp(&args[1..], sys),
            "--api" => return run_api(&args[1..], sys),
            _ => {}
        }
    }
    command_line(sys, args)
}

/// A [`ParseConfigHost`] adapter over a [`System`], so the command line can be
/// parsed (to detect `--version`/`--help` and feed the build) using the
/// system's file system and working directory. Mirrors `tsgo_execute`'s own
/// internal `SysParseConfigHost`.
// Go: internal/execute/tsc.go:CommandLine (tsoptions.ParseCommandLine host)
struct SysParseConfigHost {
    fs: Arc<dyn Fs + Send + Sync>,
    current_directory: String,
}

impl ParseConfigHost for SysParseConfigHost {
    fn fs(&self) -> &dyn Fs {
        self.fs.as_ref()
    }
    fn get_current_directory(&self) -> &str {
        &self.current_directory
    }
}

/// Parses the command line and runs the reachable `tsc` driver: `--version`
/// prints the compiler version, otherwise the build/check/emit path runs.
///
/// Mirrors the reachable subset of Go's `execute.CommandLine` + the
/// pre-build branches of `tscCompilation`. DIVERGENCE(port): Go handles
/// `--version`/`--help` inside `internal/execute/tsc.go:tscCompilation`, but the
/// committed `tsgo_execute` defers them (see its crate docs), so the binary
/// bridges them here, ahead of delegating the build to
/// [`tsgo_execute::tsc_compilation`]. Like Go, command-line errors take
/// precedence over `--version`/`--help` (they are reported by the build path,
/// exiting 2). blocked-by: the `--version`/`--help`/`--init`/`--showConfig`
/// branch of `tsgo_execute` (a later execute chunk).
// Go: internal/execute/tsc.go:CommandLine / tscCompilation
fn command_line(sys: &dyn System, args: &[String]) -> ExitStatus {
    let host = SysParseConfigHost {
        fs: sys.fs(),
        current_directory: sys.get_current_directory().to_string(),
    };
    let parsed = parse_command_line(args, &host);
    if parsed.errors().is_empty() {
        let options = parsed.compiler_options();
        if options.version.is_true() {
            // Go: tscCompilation -> CompilerOptions().Version.IsTrue() -> PrintVersion
            print_version(sys);
            return ExitStatus::Success;
        }
        if options.help.is_true() || options.all.is_true() {
            // Go: tscCompilation -> Help/All.IsTrue() -> PrintHelp
            return print_help_deferred(sys);
        }
    }
    // Go: tscCompilation -> performCompilation (the reachable single-project
    // build/check/emit path). `tsc_compilation` re-reports any command-line
    // errors (exit 2) and otherwise builds the program.
    tsgo_execute::tsc_compilation(sys, parsed).status
}

/// Prints the compiler version line (`Version <x>`), matching Go's
/// `tsc.PrintVersion` (the localized `Version_0` message followed by a newline).
// Go: internal/execute/tsc/help.go:PrintVersion
fn print_version(sys: &dyn System) {
    let locale = tsgo_locale::parse("en").expect("en locale is always available");
    let line = tsgo_diagnostics::VERSION_0.localize(&locale, &[tsgo_core::version::version()]);
    sys.write(&line);
    sys.write("\n");
}

/// Prints the version header and a clear deferral notice for `--help`/`-h`.
///
/// DEFER: the full `--help` output (the command list + every compiler option,
/// produced by Go's `tsc.PrintHelp` + the `tsoptions` help machinery) is not
/// exposed by the committed `tsgo_execute`, so only the reachable version
/// header is printed. Exit code matches Go's `--help` (0). blocked-by: the help
/// generator (`internal/execute/tsc/help.go` + `tsoptions` option declarations),
/// a later chunk.
// Go: internal/execute/tsc/help.go:PrintHelp
fn print_help_deferred(sys: &dyn System) -> ExitStatus {
    print_version(sys);
    sys.write(
        "tsgo: full `--help` output is not yet implemented in this build (the help generator is deferred).\n",
    );
    ExitStatus::Success
}

/// DEFER(P8 lsp): the `--lsp` language server. blocked-by: the `internal/lsp`
/// port (P8). Go's `runLSP` builds an `lsp.Server` over stdio and runs it; that
/// server is not yet ported, so this routes to a clear stub instead of
/// crashing. Mirrors Go's arg0 (`--lsp`) dispatch in `runMain`.
// Go: cmd/tsgo/lsp.go:runLSP
fn run_lsp(_args: &[String], sys: &dyn System) -> ExitStatus {
    sys.write("tsgo: the `--lsp` language server is not yet implemented in this build (deferred to phase 8).\n");
    ExitStatus::NotImplemented
}

/// DEFER(P8 api): the `--api` server. blocked-by: the `internal/api` port (P8).
/// Go's `runAPI` builds an `api.StdioServer` and runs it; that server is not yet
/// ported, so this routes to a clear stub instead of crashing. Mirrors Go's
/// arg0 (`--api`) dispatch in `runMain`.
// Go: cmd/tsgo/api.go:runAPI
fn run_api(_args: &[String], sys: &dyn System) -> ExitStatus {
    sys.write(
        "tsgo: the `--api` server is not yet implemented in this build (deferred to phase 8).\n",
    );
    ExitStatus::NotImplemented
}

// Go: cmd/tsgo/main.go:main
fn main() {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(tsgo_bundled::wrap_fs(tsgo_vfs::osvfs::fs()));
    let cwd = match std::env::current_dir() {
        Ok(dir) => tsgo_tspath::normalize_path(&dir.to_string_lossy()),
        Err(err) => {
            eprintln!("Error getting current directory: {err}");
            std::process::exit(ExitStatus::InvalidProjectOutputsSkipped as i32);
        }
    };
    let sys = OsSystem {
        fs,
        current_directory: cwd,
        default_library_path: tsgo_bundled::lib_path(),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let status = run(&args, &sys);
    std::process::exit(status as i32);
}

#[cfg(test)]
#[path = "main_test.rs"]
mod tests;
