# P10 worklog — fourslash harness foundation

# Round 23 — fourslash harness foundation (first slice)

Round goal: **begin** the fourslash language-service test harness
(`internal/fourslash` → crate `tsgo_fourslash`) that drives the already-built
language service (`internal/ls` = `tsgo_ls`) against fourslash-markup test
cases. The full Go `internal/fourslash` is ~8K lines (`fourslash.go` 5705,
`test_parser.go` 509, `baselineutil.go` 1181, `statebaseline.go` 511); this
round builds ONLY the **foundation**: the markup parser + the test-driver
skeleton + a FIRST verify command, with smoke tests. Strict TDD red→green
vertical slices. Tree was clean at `8ed42c2e`.

This is the harness side of P10 (language-service parity), distinct from the
compiler-baseline corpus runner tracked in
[worklog-p10-corpus-runner.md](./worklog-p10-corpus-runner.md). It builds on the
already-landed `tsgo_ls` LS-root (quick-info/hover + diagnostics) and the
`tsgo_testrunner` file splitter.

## Crate created + registered

- **New crate `tsgo_fourslash`** at `internal/fourslash/` (`name =
  "tsgo_fourslash"`, `path = "lib.rs"`), added to the root `Cargo.toml`
  workspace `members`.
- **Dependencies (existing workspace crates only; NO new external deps):**
  `tsgo_ls`, `tsgo_ls_lsconv`, `tsgo_lsproto`, `tsgo_compiler`, `tsgo_tsoptions`,
  `tsgo_tspath`, `tsgo_vfs`, `tsgo_core`, `tsgo_collections`, `tsgo_stringutil`,
  `tsgo_testrunner`, plus `indexmap` + `serde_json` (both already in the
  workspace lockfile / on the PORTING §10 whitelist). `Cargo.lock` changed only
  to add the internal `tsgo_fourslash` entry (no new external crates).

## What landed (ported)

### `test_parser.rs` (Go: `internal/fourslash/test_parser.go`) — the bulk of the round

Faithful 1:1 port of the markup parser with `// Go:` anchors on every item:

- **Types**: `Marker { position, ls_position, name, data }`,
  `RangeMarker { range, ls_range, marker }`, `TestFileInfo` (impls
  `lsconv::Script`), `TestData { files, marker_positions, markers, symlinks,
  global_options, ranges }`, the `MarkerOrRange` trait (impl'd for both),
  `FourslashError`.
- **`parse_test_data(contents, file_name) -> Result<TestData, FourslashError>`**
  — splits files via `tsgo_testrunner::parse_test_files_and_symlinks_with_options`
  (`allow_implicit_first_file: true`), runs `parse_file_content` per unit,
  collects markers/ranges, builds the named-marker index, enforces
  duplicate/unnamed-marker and config-vs-global-options rules. (Go uses
  `t.Fatalf`; this port returns `Result`.)
- **`parse_file_content`** — the core rune-by-rune DSL **state machine**
  (`None` / `InSlashStarMarker` / `InObjectMarker`): recognizes `[|` / `|]` /
  `/*` / `*/` / `{|` / `|}`, tracks the running stripped-metacharacter
  `difference` (so positions land in the stripped output), 1-based line/column
  for error messages, range→embedded-marker linking, the block-comment bail-out
  for `/* ... */` with non-marker chars, `chompLeadingSpace`, the
  `(pos asc, end desc)` stable range sort, and LSP-position computation via
  `lsconv::compute_lsp_line_starts` + `Converters` (UTF-8 encoding).
- **`get_object_marker`** — parses `{| ... |}` as JSON `{ <text> }`
  (`serde_json`), rejecting empty/non-object bodies, naming the marker from a
  non-empty string `"name"` field (anonymous otherwise).
- Helpers: `is_state_baselining_enabled`, `is_config_file`,
  `has_unsupported_global_options_with_config`, `report_error`,
  `Marker::marker_with_symlink`, `RangeMarker::ls_location`.

### `driver.rs` (Go: `internal/fourslash/fourslash.go`) — driver skeleton + first command

- **`new_fourslash(content) -> FourslashTest`** (+ non-panicking
  `try_new_fourslash`) — parses the markup and builds an in-process
  `tsgo_ls::LanguageService` over the marker-stripped files (the same
  construction the `tsgo_ls` feature tests use: `MapFs` → `new_compiler_host` →
  `new_program` → `LanguageService::new`), with an in-memory
  `LanguageServiceHost`.
- **`FourslashTest`** state: `test_data`, `ls`, `active_filename`,
  `current_caret_position`, `last_known_marker_name`, plus accessors
  (`markers`, `marker_by_name`, `ranges`, `active_filename`,
  `current_caret_position`, `last_known_marker_name`, `test_data`).
- **Navigation primitive `go_to_marker(name)`** (+ `go_to_marker_or_range` /
  `ensure_active_file`) — makes the marker's file active and sets the caret to
  its LSP position.
- **First verify command `verify_quick_info_at(marker, expected_text)`** (+ the
  `quick_info_at` primitive) — REAL: drives
  `LanguageService::get_quick_info_at_position` and compares the resolved type
  string; `current_position_prefix` mirrors Go's assertion-prefix.

## RED→GREEN slices (one behavior at a time)

Parser (`test_parser_test.rs`, 21 tests):

1. headline — `/*a*/const x = 1;` → one marker `a` at byte 0, stripped content.
2. `/**/` empty-named marker indexed under `""`.
3. `[|ranged|]` → range `[0,6)` with LSP range, no embedded marker.
4. `[|/*m*/foo|]` → range carrying embedded marker `m` (`get_name()`).
5. `// @filename:` multi-file split (two files, normalized names).
6. `// @target:` global option recorded.
7. `{| "name": "foo", "kind": "value" |}` named object marker carrying JSON data.
8. anonymous object marker (`{| "kind": ... |}`) kept in `markers`, not indexed.
9. multi-line marker LSP position (`a\n/*m*/b` → `(1,0)`).
10. range `(pos, -end)` sort (nested `[|[|abc|]def|]`).
11. block-comment bail-out (`/* hello */` kept verbatim, no marker).
12. `chompLeadingSpace` (uniform leading space stripped).
13–16. error paths: duplicate marker name, unterminated range, unterminated
    marker, range-end-without-start.
17. `is_state_baselining_enabled` via `@statebaseline`.
18. `marker_with_symlink` re-homes a marker.
19. `Marker` `MarkerOrRange` accessors (file name / LSP pos / name).
20. `RangeMarker` `MarkerOrRange` accessors + `ls_location` (URI + range).
21. `TestFileInfo::emit` defaults `false`.

Driver/command (`driver_test.rs`, 11 tests):

1. `new_fourslash` builds the service + exposes markers + first file active.
2. `go_to_marker` sets active file + caret (`(0,21)`) + last-marker name.
3. `go_to_marker` switches active file across `// @filename:`.
4. `go_to_marker` unknown marker → error (guard).
5. `quick_info_at` drives the LS → resolves `number` (headline smoke).
6. `verify_quick_info_at("x", "number")` passes (headline command).
7. NEGATIVE guard — `verify_quick_info_at("x", "string")` fails with a
   "Quick info mismatch" + "At marker 'x'" message.
8. NEGATIVE guard — quick info on the `const` keyword → "got none" failure.
9. multi-file `verify_quick_info_at` resolves in the marker's file (`string`).
10. `try_new_fourslash` reports a parse error instead of panicking.
11. driver exposes `ranges()` / `test_data()`.

Total this round: **32 unit tests + 1 doctest**, all green.

## Gate results (all GREEN; never `--no-verify`)

- `cargo test -p tsgo_fourslash` — **32 passed + 1 doctest passed**.
- `cargo test -p tsgo_ls` (dependency) — **80 passed** (unmodified, stays green).
- `cargo clippy -p tsgo_fourslash --all-targets -- -D warnings` — **clean**.
- `cargo fmt -p tsgo_fourslash -- --check` — **clean** (ran `cargo fmt` first).
- `cargo build --workspace --all-targets` — **OK** (new crate integrates).

Tree clean: only `Cargo.toml` + `Cargo.lock` (internal-crate entry) + the new
`internal/fourslash/{Cargo.toml,lib.rs,test_parser.rs,test_parser_test.rs,driver.rs,driver_test.rs}`
+ this worklog. No existing crate modified; no existing diagnostic/snapshot
tests touched.

## Divergences from Go (documented in source)

1. **In-process LS, not an LSP server.** Go's `FourslashTest` drives an
   in-memory LSP server over channels (`lsptestutil.NewLSPClient` +
   `internal/lsp` + `internal/project`, all P8 / unported). This foundation
   drives `tsgo_ls::LanguageService` directly (as the `tsgo_ls` tests do). The
   markup grammar and navigation/verify semantics match Go; the transport,
   project layer, `initialize`/`didOpen`/capabilities handshake, and baseline
   machinery are deferred. Consequences: `ensure_active_file` only updates caret
   state (the program already holds every file); `verify_quick_info_at` compares
   the reachable type string rather than Go's full markdown hover body (which
   `tsgo_ls` itself has not yet ported).
2. **Splitter shape.** Go threads `parseFileContent` as the `parseFile` callback
   into `testrunner.ParseTestFilesAndSymlinksWithOptions`; the Rust splitter
   yields `(name, content)` units (no callback), so this port splits first then
   parses each unit's content. Behaviorally identical (per-unit content is the
   same). The dropped facet is the per-file `emitthisfile` directive (always
   `false` this round; only consumed by baseline emit, deferred).
3. **`RangeMarker::marker`** stores an owned clone of the embedded marker (with
   its computed LSP position) rather than Go's shared `*Marker` pointer. Markers
   are immutable post-parse, so identical behavior.
4. **`t.Fatalf` → `Result<_, FourslashError>`** for parse errors and verify
   failures (lets the negative-guard tests assert failure without panicking).
5. Symlinks / dynamic (`untitled:`) files are not added to the VFS this round
   (no smoke case needs them).

## DEFER list (remaining fourslash surface) + suggested next-slice ordering

Everything below is explicitly deferred from this foundation round:

- **D1 — `fourslash.go` command/verify API (inline-assert family).** The rest of
  `VerifyQuickInfo*` (`VerifyQuickInfoIs` / `Exists` / `NotExists`, the
  `expectedDocumentation` + markdown-fence comparison), then
  `VerifyCompletions` (+ `tests/util` completion-globals constant tables),
  `VerifySignatureHelp*`, `VerifyRename*`, `VerifyCodeFix*`,
  `VerifyDiagnostics*`, the editing commands (`Insert`/`Replace`/`Backspace`/…
  with `scriptInfo.editContent`), and the remaining `GoTo*`
  (`GoToEOF`/`BOF`/`Position`/`EachMarker`/`EachRange`/`Select`/`File*`).
  blocked-by: respective `tsgo_ls` feature surfaces (several already exist:
  completions/definition/references/rename/signaturehelp/symbols/folding/
  documenthighlights) — wire them through next.
- **D2 — `baselineutil.go`** (`baselineCommand` constants, `addResultToBaseline`,
  `getBaselineFileName`/`Extension`/`Options`, submodule `DiffFixupOld`) and the
  whole `VerifyBaseline*` family (FindAllReferences / GoToDefinition / Hover /
  SignatureHelp / Rename / CallHierarchy / DocumentHighlights / InlayHints /
  DocumentSymbol / …). blocked-by: `tsgo_testutil_baseline::run` wiring + the
  committed `testdata/baselines/reference/fourslash/**` byte-compare.
- **D3 — `statebaseline.go`** (`@statebaseline` mode). blocked-by: D1/D2 +
  project-state recording.
- **D4 — `semantictokens.go`** (default token type/modifier sets + rendering).
  blocked-by: the `tsgo_ls` semantic-tokens feature.
- **D5 — the real LSP-server transport** (`NewLSPClient` / `initialize` /
  `handleServerRequest` / capabilities / `didOpen`/`didChange`). blocked-by:
  `internal/lsp` (`tsgo_lsp`) + `internal/project` (`tsgo_project`) — P8,
  unported. Until then the in-process LS driver is the foundation.
- **D6 — the 4386-case corpus runner.** The generated `tests/{*,gen/*,manual/*}`
  cases (~4250 files) are NOT wired this round. Per `fourslash/impl.md`, the
  plan is to reuse the TS-upstream fixtures via a generator (not hand-translate
  the Go test files) and gate batches through a `failingTests.txt`-style skip
  list. blocked-by: D1+D2 (the command + baseline surface must exist first).

Suggested next slice: **D1 quick-info completion** (`VerifyQuickInfoIs/Exists/
NotExists`) since the LS surface already exists — then **D1 completions** (drive
`tsgo_ls::completions` + port the `tests/util` constant tables) and **D1
go-to-definition / find-all-references** (inline form). Defer D2 baseline +
D5 transport + D6 corpus until a few inline command families are green.
