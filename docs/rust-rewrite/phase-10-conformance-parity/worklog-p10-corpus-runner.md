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

---

# Round 10 — TS2339 property false positives (cross-file lib-interface merge)

Round goal: reduce the P10 `extra TS2339 ×18` — "Property '{0}' does not exist
on type '{1}'." FALSE POSITIVES (we report a missing property where tsc resolves
it). These had been STUCK at 18 since the Round-3 error-cascade fix, so they are
genuine property-RESOLUTION gaps, not cascade artifacts. SOLO lane, strict TDD
red→green. Edits: `internal/compiler/{multifile.rs,multifile_test.rs,program_test.rs}`
+ `internal/checker/core/declared_types.rs` (one defensive owning-view guard) +
`internal/testrunner/compiler_runner_test.rs` (re-measured snapshot) + this
worklog. No `binder`/`ast`/`parser` production change.

## Step-0 categorization of the 18 (case → property → receiver → root)

Every `+` (we-emit) `TS2339` in the 150-case FAIL diffs was extracted and
confirmed ABSENT from the committed `.errors.txt` (tsc resolves the property):

| case (file) | property → receiver | construct | bucket |
|---|---|---|---|
| `expandoFunctionAsAssertion.ts` | `isFoo` → `example` ×2 | `function example(){}; example.isFoo = …` | **H** TS expando-function |
| `expandoPropertyEmptyArrayWidening.ts` | `a` → `f1` ×1 | `function f1(){}; f1.a = []` | **H** TS expando-function |
| `expandoNoInferredIndex.ts` (`main.js`) | `foo`/`bar`/`buzz` → `{}` ×3 | `const obj = {}; obj.foo = …` (JS) | **H** JS expando-object |
| `expandoNoInferredIndex.ts` (`main.js`) | `values` → `ObjectConstructor` ×1 | `Object.values(obj)` | **G** lib-interface merge |
| `nonExpandoDeclarations.ts` (`main.js`) | `foo` → `{}` ×1 | `/** @type {Record<…>} */ let m = {}; m.foo = …` | **H** JS JSDoc-typed local |
| `nonExpandoDeclarations.ts` (`main.js`) | `merged_props` → `f2` ×3 | `function f2(){}; f2.merged_props = {}` | **H** JS expando-function |
| `jsDeclarationEmitThisAssignment.ts` (`main.js`) | `baz` → `Foo`, `x`/`y` → `Bar` ×3 | `class Foo { constructor(){ this.baz = … } }` (JS) | **H** JS `this`-property |
| `jsDeclarationsRequireImportForms.ts` (`obj.js`/`index.js`) | `x` → `Obj`, `usage`/`usage2` → `Container` ×3 | `class Obj { constructor(){ this.x = … } }` (JS) | **H** JS `this`-property |
| `objectSubtypeReduction.ts` | `entries` → `ObjectConstructor` ×1 | `Object.entries(x \|\| {})` (`@target esnext`) | **G** lib-interface merge |

**18 extra TS2339 = bucket H ×16 + bucket G ×2.**

- **Bucket H — JS/TS expando-property + `this`-property assignment (×16).** The
  binder/checker JS-expando feature: `func.prop = v` adds an expando member to a
  function type, `obj.prop = v` to a JS object-literal type, and `this.x = v` in
  a JS class/constructor adds an instance member. ROOT Go path:
  `binder.go:bindDeferredExpandoAssignments` + `checker.go:getWidenedType
  ForAssignmentDeclaration` / `getTypeOfFuncClassEnumModule` + JS `this`-property
  inference. This is the SAME deferred feature noted in Rounds 3/4/8. **DEFERRED**
  — it is a large multi-behavior feature, AND tsc reports `TS7008`/`TS7022`
  (implicit-any) on these members in the committed baselines, so resolving them
  alone would not flip the cases (the implicit-any reporting would still be
  missing). The LARGEST bucket but out of a surgical round's reach.
- **Bucket G — cross-file lib-interface member resolution (×2).** `Object.entries`
  / `Object.values` on `ObjectConstructor`. ROOT: `getPropertyOfType` finds an
  interface's members via the binder-merged symbol's member table, but the
  multi-file program's globals merge kept only the FIRST file's symbol for a
  same-named global (`or_insert`, "first file wins"). `ObjectConstructor` is
  declared across `lib.es5.d.ts` + `lib.es2015.core.d.ts` + `lib.es2017.object.d.ts`
  + …, so the es2017 members (`entries`/`values`) were dropped. **FIXED this
  round** — the largest TRACTABLE bucket.

## The fix (Go-faithful cross-file declaration merging — member-table half)

`internal/compiler/multifile.rs` `MultiFileBoundProgram::new_with_options`: the
globals merge no longer `or_insert`s (first-file-wins) for a same-named global.
When a name collides and the two symbols are *mergeable* (Go's
`getExcludedSymbolFlags` test), the first file's symbol is the merge TARGET and
each later same-named symbol's MEMBERS are unioned into it. A non-mergeable
collision still keeps the first symbol (the duplicate-identifier diagnostic stays
DEFER'd). Ports of Go:

- `excluded_symbol_flags(flags)` — 1:1 port of `getExcludedSymbolFlags`.
- `merge_global_symbol(symbols, target, source)` — the member-table half of
  `mergeSymbol`: `target.flags |= source.flags` + `mergeSymbolTable(target.Members,
  source.Members)` (insert-if-absent; member names already on the base target
  win). Snapshots the source members (both live in the merged vector) before
  mutating the target.
- Go: `internal/checker/checker.go:Checker.initializeChecker` → `mergeGlobalSymbol`
  → `mergeSymbol` / `getExcludedSymbolFlags`.

`internal/checker/core/declared_types.rs:collect_index_infos_of_members` — a
defensive owning-view guard: a merged interface's `__index` member may now come
from another file, so its `IndexSignature` declaration node is read through the
view of the file that DECLARES it (mirroring the owning-view switch in
`get_declared_type_of_symbol`), avoiding a cross-arena read. The member-TYPE
resolution path (`get_type_of_symbol`) already switched owning views (the Round-1
`Array.push` fix), so `entries`/`values` (methods) resolve their signatures in
their own (es2017) file.

**DEFERRED (NOT ported, with blocked-by):** the rest of `mergeSymbol` — merging
the `declarations` list and the `exports` table, and the recursive same-named
member merge. Only the member-table union is needed for cross-file lib-interface
property resolution. blocked-by: a per-declaration owning-view switch in the
declared-type builders (`collect_local_type_parameters` / `resolve_base_types` /
`collect_late_bound_well_known_members` would read a cross-file declaration node
through the merge target's arena) + namespace export merging + `globalThis`.

## RED→GREEN slices (one behavior at a time)

`tsgo_compiler` (real bundled-lib path, the parity runner's path):

1. **`object_entries_resolves_via_cross_file_lib_interface_merge`** (tracer) —
   `Object.entries({})` with `lib: ["es2017"]` (pulls es5 → es2015.core →
   es2017.object, so `ObjectConstructor` is declared in three lib files). RED:
   `TS2339 Property 'entries' does not exist on type 'ObjectConstructor'` →
   GREEN. (`@target esnext` would load the same chain but the full/DOM aggregate
   trips a *pre-existing, unrelated* binder panic; `lib: ["es2017"]` is the clean
   minimal repro.)
2. **GUARD `absent_object_constructor_property_still_reports_2339_after_merge`** —
   `Object.thisIsNotARealMethod` still reports `TS2339` (the merge resolves real
   members, it does not blanket-mute the receiver).
3. **GUARD `object_keys_es5_base_member_still_resolves_after_merge`** — the BASE
   `lib.es5.d.ts` member `Object.keys` still resolves (the merge ADDS later-lib
   members without dropping the first declaration's).

`tsgo_compiler` (`multifile_test.rs`, synthetic multi-file, fast / isolated):

4. **`cross_file_interface_members_merge_into_one_global_symbol`** — two files
   each `interface Foo { … }` → the merged global `Foo` symbol's member table is
   the UNION of both declarations' members.
5. **GUARD `cross_file_non_mergeable_collision_keeps_first_symbol`** — two
   block-scoped `let dup` across files are NOT merged (the gate); the first
   file's symbol wins.

Pre-existing guards `missing_property_reports_diagnostic` /
`union_property_absent_from_one_constituent_reports_2339` stay GREEN.

## Headline — measured parity BEFORE → AFTER (150-case subset)

```
BEFORE (Round 9):  150 cases — passed 69, failed 81, errored 0
  extra: TS2304 ×34, TS2339 ×18, TS2322 ×12, TS2345 ×9, TS1005 ×5, TS1003 ×3,
         TS2495 ×2, TS1109 ×1, TS1161 ×1, TS2344 ×1, TS2583 ×1, TS2591 ×1, TS5108 ×1
  categories: no_baseline 25, missing_all 36, divergent 20
AFTER  (Round 10): 150 cases — passed 69, failed 81, errored 0
  extra: TS2304 ×34, TS2339 ×16, TS2322 ×12, TS2345 ×9, TS1005 ×5, TS1003 ×3,
         TS2495 ×2, TS1109 ×1, TS1161 ×1, TS2344 ×1, TS2591 ×1, TS2769 ×1, TS5108 ×1
  categories: no_baseline 25, missing_all 37, divergent 19
```

- **extra TS2339 ×18 → ×16 (−2)** — both `ObjectConstructor` false positives
  cleared (`objectSubtypeReduction`'s `entries`, `expandoNoInferredIndex`'s
  `values`); the property genuinely resolves.
- **extra TS2583 ×1 → ×0** — a BONUS: the `Promise` global VALUE (`main.js`
  `TS2583 Cannot find name 'Promise'`) now resolves once its split
  interface/`var` declarations merge across the lib files.
- **extra TS2769 ×0 → ×1 (NEW, DEFERRED)** — an HONEST downstream exposure:
  `objectSubtypeReduction`'s `Object.entries(x || {})` now reaches overload
  resolution (previously short-circuited by the `error`-type 2339), and we report
  `No overload matches this call` because `object | {}` is not yet related to the
  empty object type `{}`. This is a SEPARATE relations/union-reduction bucket
  (neither Go's `isSimpleTypeRelatedTo` nor the structural object arm relates a
  NonPrimitive `object` source to an empty-object target — tsc's path is union
  subtype reduction of `x || {}`), out of a property-resolution round's scope.
- **Honest flip count: 0.** Both TS2339-affected cases retain OTHER reachable
  gaps (`objectSubtypeReduction` → the new TS2769; `expandoNoInferredIndex` → its
  3 deferred JS-expando 2339s). Net spurious diagnostics: −2 TS2339 −1 TS2583
  +1 TS2769 = **−2**.
- **PROOF of no over-resolution:** the `missing` histogram is BYTE-IDENTICAL
  BEFORE vs AFTER (`missing TS2339 ×5`, `top_missing TS2874 ×7`, all 52 codes
  unchanged) — the merge did NOT mask any real error. **ZERO PASS→FAIL
  regressions** (PASS/FAIL/ERR verdict set diffed byte-for-byte: identical
  69/81/0). The category shift (`missing_all 36→37`, `divergent 19`,
  `no_baseline 25`) is internal reclassification of cases that lost a false
  positive, not a verdict change.

## Test deltas

- `tsgo_compiler`: **98 → 103** unit (+5: 3 real-lib `program_test.rs`, 2
  synthetic `multifile_test.rs`); doctests unchanged (11).
- `tsgo_checker`: unit/doctest counts unchanged (771 / 178) — the
  `collect_index_infos_of_members` change is a defensive owning-view guard
  exercised by the real-lib compiler merge tests.
- `tsgo_testrunner`: unit/doctest counts unchanged (47 / 11); the
  `expanded_compiler_subset_parity_smoke` characterization re-measured
  (`missing_all 36→37`, `divergent 20→19`, `top_extra(2)` `(2339,18)→(2339,16)`,
  new guards `extra TS2339 == 16`, `extra TS2583 == None`, `extra TS2769 == 1`).
  Counts `{69,81,0}` and `top_missing TS2874 ×7` unchanged. The 30-case smoke is
  UNCHANGED (18/12/0).
- No existing test weakened or deleted; byte comparison unchanged.

## Gate results (Round 10)

- `cargo test -p tsgo_checker` — GREEN (771 unit + 178 doctests).
- `cargo test -p tsgo_compiler` — GREEN (103 unit + 11 doctests) [real bundled-lib].
- `cargo test -p tsgo_testrunner` — GREEN (47 unit + 11 doctests; 150-case re-measure).
- Sibling suites GREEN: `tsgo_binder` (54), `tsgo_transformers` (311),
  `tsgo_printer` (194, 1 ignored), `tsgo_ast` (57).
- `cargo clippy -p tsgo_checker -p tsgo_compiler -p tsgo_testrunner --all-targets
  -- -D warnings` — GREEN.
- `cargo fmt -p tsgo_checker -p tsgo_compiler -p tsgo_testrunner -- --check` — GREEN.
- `cargo build --workspace --all-targets` — GREEN.

No `--no-verify`; no test weakened/deleted; byte comparison unchanged; no new
dependency; `Cargo.toml`/`Cargo.lock` untouched. Additive only (the globals merge
+ two new free fns in `multifile.rs`; the `declared_types.rs` change is an
owning-view guard internal to `collect_index_infos_of_members`).

## DEFER list (blocked-by) — Round 10

- **Bucket H — JS/TS expando + `this`-property assignment (the remaining
  `extra TS2339 ×16`)** — `func.prop = v` / `obj.prop = v` / `this.x = v` member
  inference. A large multi-behavior binder+checker feature; AND tsc reports
  `TS7008`/`TS7022` (implicit-any) on these members (the committed baselines), so
  resolving them would not flip the cases without the implicit-any reporting too.
  blocked-by: `binder.go:bindDeferredExpandoAssignments` /
  `bindSpecialPropertyAssignment` + `checker.go:getWidenedTypeForAssignment
  Declaration` / `getTypeOfFuncClassEnumModule` (expando member synthesis) + JS
  `this`-property instance-member inference. Cases: `expandoFunctionAsAssertion`,
  `expandoPropertyEmptyArrayWidening`, `expandoNoInferredIndex`,
  `nonExpandoDeclarations`, `jsDeclarationEmitThisAssignment`,
  `jsDeclarationsRequireImportForms`.
- **`object | {}` → `{}` overload/assignability (the newly-exposed
  `extra TS2769 ×1`)** — `objectSubtypeReduction`'s `Object.entries(x || {})`.
  A NonPrimitive `object` source is not related to the empty object type `{}`
  (neither `isSimpleTypeRelatedTo` nor the structural object arm covers it); tsc's
  path is union subtype reduction of the `||` result. A separate relations /
  union-reduction bucket. blocked-by: `getUnionType` subtype reduction of the
  `||`/`&&` result type + the empty-object-target relation
  (`isEmptyObjectType(target)` short-circuit reached for a NonPrimitive source).
- **Full `mergeSymbol` (declarations + exports + recursive member merge)** — only
  the member-table union is ported this round. blocked-by: a per-declaration
  owning-view switch in the declared-type builders + namespace export merging +
  `globalThis` (see "The fix").
- **Pre-existing `@target: esnext` full/DOM-lib binder panic** — surfaced (not
  caused) by the tracer probe at `binder/symbols.rs:375` (`symbol_of(container)
  .unwrap()` on a `None` for an exported declaration whose container file has no
  symbol). Unrelated to this round (the parity harness does not load the
  full/DOM aggregate for these cases); the tracer uses `lib: ["es2017"]` to avoid
  it. blocked-by: a separate binder triage of the esnext-full lib chain.
  **→ FIXED in Round 11 (below).**

# Round 11 — esnext/DOM lib bind panic fix

Round goal: fix the **critical robustness bug** that made the compiler unusable
on real-world projects — the binder PANICKED at `internal/binder/symbols.rs:375`
(`let sym = self.symbol_of(container).unwrap();`) whenever the full
`@target: esnext` / DOM lib set was bound. Strict TDD red→green, one vertical
slice. Edits: `internal/binder/lib.rs` (the root fix) + `internal/binder/symbols_test.rs`
(focused binder tests) + `internal/compiler/program_test.rs` (the real-lib
headline) + this doc. Surgical/additive — no test weakened or deleted.

## Reproduce + pinpoint (STEP 0)

A temporary instrumented `eprintln!` just before the `symbol_of(container)
.unwrap()` in `declareModuleMember`'s export-context branch, driven by a focused
binder test binding an external-module file with a `declare global { … }`
augmentation, captured the EXACT trigger:

```
file=… container_kind=ModuleDeclaration node_kind=InterfaceDeclaration node_name=Some("IteratorObject")
panicked at internal/binder/symbols.rs:375: called `Option::unwrap()` on a `None` value
```

- **Lib file:** `internal/bundled/libs/lib.es2025.iterator.d.ts` — the only
  bundled lib with a `declare global { … }` block. It is an **external module**
  (`export {};` at the top), and the `declare global` block holds
  `interface IteratorObject<…>`, `interface IteratorConstructor`, and
  `var Iterator: IteratorConstructor`. `@target: esnext` reaches it via
  `lib.esnext.full.d.ts` → `esnext` → `es2025` → `es2025.iterator`.
- **Symbol-less container:** the `declare global { … }` node — a
  `ModuleDeclaration` with `keyword == GlobalKeyword`, i.e. a **global scope
  augmentation** (`ast.IsGlobalScopeAugmentation` ⇒ `ast.IsAmbientModule`).
- **Declaration that tripped it:** the first member, `interface IteratorObject`.
- **Why `None`:** the global block is an **ambient module**, and the Rust binder
  `bind_module_declaration` **returned early for ambient modules without creating
  their symbol**. When the block's members then bound (container = the global
  block), `declareModuleMember`'s export-context branch did
  `symbol_of(container).unwrap()` on a `None`. The block IS a locals container
  (`HAS_LOCALS`) and IS in export context (`set_export_context_flag` sets
  `EXPORT_CONTEXT` because the block is `AMBIENT` and has no export
  declarations), so binding reached the 2-symbol local+export path (line 374–375)
  rather than the `!is_locals_container` arm (line 359). This is hypothesis (c):
  a namespace/module container whose symbol isn't set before its members bind.

`internal/compiler/program.rs:bind_source_files` wraps each file's bind in
`catch_unwind`, so the panic was swallowed and `lib.es2025.iterator.d.ts` was
left **UNBOUND** (its globals silently dropped) — the headline test asserts it
is now bound.

## Go ground truth (why Go never hits a `nil` `container.Symbol()` here)

```go
// Go: internal/binder/binder.go:bindModuleDeclaration (778)
func (b *Binder) bindModuleDeclaration(node *ast.Node) {
	b.setExportContextFlag(node)
	if ast.IsAmbientModule(node) {
		if ast.HasSyntacticModifier(node, ast.ModifierFlagsExport) { /* TS2668 */ }
		if ast.IsModuleAugmentationExternal(node) {
			b.declareModuleSymbol(node)                                  // ← creates the symbol
		} else {
			symbol := b.declareSymbolAndAddToSymbolTable(node, ast.SymbolFlagsValueModule, …) // ← creates the symbol
			/* string-literal `module "…"` pattern bookkeeping */
		}
	} else {
		state := b.declareModuleSymbol(node)                              // ← creates the symbol
		/* const-enum-only-module bookkeeping */
	}
}
```

Go creates the module's symbol on **every** path (the ambient branch and the
non-ambient branch both funnel through `declareSymbolAndAddToSymbolTable`). For
the `declare global` augmentation in an external-module lib,
`ast.IsModuleAugmentationExternal` is `true` (parent is the source file and
`ast.IsExternalModule(parent)` holds because of `export {};`), so Go calls
`declareModuleSymbol` → `declareSymbolAndAddToSymbolTable` → (container is the
`SourceFile`) `declareSourceFileMember` → (external module) `declareModuleMember`.
There, `getDeclarationName` returns `InternalSymbolNameGlobal` (`__global`) and
`ast.IsAmbientModule(node)` is `true`, so the `!IsAmbientModule(node) && …`
guard at `binder.go:404` is skipped and the global block's symbol is declared
into the file's **locals** — never via the export-context branch. The symbol
exists before the block's members bind, so `container.Symbol()` is non-nil when
`declareModuleMember` runs for `interface IteratorObject`.

The divergence was NOT (a) `declareSourceFileMember` mis-routing a global-script
file (that path correctly routes to locals — guarded below), nor (b) an
`EXPORT_CONTEXT` mis-set on a global-script source file. It was (c): the Rust
port deferred the ambient-module **container-symbol assignment**.

## The fix (root — Go-faithful, surgical)

`internal/binder/lib.rs:bind_module_declaration` no longer returns early for
ambient modules; it declares the module symbol unconditionally (matching Go,
which creates it on every path):

```rust
fn bind_module_declaration(&mut self, node: NodeId) {
    self.set_export_context_flag(node);
    self.declare_symbol_and_add_to_symbol_table(
        node,
        SymbolFlags::VALUE_MODULE,
        SymbolFlags::VALUE_MODULE_EXCLUDES,
    );
}
```

This is the **identical symbol-table routing** Go uses: `declareSymbolAndAddToSymbolTable`
dispatches on the container kind, so a top-level `declare global` /
`declare module "…"` lands in the file locals/exports exactly as Go places it.
Both Go ambient sub-branches (`IsModuleAugmentationExternal` →
`declareModuleSymbol`; otherwise `declareSymbolAndAddToSymbolTable`) collapse to
this single call once the deferred details are factored out. **Deferred (with
blocked-by, documented at the call site):** the `ValueModule`-vs-`NamespaceModule`
instance-state selection (`declareModuleSymbol`/`GetModuleInstanceState`), the
const-enum-only-module bookkeeping, the TS2668 export-modifier error, and the
string-literal `module "…"` pattern tracking (`TryParsePattern` /
`PatternAmbientModules`). None change which symbol table the module symbol lands
in for the bundled libs, and `ValueModule` matches the pre-existing non-ambient
simplification — so this stays within the existing port's simplification level
while removing the panic. No defensive `if let Some(sym)` was added; the root
(missing container-symbol assignment) is fixed.

## RED→GREEN slices (one behavior at a time)

`tsgo_binder` (`symbols_test.rs`, focused unit):

1. **`bind_declare_global_augmentation_creates_container_symbol`** (HEADLINE
   routing/ordering) — an `export {};` + `declare global { interface
   IteratorObject<T> {} var Iterator: number; }` file. RED: panic at
   `symbols.rs:375` (`symbol_of(container).unwrap()` on `None`). GREEN: the
   global block owns a `__global` symbol, and its members bind through the
   export-context 2-symbol path (asserts `IteratorObject`/`Iterator` are exports
   of the block AND `IteratorObject` is a local of the block).
2. **GUARD `bind_external_module_export_produces_export_symbol`** — a real
   external module (`export const x = 1;`) STILL routes through
   `declareModuleMember`: `x` has both a file local and an export symbol on the
   file symbol. (The fix must not regress normal module-member routing.)
3. **GUARD `bind_global_script_declared_member_goes_to_locals`** — a global
   script (`declare var g: number;`, no top-level import/export) routes its
   ambient `declare`d member to the file LOCALS (not the export-context
   module-member path); there is no file symbol.

`tsgo_compiler` (`program_test.rs`, REAL bundled-lib path — the headline):

4. **`binds_full_esnext_dom_lib_without_panic`** — a trivial file
   (`let o: Object; let el: HTMLElement; let p: Promise<number>;`) under
   `@target: esnext` (no explicit `--lib`), so the default-lib graph expands to
   the full DOM + `es20xx` set including `lib.es2025.iterator.d.ts`. RED: binding
   panicked at `symbols.rs:375` and left `lib.es2025.iterator.d.ts` UNBOUND.
   GREEN: **every** esnext+dom lib binds (asserts no unbound files), and the real
   `Object`/`HTMLElement`/`Promise` globals resolve (no `TS2304`).

## Measurement

- **Exact root:** ambient-module container (`declare global` / `declare module
  "…"`) whose symbol the Rust binder deferred; member binding then hit
  `symbol_of(container).unwrap()` on `None`. Go avoids it by always creating the
  module symbol in `bindModuleDeclaration` before the members bind.
- **Full esnext+dom lib now binds without panic** — the headline test asserts
  zero unbound source files for `@target: esnext` (full DOM + `es20xx` graph),
  including `lib.es2025.iterator.d.ts` and `lib.dom.d.ts`.
- **Parity UNCHANGED at 69 / 81 / 0** — the `tsgo_testrunner`
  `expanded_compiler_subset_parity_smoke` 150-case characterization is GREEN with
  its `{passed: 69, failed: 81, errored: 0}` counts and full extra/missing
  histogram intact (the corpus tracer still uses `lib: ["es2017"]`; the lib was
  NOT widened this round, per scope). The 30-case smoke (18/12/0) is also
  unchanged. The fix is additive (ambient modules that previously panicked were
  `errored`=0 in the snapshot, i.e. absent from the subset), so no case verdict
  changed.

## Test deltas

- `tsgo_binder`: **57 → 60** unit (+3: the headline ordering test + two routing
  guards); doctests unchanged (10).
- `tsgo_compiler`: **103 → 104** unit (+1: the real-lib esnext/dom headline);
  doctests unchanged (11).
- `tsgo_checker`: unit/doctest counts unchanged (178 doctests).
- `tsgo_testrunner`: unit/doctest counts unchanged (47 / 11); parity
  characterization re-run GREEN (69/81/0).
- No existing test weakened or deleted.

## Gate results (Round 11)

- `cargo test -p tsgo_binder` — GREEN (60 unit + 10 doctests).
- `cargo test -p tsgo_checker` — GREEN (unit + 178 doctests).
- `cargo test -p tsgo_compiler` — GREEN (104 unit + 11 doctests) [real bundled lib].
- `cargo test -p tsgo_testrunner` — GREEN (47 unit + 11 doctests; parity 69/81/0).
- `cargo clippy -p tsgo_binder -p tsgo_compiler --all-targets -- -D warnings` — GREEN.
- `cargo fmt -p tsgo_binder -p tsgo_compiler -- --check` — GREEN.
- `cargo build --workspace --all-targets` — GREEN.

No `--no-verify`; no test weakened/deleted; no new dependency; `Cargo.toml`/
`Cargo.lock` untouched. Temporary instrumentation removed.

## DEFER list (blocked-by) — Round 11

- **Module instance-state flags + const-enum-only modules** — Go's
  `declareModuleSymbol`/`GetModuleInstanceState` selects `ValueModule` vs
  `NamespaceModule` (and `bindModuleDeclaration` tracks
  `ConstEnumOnlyModule`). The port declares every module symbol with
  `ValueModule` (consistent with the pre-existing non-ambient simplification).
  blocked-by: a `GetModuleInstanceState` port (the `KindModuleBlock` recursion +
  `getModuleInstanceStateForAliasTarget` ancestors walk).
- **Ambient-module diagnostics + string-literal pattern modules** — the TS2668
  `export`-modifier error on ambient modules and the `module "foo*"` wildcard
  pattern bookkeeping (`core.TryParsePattern` + `file.PatternAmbientModules`)
  are not ported (the bundled libs do not need them; deferring avoids any
  parity-span risk this round). blocked-by: `core.TryParsePattern` port +
  `errorOnFirstToken` span narrowing.
- **Widening the corpus tracer lib beyond `es2017`** — now that the esnext/DOM
  bind panic is fixed, a future measurement round MAY widen the tracer's
  `lib` toward the full default lib; left as-is here to keep the 150-case parity
  snapshot stable (per scope). blocked-by: a dedicated re-measurement round that
  re-baselines the extra/missing histogram against the wider lib.

---

# Round 12 — full compiler corpus measurement

Goal: a TRUE, FULL-corpus parity measurement (not the curated 150-subset) to
drive prioritization, now that the esnext/DOM binder panic is fixed (Round 11)
and the runner can use the real/full bundled libs via each case's own
`// @target` directive. **Measurement + reporting only** — NO checker / parser /
binder / compiler production code was touched; the changes are surgical and
testrunner-only. Tree had the Round-11 edits staged.

## Scope of the local corpus (important)

The vendored `_submodules/TypeScript` is **absent** in this checkout, so the
"full corpus" is the committed `testdata/` sample, not the upstream ~7 000-case
suite:

- `testdata/tests/cases/compiler`: **226** top-level `.ts`/`.tsx` cases (104
  committed `.errors.txt` references).
- `testdata/tests/cases/conformance`: **19** `.ts` cases nested under 6
  subdirectories (7 committed references; no basename collisions).

So "full" here = all 226 compiler cases (uncapped, vs. the 150-subset's ≤25-line
/ ×150 cap) + all 19 conformance cases. This is tractable in one run (~8 s in
`--release`).

## What landed (testrunner-only, additive)

`internal/testrunner/compiler_runner.rs` (+ `compiler_runner_test.rs`):

- **`CompilerBaselineRunner::full_corpus(denylist) -> Vec<String>`** — the full
  selector: every top-level `.ts`/`.tsx` case basename, sorted, deterministic,
  NO line cap / NO count limit, minus the denylist. A cheap directory listing
  (no per-file content read, unlike `curated_subset`). TDD'd by
  `full_corpus_returns_all_sorted_minus_denylist`.
- **`PanicLocationCapture`** — an RAII guard that installs a SILENT panic hook
  recording each panic's source SITE (`file:line:col`) into a thread-local,
  consumed by `run_case` so a caught `Errored` message is suffixed with
  `  [panic at <file:line:col>]`. With no guard installed the behavior is
  unchanged (message = downcast payload), so the existing panic tests stay
  green. It mutates the process-global hook, so it is documented as opt-in /
  isolation-only and backs only the `#[ignore]`d measurement (never the parallel
  default suite). TDD'd by `panic_location_capture_records_panic_site`.
- **`ParitySummary::top_wrong_code_pairs(n)`** — ranks `(expected -> produced)`
  code pairs by frequency (the histogram's `wrong_code` map keys only the
  expected code; this keeps the pair). TDD'd by
  `top_wrong_code_pairs_ranks_expected_to_produced`.
- **`ParitySummary::panic_groups() -> Vec<PanicGroup>`** — groups `errored`
  cases by panic SITE (count + representative case + message), the robustness
  backlog. TDD'd by `panic_groups_ranks_by_site_with_representative`.
- **`#[ignore]`d `full_compiler_corpus_measurement`** — the opt-in heavy run:
  `cargo test -p tsgo_testrunner -- --ignored --nocapture full_compiler_corpus_measurement`.
  Runs the full compiler corpus (+ conformance, walked recursively) on a 1 GiB
  stack thread with the panic-location hook, prints the full prioritization map,
  and asserts only COARSE invariants (every selected case ran; `passed ≥ 1`).
  It does NOT pin exact corpus-level counts (those churn); the curated subsets
  remain the pinned-count signal.

The fast `curated_compiler_subset_parity_smoke` (18/12/0) and
`expanded_compiler_subset_parity_smoke` (69/81/0) characterizations are
UNCHANGED and stay the default `cargo test` signal.

### Lib handling decision

The runner already feeds each case's own `// @target` directive through
`error_baseline_for_test` → `compile_files` → `set_options_from_test_config`
(an `Enum` option), and the program then loads the target-default lib graph
(full DOM + `es20xx` when `@target: esnext`, etc.). This is exactly tsc's "use
the case directives with a sensible default" behavior, and it does NOT
blanket-panic after the Round-11 esnext/DOM fix (only 3 `errored` of 222 — see
below). `// @lib` LIST directives remain a deferred harness gap
(`option_value_for` returns `None` for list kinds), but honoring them would
touch shared `harnessutil` and churn the 150-subset snapshot, so it is left as a
documented DEFER rather than changed in this measurement round.

## Measurement — `tests/cases/compiler` (FULL, 226 cases)

After excluding **4** stress cases (see the recursion-robustness backlog),
**222** cases ran:

| outcome | count | % |
|---|---|---|
| **passed** | **85** | **38.3 %** |
| **failed** | **134** | **60.4 %** |
| **errored** (caught panic) | **3** | **1.4 %** |

Category breakdown of the 134 failures:

| category | count |
|---|---|
| `no_baseline_but_errors` (expected clean, we report errors) | 45 |
| `missing_all_errors` (committed errors, we report none) | 57 |
| `divergent` (both sides error, but differ) | 32 |

### TOP-25 `extra` (FALSE-POSITIVE) codes by frequency

| rank | code | ×  | meaning |
|---|---|---|---|
| 1 | **TS2304** | **96** | Cannot find name |
| 2 | **TS2339** | **63** | Property does not exist on type |
| 3 | TS2345 | 23 | Argument not assignable to parameter |
| 4 | TS2322 | 18 | Type not assignable |
| 5 | TS1005 | 17 | `';'` / `','` expected (parser recovery) |
| 6 | TS1003 | 7 | Identifier expected (parser) |
| 7 | TS1109 | 7 | Expression expected (parser) |
| 8 | TS1128 | 6 | Declaration or statement expected (parser) |
| 9 | TS7026 | 6 | JSX element implicitly has type `any` (no `JSX.IntrinsicElements`) |
| 10 | TS2554 | 4 | Expected N arguments |
| 11 | TS2495 | 2 | Type is not an array/string |
| 12 | TS1161 | 1 | Unterminated regex |
| 12 | TS1381 | 1 | Unexpected token (`}`) |
| 12 | TS2344 | 1 | Type does not satisfy constraint |
| 12 | TS2591 | 1 | Cannot find `module`/`require` (no @types/node) |
| 12 | TS2769 | 1 | No overload matches |
| 12 | TS5108 | 1 | Deprecated/removed option |
| 12 | TS18048 | 1 | Value is possibly `undefined` |

### TOP-25 `missing` (FALSE-NEGATIVE) codes by frequency

| rank | code | ×  | meaning |
|---|---|---|---|
| 1 | **TS2300** | **94** | Duplicate identifier |
| 2 | TS1110 | 11 | Type expected (parser) |
| 3 | TS2322 | 10 | Type not assignable |
| 4 | TS6133 | 9 | Declared but never read |
| 5 | TS7027 | 9 | Unreachable code detected |
| 6 | TS2321 | 8 | Cannot assign — property types incompatible |
| 7 | TS2874 | 7 | `JSX.<X>` must be in scope (React jsx-runtime) |
| 8 | TS2339 | 6 | Property does not exist |
| 9 | TS2309 | 5 | Export assignment cannot be used in a module |
| 10 | TS7008 | 5 | Member implicitly has `any` type |
| 11 | TS1118 | 4 | Class member cannot have `;` |
| 11 | TS1119 | 4 | Property name cannot be `__proto__` etc. |
| 11 | TS2353 | 4 | Object literal may only specify known properties |
| 11 | TS2688 | 4 | Cannot find type-definition file |
| 11 | TS7006 | 4 | Parameter implicitly has `any` type |
| 11 | TS7022 | 4 | Variable implicitly `any` (no type annotation, used before init) |
| 17 | TS2304 | 3 | Cannot find name |
| 17 | TS2343 | 3 | `this` of type X is not a valid `this` |
| 17 | TS2345 | 3 | Argument not assignable |
| 17 | TS2488 | 3 | Type must have `[Symbol.iterator]()` |
| 17 | TS7026 | 3 | JSX implicitly `any` |
| 17 | TS7053 | 3 | Element implicitly `any` (index signature) |
| 23 | TS1097 | 2 | `'in'` expression error |
| 23 | TS1202 | 2 | `import =` cannot be used in ES module |
| 23 | TS1225 | 2 | catch clause variable type annotation |

### TOP `wrong_code` pairs (expected → produced)

| expected → produced | ×  | reading |
|---|---|---|
| **TS7026 → TS1128** | 3 | JSX intrinsic-element check vs. a parser "statement expected" over-report on `.tsx` |
| TS2552 → TS2304 | 1 | "Did you mean…" suggestion vs. plain "cannot find name" |
| TS7026 → TS1005 | 1 | JSX implicit-any vs. a parser `';' expected` over-report |

### TOP panic groups (errored = 3) — the robustness backlog

| panic site | ×  | representative case | note |
|---|---|---|---|
| `internal/scanner/lib.rs:3020:38` | 2 | `jsxUnicodeEscapeSequence.tsx` | **Real bug**: `byte index N is not a char boundary; it is inside '⚠'` — the scanner slices on a byte offset that lands inside a multi-byte UTF-8 character while scanning JSX text containing non-ASCII content. |
| (file read) `regexInvalidUtf8WithUnicodeFlag.ts` | 1 | `regexInvalidUtf8WithUnicodeFlag.ts` | The case file is intentionally **not valid UTF-8**, so `std::fs::read_to_string` fails (`stream did not contain valid UTF-8`). A runner I/O limitation (lossy read / byte handling), not a compiler panic. |

### Recursion-robustness backlog (denylisted — uncatchable stack overflow)

These cases overflow even a **1 GiB** harness stack. A true stack overflow is a
process abort (SIGABRT), NOT an unwinding panic, so `catch_unwind` cannot
convert it to an `errored` verdict — the whole run would abort. They are
denylisted (deterministic + documented) and tracked here as
recursion/complexity-limit gaps tsc bounds internally:

| case | suspected root |
|---|---|
| `circularControlFlowNarrowingWithCurrentElement01.ts` | flow analyzer recurses without tsc's shared-flow / depth guard |
| `varianceComputationNoCrash.ts` | variance measurement recurses without the variance/relation cache guard |
| `noTypeToStringStackOverflow.ts` (pre-existing) | self-referential `typeof` type-to-string |
| `templateLiteralTypeTooComplex.ts` (pre-existing) | 49-fold template-literal union explosion (tsc rejects with TS2590) |

## Measurement — `tests/cases/conformance` (secondary, 19 cases)

| outcome | count |
|---|---|
| passed | 10 |
| failed | 9 |
| errored | 0 |

Categories: `no_baseline_but_errors ×4`, `missing_all_errors ×5`, `divergent ×0`.
Top extra: `TS2304 ×20`, `TS2339 ×1`, `TS5108 ×1`. Top missing: `TS8024 ×2`
(JSDoc `@param`), then `TS2322 / TS2345 / TS2454 / TS5055 / TS7006 / TS7053`
×1 each. Same shape as the compiler suite: the unresolved-name cascade
dominates the false positives.

## Prioritization — highest-LEVERAGE next features (by frequency)

1. **Unresolved-name cascade — `extra TS2304 ×96` + `extra TS2339 ×63`
   (+ conformance `TS2304 ×20`).** By far the largest false-positive cluster and
   almost certainly a small set of resolution ROOTS (globals / lib members /
   module + alias resolution / JS CommonJS) cascading into hundreds of downstream
   `cannot find name` / `property does not exist` reports. Prior rounds (3–10)
   chipped at it on the 150-subset; the full corpus shows it is still #1. Highest
   leverage: each resolution root fix likely clears many cases at once.
2. **Duplicate-identifier detection — `missing TS2300 ×94`.** The single largest
   FALSE-NEGATIVE bucket, and a COHERENT binder/checker feature (duplicate-symbol
   diagnostics across declaration merging). We emit it essentially never. One
   feature ⇒ the entire ×94 bucket.
3. **Assignability / relation false positives — `extra TS2345 ×23` +
   `extra TS2322 ×18`** (and the symmetric `missing TS2322 ×10`). A
   relation/assignability accuracy cluster — we both over- and under-report
   assignability, so the comparison/relation logic is the lever.
4. **Parser recovery over-reporting — `extra TS1005 ×17` + `TS1003 ×7` +
   `TS1109 ×7` + `TS1128 ×6`** (~37 combined, plus the `TS7026→TS1128/1005`
   wrong_code pairs). Syntax errors tsc never emits on valid input, exposed by
   the larger uncapped cases (especially `.tsx`). Round 9 fixed several on the
   small subset; the full corpus reveals more, concentrated in JSX/`.tsx`
   recovery.
5. **Scanner UTF-8 char-boundary panic — `internal/scanner/lib.rs:3020:38`
   (errored ×2).** A real, cheap-to-fix robustness bug: the scanner indexes a
   byte offset inside a multi-byte UTF-8 character on JSX text with non-ASCII
   content. Fixing it removes 2 `errored` cases and de-risks any non-ASCII JSX
   input. (Bonus runner hardening: read corpus files as bytes / lossily so an
   intentionally non-UTF-8 fixture like `regexInvalidUtf8WithUnicodeFlag.ts`
   does not surface as `errored`.)

## Gate results (Round 12)

- `cargo test -p tsgo_testrunner` — GREEN (**51** unit passed + **1** ignored
  [the heavy full-corpus test] + **11** doctests; the 150-subset 69/81/0 and the
  30-case 18/12/0 characterizations UNCHANGED).
- full-corpus run — completes: compiler **222 → 85/134/3**, conformance
  **19 → 10/9/0**; the per-case `catch_unwind` keeps the batch alive (only the 4
  denylisted stack-overflow cases are excluded up front).
- `cargo clippy -p tsgo_testrunner --all-targets -- -D warnings` — GREEN.
- `cargo fmt -p tsgo_testrunner -- --check` — GREEN.
- `cargo build --workspace --all-targets` — GREEN.

No `--no-verify`; no test weakened/deleted; no new dependency; no production
(checker/parser/binder/compiler) code touched; `harnessutil` untouched. The
temporary per-case progress instrumentation used to locate the stack-overflow
cases was REMOVED; the committed `PanicLocationCapture` + `panic_groups`
location capture is the intended measurement design.

## Test deltas

- `tsgo_testrunner`: **47 → 51** unit (+4: `full_corpus`, `top_wrong_code_pairs`,
  `panic_groups`, `panic_location_capture`) + **1** new `#[ignore]`d heavy test;
  doctests unchanged (11). No sibling crate touched.

## DEFER list (blocked-by) — Round 12

- **`// @lib` list directives in the harness** — `option_value_for` returns
  `None` for `CommandLineOptionKind::List`, so a case's explicit `// @lib` is
  dropped (only `// @target`'s default lib graph applies). blocked-by: wiring
  `tsoptions` list-option parsing through `set_options_from_test_config`; left
  deferred to avoid touching shared `harnessutil` + churning the 150-subset.
- **Recursion/complexity depth guards** — the 4 denylisted cases need tsc's
  shared-flow / variance-cache / type-to-string / union-complexity (TS2590)
  bounds before they can run without aborting. blocked-by: porting those guards
  (production checker work, out of scope for a measurement round).
- **Non-UTF-8 corpus files** — `run_case` reads via `read_to_string`; a
  deliberately invalid-UTF-8 fixture surfaces as `errored`. blocked-by: a lossy
  / byte-oriented case read (a small runner change, deferred to keep this round
  measurement-only).

## Round 13 — surface binder diagnostics (TS2300 et al.)

Round goal: act on Round 12's **#1 false-NEGATIVE — `missing TS2300 ×94`**. The
binder already DETECTS duplicate identifiers (`internal/binder/symbols.rs:
report_merge_conflict` emits `DUPLICATE_IDENTIFIER_0`/TS2300,
`CANNOT_REDECLARE_BLOCK_SCOPED_VARIABLE_0`/TS2451,
`A_MODULE_CANNOT_HAVE_MULTIPLE_DEFAULT_EXPORTS`/TS2528, the enum-merge
TS2567, ...), but `Program::semantic_diagnostics` collected ONLY checker-pool
diagnostics and dropped the binder's `bindDiagnostics`, so these were produced
and silently discarded. This round wires them through, exactly as tsc's
`getBindAndCheckDiagnostics` = `bindDiagnostics ++ checkDiagnostics`.

### Go ground truth ported

`internal/compiler/program.go:getBindAndCheckDiagnosticsWithChecker` is the
per-file composition:

```go
// Go: internal/compiler/program.go:getBindAndCheckDiagnosticsWithChecker
if p.SkipTypeChecking(sourceFile, false) { return nil }
diags := slices.Clip(sourceFile.BindDiagnostics())          // binder FIRST
diags = append(diags, fileChecker.GetDiagnostics(ctx, sourceFile)...) // then checker
isPlainJS := ast.IsPlainJSFile(sourceFile, compilerOptions.CheckJs)
if isPlainJS { return core.Filter(diags, plainJSErrors.Has) } // plain-JS keeps only plainJSErrors codes
// (isCheckJS JSDocDiagnostics append + @ts-ignore/@ts-expect-error directive filtering — DEFERRED)
```

Confirmed: bind diagnostics are subject to the SAME gate as check diagnostics
(`SkipTypeChecking` → default-lib exclusion + the JS `canIncludeBindAndCheckDiagnostics`
skip), and for a *plain* JS file the combined list is filtered to the
`plainJSErrors` code set (`DUPLICATE_IDENTIFIER_0`/TS2300 is NOT in that set;
`CANNOT_REDECLARE_BLOCK_SCOPED_VARIABLE_0`/TS2451 IS). The final baseline order
is handled by the existing diagnostic-writer sort (by file then position), so
only the per-file bind-then-check MERGE is needed here.

### What landed (Rust locations, surgical/additive)

`internal/compiler/program.rs`:

- **`binder_diagnostic_to_checker(&BinderDiagnostic, text) -> tsgo_checker::Diagnostic`**
  — the conversion bridge (the two crates keep distinct diagnostic types; Go
  has one `*ast.Diagnostic`). Maps `code`/`category` from the static `Message`,
  localizes the text exactly as the checker does
  (`tsgo_diagnostics::format(&message.to_string(), args)`), and converts the
  binder's `related` list into `related_information` RECURSIVELY. The span is
  trivia-skipped against the OWNING file's text
  (`tsgo_scanner::skip_trivia(text, loc.pos())..loc.end()`), matching Go's
  `createDiagnosticForNode` → `scanner.GetErrorRangeForNode` (default case for
  the name nodes the binder reports merge conflicts on), so it byte-matches
  tsc's baseline.
- **`Program::bind_and_check_diagnostics_grouped()`** — the per-bound-file merge
  (binder diagnostics FIRST, then the pool's checker diagnostics), gated by the
  SAME `is_excluded_from_semantic_diagnostics` mask (default-lib + JS-skip) as
  the checker path. For a *plain* JS file the binder diagnostics are filtered to
  the binder slice of `plainJSErrors` (`binder_code_allowed_in_plain_js`:
  TS2451/TS2528/TS2752/TS2753 kept, TS2300/TS2567 dropped).
- **`Program::semantic_diagnostics` / `semantic_diagnostics_by_file`** now both
  derive from the grouped merge (flatten / zip-with-names), so the harness
  baseline (which consumes `semantic_diagnostics_by_file`) and the flat API both
  surface bind diagnostics attributed to the owning file.
- **`Program::is_plain_js_file`** — 1:1 port of `ast.IsPlainJSFile` (the
  `@ts-check` directive arm is DEFERRED behind the parser's check-js scan).

No production binder/checker/parser code was touched; the bridge lives entirely
in the compiler crate.

### RED→GREEN + guard tests (`internal/compiler/program_test.rs`)

- `binder_duplicate_identifier_surfaces_ts2300` (RED→GREEN) — `class C {} class C {}`
  now surfaces 2× TS2300 with trivia-skipped spans `(6,1)`/`(17,1)` and correct
  per-file attribution.
- `binder_block_scoped_redeclare_surfaces_ts2451` (RED→GREEN) — `const x=1; const x=2;`
  surfaces TS2451 (the block-scoped arm), never TS2300.
- `legal_merges_produce_no_duplicate_identifier` (GUARD, no over-report) —
  `interface I {} interface I {}`, `namespace N {} namespace N {}`,
  `function f(){} namespace f {}`, `enum E {} namespace E {}` → ZERO
  TS2300/TS2451 (the excludes/merge rules are honored on VALID input).
- `binder_diagnostics_in_default_lib_are_not_reported` (GATE) — no bind/check
  diagnostic is attributed to a `bundled:///libs/` file.
- `check_js_false_suppresses_binder_duplicate` (GATE) — a `.js` file with
  `checkJs:false` is skipped, so its binder TS2300 does not surface.
- `plain_js_filters_ts2300_but_keeps_ts2451` (GATE) — plain JS drops binder
  TS2300 (not in `plainJSErrors`) but keeps TS2451 (in `plainJSErrors`).
- `binder_multiple_default_exports_carries_related_info` (related-info) —
  `export default 1; export default 2;` surfaces TS2528 carrying a TS2752
  related entry (recursive `related` → `related_information` round-trip).

### Measurement — full corpus BEFORE→AFTER

`tests/cases/compiler` (222 cases ran):

| metric | BEFORE | AFTER | Δ |
|---|---|---|---|
| passed | 85 | 85 | 0 |
| failed | 134 | 134 | 0 |
| errored | 3 | 3 | 0 |
| **missing TS2300** | **×94** | **×52** | **−42** |
| extra TS2300 | 0 | ×8 | +8 (see below) |
| extra TS2451 | 0 | ×8 | +8 (see below) |

`missing TS2300 ×94 → ×52` (−42 surfaced correctly) is the headline. The pass
count is flat because every TS2300-bearing case ALSO carries other reachable
gaps (the dominant `extra TS2304 ×96 / TS2339 ×63` unresolved-name cascade,
missing relation codes, ...), so surfacing TS2300 alone flips no case to a
byte-exact PASS. The remaining `missing TS2300 ×52` are duplicates our partial
binder does not yet detect (cross-file / checker-level duplicate detection,
plus merge cases the binder handles differently) — DEFERRED.

`tests/cases/conformance` (19 cases): **10/9/0** BEFORE and AFTER (unchanged; no
TS2300 there — the suite's misses are the TS2304 cascade + JSDoc TS8024).

### Over-report validation (CRITICAL) — both roots are PARSER recovery, DEFERRED

The new `extra TS2300 ×8` + `extra TS2451 ×8` are confined to exactly TWO cases,
BOTH already-FAILing and BOTH parser-recovery cascades (NOT binder excludes/merge
bugs — every spurious diagnostic is on an EMPTY (`''`) name co-located with the
parser's own TS2304/TS1005/TS1003/TS1128 recovery errors):

- **`awaitObjectLiteral.ts` → extra TS2451 ×8.** Our parser does not yet support
  TOP-LEVEL `await` (`const foo = await { bar: 42 }`); recovery synthesizes
  empty-named declarations the binder then flags as block-scoped redeclares.
  tsc's baseline is CLEAN (no `.errors.txt`). DEFER — blocked-by: top-level
  `await` parsing.
- **`jsxTernaryWithObjectInAttribute.tsx` → extra TS2300 ×8.** Our `.tsx` parser
  fails on the complex ternary-with-object-in-attribute (the same JSX-recovery
  backlog as Round 12's `extra TS1005/TS1003/TS1128`); recovery synthesizes
  empty-named declarations the binder flags as duplicate identifiers. tsc's
  baseline is 8× TS7026 only. DEFER — blocked-by: JSX ternary / object-in-attribute
  parser recovery.

Neither case regressed PASS→FAIL (both were already failing). The binder's
merge/excludes rules are CORRECT on valid input, proven by the
`legal_merges_produce_no_duplicate_identifier` guard. NO binder diagnostic was
broadly suppressed to hit a number.

### 150-subset characterization BEFORE→AFTER

`expanded_compiler_subset_parity_smoke` is UNCHANGED on the headline numbers
(**passed 69, failed 81, errored 0**; categories `25/37/19`; `top_extra(2) =
[(2304,34),(2339,16)]`). The ≤25-line subset has NO missing-TS2300 case, so the
duplicate-identifier improvement does not show there; the only new extra is the
`awaitObjectLiteral.ts` top-level-await TS2451 ×8 cascade (it is ≤25 lines;
`jsxTernary…` is longer and outside the subset). Added Round-13 guards pinning
`extra TS2451 == 8`, `missing TS2300 == None`, and the unchanged `top_extra(2)`
so the new extra is explicit and cannot drift silently.

### Gate results (Round 13)

- `cargo test -p tsgo_binder` — 60 GREEN.
- `cargo test -p tsgo_checker` — GREEN (771/10/178 across the test binaries).
- `cargo test -p tsgo_compiler` — 111 unit + 11 doctests GREEN (7 NEW Round-13
  tests).
- `cargo test -p tsgo_testrunner` — 51 unit + 1 ignored (heavy measurement) + 11
  doctests GREEN; the 18/12/0 and 69/81/0 characterizations hold with the new
  Round-13 guards.
- `cargo test -p tsgo_testutil_harnessutil` — 11 unit + 4 doctests GREEN (bind
  diagnostics now flow into the baseline).
- `cargo clippy -p tsgo_compiler -p tsgo_testrunner --all-targets -- -D warnings`
  — clean.
- `cargo fmt -p tsgo_compiler -p tsgo_testrunner -- --check` — clean.
- `cargo build --workspace --all-targets` — clean.

No `--no-verify`; no test weakened/deleted; no new dependency; no production
binder/checker/parser code touched (the bridge is compiler-crate-only).

### DEFER list (blocked-by) — Round 13

- **Remaining `missing TS2300 ×52`** — duplicates the partial binder does not
  detect (cross-file duplicate identifiers, checker-level duplicate-member /
  duplicate-declaration checks, and merge cases the binder handles differently).
  blocked-by: the checker's duplicate-symbol diagnostics + a fuller binder merge
  surface.
- **`extra TS2451 ×8` (`awaitObjectLiteral.ts`)** — blocked-by: top-level
  `await` parsing (parser recovery synthesizes empty-named declarations).
- **`extra TS2300 ×8` (`jsxTernaryWithObjectInAttribute.tsx`)** — blocked-by:
  complex JSX ternary / object-in-attribute parser recovery.
- **plain-JS `plainJSErrors` filter (checker half) + `isCheckJS`
  `JSDocDiagnostics` append + `@ts-ignore`/`@ts-expect-error` directive
  filtering** — Go filters the COMBINED bind+check list for plain JS and appends
  JSDoc diagnostics for checkJS; this round applies the `plainJSErrors` filter
  to the BINDER contribution only (the checker-half plain-JS filter is a
  pre-existing divergence). blocked-by: the checker's JSDoc-diagnostic +
  comment-directive surface (the `@ts-*` directives are also unparsed).

# Round 14 — cross-module import/alias resolution (TS2304 cascade)

Round goal: attack Round 12's **#1 false-positive cascade — `extra TS2304 ×96`
("Cannot find name") + the `extra TS2339 ×63` it amplifies**. The dominant root
is cross-module import/ALIAS resolution: the binder already binds `import { x }
from "m"` / `import d from "m"` / `import * as ns from "m"` / `import x =
require("m")` local names as `SymbolFlagsAlias` symbols and gives each external
module a `ValueModule` symbol with an `exports` table, but the checker's
`resolveAlias` chain was a stub (`skip_alias` only followed an already-computed
`aliasTarget`), so EVERY imported name failed the `Value`-meaning lookup in
`checkIdentifier` and cascaded into TS2304 (member access on the unresolved
namespace/default then cascaded TS2339).

## Step-0 categorization of the 96 (+conformance 20) — root → count

A temporary `#[ignore]`d dump (`dump_extra_ts2304`, since REMOVED) ran the full
corpus through `error_baseline_for_test` + the real `parse_error_baseline` /
greedy-extra diff, printing every `+TS2304` with its name and the case's
import/`@filename` context. The 96 compiler (+~21 conformance) extra TS2304
group by ROOT:

| root | × (compiler) | tractable? |
|---|---|---|
| **A — cross-file ES import binding** (`import {x}`/`import d`/`import * as ns`, specifier resolves to a LOADED module) | **~55** | **YES — fixed** |
|   · A1 relative ES named/default/namespace (`./m`, `./dep`) | ~13 | |
|   · A2 bare / node_modules package specifiers (`'foo'`, `"x"`, `@scope/pkg`) | ~17 | |
|   · A3 `#`-subpath / package.json `imports` + tsconfig `paths` | ~15 | |
|   · conformance `import * as` from package exports (`nodeModulesDeclarationEmit…` ×18 + `jsExports…`) | ~19 | |
| **B — `import x = require()` / `export =` alias** (`exportAssignmentMerging1-4`) | 8 | **YES — fixed** |
| C — expando / namespace-function merge / same-name-as-class VALUE access (`classFields…` ×14, `*DecoratorsEnumAccessSameNameAsClass` ×7, `declarationEmitExpandoFunction/Overloads` ×7) | ~28 | DEFER |
| F — parser recovery (`.tsx` JSX, top-level `await`, `using`, assertion fn) | ~11 | DEFER (separate lanes) |
| E — JS/JSDoc class exports (`controlFlowJSClassProperty.js`, `jsdocVariadicInOverload.js`) | ~3 | DEFER |
| other (self `typeof`, expando contextual) | ~3 | DEFER |

The DOMINANT TRACTABLE root is **A (+ B) — cross-module import/alias binding to
an already-loaded module** (~55 compiler + ~19 conformance + 8 import-equals).
Crucially the corpus shows TS2304 (not TS2307) on these, proving the target
modules ARE resolved+loaded by the compiler — only the checker's alias
resolution was missing. This round fixes A (all variants) and B.

## Go functions ported (→ Rust locations)

Binder (verified ALREADY done, untouched): import/alias declarations bind as
`SymbolFlagsAlias` locals (`internal/binder/lib.rs:861` Import{Specifier,
EqualsDeclaration}/NamespaceImport/ExportSpecifier; `:1027` `bind_import_clause`
default name), and `bind_source_file_as_external_module` (`:908`) gives each
module file a `ValueModule` symbol whose `exports` table carries its exports.

Compiler — the specifier → module-symbol BRIDGE (Go's `program.GetResolvedModule`
→ `GetSourceFileForResolvedModule` → `file.Symbol`):
- **`FileLoader::resolve_import_specifiers`** (`internal/compiler/fileloader.rs`)
  — returns each `(specifier text, resolved file name)` (the names-only
  projection seeds the load worklist as before).
- **`ProcessedFiles.resolved_modules`** / `FilesParser` — records
  `(containing file, specifier, resolved file)` while loading the graph.
- **`MultiFileBoundProgram::new_with_options_and_modules`**
  (`internal/compiler/multifile.rs`) — builds `(importing-file-index, specifier)
  → target ValueModule symbol` and implements **`BoundProgram::resolve_module_symbol`**
  (the new trait method, default `None`).

Checker — the `resolveAlias` chain (`internal/checker/core/declared_types.rs`,
all `// Go:`-anchored to `internal/checker/checker.go`):
- **`resolve_alias`** ← `Checker.resolveAlias` (cached `alias_targets`; cycle
  guard `aliases_resolving`; a missing export caches `None` = Go's
  `unknownSymbol`).
- **`get_target_of_alias_declaration`** ← `getTargetOfAliasDeclaration`, with
  **`get_target_of_import_specifier`** / **`get_target_of_import_clause`** /
  **`get_target_of_namespace_import`** / **`get_target_of_import_equals_declaration`**.
- **`get_external_module_member`** ← `getExternalModuleMember`,
  **`get_target_of_module_default`** ← `getTargetOfModuleDefault`,
  **`resolve_external_module_name`** ← `resolveExternalModuleName`,
  **`resolve_external_module_symbol`** ← `resolveExternalModuleSymbol` (`export =`),
  **`get_export_of_module`** ← `getExportOfModule`/`getExportsOfModule`,
  **`resolve_symbol`** ← `resolveSymbolEx`.
- **`check_identifier`** (`check.rs`) — the `getSymbol` ALIAS fallback: when the
  direct `Value` lookup misses, re-lookup with `ALIAS` meaning, `resolve_alias`,
  and if the target denotes a value (or resolution failed → Go's `unknownSymbol`,
  reported once, returned to suppress a cascading TS2304) the alias resolves.
- **`get_type_of_symbol`** (`declared_types.rs`) — the alias arm: a resolved
  alias's value type is its target's type (`get_type_of_module` already builds a
  module's object type from `exports`, so `ns.x` reads an export).

## RED→GREEN + guard tests (`internal/compiler/program_test.rs`)

- `cross_module_named_import_resolves_no_2304` (RED→GREEN, headline) —
  `export const a = 1;` + `import { a } from "./m"; a;` → ZERO TS2304.
- `cross_module_default_import_resolves_no_2304` (RED→GREEN) — `export default
  function greet(){}` + `import greet from "./m"; greet();`.
- `cross_module_namespace_import_member_resolves_no_2304_no_2339` (RED→GREEN) —
  `import * as ns from "./m"; ns.a;` → no TS2304 on `ns`, no TS2339 on `ns.a`.
- `cross_module_import_equals_require_export_assignment_resolves_no_2304`
  (RED→GREEN) — `export = { x: 1 }` + `import mod = require("./a"); mod;`.
- `cross_module_missing_named_export_reports_2305_not_2304` (GUARD) — `import {
  nope } from "./m"` → TS2305 `Module '"./m"' has no exported member 'nope'.`,
  never a silent resolve and never a TS2304 (module name = the quoted specifier,
  byte-matching the committed baseline).
- `cross_module_undefined_name_still_reports_2304` (GUARD) — a genuinely
  undefined bare name still reports TS2304 (alias resolution does not blanket-mute
  the diagnostic).

## Over-report validation (CRITICAL) — the synthetic-default false TS2305

The first full-corpus run after the chain surfaced ONE new `extra TS2305` in
`allowSyntheticDefaultImports9.ts` (`import { default as Foo } from "./b"`),
where tsc's committed baseline is CLEAN (synthetic default resolves `Foo` to the
whole CommonJS module). Fixed faithfully: `get_target_of_import_specifier` routes
a `default`-named import through `get_target_of_module_default` (Go's
`ModuleExportNameIsDefault` branch), and the no-default arm DEFERs the
synthetic-default / TS1192 decision (returns unresolved WITHOUT a diagnostic)
rather than emitting a false TS2305/TS1192. The GUARD for genuine NAMED exports
(TS2305) is intact — proven by `cross_module_missing_named_export_reports_2305_not_2304`.

## Measurement — full corpus BEFORE→AFTER

`tests/cases/compiler` (222 ran):

| metric | BEFORE (R13) | AFTER (R14) | Δ |
|---|---|---|---|
| **passed** | **85** | **100** | **+15** |
| failed | 134 | 119 | −15 |
| errored | 3 | 3 | 0 |
| category no_baseline_but_errors | 45 | 30 | −15 (15 clean cases now PASS) |
| category missing_all_errors | 57 | 62 | +5 (divergent→missing as a spurious TS2304 is removed) |
| category divergent | 32 | 27 | −5 |
| **extra TS2304** | **×96** | **×50** | **−46** |
| extra TS2339 | ×63 | ×63 | 0 (net; namespace members resolve, JS-expando modules surface a DEFERRED `{}`-shape TS2339) |
| extra TS2305 (false) | 0 | 0 | guard fires only for genuine missing named exports |

`tests/cases/conformance` (19 ran): **10/9/0 → 11/8/0 (+1 PASS)**; **extra TS2304
×20 → ×2 (−18)**.

**Honest flip count: +16 cases to byte-exact PASS** (15 compiler + 1
conformance). NO PASS→FAIL regression: every import-bearing case was already
FAILing on the TS2304 cascade, so resolving imports can only remove a
false-positive (the passed count rose monotonically and the only category to
shrink among the "we-emit-errors" buckets is `no_baseline_but_errors`, i.e. clean
cases that now produce nothing).

`extra TS2339 ×63` is unchanged NET: namespace-member accesses that used to
short-circuit on an unresolved (`error`) receiver now resolve, while a handful of
JS-expando / CommonJS-JS `import * as`/default imports resolve their module to an
incomplete `{}`-shape object type (the expando-export root C/E is DEFERRED), so
`ns.x` reports a `{}`-shape TS2339 instead of the old TS2304. Net flat, no
regression.

## 150-subset characterization BEFORE→AFTER

`expanded_compiler_subset_parity_smoke`: **passed 69 → 78 (+9)**, failed 81 → 72,
errored 0; categories `25/37/19 → 16/41/15`; **`top_extra(2) = [(2304,34),
(2339,16)] → [(2304,17),(2339,16)]`**. The pinned guards (`extra TS2451 ==8`
top-level-await, `extra TS1005==5`/`TS1003==3` parser, `extra TS2769==1`,
`missing TS2300==None`, `top_missing(1)=[(2874,7)]`, no `extra TS7026`) are
UNCHANGED and re-asserted. The curated 30-case smoke (`18/12/0`) is unaffected
(its cases have no resolved cross-module imports).

## Gate results (Round 14)

- `cargo test -p tsgo_checker` — GREEN (771 lib + 178 doctests; alias arm covered
  by the compiler integration tests via the real `MultiFileBoundProgram` bridge).
- `cargo test -p tsgo_compiler` — 117 lib (111 + 6 NEW Round-14 cross-module
  tests) + 11 doctests GREEN.
- `cargo test -p tsgo_testrunner` — 51 lib + 2 ignored (heavy measurement) + 11
  doctests GREEN; the 150-subset 78/72/0 and 30-case 18/12/0 characterizations
  re-pinned.
- `cargo test -p tsgo_binder` (60) / `-p tsgo_testutil_harnessutil` (11) /
  `-p tsgo_ls` (39) / `-p tsgo_execute` (80) — GREEN (no sibling regression).
- `cargo clippy -p tsgo_checker -p tsgo_compiler -p tsgo_testrunner --all-targets
  -- -D warnings` — clean.
- `cargo fmt -p tsgo_checker -p tsgo_compiler -p tsgo_testrunner -- --check` —
  clean. `cargo build --workspace --all-targets` — clean.

No `--no-verify`; no test weakened/deleted; no new dependency; the binder was
verified already-correct and left untouched; the temporary `dump_extra_ts2304`
categorization test was REMOVED (the tree is clean of throwaway code).

## DEFER list (blocked-by) — Round 14

- **Expando / namespace-function merge / same-name-as-class VALUE access
  (`extra TS2304 ~28`, root C)** — `Foo`/`MyEnum`/`A`/`B`/`C` used as values where
  the binder did not merge the function/namespace/expando declaration into a
  value symbol. blocked-by: expando-member + function/namespace merge in the
  binder/checker (a separate root).
- **JS-expando / CommonJS-JS module exports (the new `{}`-shape TS2339)** — a JS
  module's `module.exports.x = …` / expando exports are not fully captured, so
  `import * as ns` / default import resolves to an incomplete `{}` object type.
  blocked-by: the CommonJS/expando export-table population (root C/E).
- **Synthetic-default / `esModuleInterop` / `allowSyntheticDefaultImports`** —
  `import d from "m"` / `import { default as X }` over a CommonJS module with no
  explicit `default`: the no-default arm is left unresolved WITHOUT a diagnostic
  (so no false positive), DEFERRing both the synthetic-default resolution and the
  TS1192/TS2613 "no default export" reports. blocked-by: `canHaveSyntheticDefault`.
- **`export *` star re-exports + `export { x } from "m"` re-export specifiers +
  `export =` supplemental type exports** — `get_export_of_module` reads direct
  exports only (no `getExportsOfModuleWorker` star visit), and
  `get_target_of_alias_declaration` DEFERs `ExportSpecifier`/`NamespaceExport`/
  `ExportAssignment`. blocked-by: the re-export visit + export-assignment alias.
- **Type-position imports** — `resolve_entity_name` (type references) does not yet
  follow alias targets (only `check_identifier` value references do), so an
  imported TYPE used in a type annotation is not resolved through the alias chain.
  blocked-by: threading checker alias state into `resolve_entity_name`.
- **`import x = M.N` entity-name module references** — only the `require("m")` /
  external-module form of import-equals is ported. blocked-by: `resolveEntityName`
  alias chaining.
- **Parser-recovery TS2304 (root F: `.tsx` JSX, top-level `await`, `using`)** and
  **JS/JSDoc class exports (root E)** — separate parser / JS-checking lanes.

## Round 15 — parser top-level-await reparse + JSX-adjacent recovery

**Root / Go ground truth.** Two parser-recovery false-positive roots from the
full-corpus map:
- **Top-level `await`** (`awaitObjectLiteral.ts`): a module file (each file has
  `export`) whose top-level `const foo = await { ... }` was parsed with `await`
  as an IDENTIFIER outside await context, synthesizing empty-named declarations
  → spurious `extra TS1005 ×5 / TS1003 ×3 / TS2304 ×2` and (since Round 13
  surfaced binder diagnostics) `extra TS2451 ×8` (empty-named block-scoped
  redeclares). Go reparses the file under await context when it discovers a
  top-level `await` identifier in a module — `parser.go:reparseTopLevelAwait`
  (gated by `hasExternalModuleIndicator`), driven by `statementHasAwaitIdentifier`
  + `possibleAwaitSpans`.
- **JSX attribute-value recovery** (`jsxAttributeValueBinaryExpression.tsx`): our
  parser mis-recovered, emitting `wrong_code TS7026→TS1128` + `extra TS1109` +
  an empty-name `TS2304`; Go parses it cleanly so the checker emits exactly
  `TS2657` (+ the checker's `TS2304` + 2× `TS7026`).

**Rust landing** (`internal/parser/lib.rs`, all `// Go:`-anchored): ported
`statement_has_await_identifier`, `possible_await_spans`,
`has_external_module_indicator`, and `reparse_top_level_await` (rewinds
scanner/context state and reparses the affected top-level statements under await
context, keeping reparse diagnostics) wired into `parse_source_file_worker`;
plus JSX/arrow recovery helpers (`is_missing_node_list`,
`type_has_arrow_function_blocking_parse_error`, `create_missing_list`). Small
supporting `internal/ast/lib.rs` (+8), `ast/visitor.rs` (+2),
`transformers/modifiervisitor.rs` (+1) changes.

**RED→GREEN + guards** (parser 122→129 unit): top-level `await foo;` in a module
parses as an await expression (no TS2304/TS1005); a SCRIPT file still errors per
Go; JSX `<a b={x ? {y:1} : {z:2}} />` / `<a b={1 + 2} />` parse clean.

**Parity BEFORE→AFTER.** 150-subset **78/72 → 80/70 (+2)**:
`awaitObjectLiteral.ts` (top-level await) + `jsxAttributeValueBinaryExpression.tsx`
flip to byte-exact PASS. 30-case smoke 18→19. `extra TS2304 17→14`,
`extra TS1005 5→0`, `extra TS1003 3→0`, the `TS2451 ×8` cascade cleared, and the
`TS7026→TS1128` wrong-code cleared; `top_extra(2)` becomes `[(2339,16),(2304,14)]`.
Zero regressions. `jsxTernaryWithObjectInAttribute.tsx` (40 lines, outside the
≤25-line subset) + `using`/other recovery roots deferred.

## Gate results (Round 15)
- `cargo test -p tsgo_parser` (129 unit + 7 doctests) · `-p tsgo_checker` (771) ·
  `-p tsgo_compiler` (117) · `-p tsgo_testrunner` (51 + 1 ignored) — all GREEN.
- `cargo clippy … -- -D warnings` + `cargo fmt -- --check` + `cargo build
  --workspace --all-targets` — GREEN.

No `--no-verify`; additive; no new deps; not committed by the subagent (the
round aborted on a network error after completing the implementation + tests +
snapshot; the parent finished verification, added this worklog section, and
committed).

# Round 16 — assignability false positives (TS2345/TS2322): rest-parameter expansion

Round goal: act on Round 12's prioritization #3 — the assignability/relation
FALSE POSITIVES `extra TS2345 ×23` + `extra TS2322 ×18` (we REJECT code tsc
ACCEPTS). These drive `no_baseline_but_errors` cases, so each fix is a clean
case-flip. This round fixes the LARGEST tractable root.

## Step-0 categorization (full corpus + conformance) — root → count

A TEMPORARY `#[ignore]`d dump (`dump_extra_assignability`, since REMOVED) ran the
full corpus through `error_baseline_for_test` + the real `parse_error_baseline`
diff, printing every extra `+TS2345`/`+TS2322` with its FULL message (source +
target types) and case file. The 23 TS2345 + 18 TS2322 group by ROOT:

| root | TS2345 | TS2322 | tractable? |
|---|---|---|---|
| **R1 — REST PARAMETER not expanded in call checking** (`f(...a: any[])` / `console.log(...data: any[])`: each arg related to the whole rest array `Array<any>` instead of its element type) | **~15** | 0 | **YES — fixed** |
|   · `reachabilityChecks9/10`, `reachabilityChecksIgnored`, `removeComments`, `assertsPredicateParameterMismatch`, `typePredicateParameterMismatch` (`console.log("…")`) | 14 | | |
|   · `keyofUnresolvedBaseMembers` (`new () => …` arg vs `Array<any>` rest) | 1 | | |
| R2 — union target with object/discriminant constituents (`missingDiscriminants`/`missingDiscriminants2`: `{str;num}` vs `{str:"a";num:0} \| …`, and `string` vs `"a"\|"b"`) | 0 | 10 | DEFER (discriminant/literal narrowing) |
| R3 — generic-method rest instantiation (`freshObjectLiteralSubtype`: `.push({…})` rest `Array<T>` not instantiated to element) | 2 | 0 | DEFER (generic member instantiation — see below) |
| R4 — conditional / never / inference gaps (`inferenceWithNeverSource1` ×3, `switchExhaustiveNarrowing` `… -> never`, `conditionalContextualReturnSubstitutionCache` `T -> conditional`) | 4 | 1 | DEFER (conditional types / narrowing) |
| R5 — `error`-typed member / intersection (`jsxFunctionTypeChildren`, `declarationEmitExpandoArrowFunctionParameter`) | 2 | 1 | DEFER (cascade from an upstream `error` type) |
| R6 — `undefined -> string` / construct-signature (`settingsSimpleTest`, `keyofUnresolvedBaseMembers` `_Foo -> new () => …`) | 0 | 2 | DEFER |

The DOMINANT TRACTABLE root is **R1 — rest-parameter expansion** (~15 of 23
TS2345 + the symmetric `extra TS2554 ×3` multi-arg arity over-reports). This is
NOT a structural-relation gap in `relations.rs`: the relation `string`/`number`
↔ `Array<any>` correctly FAILS — the bug is UPSTREAM in the call/argument path,
which fed the WHOLE rest array (`...data: any[]` → `Array<any>`) as the target
type for each argument instead of the rest ELEMENT type (`any`). Fixing the
argument→parameter relation's TARGET is the precise port.

## Go ground truth ported (→ Rust locations)

```go
// Go: internal/checker/relater.go:Checker.tryGetTypeAtPosition(1762)
paramCount := len(signature.parameters) - core.IfElse(signatureHasRestParameter(signature), 1, 0)
if pos < paramCount { return c.getTypeOfParameter(signature.parameters[pos]) }
if signatureHasRestParameter(signature) {
    restType := c.getTypeOfSymbol(signature.parameters[paramCount])
    index := pos - paramCount
    if !isTupleType(restType) || ... { return c.getIndexedAccessType(restType, c.getNumberLiteralType(jsnum.Number(index))) }
}
return nil
// Go: internal/checker/checker.go:hasRestParameter / isRestParameter / getSignatureFromDeclaration
if hasRestParameter(declaration) { flags |= SignatureFlagsHasRestParameter }
// Go: internal/checker/checker.go:Checker.hasCorrectArity(9070)
if !c.hasEffectiveRestParameter(signature) && argCount > effectiveParameterCount { return false }
```

Rust landing (all `// Go:`-anchored):
- **`internal/checker/core/declared_types.rs`** — `get_signature_from_declaration`
  now sets `SignatureFlags::HAS_REST_PARAMETER` when its last parameter carries a
  `...` token (new `has_rest_parameter(program, param_nodes)` ←
  `hasRestParameter`/`isRestParameter`). The flag propagates to instantiated
  signatures (`SignatureFlags::PROPAGATING_FLAGS` already includes it).
- **`internal/checker/core/contextual.rs`** — `try_get_type_at_position` (the
  canonical `tryGetTypeAtPosition`, already shared by contextual callback typing
  AND the call path) gains the rest arm: a position at/past the last fixed
  parameter of a rest signature reads `getIndexedAccessType(restType,
  numberLiteral(index))`, which for the reachable non-tuple array `T[]` resolves
  to the element type `T` via Array's numeric index signature. Factored the
  mapper application into `parameter_type_with_mapper` (so an instantiated
  signature's rest type is substituted before the element access).
- **`internal/checker/core/check.rs`** — `get_type_at_position` now delegates to
  `try_get_type_at_position` (`.unwrap_or(any)`). `has_correct_arity` ports the
  `!hasEffectiveRestParameter && argCount > parameterCount` rejection so a rest
  parameter lifts the "too many arguments" cap. New `signature_has_rest_parameter`
  (← `signatureHasRestParameter`) + `has_effective_rest_parameter` (←
  `hasEffectiveRestParameter`, non-tuple-array subset) helpers.

This is a CALL/argument-path fix (not `relations.rs` structural relation) — the
relation engine was already correct; the bug was the wrong TARGET fed into it.
`relations.rs`'s own signature-relation `try_signature_type_at_position` is left
as-is (a separate function-type-assignability path, not this bucket); setting the
flag is behavior-neutral for it (it does not read `HAS_REST_PARAMETER`).

## RED→GREEN + guard tests

Checker (`internal/checker/core/check_test.rs`, +4):
- `rest_parameter_call_accepts_assignable_argument` (RED→GREEN headline) —
  `function f(...args: number[]){}; f(1)` → ZERO diagnostics (was `2345: number
  not assignable to Array<number>`).
- `rest_parameter_call_accepts_many_assignable_arguments` (RED→GREEN) —
  `f(1, 2, 3, 4)` → no `2554` (the effective-rest arity cap).
- `rest_parameter_call_incompatible_argument_still_reports_2345` (GUARD) —
  `f("x")` STILL reports `2345` with the ELEMENT type `number` (not `number[]`),
  proving the target narrowed without muting.
- `rest_parameter_after_fixed_parameter_relates_each_position` (GUARD) —
  `f(first: string, ...rest: number[])`: `f("a",1,2)` clean; `f("a","b")` → `2345`
  on the rest element.

Compiler real-lib (`internal/compiler/program_test.rs`, +2):
- `rest_parameter_lib_call_accepts_elements_with_real_lib` (RED→GREEN) —
  `String.fromCharCode(65, 66, 67)` over the es5 lib's
  `fromCharCode(...codes: number[])` → no `2345`/`2554`.
- `rest_parameter_lib_call_rejects_incompatible_with_real_lib` (GUARD) —
  `String.fromCharCode("bad")` STILL reports `2345`.

(`Array<T>.push` was tried first but exposed an ORTHOGONAL deferred gap —
generic-method rest types are not instantiated through the receiver's
`Array<T> -> Array<number>` mapper, so `push`'s rest stays `T[]`; a non-generic
lib rest signature (`fromCharCode`) is the faithful real-lib probe of THIS
bucket, matching the corpus's all-concrete `Array<any>` false positives.)

## Measurement — full corpus BEFORE→AFTER

`tests/cases/compiler` (222 ran):

| metric | BEFORE (R15) | AFTER (R16) | Δ |
|---|---|---|---|
| **passed** | **102** | **104** | **+2** |
| failed | 117 | 115 | −2 |
| errored | 3 | 3 | 0 |
| no_baseline_but_errors | 29 | 27 | −2 (2 clean cases now PASS) |
| missing_all_errors | 62 | 66 | +4 (divergent→missing as the lone extra TS2345 is removed) |
| divergent | 26 | 22 | −4 |
| **extra TS2345** | **×23** | **×8** | **−15** |
| **extra TS2554** | **×3** | **×1** | **−2** |
| extra TS2322 | ×18 | ×18 | 0 (R2/R4/R6 buckets — DEFERRED) |
| missing TS2345 | ×3 | ×3 | **0 (NO over-relaxation)** |
| missing TS2322 | ×10 | ×10 | **0 (NO over-relaxation)** |

No other extra/missing code changed; no new code appeared. `conformance`
(19 ran): **11/8/0 → 11/8/0** (its extra TS2345/TS2554 set is empty — the rest
cases live in the compiler suite).

**Honest flip count: +2 compiler cases to byte-exact PASS** (both were
`no_baseline_but_errors` — clean files we wrongly errored on with a rest-param
TS2345). The other ~13 cleared TS2345 sit in cases that ALSO miss a committed
error (e.g. `reachabilityChecks*` expects `TS7027` "unreachable code", a deferred
false-negative), so removing the false positive shifts them divergent→missing
(failed unchanged, but the over-report is gone). NO PASS→FAIL regression: a
removed false positive can only help, and `missing TS2345/TS2322` are UNCHANGED
(the relation still fires for genuinely-incompatible rest arguments — proven by
the four GUARD tests).

## 150-subset characterization BEFORE→AFTER

`expanded_compiler_subset_parity_smoke`: **passed 80 → 81 (+1)**, failed 70 → 69;
categories `no_baseline 15→14, missing_all 41→43, divergent 14→12` (one clean
no-baseline case flips to PASS; two divergent cases — whose only extra was the
cleared rest TS2345 — shift to missing_all_errors). The pinned guards
(`top_extra(2)=[(2339,16),(2304,14)]`, `extra TS2451/1005/1003/1155==None`,
`missing TS2300==None`, `top_missing(1)=[(2874,7)]`, no `extra/missing TS7026`)
are UNCHANGED and re-asserted; `extra TS2345 ×3` (the deferred R4 never/inference
cases `inferenceWithNeverSource1`) stays out of the top-2. The 30-case smoke
(`19/11/0`) is unaffected.

## Gate results (Round 16)

- `cargo test -p tsgo_checker` — GREEN (**775** lib [+4 rest tests] + 178
  doctests).
- `cargo test -p tsgo_compiler` — GREEN (**119** lib [+2 real-lib rest tests] +
  doctests).
- `cargo test -p tsgo_testrunner` — GREEN (51 unit + **1** ignored
  [full-corpus measurement] + 11 doctests; 150-subset 81/69/0 + 30-case 19/11/0
  re-pinned).
- `cargo test -p tsgo_binder` (60) / `-p tsgo_execute` (39) / `-p tsgo_ls` (80) /
  `-p tsgo_testutil_harnessutil` (11) — GREEN (no sibling regression).
- `cargo clippy -p tsgo_checker -p tsgo_compiler -p tsgo_testrunner --all-targets
  -- -D warnings` — clean. `cargo fmt … -- --check` — clean.
  `cargo build --workspace --all-targets` — clean.

No `--no-verify`; additive/surgical; no test weakened/deleted; no new dependency;
the relation was NOT broadly relaxed (the precise rest-element TARGET was ported,
invalid rest arguments still report 2345); the temporary `dump_extra_assignability`
categorization test was REMOVED (tree clean — only the 6 intended files modified,
`Cargo.lock` untouched). Not committed by the subagent (left to the parent).

## DEFER list (blocked-by) — Round 16

- **Tuple-rest parameters** (`...args: [number, string]`) — `getParameterCount` /
  `getMinArgumentCount` / `hasEffectiveRestParameter` / `tryGetTypeAtPosition`
  all have the non-tuple-array subset; the fixed/variadic tuple-element arms are
  DEFERRED. blocked-by: tuple types.
- **Generic-method rest instantiation (R3, `freshObjectLiteralSubtype` ×2)** —
  `xs.push(x)` over `Array<number>` reads `push`'s rest as `T[]` (not
  `number[]`), because a method signature accessed through a generic receiver is
  not instantiated with the receiver's `T -> number` mapper on the CALL path.
  blocked-by: instantiating member call signatures through the receiver's type
  mapper (the relation engine's `instantiated_property_type` does this for
  property RELATION but the call path resolves the bare signature).
- **Union-target discriminant relation (R2, `missingDiscriminants*` `extra
  TS2322 ×10`)** — a source object related to a union of object/discriminant
  constituents: needs discriminant-property matching + literal-property narrowing
  in `typeRelatedToSomeType`. blocked-by: discriminant matching + the literal
  source→`"a"|"b"` union relate elaboration.
- **Conditional / never / inference (R4, `extra TS2345 ×4` + `TS2322 ×1`)** —
  `inferenceWithNeverSource1` / `switchExhaustiveNarrowing` (`… -> never`) /
  `conditionalContextualReturnSubstitutionCache` (`T -> conditional`).
  blocked-by: conditional-type relation + exhaustive-narrowing-to-`never`.
- **`error`-typed cascade (R5) / `undefined -> string` + construct-signature
  (R6)** — downstream of an upstream `error` type or a deferred construct-sig /
  non-strict-null path. blocked-by: those upstream roots.

# Round 17 — JS expando / this-property member synthesis

Round goal: attack the #1 false-positive root in the full compiler corpus —
`extra TS2339 ×63` ("Property does not exist") — driven by JS/TS EXPANDO
assignments (`function f(){}; f.x = v`) and `this.x = v` whose synthesized
members we did not resolve. tsc treats these as adding a member to the target's
symbol, so `f.x` / `this.x` resolve; we did not, so member access reported a
spurious TS2339. Fix the LARGEST TRACTABLE shapes: (a) function-expando and
(b) `this`-property.

## Step-0 categorization (full corpus, temporary `#[ignore]`d dump, REMOVED)

A throwaway dump (`dump_extra_2339_2304`, since REMOVED) ran the full corpus
through `error_baseline_for_test` + `categorize_diags`, printing every case with
an `extra TS2339`/`extra TS2304`, its produced messages, the committed-baseline
codes (so tsc's `TS7008`/`TS7022` are visible), and the case source. The
`extra TS2339 ×63` group by ROOT shape:

| shape | cases (TS2339 count) | root Go fn | flip needs |
|---|---|---|---|
| **(a) function-expando** (`function f(){}; f.x=v`) | `expandoFunctionAsAssertion`(2, clean), `expandoPropertyEmptyArrayWidening`(1, baseline TS7008), `nonExpandoDeclarations`(3 fn + 1 obj, clean) | `bindDeferredExpandoAssignment` + `getTypeOfFuncClassEnumModuleWorker` | resolution (clean cases); +TS7008 for the empty-array one |
| **(b) `this`-property (JS)** | `jsDeclarationEmitThisAssignment`(3, clean), `widenedThisPropertyAssignment`(2, clean), `jsDeclarationsRequireImportForms`(3, clean+require), `thisPropertyAssignmentTyping`(28, baseline TS2532/2322/7008) | `bindThisPropertyAssignment` + class member resolution | resolution (clean); +CFA/TS7008/TS2322 for the big divergent one |
| (c) namespace/enum/export= VALUE access (`extra TS2304`) | `esDecorators…SameNameAsClass`, `legacyDecorators…`, `exportAssignmentMerging2/3`, `globalArray…` | `getExpandoSymbol`/enum-value/export= alias | DEFER (separate roots) |
| (d) object-literal-in-JS expando (`obj.x=v` on `{}`) | `expandoNoInferredIndex`(3, TS7022), `expandoObjectIndexSignatures`(6, TS7022) | empty-object expando initializer + circular TS7022 | DEFER (obj-literal expando + circularity) |
| (e) `Object.defineProperty` | (none in corpus) | `bindExportsOrObjectDefineProperty` | DEFER |

**Flip analysis.** The clean-baseline cases of shapes (a)+(b) flip with
RESOLUTION ALONE (no implicit-any needed, since the assigned values are typed
and the baselines are empty). The cases that ALSO need `TS7008`/`TS7022`
(`expandoPropertyEmptyArrayWidening`, `thisPropertyAssignmentTyping`,
`expandoNoInferredIndex`, `expandoObjectIndexSignatures`) ALSO need
object-literal-expando typing and/or constructor CFA, so emitting TS7008/TS7022
ALONE would not flip them — they are DEFERRED as a coherent block. This round
ships shapes (a)+(b) RESOLUTION, which clears the bulk of `extra TS2339` and
flips the clean cases.

## Go ground truth ported (→ Rust locations)

Binder (`internal/binder/`, all `// Go:`-anchored to `internal/binder/binder.go`):
- **`bind_expando_property_assignment`** ← `bindExpandoPropertyAssignment` —
  defers the `F.x = v` assignment (capturing the active container /
  block-scope-container) into a new `expando_assignments` collection.
- **`bind_deferred_expando_assignments`** / **`bind_deferred_expando_assignment`**
  ← `bindDeferredExpandoAssignments` / `bindDeferredExpandoAssignment` — after
  the main walk, looks up the target (`F`), resolves its initializer symbol, and
  declares a `Property | Assignment` member into its `exports` (unless a
  non-assignment member already shadows the name). Wired into
  `bind_source_file_inner` (Go's `bindSourceFile`).
- **`get_parent_of_property_assignment`** / **`lookup_entity`** /
  **`lookup_name`** / **`get_initializer_symbol`** / **`is_expando_initializer`**
  ← the matching Go helpers (`getParentOfPropertyAssignment` / `lookupEntity` /
  `lookupName` / `getInitializerSymbol` / `IsExpandoInitializer`). The initializer
  symbol resolves for a `FunctionDeclaration`, a JS `ClassDeclaration`, and a
  `const`/JS variable or JS binary expr whose initializer is a
  function/arrow/JS-class-expression or empty JS object literal.
- **`bind_this_property_assignment`** / **`get_this_class_and_symbol_table`** ←
  `bindThisPropertyAssignment` / `getThisClassAndSymbolTable` — JS-only; declares
  a `Property | Assignment` member into the enclosing class's `members` (instance
  `this` container) or `exports` (static container) table. The private-identifier
  and computed-name guards match Go (computed `addLateBoundAssignment…` DEFERRED).
- **`name_of_declaration`** (`astquery.rs`) ← `GetNonAssignedNameOfDeclaration`'s
  `KindBinaryExpression` arm — an assignment declaration's name is its access
  member (`x` of `f.x`), so `get_declaration_name` names the synthesized member.

Checker (`internal/checker/core/`, all `// Go:`-anchored to
`internal/checker/checker.go`):
- **`get_type_of_func_class_enum_module`** (`declared_types.rs`) ←
  `getTypeOfFuncClassEnumModuleWorker` — the function/class value object type now
  carries `members`/`properties` from the symbol's `exports` (the expando
  members), so `get_property_of_type` resolves `f.x`. (`this.x` resolves through
  the class instance type, whose `members` table the binder populated.)
- **`get_widened_type_for_assignment_declaration`** (`declared_types.rs`) ←
  `getWidenedTypeForAssignmentDeclaration` (reachable subset) — routed from
  `get_type_of_variable_or_property` for a binary-expression value declaration
  (Go's `getTypeOfVariableOrParameterOrPropertyWorker` `KindBinaryExpression`
  arm). Computes the widened union of the assigned right-hand sides
  (`checkExpressionForMutableLocation` widening), with an empty-array RHS widened
  to `any` (Go's empty-array branch yields `any[]`; we use `any`, deferring the
  precise `any[]` shape + the TS7008 report) and a re-entrancy guard
  (`assignment_declaration_resolving`) returning `any` on a self-referential
  `this.x = f(this.x)` (Go's `pushTypeResolution` + `containsSameNamedThisProperty`).

## RED→GREEN + guard tests

Binder (`internal/binder/symbols_test.rs`, +3):
- `bind_function_expando_property_assignment` (RED→GREEN headline) —
  `function f(){}; f.x = 1;` synthesizes `x` (Property|Assignment) into `f`'s
  exports.
- `bind_this_property_assignment_js_class_member` (RED→GREEN) — a JS class
  `constructor(){ this.x = 1; }` synthesizes instance member `x`.
- `bind_this_property_assignment_ts_class_does_not_synthesize` (GUARD) — a TS
  class does NOT synthesize (Go's JS-only guard).

Checker (`internal/checker/core/check_test.rs`, +4):
- `function_expando_property_resolves_no_2339` (RED→GREEN headline) —
  `function f(){}; f.x = 1; f.x;` → ZERO TS2339.
- `function_expando_property_yields_assigned_type` — `f.x` types as the widened
  assigned value `number` (faithful, not bare `any`).
- `function_expando_absent_property_still_reports_2339` (GUARD) — `f.y` (a
  non-expando property) STILL reports TS2339; `f.x` does not.
- `this_property_assignment_resolves_no_2339` (RED→GREEN) — a JS class reading
  `this.x` after `this.x = 1` → ZERO TS2339.

Compiler real-lib (`internal/compiler/program_test.rs`, +2):
- `function_expando_member_resolves_with_real_lib_no_2339` (RED→GREEN, mirrors
  the corpus `expandoFunctionAsAssertion`) — `function example(){}; example.isFoo
  = …; example.isFoo('test');` over the bundled lib → no TS2339.
- `function_expando_absent_member_still_reports_2339_with_real_lib` (GUARD) — a
  genuinely-absent function property still reports TS2339.

## Measurement — full corpus BEFORE→AFTER

`tests/cases/compiler` (222 ran):

| metric | BEFORE (R16) | AFTER (R17) | Δ |
|---|---|---|---|
| **passed** | **105** | **109** | **+4** |
| failed | 116 | 112 | −4 |
| errored | 1 | 1 | 0 |
| no_baseline_but_errors | 28 | 24 | −4 (4 clean cases now PASS) |
| missing_all_errors | 66 | 68 | +2 (divergent→missing as the cleared TS2339 was the only extra) |
| divergent | 22 | 20 | −2 |
| **extra TS2339** | **×63** | **×22** | **−41** |
| extra TS2345 | ×8 | ×8 | **0 (NO regression)** |
| extra TS2304 | ×43 | ×43 | 0 (the namespace/enum/export= value-access cascade — DEFERRED) |
| missing TS7008 | ×5 | ×5 | **0 (no over-resolution masked a real error)** |
| missing TS7022 | ×4 | ×4 | **0** |
| missing TS2339 | ×6 | ×6 | **0** |

`conformance` (19 ran): **11/8/0 → 11/8/0**; its lone `extra TS2339 ×1`
(`esDecoratorsPropertyAccessSameNameAsClass`'s static-member access) clears
(`top extra [(2304,2),(2339,1),(5108,1)] → [(2304,2),(5108,1)]`), but the case
keeps a deferred `TS2304` (enum-as-value), so it does not flip.

**Honest flip count: +4 compiler cases to byte-exact PASS** — the four
clean-baseline expando/this-property cases (`expandoFunctionAsAssertion`,
`jsDeclarationEmitThisAssignment`, `widenedThisPropertyAssignment`,
`nonExpandoDeclarations`), all formerly `no_baseline_but_errors` we wrongly
errored on with an expando/this TS2339. NO PASS→FAIL regression: a removed false
positive can only help, `passed` rose monotonically, and the two divergent cases
that shifted to missing_all_errors (their only extra was the cleared TS2339) stay
failing on a DEFERRED committed error.

**No over-resolution proven:** `missing TS7008/TS7022/TS2339` are UNCHANGED (no
masked real diagnostic), and the genuinely-absent-property GUARDs (checker +
real-lib) keep TS2339 firing. The empty-array→`any` widening keeps
`extra TS2345` at ×8 (an early attempt that left the empty-array element as
`never[]` produced +2 spurious TS2345 in `thisPropertyAssignmentTyping`'s
`this.bar.push("baz")`; faithfully widening the empty array removed it).

## 150-subset characterization BEFORE→AFTER

`expanded_compiler_subset_parity_smoke`: **passed 81 → 84 (+3)**, failed 69 → 66,
errored 0; categories `no_baseline 14→11, missing_all 43→44, divergent 12→11`;
**`top_extra(2) = [(2339,16),(2304,14)] → [(2304,14),(2322,12)]`** and
`extra TS2339 ×16 → ×5` (the residual ×5 are the DEFERRED object-literal expando
+ cross-module-require this-members). The pinned guards (`extra TS2769==1`,
`extra TS2583/1005/1003/1155==None`, `top_missing(1)=[(2874,7)]`,
`missing TS2300==None`, no `extra/missing TS7026`, `extra TS2451==None`) are
UNCHANGED and re-asserted. The curated 30-case smoke (`19/11/0`) is unaffected.

## Gate results (Round 17)

- `cargo test -p tsgo_binder` — GREEN (**63** unit [+3 expando] + 10 doctests).
- `cargo test -p tsgo_checker` — GREEN (**779** lib [+4 expando] + 178 doctests).
- `cargo test -p tsgo_compiler` — GREEN (**121** lib [+2 real-lib expando] +
  doctests).
- `cargo test -p tsgo_testrunner` — GREEN (51 unit + 1 ignored [full-corpus
  measurement]; 150-subset 84/66/0 + 30-case 19/11/0 re-pinned).
- `cargo test -p tsgo_ls` (39) / `-p tsgo_execute` (80) /
  `-p tsgo_transformers` (11) / `-p tsgo_testutil_harnessutil` (311) — GREEN
  (no sibling regression; the binder is upstream).
- `cargo clippy -p tsgo_binder -p tsgo_checker -p tsgo_compiler -p tsgo_testrunner
  --all-targets -- -D warnings` — clean. `cargo fmt … -- --check` — clean.
  `cargo build --workspace --all-targets` — clean.

No `--no-verify`; additive/surgical vertical slice; no test weakened/deleted; no
new dependency (`Cargo.lock` untouched); member access is NOT broadly resolved
(only the binder-synthesized expando/this members resolve — proven by the GUARD
tests); the temporary `dump_extra_2339_2304` categorization test was REMOVED
(tree clean — only the 9 intended files modified). Not committed by the subagent
(left to the parent).

## DEFER list (blocked-by) — Round 17

- **TS7008 / TS7022 implicit-any reports** — `reportImplicitAny`
  (`Member '{0}' implicitly has an '{1}' type` / `'{0}' implicitly has type
  'any' …`) gated by `noImplicitAny` + `IsCheckJSEnabledForFile`, plus the
  empty-array (`any[]`) / all-nullable widening and the circular-initializer
  TS7022. blocked-by: the implicit-any reporting surface + the precise
  `getAssignmentDeclarationInitializerType` widening branches. (These alone would
  not flip the cases that need them — see below — so they are deferred together.)
- **Object-literal-in-JS expando** (`const obj = {}; obj.x = v` on a plain `{}`;
  `expandoNoInferredIndex` ×3, `expandoObjectIndexSignatures` ×6) — needs the
  empty-object-literal expando initializer typing + the circular-`Object.values`
  TS7022. blocked-by: object-literal expando member typing + circularity.
- **Constructor-`this` CFA typing** (`thisPropertyAssignmentTyping`'s
  `this.foo = [3]; this.foo = [this.foo[0]*2]` → `number[]`; the union-of-methods
  + `undefined` typing) — `isConstructorDeclaredThisProperty` /
  `getFlowTypeInConstructor` / `getTypeOfPropertyInBaseClass`. blocked-by:
  constructor control-flow analysis. (Resolution clears its TS2339s; the
  committed TS2532/TS2322/TS7008 stay missing.)
- **Namespace/function-merge & enum/export= VALUE access (`extra TS2304`)** —
  `Foo`/`MyEnum`/`A` used as a value where the binder did not merge the
  function/namespace/enum/export= declaration into a value symbol
  (`esDecorators…`, `legacyDecorators…`, `exportAssignmentMerging2/3`). blocked-by:
  `getExpandoSymbol` value-merge + enum-as-value + export= alias resolution.
- **`Object.defineProperty(obj, "x", …)` (shape e)** — the call-expression
  expando form (`bindExpandoPropertyAssignment` CallExpression arm +
  `getParentOfPropertyAssignment` args[0] + `getTypeFromPropertyDescriptor`).
  blocked-by: bindable `Object.defineProperty` calls. (None in the corpus.)
- **Computed expando names** (`F[expr] = v` / `this[expr] = v`) — the
  `HasDynamicName` branch + `addLateBoundAssignmentDeclarationToSymbol`.
  blocked-by: late-bound computed member declaration.
- **Constructor-function `this`** (`function F(){ this.x = … }`) — Go's `this`
  container is the function; not yet typed as a class. blocked-by:
  constructor-function expando typing.

# Round 18 — duplicate-identifier TS2300 (missing)

Round goal: reduce Round 12's **#1 FALSE-NEGATIVE — `missing TS2300 ×52`**
("Duplicate identifier '{0}'"), the residual after Round 13 wired binder
duplicate diagnostics through and Round 10 ported the member-MERGE half of
`mergeSymbol`. SOLO lane, strict TDD red→green. Edits: `internal/binder/
symbols.rs` (the accessor-marking fix) + `symbols_test.rs`; `internal/checker/
core/check.rs` (the `checkObjectTypeForDuplicateDeclarations` port) +
`check_test.rs`; `internal/compiler/program.rs` (the per-file diagnostic
sort+dedup) + `program_test.rs`; `internal/testrunner/compiler_runner_test.rs`
(re-measured) + this worklog.

## Step-0 categorization — root → count (the 52 are ALL in ONE case)

A temporary `#[ignore]`d dump (`dump_missing_ts2300`, since REMOVED) ran the full
compiler corpus and extracted every `missing TS2300`. **All 52 are in a SINGLE
case, `duplicateIdentifierChecks.ts`** (214 lines, `@strict @target esnext
@noEmit`), which exercises every get/set/property/method/computed-name duplicate
combination inside one interface / `declare class` / object-literal. We already
emit 42 of the 94 committed TS2300 (the binder's `declareSymbol` excludes); the
52 missing split by ROOT:

| bucket | root | shapes | count |
|---|---|---|---|
| **B1** | binder **accessor "mark full accessor"** (`declareSymbolEx` 286-292) | `get x; <other>; set x;` — the THIRD member (I7/I8/C3/C4/C7/C8/o7/o8) | **8** |
| **B2** | checker **`checkObjectTypeForDuplicateDeclarations`** | property-vs-accessor (`get x; x; set x;` — I5/I6/C5/C6) + property-vs-property (I14's `foo; foo`) | **14** |
| **A** | checker **late-binding** (`getLateBoundSymbol`) | computed `[foo]`/`[sym]` (I10/C10/C11/C12/I20/C20) + cross-name `[foo]` vs literal `foo` (I11–I15) | **30** |

`8 + 14 + 30 = 52`. The case will **NOT FLIP** to PASS regardless — it also
misses `TS1118 ×4` (object-literal multiple accessors) + `TS1119 ×4` (object-literal
property+accessor) grammar diagnostics, and the 30 Bucket-A computed/late-bound
TS2300. So the honest **flip count is 0**; the win is the headline
`missing TS2300 ×52 → ×30` (−22) with **no over-fire** (`extra TS2300` stays 0).

Chosen root: **single-container duplicate-member detection** = B1 (binder) + B2
(checker `checkObjectTypeForDuplicateDeclarations`), the two halves the task's Go
ground-truth section calls out. **DEFER Bucket A** (late-binding) — see below.

## Go ground truth ported

**B1 — binder accessor marking.** `internal/binder/binder.go:declareSymbolEx`
after reporting a merge conflict:

```go
// Go: internal/binder/binder.go:declareSymbolEx (286-292)
// When get or set accessor conflicts with a non-accessor or an accessor of a
// different kind, we mark the symbol as a full accessor such that all subsequent
// declarations are considered conflicting.
if symbol.Flags&ast.SymbolFlagsAccessor != 0 && symbol.Flags&ast.SymbolFlagsAccessor != includes&ast.SymbolFlagsAccessor {
    symbol.Flags |= ast.SymbolFlagsAccessor
}
```

The Rust port used `existing_flags.contains(SymbolFlags::ACCESSOR)` — but
`ACCESSOR = GetAccessor|SetAccessor`, so `contains` requires BOTH bits and never
fired for a lone getter/setter. Go tests `& Accessor != 0` (EITHER bit), i.e.
`intersects`. **Fix:** `.contains(ACCESSOR)` → `.intersects(ACCESSOR)`. After a
`get x` conflicts with a method, the symbol becomes a full accessor so the
trailing `set x` (which would otherwise legally merge with the lone getter)
ALSO conflicts.

**B2 — checker duplicate-member detection.**
`internal/checker/checker.go:checkObjectTypeForDuplicateDeclarations(3122)`: a
per-name state machine over the merged member symbols. Only a member that MERGED
into a symbol with `len(Declarations) > 1` is a candidate; state `0` records the
kind (`1`=property, `2`=accessor); a second property, or a property-after-accessor
/ accessor-after-property (`state==1 || state==2 && kind!=2`), reports
`Duplicate_identifier_0` on EVERY same-named member (`reportDuplicateMemberErrors`,
3193) and records state `3`. A LEGAL get/set pair stays `state==2, kind==2` → no
error; same-kind accessors / methods colliding with anything are flagged by the
binder's excludes (which don't let them merge), so this checker half fires ONLY
for the property/property and property/accessor merges the binder intentionally
allows. Called from `checkClassLikeDeclaration` (4279, classes) and
`checkInterfaceDeclaration` (4990, interfaces).

## What landed (Rust locations, surgical/additive)

- `internal/binder/symbols.rs:report_merge_conflict` — `.contains(ACCESSOR)` →
  `.intersects(ACCESSOR)` (B1, the 1:1 Go-fidelity bug fix).
- `internal/checker/core/check.rs` — `check_object_type_for_duplicate_declarations`
  + `report_duplicate_member_errors` (1:1 port of the two Go functions), wired
  into the `InterfaceDeclaration` arm of `check_statement` and into
  `check_class_like_declaration`. Plus free helpers `object_type_member_nodes`
  / `classify_property_or_accessor` / `has_accessor_modifier` /
  `member_name_node_for_duplicate`. The error span is the member NAME node with
  leading trivia skipped (`error_skipping_leading_trivia`), byte-matching Go's
  `c.error(member.Name(), …)` → `GetErrorRangeForNode`.
- `internal/compiler/program.rs:bind_and_check_diagnostics_grouped` — each file's
  combined bind+check list is now passed through
  `sort_and_deduplicate_diagnostics` (+ `compare_checker_diagnostics` /
  `equal_diagnostics_no_related_info`), the per-file reachable subset of Go's
  `GetSemanticDiagnosticsWithoutNoEmitFiltering` → `SortAndDeduplicateDiagnostics`
  (`compactAndMergeRelatedInfos`). **This was a pre-existing pipeline gap** exposed
  by B1: a binder merge conflict re-emits the SAME prior declaration's diagnostic
  on EACH later conflict (a `get x` flagged once when a method collides and again
  when `set x` collides), and Go relies on the program-level dedup to collapse the
  identical pair. Without it B1 produced `extra TS2300 ×8` (8 double-emitted first
  members); the dedup removes them faithfully.

## RED→GREEN + guard tests (one behavior at a time)

`tsgo_binder` (`symbols_test.rs`):
- `bind_accessor_conflict_marks_full_accessor_get_method_set` (RED→GREEN) —
  `interface I { get x; x(); set x; }` flags ALL THREE `x` names (RED: only get +
  method; the trailing set merged silently).
- `bind_legal_get_set_accessor_pair_no_duplicate` (GUARD) — `get x; set x;` → no
  TS2300 (the marking runs only on a conflict).

`tsgo_checker` (`check_test.rs`, StubProgram):
- `interface_property_accessor_duplicate_reports_2300` (RED→GREEN) — `interface
  I { get x; x; set x; }` → 3 TS2300.
- `class_property_accessor_duplicate_reports_2300` — the `declare class` form.
- `interface_duplicate_property_reports_2300` — `x: number; x: string;` (prop+prop).
- GUARDS `interface_legal_get_set_pair_no_duplicate`,
  `interface_distinct_members_no_duplicate`,
  `class_static_and_instance_same_name_no_duplicate`,
  `merged_empty_interfaces_no_duplicate` — all 0 TS2300.

`tsgo_compiler` (`program_test.rs`, real `Program`):
- `interface_property_accessor_duplicate_surfaces_ts2300` — the B2 case through
  the multi-file checker pool, asserting the three single-char `x` spans
  byte-match (the only `x` chars in the source).
- `accessor_marking_third_member_surfaces_ts2300` — the B1 case end-to-end.
- `legal_accessor_pair_and_overloads_no_duplicate` (GUARD) — a get/set pair, a
  method overload set, and distinct members → 0 TS2300.

The Round-13 guards (`legal_merges_produce_no_duplicate_identifier`,
`binder_duplicate_identifier_surfaces_ts2300`, …) stay GREEN.

## Measurement — full corpus BEFORE→AFTER

`tests/cases/compiler` (222 cases ran):

| metric | BEFORE | AFTER | Δ |
|---|---|---|---|
| passed | 109 | 109 | 0 |
| failed | 112 | 112 | 0 |
| errored | 1 | 1 | 0 |
| **missing TS2300** | **×52** | **×30** | **−22** |
| extra TS2300 | 0 | 0 | 0 (no over-fire) |
| extra TS2304 | ×43 | ×37 | −6 (dedup bonus) |
| extra TS2339 | ×22 | ×19 | −3 (dedup bonus) |

The `−22` = B1 (8) + B2 property/accessor (12: I5/I6/C5/C6) + B2 property/property
(2: I14's `foo; foo`). The **30 remaining missing are ALL Bucket A** (computed
`[foo]`/`[sym]` + cross-name late-bound `foo`), verified by the dump — DEFERRED.
**No PASS→FAIL** (passed/failed/errored verdict counts identical 109/112/1); the
case `duplicateIdentifierChecks.ts` stays FAILED (still missing the 30 Bucket-A
TS2300 + TS1118 ×4 + TS1119 ×4). **No over-fire**: `extra TS2300` is 0 — the
binder double-emission is collapsed by the Go-faithful per-file dedup, proven by
the legal-merge guards (get/set pair, overloads, empty merged interfaces). The
`extra TS2304/TS2339` drop is a BONUS of the same dedup (identical re-emitted
diagnostics across the corpus), faithful to Go's `SortAndDeduplicateDiagnostics`.

`tests/cases/conformance` (19 cases): **11/8/0** BEFORE and AFTER (unchanged; no
TS2300 there).

## 150-subset / 30-subset characterization BEFORE→AFTER

`expanded_compiler_subset_parity_smoke` (150) and `curated_compiler_subset_parity_smoke`
(30) are **UNCHANGED** (`84/66/0` and `19/11/0`, all pinned guards intact): the
≤25-line subset has no missing-TS2300 case and none of its cases double-emit, so
neither the new checks nor the dedup move its numbers. No snapshot update needed.

## Gate results (Round 18)

- `cargo test -p tsgo_binder` — GREEN (**65** unit [+2] + doctests).
- `cargo test -p tsgo_checker` — GREEN (**786** lib [+7]).
- `cargo test -p tsgo_compiler` — GREEN (**124** lib [+3 real-lib]).
- `cargo test -p tsgo_testrunner` — GREEN (51 unit + 1 ignored [full-corpus
  measurement]; 150-subset 84/66/0 + 30-case 19/11/0 unchanged).
- Sibling suites GREEN: `tsgo_ast` (54), `tsgo_printer` (194), `tsgo_transformers`
  (311), `tsgo_testutil_harnessutil` (11).
- `cargo clippy -p tsgo_binder -p tsgo_checker -p tsgo_compiler -p tsgo_testrunner
  --all-targets -- -D warnings` — clean. `cargo fmt … -- --check` — clean.
  `cargo build --workspace --all-targets` — clean.

No `--no-verify`; additive/surgical vertical slices; no test weakened/deleted; no
new dependency (`Cargo.lock` untouched); duplicate detection is NOT broadly
emitted (legal merges proven clean by the guards); the temporary
`dump_missing_ts2300` categorization test was REMOVED (tree clean). Not committed
by the subagent (left to the parent).

## DEFER list (blocked-by) — Round 18

- **Bucket A — computed / late-bound name duplicates (`missing TS2300 ×30`)** —
  `[foo]`/`[sym]` (I10/C10/C11/C12/I20/C20) and cross-name `[foo]` vs literal
  `foo` (I11–I15). The binder binds a non-literal computed name ANONYMOUSLY under
  `__computed` (a fresh symbol per member, 1 declaration each), so they never
  merge at bind time and `checkObjectTypeForDuplicateDeclarations` (which needs
  `len(Declarations) > 1`) skips them; tsc detects them after LATE-BINDING the
  computed name to its literal / unique-symbol property name and re-grouping the
  member symbols. blocked-by: checker late-binding (`getLateBoundSymbol` /
  `lateBindMember` resolving a computed name's `getLiteralTypeFromPropertyName` →
  property key) + the late-bound `getSymbolOfDeclaration`.
- **`duplicateIdentifierChecks.ts` FLIP** — even with Bucket A it would still miss
  `TS1118 ×4` ("An object literal cannot have multiple get/set accessors with the
  same name") + `TS1119 ×4` ("…property and accessor with the same name"), the
  object-literal GRAMMAR checks. blocked-by:
  `checkGrammarObjectLiteralExpression` (the seen-name flags for object-literal
  property/accessor) — a separate grammar root.
- **`checkObjectTypeForDuplicateDeclarations` remainder** — the `checkPrivateNames`
  static/instance private-name conflict (TS2300 variant), the static `prototype`
  name conflict, constructor parameter-property declarations, and TYPE-LITERAL
  members (`checkTypeLiteral` at 3119). blocked-by: private-identifier symbol
  naming + parameter-property binding + type-literal traversal in
  `check_type_node` (no corpus case needs them this round).
