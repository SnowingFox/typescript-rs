# ls — worklog (P7: inlay hints provider)

> P7 `ls` (root crate `tsgo_ls`). Strict TDD (red→green vertical slices, one
> behavior at a time, verified against the Go ground truth in
> `internal/ls/inlay_hints.go`). Crate-scoped gates (`-p tsgo_ls`,
> `-p tsgo_ls_lsutil`) + the workspace build.
> This round touched: `internal/ls/inlay_hints.rs` (new),
> `internal/ls/inlay_hints_test.rs` (new), the `pub mod inlay_hints;`
> registration in `internal/ls/lib.rs`, the additive `InlayHintsPreferences` /
> `IncludeInlayParameterNameHints` port in `internal/ls/lsutil/userpreferences.rs`
> (+ its export in `lsutil/lib.rs`, + tests in `userpreferences_test.rs`), and a
> one-line `tsgo_evaluator` path dependency in `internal/ls/Cargo.toml` (mirrors
> Go's `import internal/evaluator`; `Cargo.lock` gains exactly that edge — no new
> external crate) — plus this worklog. No sibling test weakened or deleted.

Ports Go's `internal/ls/inlay_hints.go` (`textDocument/inlayHint`): the
range-pruned source walk that, per the editor's `InlayHintsPreferences` gates,
emits inline hints for parameter names, inferred variable / property / parameter
/ return types, and enum member values.

## What landed (the reachable subset)

| File | Go source | What |
|---|---|---|
| `inlay_hints.rs` | `internal/ls/inlay_hints.go` | `LanguageService::provide_inlay_hints(file, range, &prefs) -> Option<Vec<InlayHint>>` (Go's `ProvideInlayHint` → `InlayHintsOrNull`); the `is_any_inlay_hint_enabled` gate; the `visit` walk (zero-width / reparsed / span-intersection / type-node skips + source-order recursion); `visit_enum_member` + `add_enum_member_value_hints` (live, via `EmitResolver::get_constant_value` + `evaluator::any_to_string`); the `is_type_node_kind` walk guard (`IsTypeNodeKind`) |
| `lsutil/userpreferences.rs` | `internal/ls/lsutil/userpreferences.go` | `InlayHintsPreferences` (all 8 gate fields, faithful) + `IncludeInlayParameterNameHints` (None/All/Literals, `as_str` wire values) |
| `lib.rs` | — | `pub mod inlay_hints;` |
| `inlay_hints_test.rs` | — | 12 unit tests |
| `userpreferences_test.rs` | — | +3 unit tests (enum wire values, defaults, all-gates-off) |

Public API is **additive**: one `pub fn` on `LanguageService`
(`provide_inlay_hints`), the `pub mod inlay_hints;` registration, and the two
new `pub` preference types in `tsgo_ls_lsutil`. Nothing existing changed.

## Architecture: checker-backed range walk, position conversion after the borrow

Like hover / semantic tokens, the provider builds a per-file `Checker` via
`LanguageService::file_check_context` (bind every file → join into one
`MultiFileBoundProgram` → take the file view). It converts the LSP `range` to a
byte span first (immutable `Converters` borrows), then walks the file's
`NodeArena` from the root (Go's `ForEachChild`, in source order, so collected
hints are already position-sorted), pruning subtrees that fall outside the span
or are type-node annotations. Enum-member values are folded through the
checker's `EmitResolver` (a read-only constant fold — no checker mutation). The
walk collects `RawInlayHint`s carrying byte positions; after the
checking-context borrow ends, each byte position is converted to a UTF-16
`Position` through the round-1 `Converters`. The provider returns
`Option<Vec<InlayHint>>` — `None` is Go's `null` (no hint kind enabled / unknown
file), `Some(vec![...])` is Go's (possibly empty) array.

## RED → GREEN slices (observed symptoms)

1. **`enum E { A }` (enum-member-value pref on) → one `= 0` hint, padding-left,
   no kind, position at `member.End()` (char 10)** — headline. RED: scaffold
   returned `Some(vec![])` (0 hints). GREEN: the `visit` walk + `visit_enum_member`
   (no-initializer guard → `get_constant_value` → `any_to_string`) +
   `add_enum_member_value_hints`.
2. **`enum E { A = 5 }` → no hint** (Go's `member.Initializer() != nil` early
   return).
3. **`enum E { A, B, C }` → `= 0`, `= 1`, `= 2`** (auto-numbering, source order).
4. **`enum E { A = 1, B }` → only `= 2`** (initialized member skipped; the next
   auto-numbers from it).
5. **`const enum E { A }` → `= 0`** (a member *node* folds regardless of
   const-ness — Go's first `KindEnumMember` branch).
6. **Range**: members in two enums across lines; a range covering only the first
   returns just its hint (the `span.Intersects` prune).
7. **GUARDs**: every gate off → `null` (None), not an empty array; unknown file
   → `null`.
8. **Gate predicates** (pure): `is_any_inlay_hint_enabled` false for the default,
   true per individual gate (incl. parameter-name All / Literals), and *not*
   flipped by the suppression-only toggles; `is_type_node_kind` matches Go's
   explicit-keyword / JSDoc / `ExpressionWithTypeArguments` cases + the
   `FirstTypeNode..=LastTypeNode` range, excluding non-type kinds.

## Ported vs DEFERRED

**Ported (faithful 1:1):**
- The request gate `isAnyInlayHintEnabled` (all 6 conditions).
- The `visit` walk skeleton: zero-width / reparsed / span-intersection /
  type-node-kind skips and the source-order child recursion.
- `IsTypeNodeKind` (the explicit keyword + JSDoc + `ExpressionWithTypeArguments`
  cases, plus the `FirstTypeNode..=LastTypeNode` range).
- **Enum-member-value hints**: `visitEnumMember` (initializer-present guard +
  the `GetConstantValue` fold, including auto-numbering and const-enum members)
  and `addEnumMemberValueHints` (`= <value>` label, left padding, no kind,
  anchored at `member.End()`).
- `InlayHintsPreferences` + `IncludeInlayParameterNameHints` (full, faithful).

**DEFERRED (with `blocked-by`):**
- **Parameter-name hints** (`visitCallOrNewExpression`, `addParameterHints`,
  `getParameterIdentifierInfoAtPosition`, `isHintableLiteral`, the
  arg-name-matches-param suppression, leading-`...`/rest tuple handling).
  blocked-by: a **public `getResolvedSignature`** (call / overload resolution).
  Only a *private* contextual-argument resolver exists in the checker today, so
  an arbitrary call/new site cannot be mapped to its signature's parameter
  names. Without it there is no faithful ground truth to assert.
- **Variable-type / property-declaration-type hints**
  (`visitVariableLikeDeclaration`, with the annotation / `…WhenTypeMatchesName`
  suppression). blocked-by: **`getTypeAtLocation` + initializer-inferred types**
  — the checker yields `any` for an un-annotated `const x = 1` (documented in the
  hover round), so the rendered type would diverge from Go's `number`. (And Go
  suppresses the hint whenever there *is* an annotation, so there is no
  inference-free scenario to test.)
- **Function parameter-type / return-type hints**
  (`visitFunctionLikeForParameterType`, `visitFunctionDeclarationLikeForReturnType`,
  `addParameterTypeHint`, `getParameterDeclarationTypeHints`,
  `typeToInlayHintParts`, `typePredicateToInlayHintParts`, `addTypeHints`).
  blocked-by: a public `getSignatureFromDeclaration` /
  `getReturnTypeOfSignature` / `getTypePredicateOfSignature`, plus the type-node
  → label-parts renderer `getInlayHintLabelParts` (it walks `TypeToTypeNode`
  with the identifier→symbol side map and `getNodeDisplayPart` / `getLiteralText`
  location parts).
- The `context.Context` cancellation checks in `visit` (module / class /
  function boundaries). blocked-by: the LS has no cancellation-token plumbing yet
  (matching the sibling providers).
- `GetQuotePreference` + `quotePreference` state (only used by the deferred
  string-literal label rendering above).

## Gate results (all GREEN, never `--no-verify`)

- `cargo test -p tsgo_ls`: **113 lib tests + 1 doctest pass** (was 101; **+12**).
- `cargo test -p tsgo_ls_lsutil`: **32 pass** (incl. the +3 new preference tests
  / doctests).
- `cargo clippy -p tsgo_ls -p tsgo_ls_lsutil --all-targets -- -D warnings`: clean.
- `cargo fmt -p tsgo_ls -p tsgo_ls_lsutil -- --check`: clean.
- `cargo build --workspace --all-targets`: success.

---

# P7 — LS type-at-location query + inlay variable-type hint

> This round added the checker-query keystone **`get_type_at_location`** (a
> faithful subset of Go's `GetTypeAtLocation` → `getTypeOfNode`) and re-enabled
> the previously-DEFERRED **variable-type** and **property-declaration-type**
> inlay hint kinds on top of it. Strict TDD (red→green vertical slices) verified
> against `internal/checker/checker.go:getTypeOfNode` / `getTypeOfSymbol` and
> `internal/ls/inlay_hints.go:visitVariableLikeDeclaration`.
> Touched: `internal/checker/core/symbols_query.rs` (+ `symbols_query_test.rs`),
> `internal/checker/lib.rs` (one additive re-export), `internal/ls/inlay_hints.rs`
> (+ `inlay_hints_test.rs`). No Cargo manifest / `Cargo.lock` edit; no external
> dep; no sibling test weakened or deleted.

## Step 0 — diagnosis: bridge gap, NOT a checker gap

The prior round's DEFER note claimed "the checker yields `any` for an
un-annotated `const x = 1`". That note was **stale**: `getTypeOfVariableOrProperty`
(`declared_types.rs`) already infers initializer types
(`check_expression(initializer)` → `getWidenedLiteralTypeForInitializer` →
`getWidenedType`). Verified empirically — `getTypeOfSymbol` already returns
`number` for `let x = 1`, `1` for `const x = 1` (a `const` keeps the literal),
`string` for `const x = f()`, an anonymous object for `const o = { a: 1 }`.

The real gap was a **bridge / query gap**: there was no faithful
`getTypeAtLocation`/`getTypeOfNode` query mapping a *declaration node* (or
declaration-name node) to its type — exactly what Go's inlay-hint
`visitVariableLikeDeclaration` calls (`s.checker.GetTypeAtLocation(decl)`). The
minimal faithful fix is **additive on the checker**: port `get_type_at_location`
next to `get_symbol_at_location` / `get_symbol_of_declaration`, then consume it
from the LS provider. No checker inference change was needed.

## What landed

| File | Go source | What |
|---|---|---|
| `checker/core/symbols_query.rs` | `checker.go:GetTypeAtLocation` / `getTypeOfNode` | `pub fn get_type_at_location(checker, program, node, globals) -> TypeId`: SourceFile / `InWithStatement` → error type; `IsDeclaration` (var/property/property-signature) → `get_type_of_symbol(get_symbol_of_declaration(node))`; declaration-name → `get_type_of_symbol(get_symbol_at_location(node))`; else error type. `is_declaration` helper (reachable kind subset) |
| `checker/lib.rs` | — | additive `pub use … get_type_at_location` |
| `ls/inlay_hints.rs` | `inlay_hints.go:visitVariableLikeDeclaration` + helpers | re-enabled var-type + property-decl-type dispatch in `visit` (now `&mut FileCheckContext`); `visit_variable_like_declaration`, `add_type_hints`, `is_module_reference_type`, `is_hintable_declaration`, `is_hintable_literal`, `is_literal_expression`, `is_infinity_or_nan_string`, `skip_parentheses`, `is_var_const` / `combined_node_flags`, `declaration_name_text`, inlined `equate_string_case_insensitive` |
| `checker/core/symbols_query_test.rs` | — | +5 checker tests |
| `ls/inlay_hints_test.rs` | — | +11 LS tests |

### Divergence (noted in code): type STRING label, not structured parts

Go's `visitVariableLikeDeclaration` renders the type via `typeToInlayHintParts`
(`TypeToTypeNode` + `getInlayHintLabelParts`) into a structured label whose
identifier parts carry `Location` links. This round renders the plain type
STRING (`type_to_string`) into the `StringOrInlayHintLabelParts` `String` arm —
which Go's `addTypeHints` also supports. The hint TEXT and the
`…WhenTypeMatchesName` comparison text (Go derives it by concatenating the same
parts) are identical; only the clickable per-identifier `Location` links are
deferred (same shape as the hover provider). blocked-by: the
`getInlayHintLabelParts` structured renderer.

## RED → GREEN slices (one behavior at a time)

Checker (`get_type_at_location`):
1. **`let x = 1` → name & decl resolve to `number`** (headline; widened, not
   `any`). RED: function absent. GREEN: the `getTypeOfNode` subset.
2. `const x = 1` → `1` (a `const` keeps the literal).
3. `declare function f(): string; const x = f()` → `string` (call inference).
4. `const o = { a: 1 }` → an anonymous object type (not `any`).
5. GUARD: the source-file node → error type (no panic).

LS (variable-type, then property-declaration-type):
6. **`let x = 1` → `: number` hint** at the name end, `Type` kind, left padding
   (headline). RED: 0 hints (kind DEFERRED). GREEN: the dispatch + walk.
7. `let x = "s"` → `: string`.
8. GUARD `const x = 1` / `const x = "s"` / `const x = true` → **no hint**
   (`isHintableDeclaration`: a `const` bound to a hintable literal is suppressed —
   matches the `inlayHintsVariableTypes1` baseline's `const b = 1` → no hint).
9. GUARD annotated (`let x: number = 1`, `const x: number = 1`) → no hint.
10. `declare function f(): string; const x = f()` → `: string` (a `const` call is
    hintable).
11. GUARD binding pattern (`const { a } = o`) → no hint.
12. `…WhenTypeMatchesName`: `const foo = make()` (type `Foo`) → suppressed by
    default, shown (`: Foo`) when the toggle is on.
13. property-decl `class C { a = 1 }` → `: number` (matches the
    `inlayHintsPropertyDeclarations` baseline); annotated `b: number = 2` → none;
    bare `c;` (no initializer, `any`) → none.
14. GUARD: the variable / property toggles are independent (one does not fire on
    the other's node kind).

## Ported vs still DEFERRED

**Newly ported (faithful):** `get_type_at_location` (the `IsDeclaration` /
declaration-name / SourceFile-guard arms); the **variable-type** and
**property-declaration-type** hint kinds with all of Go's gates
(`isHintableDeclaration` / `isHintableLiteral`, the no-initializer + property-any
rule, the annotation skip, `isModuleReferenceType`, and the
`…WhenTypeMatchesName` case-insensitive suppression).

**Still DEFERRED (with `blocked-by`):**
- `get_type_at_location`'s other `getTypeOfNode` arms — `IsPartOfTypeNode`
  (`getTypeFromTypeNode` + class extends/implements `this`-arg), `IsExpressionNode`
  (`getRegularTypeOfExpression`), `IsTypeDeclaration(Name)`, `IsBindingElement` /
  `IsBindingPattern`, import/export-assignment RHS, meta-properties, import
  attributes. blocked-by: faithful `isExpressionNode` / `isPartOfTypeNode`
  predicates + `getRegularTypeOfExpression` + binding-element typing.
- The structured `getInlayHintLabelParts` renderer (clickable `Location` links) —
  see the divergence note above.
- **Parameter-name hints** — blocked-by: a public `getResolvedSignature`.
- **Function parameter-type / return-type hints** — blocked-by: a public
  `getSignatureFromDeclaration` / `getReturnTypeOfSignature` /
  `getTypePredicateOfSignature` + the label-parts renderer.
- The property `d;` → `number | null` baseline case needs constructor-`this` CFA
  (`getWidenedTypeForAssignmentDeclaration` DEFERs constructor CFA), so it is not
  asserted here.

## Gate results (all GREEN, never `--no-verify`)

- `cargo test -p tsgo_checker`: **815 lib + 179 doctests pass** (+5 query tests).
- `cargo test -p tsgo_ls`: **124 lib + 1 doctest pass** (+11; hover/quick-info
  tests unchanged — no regression).
- `cargo test -p tsgo_ls_lsutil`: **59 + 32 pass**.
- `cargo test -p tsgo_compiler`: **132 + 11 pass** (downstream of the checker
  public-surface change, per the README gate).
- `cargo test -p tsgo_fourslash`: **51 + 1 pass** (the quick-info commands stay
  green).
- `cargo clippy -p tsgo_checker -p tsgo_ls --all-targets -- -D warnings`: clean.
- `cargo fmt -p tsgo_checker -p tsgo_ls -- --check`: clean.
- `cargo build --workspace --all-targets`: success.
