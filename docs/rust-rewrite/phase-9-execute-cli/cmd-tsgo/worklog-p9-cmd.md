# P9 cmd/tsgo · worklog (p9-cmd) — the runnable `tsgo` CLI binary

> Code-round worklog (companion to `impl.md`/`tests.md`). This round delivers the
> **runnable `tsgo` bin crate**: argv dispatch into the committed `tsgo_execute`
> driver, a real-filesystem `System`, exit-code propagation, `--version`/`--help`
> recognition, and `--lsp`/`--api` deferral stubs — a 1:1 port of the reachable
> subset of Go `cmd/tsgo/main.go` (+ `sys.go`). Strict TDD (Go ground truth
> captured first, then vertical red→green slices). **Edited only `cmd/tsgo/**`**
> (+ this doc). Did **not** touch the root `Cargo.toml`, `internal/execute/**`,
> `internal/format/**`, or any other crate.

## Scope (what this round did)

The `tsgo` binary's argv → `tsc` driver path:

1. `main()`: reads `std::env::args`, builds a real-filesystem `OsSystem`
   (`tsgo_bundled::wrap_fs(tsgo_vfs::osvfs::fs())` so `bundled:///libs` resolves,
   `tsgo_bundled::lib_path()` as the default library path, normalized cwd, output
   to real stdout), calls `run(args, &sys)`, and `std::process::exit(status as i32)`.
   Mirrors Go `main` + `newSystem`.
2. `run(args, sys) -> ExitStatus` (testable inner entry, mirrors Go `runMain`):
   `apply_debug_stack_limit()`, then arg0 dispatch — `--lsp` → `run_lsp`, `--api`
   → `run_api` — otherwise `command_line(sys, args)`.
3. `command_line(sys, args)`: parses the command line (`tsgo_tsoptions::parse_command_line`
   over a `System`-backed `ParseConfigHost`); if there are **no** command-line
   errors and `--version` is set → `print_version` + exit 0; if `--help`/`--all`
   → deferred help stub + exit 0; otherwise delegates the build/check/emit to the
   committed `tsgo_execute::tsc_compilation` (which re-reports command-line errors
   → exit 2, or builds the program). Exit codes (0/1/2) propagate verbatim.
4. `run_lsp`/`run_api`: arg0-recognized "not yet implemented" stubs (P8) — clear
   message + `ExitStatus::NotImplemented` (5), no crash.

## Go ground truth (measured against the real Go `cmd/tsgo`, `NO_COLOR=1`, piped = non-TTY = plain)

`go build -o /tmp/tsgo-go ./cmd/tsgo`; `good.ts` = `const x: number = 1;`,
`bad.ts` = `const x: number = "s";`; cwd in the project dir; stdin `</dev/null`.

| case | argv | exit | stdout | stderr | artifacts |
|---|---|---|---|---|---|
| version (long) | `--version` | **0** | `Version 7.0.0-dev\n` | — | — |
| version (short) | `-v` | **0** | `Version 7.0.0-dev\n` | — | — |
| help (long) | `--help` | **0** | full `tsc: The TypeScript Compiler - Version 7.0.0-dev` help (commands + every option) | — | — |
| help (short) | `-h` | **0** | same full help | — | — |
| clean | `good.ts` | **0** | (empty) | — | `good.js` = `"use strict";\nconst x = 1;\n` |
| type error | `bad.ts` | **1** | `bad.ts(1,7): error TS2322: Type 'string' is not assignable to type 'number'.\n` | — | `bad.js` (still emitted) |
| noEmit error | `--noEmit bad.ts` | **2** | same TS2322 line | — | no `.js` |
| noEmit clean | `--noEmit good.ts` | **0** | (empty) | — | no `.js` |
| unknown option | `--badOption good.ts` | **2** | `error TS5023: Unknown compiler option '--badOption'.\n` | — | no `.js` |
| version + bad option | `--version --badOption` | **2** | `error TS5023: Unknown compiler option '--badOption'.\n` | — | — |
| version after file | `good.ts --version` | **0** | `Version 7.0.0-dev\n` | — | no `.js` |
| lsp (no `--stdio`) | `--lsp` | **1** | — | `only stdio is supported\n` | — |
| lsp (stdio, EOF) | `--lsp --stdio` | **0** | — | — | (server runs, EOF, clean exit) |
| api (EOF) | `--api` | **0** | — | — | (server runs, EOF, clean exit) |

**Key precedence facts (verified):** command-line errors are reported **before**
the `--version`/`--help` branches (Go `tscCompilation` checks `len(Errors) > 0`
first), so `--version --badOption` exits 2 and prints TS5023 (not the version).
`--version`/`-v`/`--help`/`-h` are parsed *options* (detected anywhere on the
line), while `--lsp`/`--api` are arg0-only tokens (`runMain` switches on
`args[0]`).

## Rust reality (real `tsgo` binary, measured) and the recorded divergences

Built `cargo build -p tsgo` → `target/debug/tsgo`, run against the same temp files
(`NO_COLOR=1`, `</dev/null`):

| case | Rust exit | Rust stdout/stderr | vs Go |
|---|---|---|---|
| `--version` / `-v` | 0 | `Version 7.0.0-dev` | ✅ exact |
| `--help` / `-h` | 0 | `Version 7.0.0-dev\n` + deferral notice | header ✅; full option table DEFERRED (see below) |
| `--lsp [--stdio]` | 5 | `tsgo: the \`--lsp\` … not yet implemented … (deferred to phase 8).` | stub (Go runs the real server — DEFER P8) |
| `--api` | 5 | `tsgo: the \`--api\` … not yet implemented … (deferred to phase 8).` | stub (DEFER P8) |
| `good.ts` (clean) | **101 (panic)** | `good.js` written, then panic | ⚠️ **downstream defect, see below** |
| `bad.ts` | **101 (panic)** | TS2322 line printed, then panic | ⚠️ downstream defect |
| `--noEmit bad.ts` | **101 (panic)** | TS2322 line printed, then panic | ⚠️ downstream defect |

### Downstream defect: real-libs compile path panics (out-of-scope crates)

When the real bundled libs are loaded (the production wiring: `default_library_path
= bundled:///libs`), the actual **compile** path panics:

```
thread 'main' panicked at internal/diagnosticwriter/lib.rs:125:43:
byte index 1029 is out of bounds of `const x: number = 1;\n`
stack: tsc_compilation -> perform_compilation -> emit_and_report_statistics
    -> emit_files_and_report_errors -> DiagnosticReporter::report
    -> write_format_diagnostic -> get_ecma_line_and_utf16_character_of_position
```

Root cause (entirely in committed, out-of-scope crates — **not** `cmd/tsgo`):
the port's checker, once the real `lib.*.d.ts` are loaded, produces a semantic
diagnostic whose span (`pos = 1029`) lives in a **lib file**; `tsgo_execute`'s
**documented** single-file semantic-diagnostic attribution (it has no file
back-pointer on `tsgo_checker::Diagnostic`, so it attributes semantic diagnostics
to `config.file_names()[0]` — see `execute/worklog-p9-river-a.md` divergence #3)
renders that diagnostic against the 21-byte root file → out-of-bounds slice in
`tsgo_diagnosticwriter`. This is **latent in `tsgo_execute`**: its own tests use a
fake `/lib` with **no** real libs, so the checker never loads libs and the
attribution bug never fires. The first real CLI to wire real bundled libs (this
binary, exactly as Go's `newSystem` does) is what surfaces it.

- blocked-by: `tsgo_checker::Diagnostic` lacking a file back-pointer +
  `tsgo_execute`'s single-file attribution (`internal/execute/tsc/emit.rs`,
  `internal/diagnosticwriter/lib.rs`) — all committed, **must not edit** (scope +
  parallel-safety with the concurrent `internal/format/**` lane). Possibly also a
  `tsgo_checker` completeness gap (Go emits **no** diagnostic for `good.ts`).
- `cmd/tsgo` wiring is correct and 1:1 with Go (`newSystem` → bundled-wrapped
  osvfs + `lib_path()` + normalized cwd + stdout writer → `execute` driver). The
  binary's dispatch, real-FS `System`, exit-code propagation, and
  `--version`/`--help`/`--lsp`/`--api` paths all work end-to-end on the real
  binary; only the underlying `execute` real-libs compile path is blocked.

The unit tests below run in the **same in-memory regime as the committed
`tsgo_execute` tests** (`VfsSystem` + `MapFs` + fake `/lib`), so they exercise and
prove the binary's dispatch/wiring/exit-code logic deterministically without the
downstream defect — exactly mirroring how `tsgo_execute` itself is validated.

## TDD red→green (vertical slices, one behavior at a time)

Bootstrapped with a placeholder `command_line` (→ `NotImplemented`) and placeholder
`run_lsp`/`run_api` (→ `Success`) so each new behavior had a genuine RED first.

- **Slice 1 — `--version`** (`version_flag_prints_version_and_exits_zero`):
  - RED: `run(["--version"])` → placeholder `NotImplemented` (`left: NotImplemented, right: Success`).
  - GREEN: parse command line, `errors().is_empty() && options.version.is_true()` →
    `print_version` (localized `VERSION_0` + `\n`) → `Success`. Output exactly
    `Version 7.0.0-dev\n`.
- **Slice 2 — `good.ts`** delegation tracer (`good_file_compiles_clean_exits_zero_and_emits_js`):
  - RED: non-version default path still `NotImplemented`, no `.js` (`left: NotImplemented, right: Success`).
  - GREEN: delegate the build to `tsgo_execute::tsc_compilation(sys, parsed)` →
    `Success`, `/p/good.js` written containing `const x = 1`.
- **Slice 3 — `bad.ts`** (`bad_file_reports_ts2322_and_exits_one`): exit
  `DiagnosticsPresentOutputsGenerated` (1), output `bad.ts(1,6): error TS2322: …`
  (Rust column 6 vs Go 7 — documented `tsgo_checker` span divergence, out of
  scope), `.js` still emitted. Green via the slice-2 delegation.
- **Slice 4 — `--noEmit bad.ts`** (`no_emit_bad_file_exits_two_without_writing_js`):
  exit `DiagnosticsPresentOutputsSkipped` (2), TS2322 line, no `.js`. Green via
  delegation.
- **Slice 5 — deferred modes** (`lsp_mode_is_deferred_with_clear_message`,
  `api_mode_is_deferred_with_clear_message`):
  - RED: placeholder stubs returned `Success` with no message (`left: Success, right: NotImplemented`).
  - GREEN: `run_lsp`/`run_api` write a clear "not yet implemented … (deferred to
    phase 8)." message and return `NotImplemented` (5).
- **Slice 6 — `--help`/`-h`** (`help_flag_prints_version_header_and_is_deferred`,
  `short_help_flag_is_deferred`):
  - RED: with no help branch, `--help` fell through to `tsc_compilation` (empty
    program, no output) — version-header assertion failed (`missing version: ""`).
  - GREEN: added the `help.is_true() || all.is_true()` branch → deferred help stub
    (prints the version header + a clear deferral notice, exit 0 matching Go).
  - (`-v` was already green: `tsoptions` parses the `-v`/`-h` short aliases natively.)

Additional behavior-level tests (beyond Go — Go's `cmd/tsgo` has **no** tests):
`short_version_flag_prints_version` (`-v`), `unknown_option_reports_ts5023_and_exits_two`
(command-line-error delegation), `command_line_errors_take_precedence_over_version`
(`--version --badOption` → exit 2, no version — Go precedence),
`version_detected_after_file_name_skips_build` (`good.ts --version` → version, exit
0, no emit), `lsp_mode_dispatch_is_arg0_only` (`good.ts --lsp` → not the LSP stub),
and `version_works_over_real_bundled_file_system` (drives `run` over the exact
production wiring: `tsgo_bundled::wrap_fs(osvfs)` + `lib_path()`).

## Go functions mirrored (`// Go:` anchors)

| Rust (`cmd/tsgo/main.rs`) | Go |
|---|---|
| `main` | `cmd/tsgo/main.go:main` |
| `run` | `cmd/tsgo/main.go:runMain` |
| `command_line` | `internal/execute/tsc.go:CommandLine` / `tscCompilation` (reachable subset) |
| `print_version` | `internal/execute/tsc/help.go:PrintVersion` |
| `print_help_deferred` | `internal/execute/tsc/help.go:PrintHelp` (deferred) |
| `run_lsp` | `cmd/tsgo/lsp.go:runLSP` (DEFER P8) |
| `run_api` | `cmd/tsgo/api.go:runAPI` (DEFER P8) |
| `OsSystem` (+ `System` impl) | `cmd/tsgo/sys.go:osSys` (reachable subset) |
| `OsSystem.fs/default_library_path/get_current_directory/write/write_output_is_tty/get_environment_variable` | `sys.go:osSys.FS/DefaultLibraryPath/GetCurrentDirectory/Writer/WriteOutputIsTTY/GetEnvironmentVariable` |
| `SysParseConfigHost` | `internal/execute/tsc.go:CommandLine` (the `tsoptions.ParseCommandLine` host) |

## Divergences from Go (documented)

1. **`--version`/`--help` bridged in `cmd/tsgo`.** Go handles them inside
   `internal/execute/tsc.go:tscCompilation`, but the committed `tsgo_execute`
   **defers** version/help/init/showConfig (see its crate docs), so the binary
   bridges them ahead of delegating the build. Precedence is preserved
   (command-line errors are still reported by the build path → exit 2 — verified by
   `command_line_errors_take_precedence_over_version`). blocked-by: the
   version/help branch of a later `tsgo_execute` chunk.
2. **`--help` is a stub** (version header + deferral notice, exit 0). The full help
   text (command list + every option) needs `tsc.PrintHelp` + the `tsoptions` help
   machinery, which `tsgo_execute` does not expose. blocked-by: the help generator
   (a later chunk).
3. **`--lsp`/`--api` stub message goes to stdout** (via `sys.write`, testable)
   rather than Go's stderr. These are full P8 servers in Go; the stream is moot for
   a deferral stub.
4. **TS2322 column 6 vs Go 7** and **emit missing `"use strict";`** — both inherited
   from out-of-scope crates (`tsgo_checker` span / `tsgo_compiler` emitter), already
   documented in `execute/worklog-p9-river-a.md`. Tests assert Rust reality + note
   the Go truth.
5. **Process-level facets not yet wired** (see DEFER) — signals, the parent-process
   watchdog, Windows VT processing, terminal width.

## Files added (only `cmd/tsgo/**` + this doc)

- `cmd/tsgo/Cargo.toml`: added `[dependencies]` (path deps:
  `tsgo_{bundled,core,diagnostics,execute,locale,tsoptions,tspath,vfs}`). **Did not
  touch the root `Cargo.toml`** (the bin was already registered). `Cargo.lock` is
  updated automatically by cargo.
- `cmd/tsgo/main.rs`: `OsSystem` (real-FS `System`), `run` (argv dispatch),
  `command_line` (version/help bridge + build delegation), `print_version`,
  `print_help_deferred`, `run_lsp`/`run_api` stubs, `SysParseConfigHost`, `main`.
- `cmd/tsgo/main_test.rs`: 14 behavior-level unit tests (`#[path]`-included into the
  bin crate's `#[cfg(test)] mod tests`).

## Gate results (crate-scoped, all GREEN)

- `cargo test -p tsgo`: **14 passed; 0 failed** (no doctests — bin crate).
- `cargo clippy -p tsgo --all-targets -- -D warnings`: **clean**.
- `cargo fmt -p tsgo -- --check`: **clean**.
- `cargo build -p tsgo`: **Finished** → runnable `target/debug/tsgo` (21.7 MB).

> Gated only with `-p tsgo` throughout (never `--workspace`), avoiding the
> concurrent `internal/format/**` lane. `tsgo`'s dependency graph does not include
> `tsgo_format`, so the gates never compile the other lane's in-flight crate.

## Public API

`cmd/tsgo` is a **bin crate** with no exported library API; all items
(`OsSystem`, `run`, `command_line`, `print_version`, …) are crate-private and
unit-tested via the in-crate `#[cfg(test)]` module. No existing test was weakened
or deleted; no other crate's source was modified.

## Milestone

This completes the **runnable single-project `tsgo` tsc CLI** wiring: a real
binary that dispatches argv, builds a real-filesystem `System`, recognizes
`--version`/`-v`/`--help`/`-h`, routes `--lsp`/`--api` to clear P8 stubs, and
propagates `tsgo_execute`'s exit codes (0/1/2/5). The `--version`/`--help`/`--lsp`/
`--api` paths and the in-memory compile/error paths are validated green; the
real-binary **compile** path is blocked by a documented downstream `tsgo_execute`/
`tsgo_diagnosticwriter` defect that surfaces only once real bundled libs load
(see above) — to be unblocked by a later execute/checker chunk, not editable here.

## DEFER (blocked-by)

- `--lsp` language server — blocked-by: `internal/lsp` port (P8).
- `--api` server — blocked-by: `internal/api` port (P8).
- `--build`/`-b` (build-mode orchestration) — blocked-by: the `tsgo_execute` build
  orchestrator chunk.
- `--watch`/`-w` — blocked-by: the p9-watcher chunk.
- Full `--help` option table, `--init`, `--showConfig`, `--locale` — blocked-by: the
  help generator + those `tsgo_execute` branches.
- Real-libs compile path (panic) — blocked-by: `tsgo_execute` semantic-diagnostic
  file attribution + `tsgo_checker::Diagnostic` file back-pointer (committed,
  out-of-scope).
- Process-level facets: SIGINT/SIGTERM handling, the parent-process watchdog
  (`isprocessalive_*`), Windows VT processing (`enablevtprocessing_windows`),
  terminal width (`GetWidthOfTerminal`), pprof — blocked-by: P8 servers (only used
  by lsp/api) + a later facets chunk.
