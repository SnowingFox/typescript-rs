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

---

# Round 5 — TS7026 (JSX intrinsic implicit-any) leverage probe → STOP (gate)

Round goal: close the top FALSE-NEGATIVE `missing TS7026 ×15`
("JSX element implicitly has type 'any' because no interface
'JSX.IntrinsicElements' exists"). Per the round's Step-0 gate, MEASURE the real
case-flip leverage of TS7026 *before* implementing, and STOP + report if fewer
than ~4 cases would flip. **Outcome: STOP — TS7026-only flips exactly 1 case
(< 4).** No production code changed (a throwaway measurement test was added,
run, and reverted; tree left clean).

## Step-0 leverage measurement (the deliverable of this probe)

The 150-case curated subset (`curated_subset(25, 150, EXPANDED_DENYLIST)`)
contains **9** `.tsx` cases. I read each committed `.errors.txt` baseline AND
ran each case through `error_baseline_for_test` (the real-lib parity path) to
get the EXACT current produced baseline + categorized mismatch. The `15` missing
TS7026 diagnostics live in **3** cases; only **1** is TS7026-ONLY:

| case (`@jsx`) | committed codes | TS7026-only? | current produced | flips on… |
|---|---|---|---|---|
| `jsxMultilineAttributeStringValues2` (`preserve`) | 4×**7026** | **YES** | `<no content>` | **TS7026 → FLIPS** |
| `jsxEntityDecoderAfterNonEntityAmpersand` (`react`) | 10×7026 + 5×2874 | no | `<no content>` | TS7026 **+ TS2874** |
| `jsxAttributeValueBinaryExpression` (`preserve`) | 2304 + 2×7026 + 2657 | no | 2304 + 1128 + 2×2304 + 1109 + 1161 (parser divergence) | needs parser-recovery + 7026 + 2657 |
| `jsxElementTypeUnexpectedType` (`react`) | 2874 | n/a (no 7026) | `<no content>` | TS2874 |
| `jsxLibraryManagedAttributesUnexpectedType` (`react`) | 2874 | n/a | `<no content>` | TS2874 |
| `jsxSpreadWithAssertion` (`react-jsx`) | 2875 | n/a | `<no content>` | TS2875 |
| `emitReactJsxSelfClosingElement` (`react-jsx`) | 2875 + 2552 | n/a | 2304 (`App`) | TS2875 + TS2552-suggestion |
| `jsxNestedIndentation` (`react`) | — (clean) | guard | `<no content>` (PASS) | must NOT regress |
| `jsxPragmaAfterTags` (`react`+`@jsx h`) | — (clean) | guard | `<no content>` (PASS) | must NOT regress |

`missing TS7026 ×15` = `jsxMultiline` (4) + `jsxEntityDecoder` (10) +
`jsxAttributeValueBinaryExpression` (1; the other co-locates with our `TS1128`
as a `wrong_code`).

**Flip leverage by feature scope (measured):**
- **TS7026 alone** → **1** case (`jsxMultilineAttributeStringValues2`).
  `< ~4` → the Step-0 gate says STOP.
- **TS7026 + TS2874** (the React-in-scope check, `markJsxAliasReferenced`) →
  **4** cases (adds `jsxEntityDecoder`, `jsxElementTypeUnexpectedType`,
  `jsxLibraryManagedAttributesUnexpectedType`).
- **+ TS2875** (automatic-runtime `react/jsx-runtime` check) → **5** cases
  (adds `jsxSpreadWithAssertion`).

So the real win is the **whole** JSX opening-element implicit-any/scope check
(TS7026 **+ TS2874 + TS2875**, all emitted from one Go function chain), not
TS7026 in isolation — and that is a large feature with two hard blockers below.

## Why TS7026 cannot be cheaply expanded to TS2874/TS2875 in a surgical round

`TS7026`, `TS2874`, `TS2875` all originate in
`checkJsxOpeningLikeElementOrOpeningFragment` (`jsx.go:129`):
`checkJsxPreconditions` → `markJsxAliasReferenced` (TS2874/TS2875) →
`getResolvedSignature` → `getIntrinsicAttributesTypeFromJsxOpeningLikeElement` →
`getIntrinsicTagSymbol` (TS7026). Co-implementing the siblings is blocked by:

- **TS2874** needs `@jsx`/`@jsxfrag` **pragma scanning** (Go
  `getLocalJsxNamespace` / `GetPragmaFromSourceFile`). Without it, the guard case
  `jsxPragmaAfterTags` (a `/** @jsx h */` fileoverview pragma; `h` is declared,
  `React` is NOT) would resolve the factory namespace to the default `"React"`,
  fail `resolveName("React")`, and emit a **spurious TS2874** → a real
  false-positive regression of a currently-PASSING case. The Rust parser
  explicitly DEFERS pragmas (`internal/parser/lib.rs:386` "DEFER(phase-4):
  comment directives, pragmas … blocked-by: JSDoc/pragma scanning subsystem").
- **TS2875** needs the **JSX-runtime module resolution** path
  (`getJsxNamespaceContainerForImplicitImport` → `program.GetJSXRuntimeImportSpecifier`
  → `resolveExternalModule("react/jsx-runtime", …)`), which is checker/compiler
  module-resolution plumbing not yet wired for the implicit JSX import.

## Go ground truth (read; anchors for the eventual implementation)

- TS7026 predicate: `internal/checker/jsx.go:getIntrinsicTagSymbol` (1215) — for
  an intrinsic tag, `getJsxType(IntrinsicElements, node)` (1294) is `errorType`
  AND `c.noImplicitAny` → `c.error(node,
  JSX_element_implicitly_has_type_any_because_no_interface_JSX_0_exists,
  "IntrinsicElements")` (1252). Span = the **element node** (opening /
  self-closing / closing element), NOT the tag name. Paired `<div>…</div>`
  reports TWICE: opening (via `getResolvedSignature` in
  `checkJsxOpeningLikeElementOrOpeningFragment`) and closing (via
  `checkJsxElementDeferred` (76) calling `getIntrinsicTagSymbol(closingElement)`
  when `isJsxIntrinsicTagName`).
- `noImplicitAny` is `compilerOptions.GetStrictOptionValue(NoImplicitAny)`
  (`checker.go:918`); `GetStrictOptionValue` returns `Strict != TSFalse`
  (`compileroptions.go:292`), i.e. **true by default** in this model — which is
  why these non-`strict` cases DO get TS7026. The Rust `Checker` mirrors this
  (`mod.rs:get_strict_option_value`), so `no_implicit_any()` would be true by
  default too. `isJsxIntrinsicTagName` = lowercase-initial / namespaced tag.
- TS2874: `checker.go:markJsxAliasReferenced` (28178) — `jsxFactoryRefErr =
  (Jsx == JsxEmitReact) ? TS2874 : nil`; `resolveName(tagName, getJsxNamespace,
  Value, jsxFactoryRefErr, …)` errors when the factory namespace (default
  `React`, or the `@jsx`/`jsxFactory`/`reactNamespace` override) is not a value
  in scope. `getJsxNamespace` = `jsx.go:1340`.
- TS2875: `jsx.go:getJsxNamespaceContainerForImplicitImport` (1450) →
  `resolveExternalModule(specifier, moduleReference, TS2875, …)` (1465).

## Rust landing site (for the eventual implementation)

`internal/checker/core/jsx.rs` already has a reachable JSX core. The TS7026 hook
is `get_jsx_intrinsic_attributes_type` (238): today it returns `None` (no error)
when the injected `jsx_intrinsic_elements` table is absent (the real-lib path),
which is exactly the current false-negative. The Go-faithful change would
resolve the real `JSX.IntrinsicElements` type (a `getJsxType`-style
`resolve_name("JSX", NAMESPACE)` → exports → `IntrinsicElements` type →
`get_declared_type_of_symbol`; primitives all exist:
`symbols.rs:resolve_name`, `program.symbol(_).exports`,
`declared_types.rs:get_declared_type_of_symbol`), gate the error on a new
`no_implicit_any()` (mirroring `strict_null_checks()`), emit on the **element**
node, and add the closing-element resolution from `checkJsxElementDeferred`.
Blast radius is contained to `.tsx` cases (a `.ts` `<T>x` parses as a type
assertion, never JSX), and the 2 clean guards are value-elements (no intrinsic
tag → no TS7026), so TS7026-only is regression-free.

## Recommendation (for the parent to redirect)

TS7026 in isolation is a small, regression-free change worth **+1 PASS** (and
collapses `missing TS7026 ×15 → ×1`, emitting 14/15 of the false-negatives), but
it does NOT meet the Step-0 ~4-case bar on its own. To bank the full **+5**, the
prerequisite is to first land (a) `@jsx`/`@jsxfrag` **pragma scanning** in the
parser (unblocks TS2874 without regressing `jsxPragmaAfterTags`) and (b) the
**implicit JSX-runtime module resolution** (unblocks TS2875); then implement the
whole `checkJsxOpeningLikeElementOrOpeningFragment` precondition/alias-reference
check (TS7026 + TS2874 + TS2875) as one cohesive feature. Per the gate, this
round STOPS here for the parent to choose: ship the +1 TS7026-only slice now, or
sequence the two prerequisite subsystems first for the +5.

No production code changed this round (measurement test reverted; `cargo test
-p tsgo_testrunner` and the rest of the tree are untouched/green at the prior
Round-4 numbers `{passed 60, failed 90, errored 0}`, `top_missing TS7026 ×15`).

---

# Round 6 — TS7026 (JSX intrinsic implicit-any) implementation → +1 PASS

Round goal: land the TS7026-ONLY slice the Round-5 probe scoped — emit
"JSX element implicitly has type 'any' because no interface
'JSX.IntrinsicElements' exists." for the exact Go condition, and NOTHING more
(TS2874 / TS2875 stay DEFERRED behind their two unbuilt subsystems). SOLO lane,
strict TDD red→green. Edits limited to `internal/checker/**`,
`internal/compiler/{boundfile.rs,multifile.rs,program_test.rs}`,
`internal/testrunner/compiler_runner_test.rs` (re-measured characterization) +
this worklog. No `internal/binder`/`parser`/`ast` production change.

## Go ground truth (ported predicate)

`internal/checker/jsx.go:getIntrinsicTagSymbol` (1215): for an intrinsic tag,
`intrinsicElementsType := c.getJsxType(JsxNames.IntrinsicElements, node)` (1220);
when `c.isErrorType(intrinsicElementsType)` (the `JSX` namespace / its
`IntrinsicElements` member cannot be resolved) AND `c.noImplicitAny` (1251) →
`c.error(node, diagnostics.JSX_element_implicitly_has_type_any_because_no_interface_JSX_0_exists, "IntrinsicElements")`.
The diagnostic is on the **element node** (opening / self-closing / closing). A
paired `<div>…</div>` reports TS7026 **twice** — `checkJsxElementDeferred` (76)
checks the opening element via `checkJsxOpeningLikeElementOrOpeningFragment` →
`getResolvedSignature` → `getIntrinsicAttributesTypeFromJsxOpeningLikeElement`
→ `getIntrinsicTagSymbol(openingElement)`, then resolves the closing tag via
`getIntrinsicTagSymbol(closingElement)` (when `isJsxIntrinsicTagName`); a
self-closing `<div/>` reports once. `noImplicitAny =
compilerOptions.GetStrictOptionValue(NoImplicitAny)` (`checker.go:918` +
`compileroptions.go:292`, `Strict != TSFalse`) → **true by default**, which is
why these non-`strict` `.tsx` cases DO get TS7026. The span is the node's
error range (`scanner.GetErrorRangeForNode` default case: `SkipTrivia(text,
node.Pos())..node.End()`) — the element `pos` is its FULL-start (the leading
whitespace before `<` is included), so the start MUST skip trivia.

Confirmed `TS7026` text/code byte-identical to the committed baselines:
`diagnostics_generated.rs:JSX_ELEMENT_IMPLICITLY_HAS_TYPE_ANY_BECAUSE_NO_INTERFACE_JSX_0_EXISTS`
(`code: 7026`, `"JSX element implicitly has type 'any' because no interface
'JSX.{0}' exists."`), arg `"IntrinsicElements"`.

## The fix (Rust, surgical/additive)

- **`Checker::no_implicit_any()`** (`core/mod.rs`) — mirrors
  `strict_null_checks()`: `get_strict_option_value(options.no_implicit_any)`
  (true by default). Go: `NewChecker` (`c.noImplicitAny`).
- **`core/jsx.rs:get_intrinsic_tag_symbol`** (renamed predicate of
  `get_jsx_intrinsic_attributes_type`) now resolves `JSX.IntrinsicElements`
  Go-faithfully when no table is injected: new private
  **`get_jsx_type(program, name, location)`** does
  `resolve_name(location, "JSX", NAMESPACE)` → `JSX` symbol's `exports` →
  `getSymbol(name, TYPE)` → `get_declared_type_of_symbol`, returning
  `error_type` when the `JSX` namespace / member is absent (the reachable core
  of Go's `getJsxType` / `getJsxNamespaceAt` global fallback). When it is
  `error_type` and `no_implicit_any()` → emit TS7026 on the element node; when
  it resolves but lacks the tag → the existing TS2339 (now also on the element
  node, matching Go). A StubProgram-injected `JSX.IntrinsicElements` table still
  short-circuits resolution (keeps the existing unit tests green).
- **Closing-element resolution wired** (`check_jsx_element`, Go's
  `checkJsxElementDeferred`): intrinsic closing tag → `get_intrinsic_tag_symbol`
  (TS7026 on the closing element); value closing tag → `check_expression`. This
  is what makes a paired `<div>…</div>` report TS7026 twice (open + close).
- **Span = trivia-skipped element range** — new
  `Checker::error_skipping_leading_trivia` (`core/check.rs`) ports the default
  case of `scanner.GetErrorRangeForNode` (`SkipTrivia(text, node.Pos())..end`),
  used ONLY by the JSX TS7026 / TS2339 emits so all existing
  raw-range diagnostics are byte-unchanged. It reads the file text via a new
  `BoundProgram::source_text()` (default `None`; implemented on `StubProgram`,
  the compiler's `BoundFile` / `FileView` / `MultiFileBoundProgram`). Without
  this the element start landed on the whitespace before `<` (off by one
  column), turning every whitespace-preceded TS7026 into an `extra` + `missing`
  pair (the first measurement showed `extra TS7026 ×9`); skipping trivia made
  them byte-match `tsc` exactly.

**Left DEFERRED (NOT implemented, per scope):** **TS2874** (`markJsxAliasReferenced`,
the `This JSX tag requires 'React' to be in scope` check) — blocked-by
`@jsx`/`@jsxfrag` **pragma scanning** in the parser (`getLocalJsxNamespace` /
`GetPragmaFromSourceFile`), without which the guard case `jsxPragmaAfterTags`
would emit a spurious TS2874 regression. **TS2875** (automatic
`react/jsx-runtime` check) — blocked-by the implicit **JSX-runtime module
resolution** (`getJsxNamespaceContainerForImplicitImport` →
`GetJSXRuntimeImportSpecifier` → `resolveExternalModule`). Both originate from
the same `checkJsxOpeningLikeElementOrOpeningFragment` chain; implementing them
now would regress currently-passing cases.

## RED→GREEN slices (one behavior at a time)

`tsgo_checker` (`core/jsx_test.rs`), driven through `check_source_file` (the
real dispatch):

1. **self-closing `<div/>` → 1 TS7026** —
   `self_closing_intrinsic_without_jsx_intrinsic_elements_reports_one_ts7026`.
   RED (0 produced; `get_jsx_intrinsic_attributes_type` returned `None`) → GREEN.
2. **paired `<div></div>` → 2 TS7026 (open + close, distinct spans)** —
   `paired_intrinsic_element_without_jsx_intrinsic_elements_reports_two_ts7026`.
3. **span skips leading trivia** —
   `self_closing_intrinsic_ts7026_span_skips_leading_trivia` (`  <div/>` → start
   byte 2 = the `<`, length 6, NOT the node full-start byte 0). RED (start 0) →
   GREEN (`error_skipping_leading_trivia`).
4. **GUARD — declared `JSX.IntrinsicElements` suppresses TS7026** —
   `intrinsic_element_with_declared_jsx_intrinsic_elements_reports_no_ts7026`
   (`declare namespace JSX { interface IntrinsicElements { div: any } }` resolves
   via the real `get_jsx_type` path → no TS7026).
5. **GUARD — value element emits no TS7026** — `value_element_reports_no_ts7026`
   (a resolved `<Foo/>` is value-based, intrinsic-only check never fires).
6. **GUARD — `noImplicitAny` disabled suppresses TS7026** —
   `intrinsic_element_without_no_implicit_any_reports_no_ts7026`
   (`strict: false` + `noImplicitAny: false`).

`tsgo_compiler` (`program_test.rs`, REAL bundled-lib path the parity runner
drives):

7. **`jsx_intrinsic_self_closing_reports_one_ts7026_with_real_lib`** — a
   `@jsx: preserve` `.tsx` (jsxMultiline shape) → exactly ONE TS7026, no cascade.
8. **`jsx_intrinsic_paired_reports_two_ts7026_with_real_lib`** — paired `<div></div>`
   → exactly TWO TS7026 (open + close), nothing else.

No existing test weakened or deleted; the injected-table unit tests
(`check_intrinsic_self_closing_element_resolves`,
`unknown_intrinsic_tag_reports_diagnostic`, `attribute_type_mismatch...`) stay
green.

## Headline — measured parity BEFORE → AFTER (150-case subset)

```
BEFORE (Round 4):  150 cases — passed 60, failed 90, errored 0
                   missing: TS7026 ×15  | extra: TS2304 ×57, TS2339 ×18  | wrong_code: TS7026 ×1
                   categories: no_baseline 31, missing_all 34, divergent 25
AFTER  (Round 6):  150 cases — passed 61, failed 89, errored 0
                   missing: (TS7026 cleared)  | extra: TS2304 ×57, TS2339 ×18  | wrong_code: TS7026 ×1
                   categories: no_baseline 31, missing_all 32, divergent 26
```

- **passed 60 → 61 (+1)** — `jsxMultilineAttributeStringValues2` (4 self-closing
  intrinsic `<div .../>`, committed `4×7026`) flips to PASS, exactly as the probe
  predicted.
- **`missing TS7026 ×15` → cleared** — all 14 reachable false-negative 7026
  emit with byte-exact spans (`jsxMultiline` 4 + `jsxEntityDecoder*` opening 5 +
  closing 5); the 15th co-locates with our `TS1128` in
  `jsxAttributeValueBinaryExpression` (parser-recovery divergence) so it is a
  `wrong_code TS7026 ×1` (unchanged from before), not a `missing`. The probe's
  `×1` prediction lands as that wrong_code.
- **NO new `extra TS7026`** anywhere — the first measurement (pre-trivia-fix)
  showed `extra TS7026 ×9` from off-by-one spans; after `error_skipping_leading_trivia`
  the produced 7026 are byte-identical to `tsc`, so they pair away (0 extra).
  Verified the guard cases stay clean: `jsxNestedIndentation` PASS,
  `jsxPragmaAfterTags` PASS (both value-element-only), `jsxElementTypeUnexpectedType`
  still FAIL on its DEFERRED `TS2874` with no spurious 7026.
- **`extra TS2304 ×57`, `extra TS2339 ×18`, and EVERY other extra/missing/wrong
  bucket unchanged** — the full histogram was diffed BEFORE vs AFTER; the only
  delta is `missing TS7026 ×15` → removed. No regression.
- Category shift: `missing_all 34 → 32` (`jsxMultiline` → PASS,
  `jsxEntityDecoder` → divergent), `divergent 25 → 26`, `no_baseline 31`
  unchanged. Byte comparison unchanged; no diagnostic blanket-suppressed.

## Test deltas

- `tsgo_checker`: **744 → 750** unit (+6, the six slices above); **177 → 178**
  doctests (+1, `Checker::no_implicit_any`). New test-support:
  `StubProgram::parse_and_bind_tsx_with_options` + `StubProgram::source_text`.
- `tsgo_compiler`: **93 → 95** unit (+2, the two real-lib JSX gates); doctests
  unchanged (11). `BoundFile` / `FileView` / `MultiFileBoundProgram` gained
  `source_text`.
- `tsgo_testrunner`: unit/doctest counts unchanged (47 / 11); the
  `expanded_compiler_subset_parity_smoke` characterization re-measured to
  `{passed: 61, failed: 89, errored: 0}`, `missing_all 34→32`, `divergent 25→26`,
  `top_missing(1) == [(2874, 7)]` (was `[(7026, 15)]`), plus new asserts that
  `missing TS7026` and `extra TS7026` are both absent. The 30-case
  `curated_compiler_subset_parity_smoke` is UNCHANGED and green.

## Gate results (Round 6)

- `cargo test -p tsgo_checker` — GREEN (750 unit + 178 doctests).
- `cargo test -p tsgo_compiler` — GREEN (95 unit + 11 doctests) [real-lib path].
- `cargo test -p tsgo_testrunner` — GREEN (47 unit + 11 doctests; 150-case
  re-measure).
- `cargo test -p tsgo_transformers` — GREEN (311; sibling jsx-transform suite
  unaffected).
- `cargo clippy -p tsgo_checker -p tsgo_compiler -p tsgo_testrunner --all-targets
  -- -D warnings` — GREEN.
- `cargo fmt -p tsgo_checker -p tsgo_compiler -p tsgo_testrunner -- --check` —
  GREEN.
- `cargo build --workspace --all-targets` — GREEN.

No `--no-verify`; no test weakened or deleted; the byte comparison and the
30-case smoke are unchanged. Public API additive only (`Checker::no_implicit_any`,
`BoundProgram::source_text` with a `None` default; the JSX resolution + the
trivia-skipping emit are internal). No new dependency; root `Cargo.toml` /
`Cargo.lock` untouched. TS2874 / TS2875 left UNIMPLEMENTED (deferred, blocked-by
pragma scanning + implicit jsx-runtime module resolution).

## Round 7 — getCannotFindNameDiagnosticForName (specialized cannot-find-name codes)

**Root / Go ground truth.** An unresolved identifier was always reported as the
bare `TS2304` "Cannot find name '{0}'.". tsc instead dispatches on the name in
`internal/checker/checker.go:Checker.getCannotFindNameDiagnosticForName`
(~13821), passed by `getResolvedSymbol` to `resolveName` which emits it on
failure:
- `process` / `require` / `Buffer` / `module` / `NodeJS` → **TS2580** when
  `UsesWildcardTypes()` (`types: ["*"]`, `compileroptions.go:324`) else **TS2591**
  (install `@types/node`).
- `document` / `console` → **TS2584** (change `lib` to include `dom`).
- `Map`/`Set`/`Promise`/`Symbol`/`WeakMap`/`WeakSet`/`Iterator`/`AsyncIterator`/
  `SharedArrayBuffer`/`Atomics`/`AsyncIterable`/`AsyncIterableIterator`/
  `AsyncGenerator`/`AsyncGeneratorFunction`/`BigInt`/`Reflect`/`BigInt64Array`/
  `BigUint64Array` → **TS2583** (change target `lib` to '{1}' or later); the
  `{1}` lib is filled from `getSuggestedLibForNonExistentName`
  (`utilities.go:getFeatureMap` first-lib reduction).
- `$` → jQuery hints; `beforeEach`/`describe`/`suite`/`it`/`test` → test-runner
  hints; `Bun` → Bun hints (all wildcard-gated).
- `await` whose parent is a `CallExpression` → "Did you mean to write this in an
  async function"; otherwise FALLTHROUGH.
- parent is `ShorthandPropertyAssignment` → **TS18004**; default → **TS2304**.

**Rust landing.** `internal/checker/core/check.rs`:
`Checker::get_cannot_find_name_diagnostic_for_name(program, node)` reproduces the
switch (the emission lives in `check_identifier` because the Rust `resolve_name`
is a pure lookup); free fn `get_suggested_lib_for_non_existent_name(name)` ports
the feature-map first-lib table for the TS2583 `{1}` arg. `uses_wildcard_types()`
pre-existed on `CompilerOptions`. The dead Go arm `"ast.Symbol"` (a sed artifact
of `Symbol`) is ported as the real `"Symbol"` with a note.

**RED→GREEN + guards** (checker +14 unit, 750→764): node-globals→TS2591, bare
`require`→TS2591, wildcard→TS2580, `document`/`console`→TS2584, `Map`-family→
TS2583 with `{1}`, shorthand→TS18004, ordinary name still TS2304 (default-arm
guard).

**Parity BEFORE→AFTER (150-case).** passed/failed/errored **61/89/0 → 61/89/0**
(unchanged); categories `31/32/26` unchanged; `top_missing(1)==[(2874,7)]`
unchanged. `top_extra(2)`: **`(2304, 57) → (2304, 44)`** (−13) with
`extra TS2591 ×12` surfacing. This is a CORRECTNESS / code-fidelity round, not a
pass-count round: on this subset tsc RESOLVES `module` (CommonJS binding), so our
`module` diagnostics remain false positives — Round 7 relabels them from the
generic `extra TS2304` to the Go-faithful `extra TS2591`. The genuine fix
(resolving `module`/`exports`) is the DEFERRED CommonJS-module-binding root;
`exports` is not in the node list and stays TS2304.

## Gate results (Round 7)

- `cargo test -p tsgo_checker` — GREEN (764 unit + 178 doctests).
- `cargo test -p tsgo_compiler` — GREEN (95 unit + 11 doctests) [real-lib path].
- `cargo test -p tsgo_testrunner` — GREEN (150-case re-measure; snapshot updated).
- `cargo clippy … -- -D warnings` + `cargo fmt -- --check` — GREEN.
- `cargo build --workspace --all-targets` — GREEN.

No `--no-verify`; no test weakened/deleted; additive only; no dependency / no
`Cargo.toml`/`Cargo.lock` change. Default arm (ordinary names) stays TS2304.

---

# Round 8 — CommonJS module/exports resolution (bind them as file locals)

Round goal: make `module` and `exports` RESOLVE inside CommonJS-context JS files
so they stop producing false-positive "cannot find name" diagnostics — the
dominant remaining `extra` false-positive root on the P10 subset. SOLO lane,
strict TDD red→green. Edits: `internal/binder/{lib.rs,astquery.rs,symbols.rs}` +
their `_test.rs`, `internal/checker/core/check_test.rs` (tests only),
`internal/compiler/{multifile.rs,program_test.rs}`,
`internal/testrunner/compiler_runner_test.rs` (re-measured snapshot) + this
worklog. No `tsgo_checker` production change (the `any`-like CJS typing was
already benign).

## Step-0 root + leverage finding (validated before any code)

The Round-7 note was confirmed and refined against the Go source + committed
baselines. The Rust binder had a `common_js_module_indicator` field that was
NEVER set and never declared the `module`/`exports` CommonJS variables. tsc
resolves them via the **CommonJS binder** (`declareCommonJSVariable`), NOT via
ambient `@types/node` — proven by the committed baselines:

- Pure-CJS files emit NO `module`/`exports` error: `exportAssignmentMerging5`
  (`module.exports = X`), `numericExportNameDeclaration`
  (`exports[1] = 2; module.exports[1] = 2;`),
  `jsDeclarationExportDefaultAssignmentCrash` (`exports.default = …`),
  `erasableSyntaxOnlyJS` (`bar.cjs`/`foo.js`), `multipleModuleExportsAssignments`
  (`x.js`), `nestedJSDocImportType` (`b.js`) — all committed-clean for
  `module`/`exports`, so our `extra TS2591`/`extra TS2304` on them are false
  positives.
- The ONE file where tsc DOES emit `module` TS2591 is `exportAssignmentMerging6`'s
  `a.js`, which has `export const x = 1` — i.e. a real **external-module
  indicator** → `setCommonJSModuleIndicator` returns false → `module` stays
  unresolved. This is the GUARD case (ES module must NOT be treated as CJS), and
  it confirms the root is the CJS binder, not ambient node types.

Measured BEFORE (Round 7): `extra TS2591 ×12` (module) + `extra TS2304 ×44`
(includes the `exports` sub-cluster). Predicted leverage: resolving
`module`/`exports` clears the `extra TS2591`/`exports`-`TS2304` false positives
and flips the committed-clean cases. Measured actual: **+5 PASS** (better than
the +3 predicted).

## Go functions ported (→ Rust locations)

- `internal/ast/utilities.go:GetAssignmentDeclarationKind` (binary-expression
  cases) → `binder/astquery.rs:get_assignment_declaration_kind` +
  `JsDeclarationKind` enum. (The `Object.defineProperty` call cases are DEFERRED
  — the require-call path covers the indicator they would set.)
- `IsRequireCall`, `IsModuleExportsAccessExpression`, `IsExportsIdentifier`,
  `IsModuleIdentifier`, `IsAccessExpression`, `GetElementOrPropertyAccessName`,
  `IsEntityNameExpressionEx`, `SkipParentheses`, `IsInJSFile` →
  `binder/astquery.rs` (same names, `// Go:` anchored, unit-tested).
- `binder.go:setCommonJSModuleIndicator` → `binder/lib.rs:set_common_js_module_indicator`.
- `binder.go:bindCallExpression` → `binder/lib.rs:bind_call_expression` (wired in
  the `bind` dispatch `KindCallExpression` arm, JS-gated).
- `binder.go:bindModuleExportsAssignment` / `bindExportsOrObjectDefineProperty`
  → `binder/lib.rs:bind_module_exports_assignment` /
  `bind_exports_or_object_define_property` (wired in the `KindBinaryExpression`
  arm via `get_assignment_declaration_kind`). The export-symbol declaration on
  the file symbol + `trackNestedCJSExport` are DEFERRED (SECONDARY scope); only
  the indicator is set, which is what resolution needs.
- `binder.go:declareCommonJSVariable` → `binder/symbols.rs:declare_common_js_variable`
  (file-local `FunctionScopedVariable|ModuleExports`; `module` owns an `exports`
  member `ModuleExports|Property`; both declared on the source file). Invoked in
  the `bind_container` SourceFile finalizer when the indicator is set and the
  file is JS (Go's `bindContainer` SourceFile tail).

The checker needed NO change: a `FunctionScopedVariable|ModuleExports` symbol
whose value declaration is the SourceFile flows through
`get_type_of_variable_or_property` → `any` (no type node / initializer), so
`module.exports` / `exports.foo` member access short-circuits on the existing
any-like-receiver guard (Round 3) — verified by tests (no TS2339).

## Over-resolution fix (compiler, Go-faithful, necessary)

A multi-file program merged EVERY file's root `locals` into the program globals
(`multifile.rs`), so a CJS file's newly-declared `module`/`exports` would LEAK
into globals and resolve in sibling files (caught reproducing
`exportAssignmentMerging6`: its ES-module `a.js` stopped reporting `module`).
Fixed surgically: the globals merge now SKIPS `SymbolFlags::MODULE_EXPORTS`
symbols (they are always per-file CommonJS constructs — Go's `Checker.globals`
likewise excludes `IsExternalOrCommonJSModule` files entirely). Guarded by
`commonjs_module_locals_do_not_leak_into_sibling_ts_globals` (a `.ts` sibling of
a CJS `.js` keeps `module` unresolved → TS2591).

## RED→GREEN slices + guard tests

`tsgo_binder` (`lib_test.rs`, +8): `js_module_exports_assignment_declares_module_and_exports`
(headline: `module` + `exports` locals, `module` owns the `exports` member,
correct flags), `js_require_call_declares_module_and_exports`,
`js_exports_property_assignment_declares_module_and_exports`,
`js_module_exports_property_assignment_sets_indicator`,
`js_exports_element_access_assignment_sets_indicator`; GUARDS
`ts_module_exports_assignment_does_not_declare_commonjs_locals` (TS file),
`js_without_commonjs_indicator_does_not_declare_module` (no indicator → still
unresolved), `js_es_module_does_not_declare_commonjs_locals` (ES module). Plus
`astquery_test.rs` (+9) unit-testing every new predicate.

`tsgo_checker` (`check_test.rs`, +4): `js_module_exports_assignment_resolves_no_diagnostics`
(no 2304/2591 AND no 2339), `js_exports_property_assignment_resolves_no_diagnostics`,
`js_require_call_makes_bare_module_resolve`; GUARD
`js_without_commonjs_indicator_module_still_reports_2591`.

`tsgo_compiler` (`program_test.rs`, real bundled-lib path, +3):
`js_module_exports_assignment_resolves_no_2591_with_real_lib`,
`js_exports_property_resolves_no_2304_with_real_lib`,
`commonjs_module_locals_do_not_leak_into_sibling_ts_globals` (the leak guard).
Two pre-existing guards (`plain_js_file_is_still_checked`,
`js_file_with_check_js_true_is_checked`) had their WITNESS updated from
`module.exports = {}` (which now correctly resolves) to a bare undefined name —
intent (plain/checkJs JS IS bind-and-checked) preserved, not weakened.

## Headline — measured parity BEFORE → AFTER (150-case subset)

```
BEFORE (Round 7):  150 cases — passed 61, failed 89, errored 0
                   extra: TS2304 ×44, TS2339 ×18, TS2591 ×12
                   categories: no_baseline 31, missing_all 32, divergent 26
AFTER  (Round 8):  150 cases — passed 66, failed 84, errored 0
                   extra: TS2304 ×41, TS2339 ×18, TS2591 ×1
                   categories: no_baseline 26, missing_all 35, divergent 23
```

- **passed 61 → 66 (+5)** — flipped to PASS (verified produced==committed; the
  two bonus cases beyond the 3 Step-0 predicted have NO committed baseline):
  `exportAssignmentMerging5`, `numericExportNameDeclaration`,
  `jsDeclarationExportDefaultAssignmentCrash`, `cjsExportGenericTypes`,
  `panicSatisfiesOnExportEqualsDeclaration`.
- **extra TS2591 ×12 → ×1** — all false positives cleared; the lone survivor is
  `exportAssignmentMerging6`'s `a.js`, an ES module where tsc ALSO reports
  TS2591 (committed `a.js(5,1)` vs our `a.js(4,20)` — a pre-existing error-range
  POSITION discrepancy, NOT over-resolution; `module` correctly stays
  unresolved there). It pairs with `missing TS2591 ×1`.
- **extra TS2304 ×44 → ×41 (−3)** — the `exports` sub-cluster
  (`numericExportNameDeclaration` ×2, `jsDeclarationExportDefaultAssignmentCrash`
  ×1) cleared.
- **extra TS2339 ×18 — UNCHANGED** — no new cascade; member access on the benign
  `any`-like `module`/`exports` symbols does not spuriously 2339 (proved no
  over-resolution regression).
- **ZERO PASS→FAIL regressions** (PASS sets diffed BEFORE vs AFTER). Category
  shift reflects cleared false positives moving `divergent` → `missing_all_errors`.

## Gate results (Round 8)

- `cargo test -p tsgo_binder` — GREEN (57 unit + 10 doctests; +8 lib + 9 astquery).
- `cargo test -p tsgo_checker` — GREEN (768 unit + 178 doctests; +4).
- `cargo test -p tsgo_compiler` — GREEN (98 unit + 11 doctests; +3, 2 witnesses
  updated) [real bundled-lib path].
- `cargo test -p tsgo_testrunner` — GREEN (47 unit + 11 doctests; 150-case
  re-measure; snapshot updated to 66/84/0).
- `cargo clippy -p tsgo_binder -p tsgo_checker -p tsgo_compiler -p tsgo_testrunner
  --all-targets -- -D warnings` — GREEN.
- `cargo fmt … -- --check` — GREEN.
- `cargo build --workspace --all-targets` — GREEN.

No `--no-verify`; no test weakened/deleted; byte comparison unchanged; no new
dependency; `Cargo.toml`/`Cargo.lock` untouched.

## DEFER list (blocked-by) — Round 8

- **CommonJS export-symbol shape** (`bindModuleExportsAssignment` /
  `bindExportsOrObjectDefineProperty` declaring the `module.exports`/`exports.x`
  export symbols on the file symbol; `trackNestedCJSExport` for declaration
  emit) — only the module indicator is set this round (the resolution-relevant
  effect). blocked-by: the full CommonJS export-symbol model + declaration emit
  (TS6424/TS6425 in `multipleModuleExportsAssignments`).
- **`require(...)` import → `typeof import(...)` member resolution** (the
  `b.js(2,14)` TS2339 in `exportAssignmentMerging6`) — `require` resolves to
  `any`, so `a.a` does not error, but the precise `typeof import("a")` member
  check is unmodeled. blocked-by: external-module require resolution.
- **`Object.defineProperty(exports, …)` assignment kinds**
  (`ObjectDefinePropertyValue`/`ObjectDefinePropertyExports`) — not classified;
  the require/`exports.x` indicator already covers the corpus cases (e.g.
  `numericExportNameDeclaration` flips without it). blocked-by:
  `IsBindableObjectDefinePropertyCall`.
- **TS `import x = require()` / `export =` alias resolution** (the `a`/`foo`
  TS2304 in `exportAssignmentMerging1-4`) — unchanged from Round 3/4
  (`resolveExternalModuleSymbol`/`resolveAlias`).
- **`module` error-range position in multi-file extracted files** (the
  `exportAssignmentMerging6` `a.js(4,20)` vs committed `a.js(5,1)`) — a
  pre-existing error-range/offset discrepancy, NOT touched by this round.
  blocked-by: multi-file error-range attribution.

---

# Round 9 — parser recovery false positives (SYNTAX over-reports tsc never emits)

Round goal: kill the PARSER false positives on the P10 subset — SYNTAX errors our
parser emits that `tsc`/Go's parser do NOT on valid input (`extra TS1005 ×9`,
`TS1003 ×5`, `TS1109 ×1`, `TS1155 ×1`, `TS1161 ×1`, plus the empty-identifier
`TS2304: Cannot find name ''.`). Since our parser is a 1:1 port of `parser.go`,
each such over-report is a PORT BUG (a missing parse path / divergent recovery).
SOLO lane, strict TDD red→green, one root at a time. Edits: `internal/parser/
{lib.rs,lib_test.rs}` (four parser roots) + `internal/checker/core/{check.rs,
check_test.rs}` (one checker root) + `internal/testrunner/compiler_runner_test.rs`
(re-measured characterization) + this worklog. No `ast`/`binder`/`scanner`
production change (no new AST node was added).

## Step-0 case list + ROOT divergences (pinpointed before any fix)

Each offending case was run through the real parity path; its `+` (we-emit)
syntax errors were confirmed ABSENT from the committed `.errors.txt` baseline
(tsc parses cleanly or with different/earlier errors), proving an over-report.
Minimal snippets were distilled and run through `parse_source_file` to isolate
the exact parser-level diagnostics (`code`/`pos`/`end`) from the full-compile
cascade.

| case | offending extra (we emit) | tsc / committed | ROOT |
|---|---|---|---|
| `emitIncompleteDoStatement.ts` | `TS2304 ''` ×2 | only `TS1109` | **R1 empty-name (CHECKER)** |
| `panicForInEmptyDeclarationList.ts` | `TS2304 ''` ×1 | only `TS1109` | **R1** |
| `jsxAttributeValueBinaryExpression.tsx` | `TS2304 ''` ×1 + `TS1109`/`TS1161`/`TS1128` | `2304`+`2×7026`+`2657` | R1 + **R5 JSX recovery (DEFER)** |
| `declarationEmitAsConstSatisfiesNonReadonlyResult.ts` | `TS1003` ×1 (at `const`) | clean | **R2 const type-param modifier** |
| `inferenceWithNeverSource1.ts` | `TS1003` ×1 (at `const`) + cascade | clean | **R2** |
| `declarationEmitTypeofIndexedAccessNoParens.ts` | `TS1005` ×2 (at `?`) + cascade | clean | **R3 optional tuple element `[T?]`** |
| `keyofUnresolvedBaseMembers.ts` | `TS1005` ×1 (at `class`) + cascade | divergent | **R4 `abstract` statement-start modifier** |
| `invalidGlobalAugmentation.ts` | `TS1005` ×2, `TS1155` ×1, `TS2304` `declare`/`global` | `TS2669`+`TS2664` | **R6 `declare global` augmentation** |
| `awaitObjectLiteral.ts` | `TS2304 await` ×2, `TS1005` ×4, `TS1003` ×3 | clean | **R7 top-level await (DEFER)** |

**Up-front estimate (delivered):** 9 cases, 5 fixable roots (R1–R4, R6) + 2 DEFER
(R5, R7). Fixing R1–R4 + R6 flips **+3** to PASS and clears
`extra TS1005 ×9→×5`, `TS1003 ×5→×3`, `TS1155 ×1→×0`, empty-name `TS2304 ×4→×0`.

## Roots diagnosed + fixed (Go ground truth → Rust landing, RED→GREEN)

### R1 — empty-name `TS2304` is a CHECKER over-report, not a parser one (traced)

The parser is CORRECT: `do`<EOF> / `for (let in)` error-recover by creating
zero-width MISSING identifier nodes (Go's `createMissingNode`), and emit exactly
the `TS1109` tsc emits — verified by parsing the snippets directly (one `TS1109`,
no `TS2304`). The divergence is in the CHECKER: Go's `getResolvedSymbol`
(`checker.go:13796`) only calls `resolveName` (which reports the
cannot-find-name diagnostic) `if !ast.NodeIsMissing(node)`; a missing identifier
resolves to `unknownSymbol` silently. Our `check_identifier` lacked that guard,
so the empty-name identifier cascaded into `TS2304: Cannot find name ''.`.
- **Fix** (`internal/checker/core/check.rs:check_identifier`): short-circuit
  `node_is_missing(arena, node) → error_type` at the top (mirrors
  unknownSymbol → `checkIdentifier` returns `errorType`).
- RED→GREEN: `missing_identifier_from_recovery_reports_no_cannot_find_name`
  (`do` → was 2× `TS2304 ''`, now none),
  `missing_identifier_in_for_in_reports_no_cannot_find_name`; GUARD
  `present_undefined_identifier_still_reports_cannot_find_name` (a real
  undefined name still `TS2304`).
- Go: `internal/checker/checker.go:Checker.getResolvedSymbol` (NodeIsMissing guard)
  + `internal/ast/utilities.go:NodeIsMissing`.

### R2 — `const` type-parameter modifier (`<const T>`, TS 5.0 const type params)

`parse_type_parameter` called `parse_modifiers()` (i.e.
`permitConstAsModifier: false`), so `const` was not accepted as a type-parameter
modifier and a spurious `TS1003` (Identifier expected) landed on the `const`
keyword. Go's `parseTypeParameter` (`parser.go:3228`) calls
`parseModifiersEx(false, true /*permitConstAsModifier*/, false)`.
- **Fix** (`internal/parser/lib.rs:parse_type_parameter`): call
  `parse_modifiers_ex(false, true, false)` (the `try_parse_modifier`
  const-modifier path already existed).
- RED→GREEN: `parse_const_type_parameter_modifier` (`<const T extends string>`,
  asserts the `CONST` flag), `parse_const_type_parameter_modifier_variants`
  (class/interface/arrow/fn-type + `in`/`out` still clean); GUARD
  `parse_const_keyword_not_misread_as_type_parameter_modifier` (`const enum E`,
  `const x = 1;` unaffected).
- Go: `internal/parser/parser.go:parseTypeParameter` / `tryParseModifier`.

### R3 — unnamed optional tuple element `[T?]`

`parse_postfix_type_or_higher` only handled the `[` (array/indexed) postfix,
never the `?` (Go's `parsePostfixTypeOrHigher` `KindQuestionToken` case →
`JSDocNullableType`), so `[string?]` / `[typeof C?]` left the `?` unconsumed and
the tuple list reported `TS1005` (',' expected). Go's `parseTupleElementType`
(`parser.go:3644`) converts a postfix `T?` into an `OptionalType`.
- **Fix** (`internal/parser/lib.rs:parse_tuple_element_type` + new
  `next_is_start_of_type`): the port does not model `JSDocNullableType`, so the
  postfix `?` for an unnamed optional tuple element is recognized directly where
  it becomes an `OptionalType` (the only position it is valid). The Go
  `nextIsStartOfType` guard is preserved so a real conditional element
  (`[A extends B ? C : D]`) is unaffected. Observationally identical to Go
  (`OptionalType` node + zero diagnostics); `OptionalType` already round-trips
  through the printer.
- RED→GREEN: `parse_optional_tuple_element` (`[string?]` → `OptionalType`),
  `parse_optional_tuple_element_variants` (`[typeof C?]`, `[(typeof C)?]`,
  `[number?, string?]`); GUARD `parse_conditional_type_in_tuple_is_not_optional`
  (`[A extends B ? C : D]` stays a `ConditionalType`).
- Go: `internal/parser/parser.go:parseTupleElementType` / `parsePostfixTypeOrHigher`.

### R4 — `abstract` (and the class-modifier keywords) at statement level

`parse_statement`'s declaration-keyword guard omitted `abstract`/`accessor`/
`static`/`readonly`/`public`/`private`/`protected`, so `abstract class C {}` fell
through to expression-statement parsing — `abstract` became an identifier and
`class` triggered a spurious `TS1005` (';' expected). Go's `parseStatement`
(`parser.go:1059`) lists all of these in its modifier-keyword case (gated on
`isStartOfDeclaration`).
- **Fix** (`internal/parser/lib.rs:parse_statement`): add the missing
  modifier-keyword arms to the guard (still gated on `is_start_of_declaration`,
  which already handled them in `scan_start_of_declaration`).
- RED→GREEN: `parse_abstract_class_statement` (asserts `ClassDeclaration` +
  `ABSTRACT` flag), `parse_abstract_class_after_type_alias` (the corpus shape);
  GUARD `parse_abstract_identifier_is_expression_statement` (`abstract;` stays an
  expression statement).
- Go: `internal/parser/parser.go:parseStatement` (modifier-keyword case).

### R6 — `declare global { ... }` augmentation

`parse_module_declaration` already handled the `global` keyword, but
`scan_start_of_declaration` was MISSING the `KindGlobalKeyword` arm, so
`is_start_of_declaration()` returned `false` for `declare global` and it never
routed to declaration parsing → `declare`/`global` became identifiers,
`TS1005`/`TS1155`/`TS2304` cascaded. Go's `scanStartOfDeclaration` has a
`case ast.KindGlobalKeyword: nextToken(); return token == { | identifier |
export`.
- **Fix** (`internal/parser/lib.rs:scan_start_of_declaration`): add the
  `GlobalKeyword` arm 1:1.
- RED→GREEN: `parse_declare_global_augmentation` (`declare global { ... }` →
  `ModuleDeclaration`, no diagnostics); GUARD
  `parse_global_identifier_is_expression_statement` (`global;` stays an
  expression statement).
- Go: `internal/parser/parser.go:scanStartOfDeclaration` (KindGlobalKeyword arm).

## Headline — measured parity BEFORE → AFTER (150-case subset)

```
BEFORE (Round 8):  150 cases — passed 66, failed 84, errored 0
  extra: TS2304 ×41, TS2339 ×18, TS2322 ×12, TS1005 ×9, TS2345 ×8, TS1003 ×5,
         TS2495 ×2, TS1109 ×1, TS1155 ×1, TS1161 ×1, TS2344 ×1, TS2583 ×1,
         TS2591 ×1, TS5108 ×1
  categories: no_baseline 26, missing_all 35, divergent 23
AFTER  (Round 9):  150 cases — passed 69, failed 81, errored 0
  extra: TS2304 ×34, TS2339 ×18, TS2322 ×12, TS2345 ×9, TS1005 ×5, TS1003 ×3,
         TS2495 ×2, TS1109 ×1, TS1161 ×1, TS2344 ×1, TS2583 ×1, TS2591 ×1,
         TS5108 ×1
  categories: no_baseline 25, missing_all 36, divergent 20
```

- **passed 66 → 69 (+3)** — verified produced==committed: `emitIncompleteDoStatement`,
  `panicForInEmptyDeclarationList` (R1 empty-name), `declarationEmitAsConstSatisfiesNonReadonlyResult`
  (R2 const type-param). ZERO PASS→FAIL regressions (PASS sets diffed).
- **extra TS1005 ×9 → ×5 (−4)** — R3 (`declarationEmitTypeofIndexedAccessNoParens`
  ×2) + R4 (`keyofUnresolvedBaseMembers` ×1) + R6 (`invalidGlobalAugmentation` ×1).
- **extra TS1003 ×5 → ×3 (−2)** — R2 (`declarationEmitAsConstSatisfiesNonReadonlyResult`,
  `inferenceWithNeverSource1`).
- **extra TS1155 ×1 → ×0** — R6 (`declare global` now parses).
- **empty-name `TS2304 ''` ×4 → ×0** — R1 (folded into `extra TS2304 ×41 → ×34`,
  which also drops `invalidGlobalAugmentation`'s `declare`/`global`).
- **extra TS2345 ×8 → ×9 (+1)** — NOT a regression and NOT a new code:
  `inferenceWithNeverSource1` (already FAILing, no committed baseline) now parses
  its `const T` correctly so its `TS1003` is gone, exposing a DEFERRED
  const-type-parameter / conditional-type CHECKER gap (false-positive `TS2345`).
  The case was FAIL before and after; no PASS→FAIL.
- No NEW diagnostic code appeared anywhere; every other extra/missing bucket is
  unchanged; byte comparison and the 30-case smoke are untouched.

## Deferred roots (blocked-by)

- **R5 — JSX attribute value with a binary expression** (`jsxAttributeValueBinaryExpression.tsx`,
  `extra TS1109 ×1` + `TS1161 ×1` + an empty-name `TS2304`): a divergent JSX
  attribute-value error-recovery; the case also needs `TS2874`/`TS2657`/`2×7026`
  to PASS. blocked-by: a Go-faithful JSX attribute-value recovery pass (large)
  + the DEFERRED `TS2874` React-in-scope check.
- **R7 — top-level `await`** (`awaitObjectLiteral.ts`, `extra TS2304 await ×2`,
  `TS1005 ×4`, `TS1003 ×3`): `const x = await { ... }` at module top level needs
  the parser to know the file is a module with top-level await permitted
  (target/module-kind-driven await context); we treat `await` as an identifier.
  blocked-by: top-level-await context detection in the parser
  (`setExternalModuleIndicator` + await-context for ES2022+ modules).
- **`declarationEmitTypeofIndexedAccessNoParens` typeof-query residue** — the R3
  parser fix cleared its `TS1005 ×2`, but it stays FAIL on a pre-existing CHECKER
  `TS2304: Cannot find name 'C'` resolving a value name inside a parenthesized
  `typeof` query (`(typeof C)`). blocked-by: a checker `typeof`-query value
  resolution gap (out of a parser round's scope).
- **`invalidGlobalAugmentation` / `keyofUnresolvedBaseMembers`** — R6/R4 cleared
  their false positives but they stay FAIL on genuinely-MISSING checker
  diagnostics (`TS2669`+`TS2664`; `TS2344`/`TS2322`/`TS2345`), now correctly
  categorized as `missing`/`divergent` rather than masked by parser noise.

## Test deltas

- `tsgo_parser`: **111 → 122** unit (+11): 3 const-type-param, 3 abstract /
  statement-start, 3 optional-tuple + conditional guard, 2 declare-global.
  Doctests unchanged (7).
- `tsgo_checker`: **768 → 771** unit (+3): two missing-identifier (do / for-in)
  + one present-undefined guard. Doctests unchanged (178).
- `tsgo_testrunner`: unit/doctest counts unchanged (47 / 11); the
  `expanded_compiler_subset_parity_smoke` characterization re-measured to
  `{passed: 69, failed: 81, errored: 0}`, `top_extra(2) == [(2304, 34), (2339, 18)]`,
  categories `{no_baseline 25, missing_all 36, divergent 20}`, plus new guards
  `extra TS1005 == 5`, `extra TS1003 == 3`, `extra TS1155 == None`. The 30-case
  smoke is UNCHANGED (18/12/0).
- No existing test weakened or deleted; byte comparison unchanged.

## Gate results (Round 9)

- `cargo test -p tsgo_parser` — GREEN (122 unit + 7 doctests).
- `cargo test -p tsgo_checker` — GREEN (771 unit + 178 doctests).
- `cargo test -p tsgo_compiler` — GREEN (98 unit + 11 doctests) [real-lib path].
- `cargo test -p tsgo_testrunner` — GREEN (47 unit + 11 doctests; 150-case
  re-measure).
- Sibling suites GREEN (unit, all run with their doctests): `tsgo_binder` (54),
  `tsgo_ast` (57), `tsgo_printer` (194, 1 ignored), `tsgo_transformers` (311).
- `cargo clippy -p tsgo_parser -p tsgo_checker -p tsgo_testrunner --all-targets
  -- -D warnings` — GREEN.
- `cargo fmt -p tsgo_parser -p tsgo_checker -p tsgo_testrunner -- --check` — GREEN.
- `cargo build --workspace --all-targets` — GREEN.

No `--no-verify`; no test weakened/deleted; byte comparison unchanged; no new
dependency; `Cargo.toml`/`Cargo.lock` untouched. Additive only (the parser fixes
extend existing dispatch/modifier paths; the checker fix is a guard in
`check_identifier`).
