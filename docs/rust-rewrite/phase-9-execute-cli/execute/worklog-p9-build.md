# P9 execute · `--build` mode — `tsc -b` project-reference orchestration

> Code-round worklog. This round wires the **`--build`/`-b`** orchestrator into
> `tsgo_execute`: resolve a project-reference graph, compute its topological
> build order, and build each out-of-date project in turn (skipping up-to-date
> ones), reusing the compiler's P6-9a project-reference API + `tsgo_incremental`
> (P6-9b) `.tsbuildinfo` machinery + the single-project compile/emit path.
> Strict TDD (capture real `tsc -b` ground truth first, then red→green per
> slice). **Edited only `internal/execute/**`** (+ this doc). No root
> `Cargo.toml` edit; new deps added to `internal/execute/Cargo.toml` only.

## Scope (what landed)

`execute(sys, args)` now dispatches `-b`/`--b`/`-build`/`--build` (case-insensitive,
matching Go `CommandLine`) to `build::perform_build`:

1. **`--build` dispatch** — `execute` matches the first arg and parses a
   `ParsedBuildCommandLine` (`tsgo_tsoptions::parse_build_command_line`, the `-b`
   flag consumed by the build-option parser, the rest become projects). Go:
   `internal/execute/tsc.go:CommandLine`.
2. **`perform_build`** — reports up-front command-line errors (exit 2), else
   builds the `Orchestrator` and runs it. Go: `tscBuildCompilation`.
3. **Graph + order** — for each requested project, `get_resolved_project_reference`
   + `resolve_project_references` (P6-9a) then `get_build_order` (topo post-order
   + `TS6202` cycle detection). Orders across multiple roots are merged deduped by
   canonical `Path`. Go: `Orchestrator.GenerateGraph` / `setupBuildTask`.
4. **Per-project build loop** — in topological order: compute up-to-date status,
   report it (verbose), then either skip or build. Building reuses
   `emit_and_report_statistics` (compiler `Program` + emit + diagnostics) and
   writes the `.tsbuildinfo` via `tsgo_incremental::Program::emit_build_info`.
   Per-project diagnostics print in order (sequential build = Go's report order,
   so no per-task output buffering is needed). Go: `buildOrClean` /
   `buildProject` / `compileAndEmit`.
5. **Up-to-date skip** — `tsgo_incremental::get_up_to_date_status` over the
   project's inputs (mtime + text) and `.tsbuildinfo` (presence + mtime + version);
   `--force` rebuilds unconditionally; `--dry` reports the plan and builds
   nothing. Go: `getUpToDateStatus` / `handleStatusThatDoesntRequireBuild`.
6. **Exit code** — the worst per-project status (`Success` 0 / `…OutputsGenerated`
   1 / `…OutputsSkipped` 2), `ProjectReferenceCycleOutputsSkipped` (4) on a cycle.
   Go: `report` (`exitStatus > Status` max) + `ExitStatus` iota.

## Go ground truth (real `cmd/tsgo -b`, `go build -o /tmp/tsgo ./cmd/tsgo`)

Composite fixture: project `b` (`composite:true`, `files:[index.ts]`) referenced
by `a` (`composite:true`, `files:[index.ts]`, `references:[{path:"../b"}]`), run
with cwd at the solution root.

| case | argv | exit | stdout | outputs |
|---|---|---|---|---|
| clean build | `-b a` | **0** | (empty) | both `b/` and `a/` get `index.js`, `index.d.ts`, `tsconfig.tsbuildinfo`; B built before A |
| up-to-date no-op | `-b a` (2nd run) | **0** | (empty) | nothing re-emitted |
| force | `-b a --force` | **0** | (empty) | all projects rebuilt |
| dry (fresh) | `-b a --dry` | **0** | `<time> - A non-dry build would build project '<abs>/b/tsconfig.json'`\n\n + same for `a` (absolute paths) | nothing built |
| dry (up-to-date) | `-b a --dry` | **0** | `<time> - Project '<abs>/b/tsconfig.json' is up to date` + same for `a` | nothing built |
| verbose (fresh) | `-b a --verbose` | **0** | `<time> - Projects in this build: \r\n    * b/tsconfig.json\r\n    * a/tsconfig.json` then per-project `… is out of date because output file '…tsbuildinfo' does not exist` + `Building project '…'...` (B then A) | both built |
| error in B | `-b a` (B has TS2322) | **1** | `b/index.ts(1,14): error TS2322: Type 'string' is not assignable to type 'number'.` | **both** still built (no `noEmitOnError`, no `--stopBuildOnErrors`) |
| missing project | `-b nope` | **2** | `error TS6053: File '<abs>/nope/tsconfig.json' not found.` | none |
| circular a↔b | `-b a` | **4** | `error TS6202: Project references may not form a circular graph. Cycle detected: <abs>/a/tsconfig.json\n<abs>/b/tsconfig.json` | none |

> Key findings: (a) plain/non-TTY `-b` prints **nothing** when clean/up-to-date/
> force (the error summary is quiet in plain mode, same as single-build);
> (b) verbose/dry status lines carry a non-deterministic `HH:MM:SS PM - ` prefix
> from `sys.Now()` (Go's `TestClock` advances 1s/call); (c) `--dry`/up-to-date
> dry messages use the **absolute** config path (`t.config`) while **verbose**
> reasons use the **relative** name (`relativeFileName`); (d) the default build
> does **not** stop on errors — B emits + A still builds, exit is the max status.

## Rust slices (red → green, TDD)

All build tests go through the public `execute(&sys, &["-b", …])` entry
(integration-style; they survive internal refactors). A deterministic monotonic
`StepClock` seeds the in-memory FS so build *order* is observable (B's outputs
strictly predate A's) and the up-to-date check is reproducible.

1. **clean build A→B** (`clean_build_builds_in_dependency_order`): RED against a
   stub `perform_build` that returned `Success` without building (`b/index.js not
   emitted`). GREEN: full graph + topo loop + emit + `.tsbuildinfo`. Asserts exit
   0, empty stdout, both `index.js` + `tsconfig.tsbuildinfo`, and `b` buildinfo
   mtime `<` `a` buildinfo mtime (order).
2. **up-to-date no-op** (`second_build_is_a_noop_when_up_to_date`): second
   `execute` leaves every output mtime unchanged, exit 0, empty stdout.
3. **--force** (`force_rebuilds_all_projects`): after a clean build, `--force`
   advances both buildinfo mtimes, exit 0, empty stdout.
4. **--dry fresh** (`dry_run_reports_plan_without_building`): no files written;
   stdout lists `A non-dry build would build project '<abs>'` for B then A.
4b. **--dry up-to-date** (`dry_run_reports_up_to_date_after_build`): stdout has
   `Project '<abs>' is up to date` for both.
5. **error in B** (`type_error_in_dependency_reports_and_continues`): exit 1,
   TS2322 reported on `b/index.ts`, **both** projects still emit.
6a. **--verbose** (`verbose_reports_projects_and_build_status`): "Projects in
   this build:" list with `\r\n    * …` + per-project out-of-date reason +
   "Building project '…'..." in B-then-A order.
6b. **missing project** (`missing_project_reports_ts6053_and_exits_two`): exit 2,
   exactly `error TS6053: File '/p/nope/tsconfig.json' not found.\n`.
6c. **circular** (`circular_references_report_ts6202_and_exit_four`): exit 4,
   TS6202 with the cycle chain, nothing built.

### Recorded divergences (all in out-of-scope downstream crates; Rust reality asserted + Go truth noted)

- **TS2322 column** — port reports `b/index.ts(1,13)` vs Go `(1,14)`: the
  `tsgo_checker` variable-declaration span is one less than Go's (same off-by-one
  already documented for the single-build path). Code/message/file/exit all match.
- **`.d.ts` / `"use strict"` emit** — orchestration writes `index.js` +
  `.tsbuildinfo` per project; any emitter content gaps live in `tsgo_compiler`
  (out of scope). Tests assert the orchestration outputs (`.js` + `.tsbuildinfo`).
- **`version_of` name match** — the reachable up-to-date check looks the input up
  by name in the buildinfo `fileNames` (relative) while inputs are absolute; this
  only affects the *text-unchanged pseudo-build* (DEFER), never the reachable
  slices (inputs are always older than the buildinfo written after them).

## Public API (additive only, within `tsgo_execute`)

- `build::perform_build(sys, ParsedBuildCommandLine) -> CommandLineResult` (re-exported as `tsgo_execute::perform_build`).
- `System::now(&self) -> SystemTime` (new trait method) + `VfsSystem` deterministic clock.
- `ReportErrorSummary::quiet()` constructor (per-project quiet summary).
- `execute` now routes `-b`/`--build` (behavior-additive; single-build path unchanged).

No existing signature changed; no existing test weakened or deleted.

## Gates (crate-scoped, all GREEN)

- `cargo test -p tsgo_execute` — **33 lib tests + 4 doctests pass** (was 24 lib + 4 doctest; +9 build slices, 0 regressions).
- `cargo clippy -p tsgo_execute --all-targets -- -D warnings` — clean.
- `cargo fmt -p tsgo_execute -- --check` — clean.
- `cargo build -p tsgo_execute` — ok.

## DEFER (blocked-by)

- `--watch -b` (downstream tracking + watch loop). blocked-by: p9-watcher.
- `--clean` (output deletion). blocked-by: this chunk's clean path.
- Pseudo-builds / `UpToDateWithUpstreamTypes` (touch-only timestamp updates,
  `.d.ts`-unchanged fast rebuilds) + the full incremental skip-emit reuse.
  blocked-by: more of P6-9b (`HasChangedDtsFile`, pending-emit reuse).
- Parallel builders (`--builders`) + `--stopBuildOnErrors` upstream-error
  propagation beyond the reachable subset.
- Pretty (colour) `-b` status lines beyond the plain renderer wiring; `--help`/
  `--version`/pprof build branches. blocked-by: the p9-watcher colour styling.
- `cmd/tsgo` argv entry for `-b` (p9-cmd binary chunk).
