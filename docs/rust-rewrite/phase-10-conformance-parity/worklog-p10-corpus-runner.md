# P10 worklog — compiler-baseline corpus runner

Round goal: build the **P10 compiler-baseline corpus runner** in `tsgo_testrunner`
— drive the just-landed harness over real `tests/cases` conformance/compiler
cases, compare the produced `.errors.txt` to the committed reference baseline,
and produce a parity pass/fail SUMMARY (the foundation of the
"identical-to-tsc acceptance gate"). Strict TDD red→green vertical slices. Tree
was clean at `15475f5a`.

This builds directly on
[worklog-p10-harness-foundation.md](./worklog-p10-harness-foundation.md) (parse →
compile → `.errors.txt` on inline cases + a committed-baseline format check). This
round layers the **corpus walk + byte-compare against committed references +
parity tally** on top of `error_baseline_for_test` / `CompilationResult`.

## What landed (`tsgo_testrunner`, `internal/testrunner/compiler_runner.rs`)

All additive; only `internal/testrunner/**` (+ its own `Cargo.toml` dev-dep) +
this doc were edited. `internal/testutil/harnessutil/**` was **not** modified
(the foundation API was already sufficient).

- **`compare_error_baseline(produced, committed: Option<&str>) -> ParityOutcome`**
  — the pure comparison core. Mirrors the baseline framework's accept rule:
  - no committed baseline + produced `<no content>` → `Passed`;
  - no committed baseline + produced errors → `Failed` (unexpected errors);
  - committed baseline + produced `<no content>` → `Failed` (errors went missing);
  - committed baseline + byte-equal produced → `Passed`;
  - committed baseline + differing produced → `Failed` with a short unified diff.
- **`ParityOutcome`** (`Passed` / `Failed{detail}` / `Errored{message}`),
  **`CaseResult{name, outcome}`**, **`ParityCounts{passed,failed,errored}`** (with
  `total()`), and **`ParitySummary`** (per-case results in run order +
  `counts()` + a deterministic `report()`).
- **`CompilerBaselineRunner`** — locates a suite's case dir
  (`<testdata>/tests/cases/<suite>`) and reference-baseline dir
  (`<testdata>/baselines/reference/<suite>`) under a `testdata` root passed in
  (so the library does not depend on `tsgo_repo`; the smoke test supplies
  `tsgo_repo::test_data_path()`). `run_case` reads the case + (optional)
  committed `.errors.txt`, runs `error_baseline_for_test` under
  `catch_unwind` (a parser/checker panic → `Errored`, never aborts the batch),
  and compares. `run_cases` tallies a `ParitySummary`. Implements the `Runner`
  trait via a recursive `.ts`/`.tsx` enumeration (sorted).
- **`baseline_name_for`** (`\.tsx?$` → `.errors.txt`, mirrors Go `tsExtension`),
  `panic_message`, `head_lines`, `short_baseline_diff` (reuses
  `tsgo_testutil_baseline::diff_text`).

## RED→GREEN slices (observed symptoms)

1. **Comparison core** (5 vertical slices, pure): no-baseline+clean→`Passed`;
   no-baseline+errors→`Failed`; baseline+equal→`Passed`; baseline+empty→`Failed`;
   baseline+mismatch→`Failed` with a `committed.errors.txt`/`produced.errors.txt`
   unified diff. RED (fns absent) → GREEN.
2. **Clean real conformance case** — `conformance/simpleTest.ts` (`1 + 2;`, no
   committed `.errors.txt`) runs to a parity **PASS** through the real fs
   runner. RED → GREEN.
3. **Reproduced committed baseline** — a temp-dir case `errored.ts`
   (`var x: number = "s";`) whose committed `errored.errors.txt` our compiler
   reproduces **byte-for-byte** → PASS via the full read-case + read-reference +
   byte-compare path. RED → GREEN.
4. **Mismatch → FAILED (not crash)** — same case with a deliberately wrong
   committed baseline (`TS2322`→`TS9999`) → `Failed` carrying a short diff that
   shows our `TS2322`. RED → GREEN.
5. **Panic → caught/`errored`, batch continues** — a temp-dir case with
   non-comment content before the first `// @filename` (the parser's hard error)
   is caught by `catch_unwind` and counted `errored`; the `clean.ts` and
   `errored.ts` cases after it still PASS in the same `run_cases` batch (counts
   `{passed:2, failed:0, errored:1}`, 3 results — nothing aborted). RED → GREEN.
6. **Curated-subset smoke summary** — characterization test over a deterministic
   30-case subset of the real compiler corpus; asserts the measured
   `{passed, failed, errored}` counts. RED → GREEN.

Plus: missing-case → `errored`; `enumerate_test_files` recursive+sorted;
`ParitySummary::report` determinism.

## Headline deliverable — measured parity on the curated subset

Curated subset = the first **30** `tests/cases/compiler` cases (sorted by name)
whose source is ≤ ~14 lines — deterministic/reproducible, biased to reachable
features, away from the heaviest emit/recursive-type cases.

```
parity: 30 cases — passed 18, failed 12, errored 0   (was 15 / 10 / 5)
```

**18 / 30 PASS, 12 FAIL, 0 ERROR** after the panic-robustness triage round
(below). The original measurement was **15 / 30 PASS, 10 FAIL, 5 ERROR**; all
five ERROR (panic) cases were root-fixed, dropping `errored` to 0. This is a
real, reproducible signal, not 100% — the port is a reachable subset of tsc, so
most real conformance cases are EXPECTED to diverge. The value is the
measurement and the failure-category breakdown below. The smoke run completes
without aborting (batch runs on a 256 MiB-stack thread so a deep checker
recursion does not overflow the small default test-thread stack; `catch_unwind`
handles unwinding panics).

## Top failure categories (directs future work)

**ERROR (0 — all five panics root-fixed; see "Panic-robustness triage" below).**
The five panic cases now either PASS (3) or degrade gracefully to FAIL (2):

- ✅ **Emit to a relative `outDir`/declaration-map path the in-memory VFS rejects**
  (`vfs: path "…/x.js[.map]" is not absolute`) — `declarationMapInlineSourcesContent.ts`,
  `emitEndOfFileJSDocComments.ts`. **FIXED** → both PASS. Root cause: the harness
  did not root the path-typed options (`outDir`, …) to the current directory
  before emit. `compile_files_ex` now mirrors Go's `CompileFilesEx`
  (`GetNormalizedAbsolutePath(value, currentDirectory)`).
- ✅ **`begin <= end` slice panic on top-level `await`** — `awaitObjectLiteral.ts`.
  **FIXED** (no panic) → FAIL (top-level-await parsing is still a reachable gap,
  so the case error-recovers and reports parse diagnostics). Root cause was in
  the **printer**: `get_source_text_of_node` ran `skip_trivia` past `end` on a
  parser-recovered MISSING (zero-width) node. Now guards `NodeIsMissing` →
  `""`, mirroring Go's `GetTextOfNodeFromSourceText`.
- ✅ **Byte-index out of bounds into source text** (`byte index 48 is out of
  bounds`) — `allowSyntheticDefaultImports9.ts` (multi-file `.d.ts` + commonjs).
  **FIXED** (no panic) → FAIL (synthetic-default import is a reachable checker
  gap). Root cause: the harness blanket-attributed every semantic diagnostic to
  the first user file, so a diagnostic located in the longer `a.ts` (offset 48)
  was rendered against the shorter `b.d.ts` (47 bytes). Now uses per-file
  attribution (`Program::semantic_diagnostics_by_file`).
- ✅ **Arena/index out of bounds** (`index out of bounds: len 44 but index 3028`) —
  `classExpressionWithComputedPropertyInLoop.ts`. **FIXED** → PASS. Root cause:
  the value-type builder read a lib-declared method's (`Array.push`) declaration
  nodes through the file-under-check's arena. `get_type_of_symbol` now switches
  to the symbol's owning file view first (the established owning-view switch).

**FAIL (12):**

- **Checker false-negative — committed errors we don't yet produce** (~4):
  `anonymousClassDecoratorEs2022.ts`, `asyncFunctionReturnNonPromiseThenable.ts`,
  `catchClauseRestProperties.ts`, `circularDestructuring.ts`,
  `contextuallyTypedFunctionOptionalAndRest.ts` ("a committed `.errors.txt`
  baseline exists but no errors were produced").
- **Checker false-positive — errors with no committed baseline** (~2):
  `conditionalContextualReturnSubstitutionCache.ts` (spurious `TS2322` on a
  nested conditional return type), `declarationEmitExpandoOverloads.ts`
  (`TS2304 Cannot find name 'A'` + `TS2339` — expando-function/namespace merging
  on an overloaded function not modeled).
- **Checker divergence — wrong code + duplicate diagnostic**:
  `checkInheritedProperty.ts` expects one `TS2729` ("used before its
  initialization"); we emit `TS2339` ("Property 'b' does not exist on type
  'any'") **twice** (`this.b` on a mixin-derived base resolves to `any`, and the
  diagnostic is duplicated).
- **Parser error-recovery divergence**: `destructuringEmptyBinding.ts` — we emit
  the `TS1003` ("Identifier expected") but miss the follow-on `TS2304`
  ("Cannot find name 'x'") that tsc recovers; `assertionWithNoArgument.ts` — we
  emit spurious `TS2304` for the function's own name (binder/scope resolution of
  an `asserts` function referenced before/within its own body).

## Panic-robustness triage round (errored 5 → 0)

A follow-up round root-fixed every panic surfaced by the parity run (a
production compiler must never panic on valid input). Strict TDD red→green, one
vertical slice per panic category, each driving the real path in the owning
crate. No `catch_unwind` masking; no existing test weakened.

- **(b) printer — `NodeIsMissing` guard** (`tsgo_printer`,
  `printer.rs:get_source_text_of_node`). RED: emitting a parser-recovered
  MISSING (zero-width) node — `const foo = await { bar: 42 }` error-recovers
  into a binding pattern with empty (`pos == end`) binding-name identifiers —
  ran `skip_trivia(pos)` past `end` and sliced `text[pos..end]` with `pos > end`
  (`begin <= end (35 <= 34)`). GREEN: short-circuit a missing node to `""`,
  exactly as Go's `GetTextOfNodeFromSourceText` does (`if NodeIsMissing { "" }`).
  Test: `printer::tests::emit_missing_node_does_not_panic`.
  Go: `internal/scanner/utilities.go:GetTextOfNodeFromSourceText` +
  `internal/ast/utilities.go:NodeIsMissing`.
- **(d) checker — owning-view switch for value types** (`tsgo_checker`,
  `core/declared_types.rs:get_type_of_symbol`). RED: `array.push(...)` (with
  `array: any[]`) resolved the `push` method's value type by reading its
  declaration nodes (which live in `lib.es5.d.ts`'s arena) through the
  file-under-check's arena → `index out of bounds: the len is 44 but the index
  is 3028`. GREEN: switch to the symbol's owning file view first (guarded by
  `file_handle()`), mirroring the switch already in `get_declared_type_of_symbol`
  / `get_constraint_of_type_parameter`; one switch at the `get_type_of_symbol`
  dispatcher covers `get_type_of_variable_or_property` and
  `get_type_of_func_class_enum_module`. Test (real multi-file lib path):
  `program::tests::property_access_on_lib_declared_method_does_not_panic`.
  Go: `internal/checker/checker.go:Checker.getTypeOfSymbol` (resolved against the
  symbol's declaring file).
- **(c) per-file semantic-diagnostic attribution** (`tsgo_compiler` +
  `tsgo_testutil_harnessutil`). RED: the harness attributed every semantic
  diagnostic to the first user file, so a `TS2304` located in the longer `a.ts`
  (offset 48) was rendered against the shorter `b.d.ts` (47 bytes) →
  `byte index 48 is out of bounds`. GREEN: new
  `Program::semantic_diagnostics_by_file` (+
  `CheckerPool::collect_diagnostics_grouped_excluding`) preserves the per-file
  partition; `compile_files_ex` attributes each diagnostic to its declaring
  file. Resolves the explicit "per-file semantic attribution" DEFER from commit
  `7c567749`. Test:
  `harnessutil::tests::multi_file_semantic_diagnostics_stay_within_their_own_file`.
  Go: `internal/compiler/program.go:getDiagnostics` (per-file) +
  `harnessutil.go:verifyDiagnostics` (a diagnostic renders against its own file).
- **(a) root path-typed options before emit** (`tsgo_testutil_harnessutil`,
  `compile_files_ex`). RED: `outDir: dist` produced the relative output path
  `dist/x.js[.map]`; the in-memory VFS rejects non-absolute paths
  (`vfs: path "dist/x.js" is not absolute`). GREEN: root `out_dir` / `project` /
  `root_dir` / `ts_build_info_file` / `base_url` / `declaration_dir` /
  `root_dirs` / `type_roots` to the current directory, a 1:1 port of Go's
  `CompileFilesEx` (`ts.convertToOptionsWithAbsolutePaths`). Test:
  `harnessutil::tests::relative_out_dir_is_rooted_before_emit`.
  Go: `internal/testutil/harnessutil/harnessutil.go:CompileFilesEx`.

Test deltas (panic-robustness round): `tsgo_printer` +1 unit; `tsgo_compiler`
+1 unit; `tsgo_testutil_harnessutil` +2 unit; `tsgo_testrunner` smoke
characterization updated to `{passed: 18, failed: 12, errored: 0}`. `tsgo_checker`
suite unchanged and green (177). No test weakened or deleted.

## Go anchors (`// Go:` )

- `internal/testrunner/compiler_runner.go`: `CompilerBaselineRunner`,
  `NewCompilerBaselineRunner`, `EnumerateTestFiles`, `runTest`, `RunTests`,
  `CompilerTestType.String`, `compilerTest.verifyDiagnostics`.
- `internal/testutil/baseline/baseline.go`: `writeComparison` (the compare/accept
  branch the parity verdict mirrors).
- `internal/testutil/tsbaseline/util.go`: `tsExtension` (the `.errors.txt`
  name rule).
- `internal/testutil/harnessutil/harnessutil.go`: `EnumerateFiles` (recursive
  `.tsx?` walk).
- `internal/testutil/testutil.go`: `RecoverAndFail` (the panic-isolation the
  `catch_unwind` + `Errored` count mirrors).

## Test deltas

- `tsgo_testrunner`: **20 → 33** unit tests (+13), **7 → 11** doctests (+4).
  New unit: 5 comparison-core, 1 clean-conformance PASS, 1 reproduced-baseline
  PASS, 1 mismatch FAIL, 1 panic-caught/continue, 1 missing-case errored, 1
  enumerate recursive/sorted, 1 report determinism, 1 curated smoke
  characterization. New doctests: `ParityCounts::total`, `ParitySummary::counts`,
  `compare_error_baseline`, `CompilerBaselineRunner::new`.
- No existing test weakened or deleted; the baseline comparison is byte-for-byte
  (not weakened to force passes).

## Gate results (crate-scoped only; a concurrent `format` lane is active)

- `cargo test -p tsgo_testrunner` — GREEN (33 unit + 11 doctests).
- `cargo clippy -p tsgo_testrunner --all-targets -- -D warnings` — GREEN.
- `cargo fmt -p tsgo_testrunner -- --check` — GREEN.
- `cargo build -p tsgo_testrunner` — GREEN.

Did not run `--workspace` (concurrent lane). `tsgo_testutil_harnessutil` was not
extended, so it was not gated separately. Public API is additive within
`tsgo_testrunner` only. No `--no-verify`. Root `Cargo.toml` untouched; the only
`Cargo.toml` change is `tempfile = "3.27.0"` added to `tsgo_testrunner`'s own
`[dev-dependencies]` (version matches the existing lockfile entry). Did not edit
`internal/format/**`, `internal/compiler/**`, `internal/execute/**`, or any other
crate (depended on, not edited).

## DEFER list (blocked-by)

- **`.js` / `.types` / `.symbols` / `.d.ts` / sourcemap baselines** — this round
  compares only `.errors.txt`. blocked-by: the language-service type writer (P7)
  + declaration emit + the JS/sourcemap baseline harness.
- **Full `tests/cases` corpus run** — this round runs a curated 30-case smoke
  subset. blocked-by: triaging the panic categories above (a full run would hit
  more `Errored` cases; some recursive-type cases risk stack overflow, which
  `catch_unwind` cannot catch). A full sweep is a separate batched round once the
  panics are reduced.
- **module/target variation matrix** (`GetFileBasedTestConfigurations`,
  `splitOptionValues`, `compilerVaryBy`) — each case runs in a single
  configuration. blocked-by: the variation-matrix port (a later P10 round).
- **In-test `tsconfig.json` / symlinks / `@libFiles`, and the
  require/triple-slash-reference toBeCompiled/otherFiles split** — `run_case`
  feeds every unit through `error_baseline_for_test` (all units compiled, in
  declaration order). blocked-by: `tsoptions` config-file parsing through a VFS
  parse-config host + the harness file-routing heuristic.
- **fourslash** — separate crate, out of scope.
- **`local`-baseline writes / `hereby baseline-accept` integration** — the runner
  only reads references and reports a verdict; it does not write
  `testdata/baselines/local`. blocked-by: wiring `tsgo_testutil_baseline::run`
  (which needs a `&mut Harness`) into the corpus runner (a later P10 round).

## Notes

The parse → compile → `.errors.txt` → byte-compare-against-committed-reference →
parity-tally pipeline now runs end-to-end over the real corpus. The curated smoke
test is a stable characterization of where the port stands today (15/30); bump
its expected `passed` upward only as real parity improves. The failure-category
breakdown above is the actionable backlog for the checker/parser/emit lanes.
