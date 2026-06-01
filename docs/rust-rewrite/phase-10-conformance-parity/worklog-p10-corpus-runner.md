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

---

# Round 2 — larger curated subset + failure categorization

Round goal: expand the corpus runner from the curated **30** to a LARGER
deterministic subset (the first **150** sorted `tests/cases/compiler` cases ≤ 25
lines), and add **failure categorization** — classify each FAILED case's
`.errors.txt` mismatch into per-code categories and aggregate a histogram of the
TOP mismatched diagnostic codes, so the parity signal directly prioritizes the
next checker/parser work. Strict TDD red→green. Tree clean at `ed8d4331`.
Additive only; only `internal/testrunner/**` (+ this doc) edited. No root
`Cargo.toml`, no `internal/ls`/`checker`/`compiler` edits; the existing 30-case
smoke + every prior test was kept (this round only ADDS tests). No new crate
dependency (`indexmap`/`regex`/`tsgo_testutil_baseline` already present).

## Headline — measured parity on the LARGER subset

```
parity: 150 cases — passed 55, failed 95, errored 0
category histogram: no_baseline_but_errors ×36, missing_all_errors ×29, divergent ×30
  missing: TS7026 ×15, TS2874 ×7, TS2322 ×6, TS2309 ×4, TS7008 ×4, TS2339 ×3,
           TS2488 ×3, TS1097 ×2, TS1202 ×2, TS1294 ×2, TS1506 ×2, TS2304 ×2,
           TS2345 ×2, TS2353 ×2, TS2688 ×2, TS2875 ×2, TS2882 ×2, TS6424 ×2,
           TS6425 ×2, TS7006 ×2, TS7010 ×2, TS7022 ×2, … (51 distinct codes)
  extra:   TS2304 ×82, TS2339 ×76, TS2322 ×12, TS1005 ×9, TS1003 ×5,
           TS2345 ×2, TS2495 ×2, TS1109 ×1, TS1155 ×1, TS1161 ×1, TS2344 ×1, TS5108 ×1
  wrong_code:    TS2540 ×1, TS2552 ×1, TS2669 ×1, TS2729 ×1, TS7026 ×1
  wrong_message: TS2339 ×2
```

**55 / 150 PASS, 95 FAIL, 0 ERROR** (deterministic across reruns). This is a
MEASUREMENT: most real conformance cases are EXPECTED to diverge because the port
is a reachable subset of tsc. The value is the categorized backlog, not a pass
rate — byte comparison is unchanged (not weakened to inflate passes), and panics
are still caught → `errored` (none on this subset).

### The prioritized backlog (what to fix next, by impact)

1. **Cascading FALSE POSITIVES dominate — fix unresolved-name resolution first.**
   `extra TS2304 (Cannot find name) ×82` + `extra TS2339 (Property does not exist) ×76`
   together are **158 of the spurious diagnostics**. These are downstream cascades:
   when our binder/checker fails to resolve a symbol (expando functions/namespace
   merging, JSDoc-declared values, `export =`/CommonJS, `this`-property typing), the
   unresolved name then triggers a swarm of `TS2304`/`TS2339`. Knocking out a few
   root resolution gaps should clear large blocks of these at once.
2. **Parser error-recovery FALSE POSITIVES** — `extra TS1005 ×9` + `TS1003 ×5`
   ("X expected" / "Identifier expected"): divergent recovery on malformed input
   (we over-report grammar errors tsc recovers from).
3. **Top FALSE NEGATIVE — JSX intrinsic-elements check** — `missing TS7026 ×15`
   ("JSX element implicitly has type 'any' because no interface 'JSX.IntrinsicElements'
   exists"): the `.tsx` cases in the subset expect this and we emit nothing
   (JSX checking is a reachable gap). Next: `missing TS2874 ×7`, `TS2322 ×6`
   (assignability false-negatives), `TS2309 ×4`, `TS7008 ×4`.
4. **`missing_all_errors ×29`** — cases where a committed baseline exists but we
   produced nothing at all (whole-feature gaps), vs **`divergent ×30`** (partial)
   and **`no_baseline_but_errors ×36`** (clean-expected cases we wrongly error on —
   these are the pure false-positive cases, mostly the TS2304/TS2339 cascades).

## What landed (`tsgo_testrunner`)

New module `internal/testrunner/failure_category.rs` (+ `_test.rs`), all additive:

- **`BaselineDiag { file, line, col, code, message, span }`** + **`parse_error_baseline(text) -> Vec<BaselineDiag>`**
  — parses an `.errors.txt`: the compact top-of-baseline lines yield each
  diagnostic's `(file, line, col, code, message)` (a regex that matches ONLY the
  compact lines, so `!!! …`/`==== ====`/source/squiggle/`!!! related` lines never
  inflate the count); the per-file squiggle underlines yield a best-effort `span`
  (tilde count, single-line proxy, symmetric across both sides).
- **`MismatchKind`** (`Missing` / `Extra` / `WrongCode` / `WrongSpan` /
  `WrongMessage`), **`CodeMismatch { kind, code, actual_code }`**,
  **`CaseCategory`** (`NoBaselineButErrors` / `MissingAllErrors` / `Divergent`),
  **`CaseDiff { category, mismatches }`**.
- **`categorize_diags(expected, actual) -> Vec<CodeMismatch>`** — the pure core:
  pass 1 removes byte-identical diagnostics; pass 2 pairs leftovers by location
  (same-code partner preferred → `wrong_span`/`wrong_message`, else different-code
  → `wrong_code`); the still-unpaired expected → `missing`, produced → `extra`.
  **`categorize_failure(produced, committed: Option<&str>)`** parses both sides
  (treating `<no content>` as empty) + derives the `CaseCategory`.
- **`CategoryHistogram`** — per-code `IndexMap`s for each kind + the three
  case-level scalars; `add_case_diff` / `from_case_diffs` / `top_missing(n)` /
  `top_extra(n)` (sorted count-desc then code-asc) / `report()` (the
  prioritized-backlog string).

Wired into the runner (`compiler_runner.rs`, additive):

- **`CaseResult` gains `diff: Option<CaseDiff>`** — populated by `run_case` only
  for a `Failed` verdict (computed from the produced + committed text it already
  has); `None` for passed/errored.
- **`ParitySummary::histogram()`** aggregates the failed cases' `CaseDiff`s; the
  prioritized-backlog histogram is now embedded at the top of
  `ParitySummary::report()`.
- **`CompilerBaselineRunner::curated_subset(max_lines, limit, denylist)`** — the
  deterministic, reproducible subset selector (sorted `.ts`/`.tsx` basenames ≤
  `max_lines` lines, minus `denylist`, capped at `limit`). A pure function of the
  committed corpus.

## RED→GREEN slices (this round)

1. **Parser — compact line** → one `BaselineDiag` with `(file,line,col,code,msg)`.
   RED (`parse_error_baseline` returned `[]`) → GREEN.
2. **Parser — squiggle span + no over-count** (driven by the real
   `destructuringEmptyBinding` 2-error baseline; spans `Some(1)`/`Some(1)`) and a
   16-tilde span (`errored.ts` TS2322). RED (`span: None`) → GREEN (squiggle pass).
3. **Categorizer — missing** (committed TS2304 we don't emit, co-located TS2322
   matches) → single `missing{2304}`. RED → GREEN (pass 1 + missing/extra).
4. **Categorizer — extra / no_baseline_but_errors / missing_all_errors**
   (case-level kinds). GREEN on the same core.
5. **Categorizer — wrong_code** (same loc, TS2304→TS2345). RED (leftover became
   missing+extra) → GREEN (pass 2 wrong_code branch).
6. **Categorizer — wrong_span / wrong_message** (synthetic `BaselineDiag` lists,
   exact span control). RED (still missing+extra) → GREEN (pass 2 span+message
   branches, same-code preference).
7. **Histogram aggregation** — a few synthetic `CaseDiff`s → correct per-code
   tally + `top_missing`/`top_extra` ranking + `report()`. RED (neutralized
   `add_case_diff`) → GREEN.
8. **Wiring** — a real runner batch (`wrongcode.ts` committed TS9999 vs produced
   TS2322 + an extra-error `extra.ts`) populates `CaseResult.diff` and
   `ParitySummary::histogram()` (`wrong_code 9999→2322`, `extra 2322`,
   `no_baseline_but_errors`). RED (field absent) → GREEN.
9. **`curated_subset` determinism** — temp dir of varied-length files, `≤10`
   lines, denylist, cap → deterministic sorted selection.
10. **Expanded smoke characterization** — the 150-case run asserts the measured
    `{passed:55, failed:95, errored:0}` + `top_extra(2)==[(2304,82),(2339,76)]` +
    `top_missing(1)==[(7026,15)]` + the case-level tally. RED (numbers/fns) →
    GREEN (assert actuals). Stable across reruns.

## Determinism + the stress-case denylist

The subset is `curated_subset(25, 150, EXPANDED_DENYLIST)` — a pure function of
the committed corpus (sorted name + on-disk line count). `EXPANDED_DENYLIST`
excludes two unbounded stress cases tsc only survives via internal complexity
limits we have not ported, so they can abort the harness with a stack overflow
(`catch_unwind` cannot catch a stack-overflow abort) or hang/OOM:
`noTypeToStringStackOverflow.ts` (self-referential `typeof f`) and
`templateLiteralTypeTooComplex.ts` (a 49-fold combinatorial template-literal
union tsc rejects with TS2590). Excluding exactly these two keeps the run
deterministic AND non-aborting; the batch still runs on a 512 MiB-stack thread.

## Test deltas

- `tsgo_testrunner`: **33 → 47** unit tests (+14): 3 parser, 7 categorizer
  (missing/extra/no-baseline/missing-all/wrong-code/wrong-span/wrong-message),
  1 histogram, 1 runner wiring, 1 `curated_subset` determinism, 1 expanded
  smoke characterization. Doctests **11 → 11** (the `ParitySummary::counts`
  doctest updated for the new `CaseResult.diff` field). No existing test
  weakened or deleted; the byte comparison is unchanged.

## Gate results (crate-scoped only; concurrent `internal/ls` lane active)

- `cargo test -p tsgo_testrunner` — GREEN (47 unit + 11 doctests; the 150-case
  smoke runs ~35 s on the large-stack thread).
- `cargo clippy -p tsgo_testrunner --all-targets -- -D warnings` — GREEN.
- `cargo fmt -p tsgo_testrunner -- --check` — GREEN.
- `cargo build -p tsgo_testrunner` — GREEN.

Did not run `--workspace` (concurrent lane). `tsgo_testutil_harnessutil` not
touched, so not gated separately. Public API ADDITIVE within `tsgo_testrunner`
only (`CaseResult` gains a field; the new `failure_category` surface is all new).
No `--no-verify`. Root `Cargo.toml` and `internal/testrunner/Cargo.toml`
untouched (no new dependency). Did not edit `internal/ls`/`checker`/`compiler`/
`harnessutil`.

## DEFER list (unchanged + this round)

- **`.js`/`.types`/`.symbols`/sourcemap baselines**, **module/target variation
  matrix**, **in-test `tsconfig.json`/symlinks**, **fourslash**, and
  **`local`-baseline writes** — all still deferred (see Round 1).
- **Multi-line span fidelity** — the squiggle parser records only the first
  line's tilde run for a multi-line span (a deterministic proxy used solely for
  `wrong_span`). blocked-by: not needed for the code histogram; full span
  reconstruction would re-derive the multi-line squiggle geometry.
- **Full corpus run** — still a curated 150-case subset (the signal is
  sufficient to prioritize). blocked-by: triaging more stress/recursion cases
  beyond the two-entry denylist (some risk uncatchable stack-overflow aborts).

---

# Round 3 — checker-parity: knock out the cascading TS2304/TS2339 roots

Round goal: attack the DOMINANT P10 false-positive diagnostics — `extra TS2304`
(Cannot find name) ×82 + `extra TS2339` (Property does not exist) ×76 — by
fixing the FEW root symbol-resolution gaps that cascade them. SOLO lane (deep
chain editable). Strict TDD red→green. Tree clean at `a741514a`. Edits limited
to `internal/checker/**` (the two root fixes) + `internal/compiler/**` test only
(real-lib gate tests) + `internal/testrunner/**` (re-measured characterization)
+ this worklog. No production `internal/compiler`/`ast`/`parser`/`binder` change.

## Headline — measured parity BEFORE → AFTER

```
BEFORE (Round 2):  150 cases — passed 55, failed 95, errored 0
                   extra: TS2304 ×82, TS2339 ×76
AFTER  (Round 3):  150 cases — passed 60, failed 90, errored 0
                   extra: TS2304 ×62, TS2339 ×18
```

- **passed 55 → 60 (+5)**, failed 95 → 90 (−5), errored 0 (unchanged).
- **extra TS2304: 82 → 62 (−20)** — all lib-global-VALUE 2304s cleared
  (`console`/`Error`/`Object`/`Date`/`Promise` no longer appear).
- **extra TS2339: 76 → 18 (−58)** — the `error`/`any`-receiver cascade is gone.
- Category shift: `no_baseline_but_errors` 36→31, `divergent` 30→26,
  `missing_all_errors` 29→33 (a few `divergent` cases lost their spurious extras
  and are now pure false-negatives — i.e. we removed false positives, leaving
  only the genuinely-missing errors). `top_missing(1)` unchanged: `TS7026 ×15`.
- Byte comparison unchanged; no diagnostic blanket-suppressed; no test weakened.

## Root causes diagnosed + fixed (2 of 4 candidate roots)

The cascade was driven by TWO root gaps (the histogram receiver-type tally was
decisive: **58 of the 76 `extra TS2339` had receiver type `'error'`** — a direct
downstream cascade of the unresolved-name 2304s, not independent failures):

1. **`checkIdentifier` dropped the globals scope** (`core/check.rs:check_identifier`).
   It passed `None` for `resolveName`'s globals table, so a bare identifier
   referencing a global VALUE (a lib global like `Error`/`Object`/`Date`, or any
   cross-file global declaration) never consulted the program's merged globals
   and cascaded into a spurious `TS2304` (and a follow-on `TS2339` on its
   `error`-typed members). Go's `resolveName` ALWAYS consults `c.globals`.
   **Fix (1 line + comment):** pass `program.globals()`. This was the only
   `resolve_name` call site in `check.rs` passing `None`; every other call
   (`new_expression_class_symbol`, the type-reference paths, the `Array` global)
   already threaded `globals`.
   - Repro / Go ground truth: `assertsPredicateParameterMismatch.ts` — tsc emits
     ONE `TS1225` and resolves `new Error(...)` / `console.log(...)`; we emitted
     `TS2304: Cannot find name 'Error'` + `'console'` + cascade. Even a bare
     `throw new Error('x')` / `const e = Error;` reproduced it.
   - RED→GREEN: `tsgo_checker` `bare_identifier_resolves_against_merged_globals`
     (file A `declare var GlobalThing`, file B references it → was 2304, now
     clean) + guard `bare_identifier_not_in_globals_still_reports_2304`.
     `tsgo_compiler` real-lib `bare_lib_global_value_reference_resolves_no_2304`
     (`Error;Object;Date;` → no 2304) + guard
     `bare_undefined_name_still_reports_2304_with_real_lib`.
   - Go: `internal/checker/checker.go:Checker.checkIdentifier` → `resolveName`
     (consults `c.globals`).

2. **`checkPropertyAccessExpression` did not short-circuit an any-like receiver**
   (`core/check.rs:check_property_access`). Go's
   `checkPropertyAccessExpressionOrQualifiedName` returns the apparent type
   immediately when `isTypeAny(apparentType)` — and Go's `errorType` carries the
   `Any` flag — so accessing any member of `any`/`error` yields that type with NO
   `TS2339`. We ran the member lookup unconditionally, so (a) `any.<x>` wrongly
   reported `Property does not exist on type 'any'`, and (b) every unresolved
   name (typed `error`) added a spurious `Property does not exist on type
   'error'` on top of its 2304 — **the cascade amplifier behind the dominant
   `extra TS2339`**.
   **Fix (3 lines + comment):** if the (narrowed) receiver type intersects
   `TypeFlags::ANY`, return it directly. Both `any_type` and `error_type` are
   intrinsics with the `ANY` flag, so one check covers both.
   - Repro / Go ground truth: `checkInheritedProperty.ts` — tsc emits one
     `TS2729`; we emitted `Property 'b' does not exist on type 'any'` TWICE
     (`this` degraded to `any`). The CommonJS / export-assignment cases
     (`exportAssignmentMerging*`, `cjsExportGenericTypes`, ...) emitted the
     `'error'`-receiver cascade on every unresolved-name member access.
   - RED→GREEN: `tsgo_checker` `property_access_on_any_reports_no_diagnostic`
     (`declare const x: any; x.whatever;` → was 2339, now clean) +
     `property_access_on_unresolved_name_reports_only_2304` (only the 2304, no
     cascade). `tsgo_compiler` real-lib
     `unresolved_name_property_access_reports_only_2304_no_cascade` +
     `property_access_chain_on_any_reports_no_2339` (`a.b.c.d` on `any`).
   - Guards proving we did NOT mute the diagnostic: the pre-existing
     `missing_property_reports_diagnostic` /
     `union_property_absent_from_one_constituent_reports_2339` still report 2339
     on a REAL object missing a property (kept green, untouched).
   - Go: `internal/checker/checker.go:Checker.checkPropertyAccessExpressionOrQualifiedName`
     (`isAnyLike` early return).

## DEFERRED roots (blocked-by) — the remaining `extra TS2304 ×62`

Two of the four candidate roots were deferred (substantial features, out of a
surgical round; the remaining 62 `extra TS2304` are dominated by these):

- **CommonJS JS-file globals** (`module` ×14, `require` ×5, `exports` ×5 — the
  single biggest remaining bucket). Root cause is a COMPILER-level gate, not a
  checker gap: tsc does NOT type-check un-`checkJs` `.js`/`.cjs` files
  (`skipTypeChecking`), so it emits no semantic diagnostics for them at all; we
  run the checker over them and surface spurious `module`/`require`/`exports`
  2304s. blocked-by: `Program.getBindAndCheckDiagnosticsForFile` /
  `skipTypeChecking` (a `internal/compiler` change — OUT of this round's checker
  edit scope). Cases: `cjsExportGenericTypes`, `erasableSyntaxOnlyJS`,
  `exportAssignmentMerging5/6`, `expandoNoInferredIndex`.
- **TS `import x = require()` / `export =` alias resolution** (`a` ×9, `foo`,
  `C`, `A`, `Foo`, ...). `import a = require("./a")` does not bind/resolve `a` as
  an alias value, so `a.<x>` reports `Cannot find name 'a'`. blocked-by: the full
  module import/export + alias resolution (`resolveExternalModuleSymbol` /
  `resolveAlias` — the `skip_alias` DEFER in `core/symbols.rs`), a later checker
  round. Cases: `exportAssignmentMerging1/2/3/4`, `cjsExportGenericTypes` (b.ts),
  `declarationEmitQualifiedName`.
- **Expando functions / namespace-function merging** (`declarationEmitExpandoFunction`,
  `expandoFunctionAsAssertion`, `expandoPropertyEmptyArrayWidening`, ...):
  `function f(){}; f.a = …; f.a` — the function-symbol expando-property merge is
  not modeled. blocked-by: binder/checker expando-property assignment + the
  function-namespace merge.
- **JSX intrinsic-elements (`TS7026 ×15`, top false-NEGATIVE)** and **parser
  error-recovery false positives (`TS1005 ×9` / `TS1003 ×5`, `''` 2304s)** —
  unchanged from Round 2; separate JSX-checking / parser-recovery lanes.

## Test deltas

- `tsgo_checker`: **737 → 741** unit tests (+4): `bare_identifier_resolves_against_merged_globals`,
  `bare_identifier_not_in_globals_still_reports_2304`,
  `property_access_on_any_reports_no_diagnostic`,
  `property_access_on_unresolved_name_reports_only_2304`. Doctests unchanged
  (177). No existing test weakened.
- `tsgo_compiler`: **84 → 88** unit tests (+4, real-lib gate, two per root):
  `bare_lib_global_value_reference_resolves_no_2304`,
  `bare_undefined_name_still_reports_2304_with_real_lib`,
  `unresolved_name_property_access_reports_only_2304_no_cascade`,
  `property_access_chain_on_any_reports_no_2339`. Doctests unchanged (11).
- `tsgo_testrunner`: unit/doctest counts unchanged (47 / 11); the
  `expanded_compiler_subset_parity_smoke` characterization re-measured to
  `{passed: 60, failed: 90, errored: 0}` + `top_extra == [(2304,62),(2339,18)]` +
  category `{no_baseline 31, missing_all 33, divergent 26}`. The 30-case
  `curated_compiler_subset_parity_smoke` is UNCHANGED (18/12/0) and stayed green.

## Gate results (Round 3)

- `cargo test -p tsgo_checker` — GREEN (741 unit + 177 doctests).
- `cargo test -p tsgo_compiler` — GREEN (88 unit + 11 doctests) [real-lib path].
- `cargo test -p tsgo_testrunner` — GREEN (47 unit + 11 doctests; 150-case
  re-measure).
- `cargo clippy` + `cargo fmt --check` on the edited crates — GREEN.
- `cargo build --workspace --all-targets` — GREEN.

No `--no-verify`; no test weakened or deleted; the byte comparison and the
30-case smoke are unchanged. Public API additive only (no signature changes; the
two checker fixes are internal to `check.rs`).

---

# Round 4 — CommonJS JS-file globals: the bind-and-check gate + `require(...)`

Round goal: attack the largest remaining `extra TS2304` sub-cluster — bare
references to the CommonJS ambient globals `module` (×~14), `require` (×~5),
`exports` (×~5) inside JS files. SOLO lane. Strict TDD red→green. Tree clean.
Edits limited to `internal/compiler/{program.rs,host.rs,program_test.rs}` +
`internal/checker/core/{check.rs,check_test.rs,test_support.rs}` +
`internal/testrunner/compiler_runner_test.rs` (re-measured characterization) +
this worklog. No production `internal/binder`/`ast`/`parser` change.

## ROOT-CAUSE CORRECTION (the prior round's diagnosis was wrong)

Round 3 deferred this cluster as a "COMPILER-level gate": *"tsc does NOT
type-check un-`checkJs` `.js`/`.cjs` files (`skipTypeChecking`), so it emits no
semantic diagnostics for them at all."* **Verified against the Go source and the
committed corpus baselines, that is FALSE for this repo.** The Go ground truth:

```go
// internal/compiler/program.go
func (p *Program) canIncludeBindAndCheckDiagnostics(sourceFile *ast.SourceFile) bool {
	if sourceFile.CheckJsDirective != nil && !sourceFile.CheckJsDirective.Enabled {
		return false // @ts-nocheck
	}
	if sourceFile.ScriptKind == core.ScriptKindTS || ...TSX || ...External {
		return true
	}
	isJS := ...JS || ...JSX
	isCheckJS := isJS && ast.IsCheckJSEnabledForFile(sourceFile, p.Options())   // checkJs==true
	isPlainJS := ast.IsPlainJSFile(sourceFile, p.Options().CheckJs)             // JS && checkJs UNSET
	return isPlainJS || isCheckJS || sourceFile.ScriptKind == core.ScriptKindDeferred
}
```

`isPlainJS` is **true** for a `.js` file with `checkJs` unset → Go *DOES*
bind-and-check plain JS, and check-JS JS, by default. It skips a JS file ONLY
when `checkJs` is explicitly `false` or there is a `// @ts-nocheck`. The
committed baselines confirm tsc type-checks these JS files: it emits
`TS2591` (`module`, the "do you need `@types/node`?" variant —
`exportAssignmentMerging6`), `TS2339`/`TS7022` (`expandoNoInferredIndex`),
`TS6424`/`TS6425` (`multipleModuleExportsAssignments`), `TS2306`
(`nestedJSDocImportType`) located *inside* the `.js`/`.cjs` files. Every cited
corpus case (`cjsExportGenericTypes`, `erasableSyntaxOnlyJS`,
`exportAssignmentMerging5/6`, `expandoNoInferredIndex`) sets `// @checkJs: true`,
so a "skip un-`checkJs` JS" gate would not touch them at all (and would *regress*
the plain-JS cases that carry committed JS-file baselines).

**Therefore the cluster's real root is checker/binder CommonJS resolution, not a
compiler gate**: tsc resolves `module`/`exports`/`require` because (a) the binder
recognizes the CommonJS module pattern and declares `module`/`exports`
(`setCommonJSModuleIndicator` + `declareCommonJSVariable`), and (b) `resolveName`
returns the synthetic `requireSymbol` for a `require(...)` callee.

## What landed (two Go-faithful, surgical changes)

### (1) `require(...)` resolution — clears the `require` sub-cluster (the parity win)

`check_identifier`'s name-not-found branch now mirrors Go's `resolveName`: when a
bare identifier is unresolved AND it is the callee of a `require(...)` call in a
JS file, it resolves to the synthetic `require` symbol (type `any`) instead of
reporting 2304. The reachable subset returns `any` directly (flow-narrowing a
fresh `any` callee is a no-op), which is observationally identical to typing the
require symbol.

- Added two private helpers in `check.rs`: `is_in_js_file` (node carries
  `NodeFlags::JAVA_SCRIPT_FILE`, set by the parser for `.js`/`.jsx`) and
  `is_require_call` (call expression, callee identifier `require`, exactly one
  argument).
- Go: `internal/binder/nameresolver.go:Resolve` (RequireSymbol branch) +
  `internal/ast/utilities.go:IsRequireCall` / `IsInJSFile` +
  `internal/checker/checker.go:Checker.getTypeOfSymbol` (`requireSymbol` → `any`).

### (2) Go-faithful `SkipTypeChecking` gate in `program.rs` (correctness; parity-neutral)

Ported `Program::skip_type_checking` + `Program::can_include_bind_and_check_diagnostics`
1:1 from Go and wired them into BOTH semantic-diagnostics collectors
(`semantic_diagnostics` + `semantic_diagnostics_by_file`) via a shared
`is_excluded_from_semantic_diagnostics` mask (alongside the existing default-lib
exclusion). `.ts`/`.tsx`/external → always checked; `.js`/`.jsx` → checked iff
plain JS (`checkJs` unset) or check-JS (`checkJs: true`); `checkJs: false` →
skipped. `effective_script_kind` (host.rs) made `pub(crate)` so the gate reads
the same script kind the file was parsed with. This is parity-neutral on the
corpus (no case uses `checkJs: false`/`@ts-nocheck`) but corrects a real gap: we
previously bind-and-checked a `checkJs: false` JS file and emitted spurious
2304s.
- DEFER: the `// @ts-check` / `// @ts-nocheck` directive
  (`SourceFile.CheckJsDirective`) is not parsed yet, so the directive arms are
  not modeled (matches Go exactly when no directive is present — all corpus
  cases). blocked-by: the parser's check-js directive scan.
- Go: `internal/compiler/program.go:Program.SkipTypeChecking` /
  `canIncludeBindAndCheckDiagnostics` + `internal/ast/utilities.go:IsPlainJSFile`
  / `IsCheckJSEnabledForFile`.

## RED→GREEN slices

1. **`require(...)` callee resolves (JS)** — `tsgo_checker`
   `require_call_in_js_file_resolves_no_cannot_find_name`
   (`const a = require("./x")` in `/a.js`). RED: `[TS2304 Cannot find name
   'require']` → GREEN: none. Plus a real-lib `tsgo_compiler`
   `require_call_in_js_file_resolves_no_2304_with_real_lib` (the path the parity
   runner drives).
2. **Guard — bare `require` (not a call) still 2304** —
   `bare_require_reference_in_js_file_still_reports_2304` (`require;` in `/a.js`).
   Green throughout (resolution conditioned on `IsRequireCall`).
3. **Guard — `require(...)` in a TS file still 2304** —
   `require_call_in_ts_file_still_reports_2304` (gated on `IsInJSFile`).
4. **`checkJs: false` JS is skipped** — `tsgo_compiler`
   `js_file_with_check_js_false_is_not_checked` (`module.exports = {}`). RED:
   `[TS2304 Cannot find name 'module']` → GREEN: none.
5. **Guard — plain JS (`checkJs` unset) is STILL checked** —
   `plain_js_file_is_still_checked` (proves NOT over-suppression; matches Go's
   `isPlainJS` branch → 2304 on `module`).
6. **Guard — `checkJs: true` JS is checked** —
   `js_file_with_check_js_true_is_checked` (gate conditioned on `checkJs`).
7. **Guard — TS is always checked regardless of `checkJs`** —
   `ts_file_is_checked_regardless_of_check_js` (`checkJs: false` + `/index.ts` →
   2304).

## Headline — measured parity BEFORE → AFTER (150-case subset)

```
BEFORE (Round 3):  150 cases — passed 60, failed 90, errored 0
                   extra: TS2304 ×62, TS2339 ×18
AFTER  (Round 4):  150 cases — passed 60, failed 90, errored 0
                   extra: TS2304 ×57, TS2339 ×18
```

- **extra TS2304: 62 → 57 (−5)** — the entire `require` sub-cluster cleared
  (`require(...)` callees across `exportAssignmentMerging5/6`,
  `multipleModuleExportsAssignments`, `cjsExportGenericTypes`, the `main.js`
  cases). No other extra/missing code changed (full histogram diffed
  byte-for-byte BEFORE vs AFTER; `TS2345 ×8` etc. were already at those values —
  the Round 3 worklog recorded only `top_extra(2)`, not the full histogram).
- **passed 60 → 60, failed 90 → 90, errored 0** — no case flips to PASS because
  the `module`/`exports` extras (the deferred CommonJS-binding root) remain.
- Category shift: `divergent` 26 → 25, `missing_all_errors` 33 → 34 — one case
  (a `require`-only-extra divergent case) lost its sole false positive and is now
  a pure false-negative. `no_baseline_but_errors` 31 (unchanged),
  `top_missing(1)` `TS7026 ×15` (unchanged).
- Byte comparison unchanged; no diagnostic blanket-suppressed; no test weakened.

## DEFERRED sub-roots (blocked-by) — the remaining `module`/`exports` 2304s

- **CommonJS module binding (`module` / `exports`)** — the bulk of the remaining
  sub-cluster. tsc resolves `module`/`exports` because the binder detects the
  CommonJS module pattern (`module.exports = X`, `exports.x = Y`, `require()`)
  and declares the `module` (+`exports` member) and `exports` file locals; the
  checker then types the `SymbolFlagsModuleExports` symbols. The Rust binder has
  a `common_js_module_indicator` field but never sets it ("bindDeferredExpando
  Assignments is JS/CommonJS only and is deferred"). This is a multi-behavior
  binder+checker feature (assignment-pattern classification, file-symbol
  creation via `bindSourceFileAsExternalModule`, `SymbolFlagsModuleExports`
  type-of-symbol, and the `TS2591` "@types/node" special-case for `module`/
  `require`/`process`/`Buffer`/`NodeJS` in non-CJS contexts), not a surgical
  round. blocked-by: `internal/binder/binder.go:setCommonJSModuleIndicator` /
  `declareCommonJSVariable` / `bindModuleExportsAssignment` /
  `bindExportsOrObjectDefineProperty` + `internal/checker/checker.go:
  getTypeOfSymbol` (`SymbolFlagsModuleExports`) + `getCannotFindNameDiagnostic`
  (TS2591). Cases: `exportAssignmentMerging5/6`, `cjsExportGenericTypes`,
  `erasableSyntaxOnlyJS`, `expandoNoInferredIndex`, `multipleModuleExportsAssignments`,
  `nestedJSDocImportType`, `numericExportNameDeclaration`,
  `jsDeclarationExportDefaultAssignmentCrash`, `jsDeclarationsRequireImportForms`.
- **`// @ts-check` / `// @ts-nocheck` directive parsing** — the gate's directive
  arms are stubbed (DEFER). blocked-by: the parser's check-js directive scan +
  `CheckJsDirective` on the source file. (No corpus case in the subset uses a
  directive, so this does not affect the current parity.)
- **TS `import x = require()` / `export =` alias resolution** (the `a`/`foo`/`C`
  2304s in `exportAssignmentMerging1-4`) — unchanged from Round 3
  (`resolveExternalModuleSymbol` / `resolveAlias`, the `skip_alias` DEFER).
- **JSX intrinsic-elements (`TS7026 ×15`)** + **parser error-recovery
  (`TS1005 ×9` / `TS1003 ×5`)** — unchanged from Round 2/3.

## Test deltas

- `tsgo_checker`: **741 → 744** unit (+3): `require_call_in_js_file_resolves_no_cannot_find_name`,
  `bare_require_reference_in_js_file_still_reports_2304`,
  `require_call_in_ts_file_still_reports_2304`. Doctests unchanged (177). New
  test-support helper `StubProgram::parse_and_bind_js`. No existing test
  weakened.
- `tsgo_compiler`: **88 → 93** unit (+5): `require_call_in_js_file_resolves_no_2304_with_real_lib`
  (real-lib require), `js_file_with_check_js_false_is_not_checked` (gate),
  `plain_js_file_is_still_checked`, `js_file_with_check_js_true_is_checked`,
  `ts_file_is_checked_regardless_of_check_js`. Doctests unchanged (11).
- `tsgo_testrunner`: unit/doctest counts unchanged (47 / 11); the
  `expanded_compiler_subset_parity_smoke` characterization re-measured to
  `extra TS2304 ×62 → ×57`, category `missing_all 33→34`, `divergent 26→25`
  (counts `{60,90,0}` and `top_missing TS7026 ×15` unchanged). The 30-case
  `curated_compiler_subset_parity_smoke` is UNCHANGED (18/12/0).

## Gate results (Round 4)

- `cargo test -p tsgo_checker` — GREEN (744 unit + 177 doctests).
- `cargo test -p tsgo_compiler` — GREEN (93 unit + 11 doctests) [real-lib path].
- `cargo test -p tsgo_testrunner` — GREEN (47 unit + 11 doctests; 150-case
  re-measure).
- `cargo clippy -p tsgo_checker -p tsgo_compiler -p tsgo_testrunner --all-targets
  -- -D warnings` — GREEN.
- `cargo fmt -p tsgo_checker -p tsgo_compiler -p tsgo_testrunner -- --check` —
  GREEN.
- `cargo build --workspace --all-targets` — GREEN.

No `--no-verify`; no test weakened or deleted; the byte comparison and the
30-case smoke are unchanged. Public API additive only (the two `program.rs` gate
methods + the `is_excluded_from_semantic_diagnostics` mask are private; the
`check.rs` require resolution is internal to `check_identifier`;
`effective_script_kind` widened to `pub(crate)`).
