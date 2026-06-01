# format — Round 1 worklog (rules engine core)

> P7 `format` round 1. Strict TDD (red→green vertical slices). Crate-scoped gates
> only (`-p tsgo_format`); a concurrent lane was editing `internal/execute/**`,
> so this round touched **only** `internal/format/**` + this doc. No root
> `Cargo.toml` edit (the crate was already registered).

> **See [Round 2 worklog](#format--round-2-worklog-ast-walking-worker--public-api)
> at the bottom of this file** for the AST-walking worker, `formattingScanner`,
> `SmartIndenter`, and the public `FormatDocument`/`FormatSpan`/`FormatOn*` entries
> (the DEFER list below is the round-2 backlog; most of it landed in round 2).

## Scope decision

`internal/format` is a large package (10 non-test `.go` files) whose public
entry points (`FormatDocument`/`FormatSpan`/`FormatOn*`) drive an AST-walking
worker (`span.go`), a trivia-aware `formattingScanner` (`scanner.go`), and a
`SmartIndenter` (`indent.go`). After reading every Go file **and** the current
state of the Rust dependency crates, the AST-walking engine was found to be
blocked on deep dependency mismatches (see DEFER list). The **deterministic
rules engine** — the genuinely reachable, fully-testable core the task
emphasizes — was ported this round:

- the rule model (`rule.go`),
- the full rule table (`rules.go`, all ~85 rule specs),
- the bucketed rules-map lookup (`rulesmap.go`),
- the `FormattingContext` + the reachable context predicates (`context.go` +
  `rulecontext.go`),
- the formatter config model (`FormatCodeSettings`, Go
  `lsutil/formatcodeoptions.go`),
- the pure indentation-string helper (`span.go:getIndentationString`).

This is the spine every formatting edit derives from: given an adjacent token
pair + a `FormattingContext`, `get_rules` returns the applicable rules in
priority order, and the winning rule's `RuleAction` (insert/delete space,
insert newline, delete token, insert semicolon) is exactly the decision the
deferred worker turns into a `TextChange`.

## Deliberate, documented divergences

1. **`FormattingContext` projection.** Go holds raw `*ast.Node` pointers and the
   predicates call accessors on them. The Rust `tsgo_ast` arena is owned by an
   `&mut` navigation context (`tsgo_astnav::SourceFile`) that cannot be threaded
   through a recursive visitor that also drives a scanner. So — like the arena
   deviation in `PORTING.md §5` — `FormattingContext` stores the **projection**
   of the AST the predicates read (token/parent kinds, the booleans Go computes
   lazily from positions/accessors, the option values). Each predicate body
   still translates 1:1. The deferred worker's `UpdateContext` will populate
   these fields from real nodes.
2. **`FormatCodeSettings` location.** In Go this lives in `lsutil`. The Rust
   `tsgo_ls_lsutil` port **deferred** it (it depends on Go reflection-based
   config marshaling, `lsproto`, and `printer`), and parallel-safety forbids
   editing `tsgo_ls_lsutil`. Since `format` is the consumer, the data model +
   defaults were ported into the `format` crate
   (`format_code_settings.rs`). `printer.GetDefaultIndentSize()` (= 4) is
   inlined as a constant to avoid a heavy `tsgo_printer` dependency.
3. **`ALL_TOKEN_KINDS` table.** Go iterates `for token := KindFirstToken;
   token <= KindLastToken; token++`. `tsgo_ast::Kind` has no `i16 -> Kind`
   reverse map, so the token range `[Unknown ..= DeferKeyword]` is materialized
   as a `const` table, guarded by a self-check test asserting it is contiguous
   and ends at `KindLastToken`.

## RED → GREEN slices (observed symptoms)

Each slice: wrote the test, ran `cargo test -p tsgo_format` and observed it fail,
then implemented the minimal code to pass.

1. **`RuleAction` / `RuleFlags`** (`rule.rs`). RED: `error[E0432]: unresolved
   import rule::RuleAction` / `undeclared type RuleAction`. GREEN: bitflags +
   enum; bit values `1<<0..1<<6` and composite masks match Go.
   — Go: `rule.go:ruleAction`, `ruleFlags`.
2. **`get_rule_bucket_index` + `get_rule_action_exclusion` + `MAP_ROW_LENGTH`**
   (`rulesmap.rs`). RED: undeclared `get_rule_bucket_index`. GREEN:
   `row*MAP_ROW_LENGTH+col`; exclusion masks (INSERT_SPACE → MODIFY_SPACE_ACTION,
   DELETE_TOKEN → MODIFY_TOKEN_ACTION). — Go: `rulesmap.go:getRuleBucketIndex`,
   `getRuleActionExclusion`, `mapRowLength`.
3. **`FormatCodeSettings` + defaults** (`format_code_settings.rs`). RED:
   undeclared `get_default_format_code_settings`. GREEN: defaults match Go
   (`insert_space_after_comma_delimiter = True`,
   `insert_space_before_and_after_binary_operators = True`,
   `insert_space_after_constructor = False`, `indent_size = tab_size = 4`,
   `indent_style = Smart`, `semicolons = Ignore`, ...). — Go:
   `formatcodeoptions.go:GetDefaultFormatCodeSettings`, `IndentStyle`,
   `SemicolonPreference`, `parseIndentStyle`, `parseSemicolonPreference`.
4. **`FormattingContext` + `FormatRequestKind`** (`context.rs`). RED: undeclared
   `FormattingContext::new`. GREEN: projection struct + line-relationship
   accessors; `FormatRequestKind` discriminants 0..5. — Go:
   `context.go:NewFormattingContext`/`TokensAreOnSameLine`/...; `api.go:FormatRequestKind`.
5. **Rule model `TokenRange`/`to_token_range`/`rule`/`rule_flags`** (`rule.rs`).
   RED: undeclared `to_token_range`/`rule`. GREEN: `From<Kind>`/`From<Vec<Kind>>`
   coercions (specific) + identity passthrough (preserves wildcard
   `is_specific`); `rule()`/`rule_flags()` build a `RuleSpec`. — Go:
   `rule.go:toTokenRange`/`rule`/`ruleImpl`.
6. **Insertion-bitmap math + `RulesPosition`** (`rulesmap.rs`). RED: undeclared
   `RulesPosition`. GREEN: discriminants 0/5/10/15/20/25;
   `get_rule_insertion_index` sums lower sub-buckets; `increase_insertion_index`
   bumps the target 5-bit counter. — Go: `rulesmap.go:RulesPosition`,
   `getRuleInsertionIndex`, `increaseInsertionIndex`.
7. **Context predicates** (`rulecontext.rs`). RED: undeclared
   `is_non_jsx_same_line_token_context` etc. GREEN: ~70 predicates + option
   selectors + higher-order builders, each a 1:1 body over the projection
   (verified `is_binary_op_context`, `is_block_context`, `is_function_decl_context`,
   `is_type_annotation_context`, `is_type_argument_or_parameter_or_assertion_context`,
   option builders, ...). — Go: `rulecontext.go:*`.
8. **`ALL_TOKEN_KINDS` + token-range helpers + `get_all_rules`** (`rules.rs`).
   RED: self-check + undeclared `get_all_rules`. GREEN: the contiguity self-check
   passed (proving the 167-entry table matches `Kind` discriminants), the full
   rule table built (> 80 rules, high → user → low priority order verified),
   `token_range_from`/`token_range_from_ex`/`token_range_from_range`. — Go:
   `rules.go:getAllRules`/`tokenRangeFrom*`.
9. **`build_rules_map` + `get_rules` + `get_rules_map`** (`rulesmap.rs`). RED:
   undeclared `get_rules`. GREEN: bucket `(Comma, Identifier)` contains
   `SpaceAfterComma`/`NoSpaceAfterComma`; `get_rules_map` cached singleton (same
   pointer across calls). — Go: `rulesmap.go:buildRulesMap`/`addRule`/`getRules`/`getRulesMap`.

### Behavior slices (assert the rule decision that the edit derives from)

10. **Space after comma** — `[1,2,3]` → `[1, 2, 3]`. `(Comma, Identifier)` in an
    array literal, default options → first space action is
    `SpaceAfterComma`/`INSERT_SPACE`. With the option disabled →
    `NoSpaceAfterComma`/`DELETE_SPACE`. — Go: `rules.go:SpaceAfterComma`/`NoSpaceAfterComma`.
11. **Space around binary operator** — `1+2` → `1 + 2`. `(NumericLiteral, Plus)`
    → `SpaceBeforeBinaryOperator`/`INSERT_SPACE`; `(Plus, NumericLiteral)` →
    `SpaceAfterBinaryOperator`/`INSERT_SPACE` (binary-op context, default
    options). — Go: `rules.go:SpaceBeforeBinaryOperator`/`SpaceAfterBinaryOperator`.
12. **No space before semicolon** — `... 1 ;` → `... 1;`. `(NumericLiteral,
    Semicolon)` same line → `NoSpaceBeforeSemicolon`/`DELETE_SPACE`. — Go:
    `rules.go:NoSpaceBeforeSemicolon`.
13. **Brace newline / close-brace** — `}`+`else` same line →
    `SpaceBetweenCloseBraceAndElse`/`INSERT_SPACE`; multi-line block
    `(Identifier, CloseBrace)` → `NewLineBeforeCloseBraceInBlockContext`/`INSERT_NEW_LINE`.
    — Go: `rules.go:SpaceBetweenCloseBraceAndElse`/`NewLineBeforeCloseBraceInBlockContext`.
14. **No-op (already formatted)** — `(Identifier, Identifier)` with no matching
    rule → `get_rules` returns empty (zero edits). — Go:
    `rulesmap.go:getRules` (empty bucket / no rule).
15. **`get_indentation_string`** (`span.rs`). RED: undeclared. GREEN: spaces by
    default (`4` → `"    "`); tabs+remainder when `convert_tabs_to_spaces` is
    false (`6`, tab_size 4 → `"\t  "`); `tab_size == 0` → `""`. — Go:
    `span.go:getIndentationString`.

## Files (Go → Rust)

| Go file | Rust file | Ported this round |
|---|---|---|
| `rule.go` | `rule.rs` | full (RuleAction/RuleFlags/TokenRange/RuleSpec/RuleImpl/rule/to_token_range) |
| `rulesmap.go` | `rulesmap.rs` | full (bucket index, exclusion, insertion bitmap, build_rules_map, get_rules, get_rules_map) |
| `rules.go` | `rules.rs` | full table (get_all_rules + token-range helpers + ALL_TOKEN_KINDS) |
| `context.go` | `context.rs` | FormattingContext (projection) + FormatRequestKind + line accessors |
| `rulecontext.go` | `rulecontext.rs` | reachable predicates + selectors + builders (4 deep-AST predicates stubbed, see DEFER) |
| `lsutil/formatcodeoptions.go` | `format_code_settings.rs` | data model + defaults + parse helpers (relocated; lsproto helpers deferred) |
| `span.go` | `span.rs` | `getIndentationString` only (worker deferred) |
| `api.go` | — | deferred (public entries) |
| `scanner.go` | — | deferred (formattingScanner) |
| `indent.go` | — | deferred (SmartIndenter) |
| `util.go` | — | deferred (node/list helpers for the worker) |

## Test deltas

Crate started at **0** tests. Round 1: **55 unit tests + 27 doctests = 82** (all
green). Coverage is more than Go (Go's `internal/format/*_test.go` are
integration/baseline tests deferred to P10 parity; this round adds behavior-level
unit tests for the rule model, bucket math, insertion bitmap, every behavior
slice, config defaults, and representative predicates).

## Gate results (crate-scoped)

- `cargo test -p tsgo_format` → **ok. 55 passed** (unit) + **27 passed** (doctests).
- `cargo clippy -p tsgo_format --all-targets -- -D warnings` → **clean**.
- `cargo fmt -p tsgo_format -- --check` → **clean**.
- `cargo build -p tsgo_format` → **clean**.

## Public API (additive, within crate)

`RuleAction`, `RuleFlags`, `TokenRange`, `RuleImpl`, `RuleSpec`, `rule`,
`rule_flags`, `to_token_range`; `FormatCodeSettings`, `EditorSettings`,
`IndentStyle`, `SemicolonPreference`, `get_default_format_code_settings`;
`FormattingContext`, `FormatRequestKind`, `ContextPredicate`, `any_context`;
`get_all_rules`; `build_rules_map`, `get_rules`, `get_rules_map`;
`get_indentation_string`; plus the `rulesmap` bucket/bitmap functions
(`get_rule_bucket_index`, `get_rule_action_exclusion`, `RulesPosition`,
`get_rule_insertion_index`, `increase_insertion_index`, `MAP_ROW_LENGTH`) and
`rules` token-range constructors. `rulecontext` is `pub(crate)` (internal).

## DEFER list (blocked-by)

- **AST-walking worker** (`span.go:formatSpanWorker`: `findEnclosingNode`,
  `processNode`/`processChildNode`/`processChildNodes`/`processPair`,
  `applyRuleEdits`, trailing-whitespace trimming, `dynamicIndenter`,
  `getOwnOrInheritedDelta`, ...).
  blocked-by: a borrow design that threads the `tsgo_astnav` `&mut SourceFile`
  (arena owner + token cache) through a recursive `ast::NodeVisitor`/`VisitEachChild`
  walk while a `tsgo_scanner` scanner runs over the same text; plus
  SourceFile-aware scanner line/position helpers
  (`GetECMALineOfPosition`/`GetECMALineStarts`/`GetTokenPosOfNode`/
  `GetECMALineAndByteOffsetOfPosition`) which are not yet exposed.
- **`formattingScanner`** (`scanner.go`): trivia-aware scan + `ReScan*` state
  machine, `tokenInfo`, `skipToEndOf`/`skipToStartOf`.
  blocked-by: the worker above + scanner cursor protocol wiring.
- **`SmartIndenter`** (`indent.go`: `GetIndentation`/`GetIndentationForNode`/
  `getIndentationForNodeWorker`/`GetContainingList`/`ShouldIndentChildNode`/
  `NodeWillIndentChild`/comment-indent edge cases).
  blocked-by: `astnav.FindPrecedingToken`/`GetTokenAtPosition`/`FindNextToken`
  over the `&mut SourceFile` context + scanner line helpers.
- **Public entries** (`api.go`: `FormatDocument`/`FormatSpan`/`FormatSelection`/
  `FormatOnEnter`/`FormatOnSemicolon`/`FormatOnOpeningCurly`/`FormatOnClosingCurly`/
  `FormatNodeGivenIndentation`). blocked-by: the worker.
- **`util.go`** worker helpers (`findEnclosingNode` support, `getOpenTokenForList`,
  `getCloseTokenForOpenToken`, `isGrammarError`, `findOutermostNodeWithinListLevel`,
  `isListElement`, `findImmediatelyPrecedingTokenOfKind`). blocked-by: the worker
  + astnav.
- **Deep-AST context predicates** (stubbed to default-safe values, documented
  inline): `isSemicolonDeletionContext` / `isSemicolonInsertionContext`
  (blocked-by: `astnav.FindNextToken`/`FindPrecedingToken` +
  `lsutil.PositionIsASICandidate`, which `tsgo_ls_lsutil` defers);
  `isEndOfDecoratorContextOnSameLine` (blocked-by: `IsExpression` parent walk;
  decorators are out of round scope). `isStartOfVariableDeclarationList` and
  `isNotPropertyAccessOnIntegerLiteral` are ported using precomputed projection
  fields the worker will populate.
- **`FromLSFormatOptions`/`ToLSFormatOptions`** (`formatcodeoptions.go`).
  blocked-by: `tsgo_lsproto::FormattingOptions` wiring.
- **Go integration tests** (`format_test.go`/`api_test.go`/`indent_test.go`/
  `indent_getindentation_test.go`/`comment_test.go`): exercise the full worker;
  deferred with the worker (and ultimately P10 baseline parity).

---

# format — Round 2 worklog (AST-walking worker + public API)

> P7 `format` round 2. Strict TDD (red→green vertical slices). Crate-scoped gates
> only (`-p tsgo_format`); a concurrent P10 lane was editing
> `internal/testrunner/**` + `internal/testutil/**`, so this round touched
> **only** `internal/format/**` + this doc. Added deps to
> `internal/format/Cargo.toml` ONLY (`tsgo_astnav`, `tsgo_scanner`,
> `tsgo_stringutil`, dev-dep `tsgo_parser`); **no root `Cargo.toml` edit**, no
> other crate touched.

## What landed

Round 1 was blocked on the AST-walking engine because the navigation context
owned the arena by `&mut`. The committed **`tsgo_astnav` shared-borrow surface**
(`NavSourceFile<'a>` / `RcSourceFile` with `&self` `get_token_at_position` /
`find_preceding_token` / `find_next_token` / `find_child_of_kind` +
`visit_each_child_and_jsdoc`) unblocks it: the worker holds a `&NavEngine<A>`
(shared) and threads it through the recursive walk while a separate
`tsgo_scanner::Scanner` runs the formatting scan. This round ports:

- **`formattingScanner`** (`scanner.rs`, Go `scanner.go`): the trivia-aware
  scanner over `tsgo_scanner::Scanner` (`set_skip_trivia(false)`), `tokenInfo`,
  `TextRangeWithKind`, the `scanAction` rescan state machine, `advance` /
  `readTokenInfo` / `getNextToken` / `fixTokenKind` / `isOnToken` / `isOnEOF` /
  `skipToEndOf` / `skipToStartOf` / `readEOFTokenRange`.
- **`SmartIndenter`** (`indent.rs`, Go `indent.go`, reachable subset):
  `ShouldIndentChildNode`, `NodeWillIndentChild` (full per-kind switch),
  `GetIndentationForNode` → `getIndentationForNodeWorker`, the column/line
  helpers, and the `childStartsOnTheSameLineWithElseInIfStatement` /
  `childIsUnindentedBranchOfConditionalExpression` /
  `argumentStartsOnSameLineAsPreviousArgument` predicates used by the worker's
  `computeIndentation`.
- **Span worker** (`span.rs`, Go `span.go`): `formatSpanWorker` (`execute` →
  `run`), `processNode`, `processChildNode`, `computeIndentation`, `processPair`,
  `applyRuleEdits`, `processRange`, `processTrivia`,
  `consumeTokenAndAdvanceScanner`, the `dynamicIndenter`
  (`getIndentation`/`getDelta`/`getIndentationForToken`/`getIndentationForComment`/
  `shouldAddDelta`/`recomputeIndentation`), `insertIndentation` /
  `characterToColumn` / `indentationIsDifferent`,
  `trimTrailingWhitespacesForLines` / `getTrailingWhitespaceStartPosition` /
  `GetECMAEndLinePosition`, `recordDelete`/`recordReplace`/`recordInsert`,
  `findEnclosingNode`, `getScanStartPosition`, `getOwnOrInheritedDelta`, and
  `UpdateContext` (as `update_formatting_context`, populating the projection).
- **`util.rs`** (Go `util.go` + `context.go` helpers): `withTokenStart`,
  `rangeIsOnOneLine`, `GetLineStartPositionForPosition`, `isGrammarError`
  (reachable), `findImmediatelyPrecedingTokenOfKind`,
  `findOutermostNodeWithinListLevel`, `isListElement` (statement-list
  containers), plus `node_parent` (resolves synthesized-token parents — see
  divergence 3).
- **Public API** (`api.rs`, Go `api.go`): `format_document`, `format_span`,
  `format_on_semicolon`, `format_on_closing_curly`, `formatNodeLines`. All
  generic over `A: Borrow<NodeArena>`, so they work with `NavSourceFile` /
  `RcSourceFile` / owned `SourceFile`.

## Deliberate, documented divergences (round 2)

1. **Flat child walk instead of Go's list-distinguishing `NodeVisitor`.** Go's
   visitor routes node *lists* to `processChildNodes` (a list indentation scope
   around the list start/end tokens) and single children to `processChildNode`.
   The `tsgo_astnav` surface exposes only the flat `visit_each_child_and_jsdoc`
   stream (its own docs say callers "replace the per-list binary search with a
   linear scan over the same (sorted) children — same result"). So the worker
   collapses both into `process_child_node`: list start/end tokens, commas, and
   close brackets are consumed as ordinary "parent tokens" between children
   (exactly how Go consumes non-list tokens), producing **identical** spacing and
   single-level-indent edits for the reachable cases. Multi-line list
   *continuation* indent (`tryComputeIndentationForListItem` + the list scope) is
   deferred.
2. **Immutable `FormatContext` + `&Ctx` copy.** The shared nav context, text,
   line map, options, and newline live behind one `&FormatContext`; each worker
   method copies that shared reference (`let ctx = self.ctx;`) and then mutates
   the worker's own state (edits / scanner / fcx) without borrow conflicts. Line
   queries use `tsgo_scanner::compute_line_of_position` over a line map computed
   once (Go caches it on the file).
3. **`node_parent` for synthesized tokens.** Go reads `token.Parent` directly on
   the `}`/`;`/`]` tokens returned by `FindPrecedingToken`. `tsgo_astnav` keeps
   synthesized tokens in a side store without an arena `parent`, so `node_parent`
   recovers it as the smallest real node enclosing the token
   (`find_enclosing_node`). Real nodes use the arena back-edge.
4. **No diagnostics → `rangeContainsError` is always `false`** (the nav context
   carries none; well-formed input has no errors).

## RED → GREEN slices (observed symptoms)

Infrastructure (scanner/indent/worker/api) was written, then each behavior slice
was driven red→green against `cargo test -p tsgo_format`:

1. **`format_document([1,2,3])` → `[1, 2, 3]`** (space after comma). First full
   run: the public-API slices **panicked** with
   `internal/ast/lib.rs:1694 index out of bounds: ... index is 2147483648`
   (`= 1<<31`, the `tsgo_astnav` synthesized-token tag) — the worker's
   trailing-edit block and `findOutermostNodeWithinListLevel` were calling
   `arena.parent`/`arena.loc` on a synthesized `]`/`}` token. GREEN after adding
   `node_parent` (divergence 3) + reading synthesized-token ranges through the
   nav accessors. Observed edits: insert `" "` at the byte after each comma →
   `[1, 2, 3]`. — Go: `rules.go:SpaceAfterComma` via the worker.
2. **`1+2` → `1 + 2`** (binary-op spaces). `(NumericLiteral, Plus)` and
   `(Plus, NumericLiteral)` in a `BinaryExpression` context →
   `SpaceBefore/AfterBinaryOperator`. — Go: `rules.go:SpaceBeforeBinaryOperator`.
3. **`const x=1` → `const x = 1`** (assignment spaces). `=` in a
   `VariableDeclaration`: `isBinaryOpContext` returns true for the
   `EqualsToken` → `SpaceBefore/AfterBinaryOperator`. Verified the projection
   `context_node_kind == VariableDeclaration` drives it. — Go:
   `rulecontext.go:isBinaryOpContext` (VariableDeclaration case).
4. **`function f() {\nreturn 1;\n}` → `return 1;` indented 4** (SmartIndenter).
   `computeIndentation` gives the `Block`'s statement indentation `0 + delta(4)`;
   `consumeTokenAndAdvanceScanner` indents the `return` token (first token after a
   newline) via `insertIndentation`. Observed edit: insert `"    "` at the line
   start before `return`. — Go: `span.go:computeIndentation` + `dynamicIndenter`.
   4b. Combined `function f() {\nreturn 1+2;\n}` → indent **and** binary-op spaces.
5. **`format_on_closing_curly` / `format_on_semicolon`** reachable cases:
   `findImmediatelyPrecedingTokenOfKind` + `findOutermostNodeWithinListLevel` →
   `formatNodeLines` → `FormatSpan`. On-`}` over the function indents the body;
   on-`;` over `const x=1;` produces `const x = 1;`. — Go:
   `api.go:FormatOnClosingCurly`/`FormatOnSemicolon`.
6. **Already-formatted → ZERO edits (idempotence).** `[1, 2, 3]`, `1 + 2`,
   `const x = 1`, `function f() {\n    return 1;\n}` all yield empty edit lists;
   the `applyRuleEdits` `InsertSpace` path no-ops when a single space is already
   present, and `trimTrailingWhitespacesForLines` finds none. Also verified the
   **fixed point**: `format(format(x)) == format(x)`. — Go: `rulesmap.go:getRules`
   (no applicable action) + `applyRuleEdits` early-outs.

## Files (Go → Rust), round 2

| Go file | Rust file | Ported this round |
|---|---|---|
| `scanner.go` | `scanner.rs` | full reachable (rescan state machine; JSX-attr-value precise check deferred) |
| `indent.go` | `indent.rs` | `ShouldIndentChildNode`/`NodeWillIndentChild`/`GetIndentationForNode`/worker predicates + column helpers (GetIndentation + actual-indentation-from-source deferred) |
| `span.go` | `span.rs` | `formatSpanWorker` + `dynamicIndenter` + edit recording + `findEnclosingNode`/`getScanStartPosition`/`getOwnOrInheritedDelta` (list scope, comments, decorators deferred) |
| `util.go` | `util.rs` | `withTokenStart`/`rangeIsOnOneLine`/`GetLineStartPositionForPosition`/`isGrammarError`/`findImmediatelyPrecedingTokenOfKind`/`findOutermostNodeWithinListLevel`/`isListElement` + `node_parent` |
| `api.go` | `api.rs` | `FormatDocument`/`FormatSpan`/`FormatOnSemicolon`/`FormatOnClosingCurly`/`formatNodeLines` |
| `context.go` (`UpdateContext`) | `span.rs:update_formatting_context` | projection population from real nodes |

## Test deltas

Round 1 ended at **55 unit + 27 doctests = 82**. Round 2 adds **27 unit tests +
1 doctest** → **82 unit + 28 doctests = 110** (all green). New tests:
`api_test.rs` (11: the 6 slices + idempotence/fixed-point/option-driven/empty),
`scanner_test.rs` (5: token sequence, trailing trivia, newline tracking, EOF),
`util_test.rs` (7: helpers), `indent_test.rs` (4: indentation + `NodeWillIndentChild`).
No existing test was weakened or deleted (round-1 `span_test.rs`/`rule_test.rs`/
`rulesmap_test.rs`/`context_test.rs`/`rulecontext_test.rs`/`rules_test.rs`/
`format_code_settings_test.rs` are all intact).

## Gate results (crate-scoped)

- `cargo test -p tsgo_format` → **ok. 82 passed** (unit) + **28 passed** (doctests).
- `cargo clippy -p tsgo_format --all-targets -- -D warnings` → **clean**.
- `cargo fmt -p tsgo_format -- --check` → **clean**.
- `cargo build -p tsgo_format` → **clean**.
- Did NOT run `--workspace` (a concurrent P10 lane was active).

## Public API (additive, within crate)

`format_document`, `format_span`, `format_on_semicolon`, `format_on_closing_curly`
(re-exported at the crate root); `FormattingScanner`, `TextRangeWithKind`,
`TokenInfo`; `FormatContext`, `FormatSpanWorker`, `LineAction`,
`find_enclosing_node`, `get_scan_start_position`, `get_own_or_inherited_delta`;
the `indent` module (`should_indent_child_node`, `node_will_indent_child`,
`get_indentation_for_node`, column helpers). Round-1 API unchanged.

## DEFER list (round 2, blocked-by)

- **Multi-line list continuation indent** (`processChildNodes`' list scope +
  `tryComputeIndentationForListItem`, `getOpenTokenForList` /
  `getCloseTokenForOpenToken`). blocked-by: the flat-child-walk divergence (no
  list/single distinction on the shared `tsgo_astnav` surface).
- **`GetIndentation`** (position-based standalone indenter for codefix / on-enter)
  + its comment-aware helpers (`getRangeOfEnclosingComment`, `getCommentIndent`,
  `getBlockIndent`, `getSmartIndent`, `nextTokenIsCurlyBraceOnSameLineAsCursor`),
  and "actual indentation from source" (`getActualIndentationForNode` /
  `getActualIndentationForListItem` / `deriveActualIndentationFromList` /
  `getListByRange` / `getVisualListRange`). The reachable format-span path always
  passes the span as the ignore range, so `useActualIndentation` is false for
  every node walked. blocked-by: `IsDeclaration`/`IsStatementButNotDeclaration` +
  per-node list accessors.
- **Comment re-indentation** (`indentMultilineComment`) + the format-selection
  "remaining leading trivia" tail in `execute`.
- **Decorators** (`getNonDecoratorTokenPosOfNode`, undecorated-start-line,
  `getFirstNonDecoratorTokenOfNode`): `has_decorators` treated as `false`.
  blocked-by: modifier-list accessors in `tsgo_ast`.
- **`FormatOnEnter` / `FormatOnOpeningCurly` / `FormatSelection` /
  `FormatNodeGivenIndentation`**. blocked-by: `FormatOnEnter` selection math +
  list-level widening (the deferred list scope).
- **JSX / template / regex rescans** in the formatting scanner (the rescan state
  machine is ported, but `shouldRescanJsxAttributeValue`'s precise
  `Initializer() == node` check is stubbed; no JSX in the reachable set).
- **The 3 deep-AST context predicates** (`isSemicolonDeletionContext`,
  `isSemicolonInsertionContext`, `isEndOfDecoratorContextOnSameLine`) remain
  stubbed (round-1 state). blocked-by: `lsutil.PositionIsASICandidate`
  (deferred in `tsgo_ls_lsutil`) + decorator/expression parent walks.
- **`rangeContainsError`** (diagnostics overlap): always `false`. blocked-by:
  diagnostics threading through the shared nav context.
- **Go integration tests** (`format_test.go`/`api_test.go`/`indent_test.go`/
  `indent_getindentation_test.go`/`comment_test.go`): exercise the full worker
  incl. the deferred pieces; remain deferred to P10 baseline parity.
