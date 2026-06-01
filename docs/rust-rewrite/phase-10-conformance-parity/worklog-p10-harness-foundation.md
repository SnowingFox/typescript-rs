# P10 worklog — conformance-harness foundation

Round goal: stand up the **conformance-harness foundation** so tsc parity can be
measured now that the `tsgo` CLI compiles real TypeScript end-to-end. Ports the
reachable subset of Go `internal/testutil/harnessutil` + `internal/testrunner`
into the `tsgo_testutil_harnessutil` + `tsgo_testrunner` crates. Strict TDD
red→green vertical slices. Tree was clean at `5aec8c80`.

This is the foundation for measuring tsc conformance parity: it proves the
parser → compile → `.errors.txt` baseline pipeline on inline cases and validates
the baseline formatter against a **real committed reference baseline**.

## What landed

### `tsgo_testrunner` (`internal/testrunner/`)

- **`test_case_parser.rs`** — ports `test_case_parser.go`:
  - `parse_test_files_and_symlinks` / `..._with_options` — the `// @filename:`
    file-splitter + `// @link:` / `// @symlink:` / `// @currentDirectory:`
    directive parser, including the fourslash `allow_implicit_first_file`
    branches and the "content before first `@filename`" panic.
  - `make_units_from_test` → `TestCaseContent` (units + symlinks).
  - `extract_compiler_settings` → `RawCompilerSettings` (lowercased names,
    trailing `;` stripped).
  - `parse_symlink_from_test`, plus `TestUnit` / `ParseTestFilesOptions` /
    `ParsedTestFiles`.
- **`compiler_runner.rs`** — ports `compiler_runner.go` (`CompilerTestType`) +
  the `.errors.txt` producer ported from `tsbaseline/error_baseline.go`
  (`iterateErrorBaseline`/`GetErrorBaseline`): `error_baseline_for_test`
  (parse → compile → baseline), `get_error_baseline`, `remove_test_path_prefixes`,
  the squiggle/`!!!`-line rendering, and the `lib*.d.ts(--,--)` location fixup.
- **`runner.rs`** — the `Runner` trait (`runner.go`).

### `tsgo_testutil_harnessutil` (`internal/testutil/harnessutil/`)

- **`recorderfs.rs`** — `OutputRecorderFs<F>`: a `vfs::Fs` wrapper recording every
  `write_file` under its real path (overwrite-in-place), so emitted documents can
  be read back (`recorderfs.go`).
- **`harnessutil.rs`** — `compile_files` / `compile_files_ex`: build a
  `tsgo_compiler::Program` over a `MapFs` wrapped with the embedded `bundled:///`
  default-lib FS + the output recorder, collect option + syntactic + semantic
  diagnostics, emit, and return a `CompilationResult`. Plus `TestFile`,
  `HarnessOptions`, `HarnessFile` (`FileLike`), `HarnessDiagnostic`
  (`diagnosticwriter::Diagnostic`), `set_options_from_test_config`, and
  `get_config_name_from_file_name` (`harnessutil.go`).

## RED→GREEN slices (observed symptoms)

1. **Parse a directive + filename** (`// @strict: true` + `// @filename: a.ts`)
   → one unit `a.ts` (`const x = 1;`) + `strict=true` setting. GREEN.
2. **Multi-file split** (mirrors Go `TestMakeUnitsFromTest`): two `@filename`
   directives → two units with comment lines routed to the right unit. GREEN.
3. **Compile a clean inline case** (`const x: number = 1;`) → 0 diagnostics +
   emitted `/.src/a.js` == `const x = 1;\r\n` (harness CRLF default, annotation
   erased). GREEN.
4. **Compile an errored case** (`var x: number = "s";`) → exactly one TS2322
   ("Type 'string' is not assignable to type 'number'."), attributed to
   `/.src/a.ts`. GREEN.
5. **`.errors.txt` baseline for the errored case** — RED first: the expected
   literal lost its leading spaces to `\`-continuation; the *implementation* was
   correct (compact top line `errored.ts(1,4): error TS2322: …`, two blank lines,
   `==== errored.ts (1 errors) ====`, the source line, the `       ~~~~~~~~~~~~~~~~`
   squiggle, the `!!! error TS2322: …` line). Fixed the test literal → GREEN.
6. **Validate against a committed reference baseline** — the formatter
   reproduces `testdata/baselines/reference/compiler/typeOnlyExportAsIfBody.errors.txt`
   **byte-for-byte**. The full parse→compile path to that case is deferred (the
   partial parser does not yet emit the TS1233 grammar diagnostic — first
   discovery run returned `<no content>`), so the diagnostic is constructed
   directly and fed through `get_error_baseline`; this isolates and proves the
   ported formatter against committed bytes, while slice 5 covers the compile
   path end-to-end.

## Validated against a real committed baseline

Yes — `error_baseline_matches_committed_reference` asserts the generated
`.errors.txt` equals the committed
`testdata/baselines/reference/compiler/typeOnlyExportAsIfBody.errors.txt`
byte-for-byte (CRLF endings, trailing blank source line, the `(1,11)` location,
the squiggle, and the `!!!` line all match).

## Go functions mirrored (`// Go:` anchors)

- `test_case_parser.go`: `ParseTestFilesAndSymlinks(WithOptions)`,
  `makeUnitsFromTest`, `extractCompilerSettings`, `parseSymlinkFromTest`,
  `testUnit`, `testCaseContent`, `ParseTestFilesOptions`, regexes.
- `compiler_runner.go`: `CompilerTestType(.String)`, `srcFolder`,
  `verifyDiagnostics`.
- `tsbaseline/error_baseline.go`: `GetErrorBaseline`, `iterateErrorBaseline`,
  `minimalDiagnosticsToString`, `outputErrorText`, `DoErrorBaseline`.
- `tsbaseline/util.go`: `removeTestPathPrefixes`, `lineDelimiter`, `nonWhitespace`.
- `runner.go`: `Runner`.
- `harnessutil.go`: `CompileFiles`, `CompileFilesEx`, `SetOptionsFromTestConfig`,
  `CompilationResult`, `TestFile`, `HarnessOptions`, `TestConfiguration`,
  `GetConfigNameFromFileName`.
- `recorderfs.go`: `OutputRecorderFS(.WriteFile/.Outputs)`, `NewOutputRecorderFS`.

## Test deltas (both crates start at 0)

- `tsgo_testrunner`: **20** unit + **7** doctests.
- `tsgo_testutil_harnessutil`: **9** unit + **4** doctests.

## Gate results (crate-scoped only; a concurrent `format` lane is active)

- `cargo test -p tsgo_testutil_harnessutil -p tsgo_testrunner` — GREEN
  (29 unit + 11 doctests).
- `cargo clippy -p tsgo_testutil_harnessutil -p tsgo_testrunner --all-targets -- -D warnings`
  — GREEN.
- `cargo fmt -p tsgo_testutil_harnessutil -p tsgo_testrunner -- --check` — GREEN.
- `cargo build -p tsgo_testrunner` — GREEN.

Public API is additive within both crates only. No existing test was weakened or
deleted. No `--no-verify`. Root `Cargo.toml` untouched (both crates were already
registered); deps were added to each crate's own `Cargo.toml`. Did not edit
`internal/format/**`, `internal/compiler/**`, `internal/execute/**`, or any other
crate (depended on, not edited).

## DEFER list (blocked-by)

- **`.types` / `.symbols` baselines** — blocked-by: the language-service type
  writer (P7) / typewriter; not reachable in this round.
- **fourslash** — separate crate, out of scope.
- **Full `tests/cases` corpus run + baseline comparison** — separate batched
  round; this round builds the harness and proves it on inline cases + one
  committed-baseline format check.
- **Multi-user-file semantic-diagnostic attribution** — every semantic
  diagnostic is attributed to the first non-library source file. blocked-by: a
  per-file semantic-diagnostics API on `tsgo_compiler::Program` (this crate must
  not edit the compiler).
- **In-test `tsconfig.json` / symlinks / `@libFiles`** — blocked-by:
  `tsoptions` config-file parsing through a VFS parse-config host + VFS symlink
  wiring.
- **`.js` / `.d.ts` / sourcemap (`.js.map`) + sourcemap-record baselines** —
  blocked-by: declaration emit wiring + the JS/sourcemap baseline harness
  (`tsbaseline/js_emit_baseline.go` / `sourcemap*_baseline.go`).
- **Pretty (`--pretty`) error baselines & related-information rendering** —
  blocked-by: a reachable inline case that exercises them (compiler tests run
  `--pretty false`; the reachable cases have no related info).
- **Exact full-pipeline reproduction of `typeOnlyExportAsIfBody`** — blocked-by:
  the parser's TS1233 "export declaration can only be used at the top level"
  grammar diagnostic (not yet ported); the formatter itself is validated against
  the committed bytes.
- **module/target variation matrix** (`GetFileBasedTestConfigurations`,
  `splitOptionValues`) — separate round; not needed for inline cases.

## Notes

This is the foundation for measuring tsc conformance parity: the
parse → compile → `.errors.txt` pipeline now runs end-to-end on inline cases, and
the baseline formatter is proven against a committed reference. Subsequent rounds
can layer the corpus walk, the variation matrix, and the JS/types/symbols
baselines on top of `CompilationResult`.
