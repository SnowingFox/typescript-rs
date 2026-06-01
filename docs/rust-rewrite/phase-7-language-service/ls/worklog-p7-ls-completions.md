# ls — Round 4 worklog (LS root: completions core)

> P7 `ls` (root crate `tsgo_ls`) round 4. Strict TDD (red→green vertical slices).
> Crate-scoped gates only (`-p tsgo_ls`). A concurrent lane was editing the
> separate `tsgo_ls_autoimport` crate (`internal/ls/autoimport/**`), disjoint
> from this lane, so this round touched **only** `internal/ls/**` root-crate
> files (NOT the `lsconv`/`lsutil`/`change`/`autoimport` sub-crates' source) +
> this doc. **No new dependency on `tsgo_ls_autoimport`** (auto-import
> completions are DEFERRED to a later round). One additive dependency was added
> to `internal/ls/Cargo.toml`: `tsgo_ls_lsutil` (for `ScriptElementKind`), whose
> own dependency closure (`ast`/`astnav`/`core`/`scanner`/`stringutil`/`tspath`)
> does **not** include `tsgo_ls_autoimport`, so it does not couple to the
> concurrent lane. No root `Cargo.toml` edit; no other crate's source touched.

This round adds the **completions core** on top of the round-1 `Converters` +
`file_check_context` (token + checker bridge) and the round-1 `hover` resolution
chain (`get_symbol_at_location` → `get_type_of_symbol`). It ports the two
reachable cores of Go's `internal/ls/completions.go`: **member-access
completions** (`obj.`) and **scope/identifier completions**.

## What landed

| File | Go source | What |
|---|---|---|
| `completions.rs` | `internal/ls/completions.go` | `provide_completions` (the `ProvideCompletion` / `getCompletionsAtPosition` / `getCompletionData` dispatch, reachable subset) → `Option<CompletionList>`; `classify` + `classify_after_dot` (`getRelevantTokens` + `isRightOfDot` + the `PropertyAccessExpression` `node = Expression()` recovery); `member_completions` (`getTypeScriptMemberSymbols` / `addTypeProperties` core); `scope_completions` + `copy_symbols` (`getGlobalCompletions` + `checker.getSymbolsInScope` reachable walk); `script_element_kind_for_symbol` (`lsutil.GetSymbolKind` reachable subset) + `completion_item_kind` (`getCompletionsSymbolKind`); `can_have_locals` (`checker.canHaveLocals`) and `is_reserved_member_name` (`checker.isReservedMemberName`); the `CompletionList` data record |
| `completions_test.rs` | — | 8 unit tests |
| `lib.rs` | `internal/ls/lib.rs` | `pub mod completions;` + `pub use completions::CompletionList;` |
| `Cargo.toml` | — | additive dependency `tsgo_ls_lsutil = { path = "lsutil" }` |

Public API is **additive within `tsgo_ls`**: one new `pub fn` on
`LanguageService` (`provide_completions`), one new public type (`CompletionList`),
and the `pub mod completions;` registration + re-export. Every other item in
`completions.rs` is crate-private (`fn` / `const` / `enum`), not public API. No
existing public item changed; no existing test was weakened or deleted.

## Architecture: reuse of the round-1 resolution chain

- **Member completions** = "type the dotted expression, list its apparent type's
  properties". `classify` runs Go's `getRelevantTokens` (preceding token, then
  the token before a member-name/keyword), and when the `contextToken` is a `.`
  (`isRightOfDot`) recovers the dotted expression. Because the Rust `.` token is
  *synthesized* by `astnav` (the AST stores no dot child, so it has no arena
  parent — unlike Go's `contextToken.Parent`), `classify_after_dot` instead
  climbs from the token left of the dot to the largest node that *ends at the
  dot*, which is exactly Go's `propertyAccessToConvert.Expression()`.
  `member_completions` then resolves that expression's symbol + type
  (`get_symbol_at_location` → `get_type_of_symbol`, the same chain `hover` uses)
  and enumerates `get_properties_of_type` (which applies the apparent type and
  unions intersection members — Go's `GetApparentProperties`).
- **Scope completions** = "the symbols visible at this identifier position".
  `scope_completions` mirrors `checker.getSymbolsInScope`: it walks the binder
  `locals` tables up the container chain (skipping a global source file's own
  `locals`, since a script's top-level declarations are reached through
  `globals` — Go's `!IsGlobalSourceFile` guard), then copies `globals`, keeping
  the first-seen symbol per name so an inner scope shadows an outer one (Go's
  `copySymbol`). The completion `meaning` is value + type + namespace + alias
  (Go's non-type-only `symbolMeanings`); reserved member names and the `this`
  keyword symbol are dropped (`symbolsToArray` + the `delete this`).
- **Kind mapping**: `script_element_kind_for_symbol` is the reachable subset of
  `lsutil.GetSymbolKind` (flags → `ScriptElementKind`: variable / function /
  accessor / method / constructor / signature / property, then class / enum /
  type-alias / interface / type-parameter / enum-member / alias / module), and
  `completion_item_kind` is the full port of `getCompletionsSymbolKind`
  (`ScriptElementKind` → `lsproto::CompletionItemKind`).

### Two cross-crate constraints handled in this lane

1. **`lsproto` has no `CompletionList`** (and may not be edited here): only
   `CompletionItem` / `CompletionItemKind` / `CompletionItemLabelDetails` /
   `CompletionItemTag` are generated. The reachable `CompletionList`
   (`is_incomplete` + `items`) is therefore defined in `completions.rs`. The
   `lsproto` `CompletionItemKind` exposes only `VARIABLE`; the remaining kinds
   are constructed from their stable LSP integer values via the public tuple
   field (`mod kinds`).
2. **Synthesized property symbols are not in the bound program.** Object-literal
   (and union/mapped) member symbols are checker-*transient*: their flags/name
   live in the checker's transient arena, and their ids carry the high-bit tag
   (`1 << 31`). A `program.symbol(id)` lookup on such an id would index out of
   bounds, and the checker's `resolved_symbol_flags` is `pub(crate)` (not
   reachable from `tsgo_ls`). So `member_completion_item_kind` detects a tagged
   id and maps it to `MemberVariableElement` → `Field` (object-literal members
   are always properties in the reachable subset) without consulting the
   program; a program (interface/class) member reads its flags normally.

## RED → GREEN slices (observed symptoms)

Genuine RED was observed by stubbing the `completions_at` dispatcher to return
`None` for both `Member` and `Scope` classifications: the 6 behavioral tests
failed (the two non-dispatch tests — `unknown_file_is_none` and the pure
`completion_item_kind` mapping — stayed green), then restoring the dispatch
turned all 8 green.

1. **Member: `const o = { a: 1, b: "x" }; o.` (cursor right after the dot) →
   `a` (Field) + `b` (Field).** RED (stub): `provide_completions` returned
   `None`; `.expect("a member completion list for \`o.\`")` panicked. GREEN: the
   dot recovery yields the expression `o`, `get_type_of_symbol` gives the
   inferred object type `{ a: number; b: string }`, and `get_properties_of_type`
   lists `a`, `b` (synthesized property symbols → `Field`).
2. **Member kind: `interface I { m(): void; p: number; } declare const o: I; o.`
   → `m` (Method) + `p` (Field).** GREEN on first run after restoring dispatch:
   `o`'s annotation type is the interface `I`, whose members `m`/`p` are *program*
   symbols, so `m`'s `Method` flag → `MemberFunctionElement` → `Method` and `p`'s
   `Property` flag → `MemberVariableElement` → `Field`.
3. **Member on a missing/error receiver: `q.` (undefined `q`) → empty list.** RED
   (stub): returned `None`; `.expect("an (empty) member completion list")`
   panicked. GREEN: `get_symbol_at_location(q)` is `None`, so `member_completions`
   returns an empty (non-`None`) list — never a panic.
4. **Scope: `const x = 1; function f(p) { p }` at the body → includes `x`
   (Variable), `f` (Function), `p` (Variable).** RED (stub): returned `None`;
   `.expect("a scope completion list…")` panicked. GREEN: the walk collects the
   parameter `p` from the function's `locals` and `x`/`f` from `globals`.
5. **Scope shadowing/visibility: `const x=1; function f(p){ const y=2; y }
   function g(q){ const z=3; }` at the inner `y` → includes `x`,`f`,`g`,`p`,`y`,
   excludes the sibling `g`'s `q`,`z`.** RED (stub): returned `None`. GREEN: the
   container walk visits only the body `Block` (→ `y`) and `f` (→ `p`), never
   `g`'s scope; `globals` adds the top-level `x`/`f`/`g`.

## Extra behavioral tests (only MORE than Go, never fewer)

- Member on a primitive without a lib loaded (`const n: number = 1; n.`) → empty
  list (the `number` apparent type has no members without `lib.d.ts`; no panic).
- `provide_completions` on an unknown file → `None` (no panic).
- A standalone unit test for `completion_item_kind` covering **every**
  `getCompletionsSymbolKind` arm (keyword/primitive → Keyword; const/let/var/
  parameter/alias → Variable; member-variable/get/set → Field; function/local-
  function → Function; method/call/construct/index-signature → Method; enum →
  Enum; enum-member → EnumMember; module → Module; class/type → Class; interface
  → Interface; warning → Text; script → File; directory → Folder; string →
  Constant; type-parameter & unknown → Property).

## Go functions mirrored (`// Go:` anchors)

- `completions.go:LanguageService.ProvideCompletion` / `getCompletionsAtPosition`
  / `getCompletionData` (the dispatch body), `getRelevantTokens` +
  `isRightOfDot` token analysis, `getTypeScriptMemberSymbols` / `addTypeProperties`
  (member core), `getGlobalCompletions` (scope core), `createLSPCompletionItem`
  (label + kind), `getCompletionsSymbolKind` (the kind table), and
  `CompareCompletionEntries` (the label tie-break sort).
- `internal/checker/services.go:getSymbolsInScope` (`copySymbol`/`copySymbols`
  first-seen-wins walk + `symbolsToArray`), ported locally into `scope_completions`
  / `copy_symbols`.
- `internal/checker/utilities.go:canHaveLocals` and `isReservedMemberName`,
  ported locally (the checker is a different crate).
- `internal/ls/lsutil/symbol_display.go:GetSymbolKind` /
  `getSymbolKindOfConstructorPropertyMethodAccessorFunctionOrVar` (reachable
  subset), ported locally into `script_element_kind_for_symbol` because the Go
  version takes a `*checker.Checker` (deferred in `tsgo_ls_lsutil`); the
  `ScriptElementKind` enum itself is reused from `tsgo_ls_lsutil`.
- `internal/checker/checker.go:getSymbolAtLocation` / `getTypeOfSymbol` /
  `getPropertiesOfType` / `getApparentType` (reused from `tsgo_checker`).

## Test deltas

Crate was at **42** unit tests (round 3). Now **50** unit tests (+0 doctests),
all green:

- `completions_test.rs` — 8 (member object-literal Field; member interface
  Method/Field; member unresolved → empty; member primitive-without-lib → empty;
  scope locals+globals; scope shadowing/visibility; unknown-file → None; the
  `completion_item_kind` mapping unit).

No existing test was weakened or deleted (rule 5); every new `pub fn` /
classification path has a behavioral test plus empty/edge coverage.

## Gates (crate-scoped, all GREEN)

```
cargo test  -p tsgo_ls                               # 50 passed; 0 failed (+ 0 doctests)
cargo clippy -p tsgo_ls --all-targets -- -D warnings # clean
cargo fmt   -p tsgo_ls -- --check                    # clean
cargo build -p tsgo_ls                               # ok
```

(`--workspace` was intentionally not run — concurrent `tsgo_ls_autoimport` lane
active.)

## DEFER list (blocked-by → future ls rounds)

- **Auto-import completions** — Go's `collectAutoImports` / the `autoImports`
  branch of `getCompletionEntriesFromSymbols` (module-export suggestions with an
  import code-action). blocked-by: the concurrently-edited `tsgo_ls_autoimport`
  crate (this lane must not depend on it) + the `compiler.Program`-level
  auto-import registry.
- **Completion details / resolve** — `getCompletionEntryDetails`
  (documentation, signature, `additionalTextEdits` for the import fix).
  blocked-by: the resolve round + the JSDoc reparser + `lsproto` resolve types.
- **JSX / string-literal / import-path / JSDoc completions** —
  `getStringLiteralCompletions`, `tryGetJsxCompletionSymbols`,
  `tryGetImportCompletionSymbols`, `getJSDocTag*Completions`. blocked-by: the JSX
  intrinsic-tag surface, `GetContextualType` for string literals, the
  module-specifier resolver, and the JSDoc tag tables.
- **Object-literal / contextual-type property suggestions** —
  `tryGetObjectLikeCompletionSymbols` / `getPropertiesForObjectExpression` /
  `getContextualType` (suggesting unfilled members of `const c: I = { | }`).
  blocked-by: `GetContextualType` + the existing-member filter.
- **Keyword completions** — `getKeywordCompletions` / the `KeywordCompletionFilters`
  tables and the `completionDataKeyword` branch. blocked-by: the keyword tables +
  `scanner.TokenToString`.
- **Snippets / replacement spans / commit characters / sort-text / preselect /
  filter-text / `CompletionItemData`** — the rest of `createLSPCompletionItem`
  (`getReplacementRangeForContextToken`, `getFilterText`, `getDotAccessor`,
  `computeCommitCharactersAndIsNewIdentifier`, `CompletionItemData`) and the rich
  `CompareCompletionEntries` (sort-text then label). blocked-by: `UserPreferences`
  / `GetQuotePreference`, the `lsproto.CompletionList` / `CompletionItemData`
  surface (not in `lsproto` yet), and the sort-text priority machinery.
- **`this.` / `super.` / optional-chain (`?.`) + module/enum member access** —
  the `thisType` properties branch of `getGlobalCompletions`, the
  `KindQuestionDotToken` (`isRightOfQuestionDot`) path, and the module/enum
  `GetExportsOfModule` enumeration in `getTypeScriptMemberSymbols`. blocked-by:
  `TryGetThisTypeAtEx`, `GetNonNullableType`/optional-chain typing, and
  `GetExportsOfModule` / `IsValidPropertyAccessForCompletions`.
- **`IsValidPropertyAccessForCompletions` accessibility filter** — the reachable
  subset lists every apparent property; Go filters private/protected members not
  accessible at the call site. blocked-by: the property-accessibility check
  (`isPropertyAccessible`).
- **`CompletionList` / `CompletionItemData` hoist into `lsproto`** — the local
  `CompletionList` should move into `tsgo_lsproto` once that crate generates it.
  blocked-by: `tsgo_lsproto` is owned by a different crate/lane (not editable
  here) and has not yet ported `CompletionList` / `CompletionItemData`.

This round establishes the completions core (member-access + scope/identifier
enumeration + the symbol-kind → `CompletionItemKind` mapping) that the later
auto-import, contextual-type, keyword, and resolve rounds will build on.
