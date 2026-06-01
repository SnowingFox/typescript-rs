# P9 execute · Watch mode — `tsc --watch`/`-w` loop

> Code-round worklog (companion to `impl.md`/`tests.md`). This round delivers
> `tsgo_execute`'s **watch mode**: the `--watch`/`-w` dispatch + the watch loop
> (initial build, then a rebuild + status report per file change), 1:1 with the
> reachable subset of Go `internal/execute/watcher.go`. Strict TDD red→green.
> **Edited ONLY `internal/execute/**`.** Did not touch the root `Cargo.toml`,
> `internal/ls/**`, or any other crate's source (concurrent lane active).

## Scope (what landed)

1. **`--watch`/`-w` dispatch** (`lib.rs:tsc_compilation`): detects watch mode and
   routes to `watch::perform_watch` instead of the one-shot `perform_compilation`.
   Also ports the reachable `watch + listFilesOnly` guard (TS6370). Mirrors Go
   `tscCompilation`'s watch branch.
2. **Watch loop** (`watch.rs`): `perform_watch` → `Watcher::{start, do_cycle,
   do_build}`. `start` reports the watch-start status + runs the initial build,
   then loops over the testable change seam; `do_build` builds a fresh program
   each cycle, emits + reports diagnostics (reusing `emit_and_report_statistics`),
   and reports the trailing "Found N error(s). Watching…" status. Mirrors Go
   `Watcher.start`/`DoCycle`/`doBuild`.
3. **Status reporting** (`tsc/diagnostics.rs`): `WatchStatusReporter` +
   `create_watch_status_reporter`, mirroring Go `CreateWatchStatusReporter`
   (time-stamped status line in plain or pretty form, written in both modes).
   The shared `HH:MM:SS PM` time formatter was consolidated here (the
   `--build` orchestrator now reuses it; behaviour unchanged).
4. **Testable watch seam** (`sys.rs:System::wait_for_change`): a new **defaulted**
   trait method (returns `false`) — the reachable, unit-testable stand-in for
   Go's `vfswatch.FileWatcher` poll loop. Tests drive a finite change sequence;
   production stubs it (one build then exit). Defaulted ⇒ additive (see below).

## Real Go watch-status ground truth (captured from `internal/diagnostics`)

| code | message text |
|---|---|
| 6031 `Starting_compilation_in_watch_mode` | `Starting compilation in watch mode...` |
| 6032 `File_change_detected_Starting_incremental_compilation` | `File change detected. Starting incremental compilation...` |
| 6193 `Found_1_error_Watching_for_file_changes` | `Found 1 error. Watching for file changes.` |
| 6194 `Found_0_errors_Watching_for_file_changes` | `Found {0} errors. Watching for file changes.` |

Go's `doBuild` uses `errorCount := len(result.Diagnostics)`; `== 1` → 6193 (no
placeholder), every other count → 6194 with `{0} = errorCount` (so "Found 0
errors", "Found 2 errors", …). Ported exactly.

### Built `cmd/tsgo` and ran real `tsc --watch` (feasible — `cmd/tsgo` deps are `tsgo_execute` + leaves, NOT `internal/ls`)

`cargo build -p tsgo` (green: proves the defaulted `System` method keeps
`OsSystem` compiling), then `NO_COLOR=1 tsgo --watch index.ts`:

```
# clean file (const x: number = 1;)
06:30:12 PM - Starting compilation in watch mode...

06:30:12 PM - Found 0 errors. Watching for file changes.

# emits index.js, exit 0
```

```
# type-error file (const x: number = "s";)
06:30:19 PM - Starting compilation in watch mode...

index.ts(1,6): error TS2322: Type 'string' is not assignable to type 'number'.
06:30:19 PM - Found 1 error. Watching for file changes.
```

Message text matches Go exactly. (Timestamp differs per-run: `OsSystem.now()` is
the real wall clock; the VFS test clock is deterministic. The `(1,6)` column is
the pre-existing `tsgo_checker` span divergence — Go reports `(1,7)` — noted in
the prior round; out of scope here.) With the defaulted `wait_for_change`
(returns `false`), the production binary does one build then exits — the real OS
file-watcher backend is DEFER, so `tsgo --watch` does not hang.

## TDD red→green (vertical slices, observed symptoms)

- **Slice 1 — initial build in watch mode** (`initial_build_in_watch_mode_reports_status_then_exits`):
  drove `perform_watch` with a zero-change fake sys. **RED**: stub `perform_watch`
  printed nothing → `missing watch-start status: ""`. **GREEN**: implemented the
  `Watcher` (start → report 6031 → do_build → report 6194) → one build, both
  status lines, `index.js` emitted, exit Success.
- **Slice 2 — one change cycle** (`one_change_drives_a_second_build_cycle`):
  fake sys queues one edit → one extra cycle. **RED** (temporarily disabled the
  `while wait_for_change { do_cycle }` loop): only the initial build ran →
  `missing change-cycle status: "12:00:01 AM - Starting…\n\n12:00:02 AM - Found 0
  errors…\n\n"`. **GREEN** (restored loop): two builds, exactly one "File change
  detected…" line.
- **Slice 3 — error→fix cycle** (`error_then_fix_cycle_reports_one_then_zero_errors`):
  build 1 errors (TS2322 + "Found 1 error…"), the queued edit fixes the file,
  build 2 is clean ("Found 0 errors…"). **RED** (temporarily forced the `{0}
  errors` branch via `if false && …`): printed the ungrammatical `Found 1 errors.
  Watching…` → `missing 'Found 1 error' status`. **GREEN** (restored the
  `error_count == 1` special-case): "Found 1 error." then "Found 0 errors.", in
  order, with the TS2322 line in between.
- **Slice 4 — dispatch** (`watch_flag_dispatches_to_watch_loop`,
  `watch_short_flag_dispatches_to_watch_loop`,
  `watch_with_list_files_only_reports_ts6370_and_exits_two`): `execute(sys,
  ["--watch", …])` / `["-w", …]` route through `tsc_compilation` to the loop and
  print the watch-start status (a plain `VfsSystem` reports no changes → one
  build then exit); `--watch --listFilesOnly` reports `error TS6370: Options
  'watch' and 'listFilesOnly' cannot be combined.` and exits 2 with no build.
- **Slice 4 (System additivity)**: `cargo build -p tsgo_execute` green proves the
  pre-existing `VfsSystem` (and, via `cargo build -p tsgo`, `cmd/tsgo`'s
  `OsSystem`) still compile without implementing `wait_for_change`.

## Go function mapping (`// Go:` anchors)

| Rust | Go |
|---|---|
| `watch.rs:perform_watch` | `internal/execute/tsc.go:tscCompilation` (watch branch) + `watcher.go:Watcher.start` |
| `watch.rs:Watcher` | `internal/execute/watcher.go:Watcher` |
| `watch.rs:Watcher::start` | `internal/execute/watcher.go:Watcher.start` |
| `watch.rs:Watcher::do_cycle` | `internal/execute/watcher.go:Watcher.DoCycle` |
| `watch.rs:Watcher::do_build` | `internal/execute/watcher.go:Watcher.doBuild` |
| `watch.rs:Watcher::report_watch_status` | `internal/execute/watcher.go:Watcher.reportWatchStatus` |
| `tsc/diagnostics.rs:WatchStatusReporter` / `create_watch_status_reporter` | `internal/execute/tsc/diagnostics.go:CreateWatchStatusReporter` |
| `tsc/diagnostics.rs:format_status_time` | `…/diagnostics.go:CreateWatchStatusReporter` (`sys.Now().Format("03:04:05 PM")`) |
| `tsc/diagnostics.rs:ReportedDiagnostic::from_compiler_message` | `internal/ast/diagnostic.go:NewCompilerDiagnostic` |
| `sys.rs:System::wait_for_change` | `internal/vfs/vfswatch/filewatcher.go:FileWatcher.Run` (testable seam) |
| `lib.rs:tsc_compilation` (watch dispatch + listFilesOnly guard) | `internal/execute/tsc.go:tscCompilation` |

## Public API (additive only, within `tsgo_execute`)

- New: `perform_watch`, `WatchStatusReporter`, `create_watch_status_reporter`,
  `ReportedDiagnostic::from_compiler_message`, and the **defaulted** trait method
  `System::wait_for_change`. Re-exported from the crate root.
- **`System` change is ADDITIVE**: `wait_for_change` has a default impl
  (`-> false`), so every existing `System` implementor compiles unchanged. This
  deliberately avoids the downstream break that occurred when `now()` was added
  without a default. Verified: `VfsSystem` (this crate) and `cmd/tsgo`'s
  `OsSystem` both compile with no edit.
- No existing test was weakened or deleted. Internal refactor: the `--build`
  orchestrator's private `format_status_time` was consolidated into
  `tsc/diagnostics.rs` (identical body) and reused; the build tests stay green.

## Test deltas

`cargo test -p tsgo_execute`: **33 → 39** unit tests (+6: 3 watch-loop in
`watch_test.rs`, 3 dispatch in `lib_test.rs`), **4 doctests** unchanged. Only
more tests than Go (Go's watch behaviour is covered by P10 tsctests baselines).

## Gate results (crate-scoped, all GREEN)

- `cargo test -p tsgo_execute`: **39 passed; 0 failed** + **4 doctests passed**.
- `cargo clippy -p tsgo_execute --all-targets -- -D warnings`: **clean**.
- `cargo fmt -p tsgo_execute -- --check`: **clean**.
- `cargo build -p tsgo_execute`: **Finished**.
- (verification, not a formal gate) `cargo build -p tsgo`: **Finished** — proves
  the defaulted `System` method keeps `cmd/tsgo`'s `OsSystem` compiling. Did NOT
  run `--workspace` (concurrent `internal/ls/**` lane); `cmd/tsgo` does not
  depend on `internal/ls`, so building it does not compile the other lane.

## DEFER (blocked-by)

- **Real OS file-watching backend** (`vfswatch.FileWatcher`: poll interval,
  watched-file/wildcard-directory state, debounce, `TS_WATCH_DEBUG`). blocked-by:
  the `internal/vfs/vfswatch` package is not yet ported. The production
  `wait_for_change` default is a no-op stub (one build then exit).
- **Ctrl-C / signal handling** that ends the production loop. blocked-by: signal
  handling.
- **`--watch -b`** (build-mode watch with downstream project tracking).
  blocked-by: the `--build` watch chunk.
- **Incremental skip-emit reuse across cycles** (Go reuses the prior
  `incremental.Program` + an mtime-keyed source-file cache). blocked-by: more of
  P6-9b; each cycle here rebuilds the program from the config.
- **`recheckTsConfig`** (re-reading a changed `tsconfig.json` mid-watch).
  blocked-by: `tsconfig.json` discovery in `tsc_compilation` (already DEFER).
- **Pretty watch UI**: `diagnosticwriter.TryClearScreen` clear-screen before each
  fresh compilation, and the `CommandLineTesting` `OnWatchStatusReport*`/
  `OnProgram` hooks. blocked-by: the pretty watch UI + testing-hook chunk.
- **`--watchOptions` / `watchFile` / `watchDirectory` polling strategies**.
  blocked-by: `vfswatch` + the watch-options chunk.

## New / changed files (only `internal/execute/**` + this doc)

- **New** `watch.rs` + `watch_test.rs`: the watch loop + 3 loop tests (+ the
  finite `WatchTestSystem` fake driving changes via `wait_for_change`).
- `sys.rs`: `System::wait_for_change` defaulted trait method.
- `tsc/diagnostics.rs`: `WatchStatusReporter`, `create_watch_status_reporter`,
  `ReportedDiagnostic::from_compiler_message`, shared `format_status_time`.
- `tsc/mod.rs`: re-export the new public items + `pub(crate)` `format_status_time`.
- `lib.rs` + `lib_test.rs`: watch dispatch + listFilesOnly guard + 3 dispatch tests.
- `build/orchestrator.rs`: reuse the shared `format_status_time` (removed its
  local duplicate; behaviour unchanged).
- Did **not** touch the root `Cargo.toml` (`Cargo.lock` is auto-updated by cargo).
