# ls — worklog (P7 — auto-insert provider)

> P7 `ls` (root crate `tsgo_ls`). Strict TDD (red→green vertical slices, one
> behavior at a time). SOLO round. Crate-scoped gates plus the touched
> `tsgo_lsproto` crate.

This round ports `internal/ls/autoinsert.go` — the
`textDocument/_vs_onAutoInsert` provider. When the user types `>`, Go's
`ProvideOnAutoInsert` **auto-closes a JSX element or fragment**: it finds the
token preceding the cursor and, if that token sits in an *unclosed* JSX
element/fragment, returns a snippet [`TextEdit`](tsgo_lsproto::TextEdit) that
inserts `$0</tag>` (or `$0</>`) at the cursor (a zero-width range). It is
**purely syntactic** (no checker): it reads the program's already-parsed source
file (its `NodeArena` + root) through `astnav`'s shared-borrow `NavSourceFile`
and converts the LSP position to a byte offset with the round-1 `Converters` +
the file's `DocumentScript` — the same `&self` pattern as `linkedediting.rs` /
`selectionranges.rs` / `folding.rs`.

## What landed

| File | Go source | What |
|---|---|---|
| `autoinsert.rs` | `internal/ls/autoinsert.go` | `provide_on_auto_insert(file, position, ch) -> Option<lsproto::VsOnAutoInsertResponseItem>` (the `ProvideOnAutoInsert` entry: the `ch != ">"` early-out, `FindPrecedingToken`, the synthesized-token guard, the element/fragment branch dispatch, the snippet text edit at the cursor); free helpers `is_unclosed_tag`, `is_unclosed_fragment`, `jsx_element_parts`, `jsx_tag_name`, `entity_name_to_string` (port of `ast.EntityNameToString`), `tag_names_are_equivalent` (port of `ast.TagNamesAreEquivalent`), `escape_snippet_text` (port of `completions.go:escapeSnippetText`), `is_synthesized_token` |
| `lib.rs` | — | `pub mod autoinsert;` |
| `autoinsert_test.rs` | — | 13 behavioral unit tests |
| `lsp/lsproto/generated.rs` | `lsp_generated.go:VsOnAutoInsertResponseItem` | **new** `VsOnAutoInsertResponseItem { vs_text_edit_format: InsertTextFormat, vs_text_edit: TextEdit }` lsp_object (`req` format / `reqnn` text edit, matching Go's `errNull` + required shape); plus `#[derive(Default)]` added to `InsertTextFormat` (needed so a required field can sit in a derived-`Default` lsp_object; default is the integer zero, matching Go's `uint32` zero value) |
| `lsp/lsproto/generated_test.rs` | — | 2 roundtrip tests (full round-trip + null/missing-field rejection) |

Public API is **additive**: one new `pub fn` on `LanguageService`
(`provide_on_auto_insert`), one new `pub` lsproto type
(`VsOnAutoInsertResponseItem`, auto-exported via `pub use generated::*`), one new
trait impl (`Default for InsertTextFormat`), and the `pub mod` registration. No
existing public item changed; no test weakened or deleted.

### lsproto note (per the brief)

The brief said to confirm whether `tsgo_lsproto` already had the result type and
to add it 1:1 if not. It did **not** — only the server-capability
`VsOnAutoInsertOptions` existed, not the response item. The Go provider returns
`lsproto.VsOnAutoInsertResponse` (= `VsOnAutoInsertResponseItemOrNull`); the
faithful, reachable result is the inner `VsOnAutoInsertResponseItem`
(`{ _vs_textEditFormat InsertTextFormat, _vs_textEdit *TextEdit }`). It was added
to `tsgo_lsproto::generated.rs` via the existing `lsp_object!` macro
(`_vs_textEditFormat` is a required value type → `req`; `_vs_textEdit` is a
required pointer that rejects an explicit `null` → `reqnn`) plus two roundtrip
tests — exactly how Rounds 34/35 added `SelectionRange` / `LinkedEditingRanges`.
The language-service method surfaces Go's `…OrNull` as
`Option<VsOnAutoInsertResponseItem>` (matching `linkedediting.rs`, which returns
`Option<LinkedEditingRanges>` rather than the LSP `…OrNull` wrapper). The
zero-width edit range is built from the original LSP `position` verbatim
(Go uses `params.VSPosition` directly), not a reconverted offset.

## RED → GREEN slices (one behavior at a time)

1. **lsproto type.** RED: the two roundtrip tests referenced a non-existent
   `VsOnAutoInsertResponseItem` (compile error), and the first `Default`-derive
   attempt failed because `InsertTextFormat` had no `Default`. GREEN: added the
   `lsp_object!` type + `#[derive(Default)]` on `InsertTextFormat`;
   `tsgo_lsproto` 307 → 309.
2. **Headline — cursor in the JSX text of an UNCLOSED `<div>`.**
   `const x = <div> text ;`, position inside ` text ` → a snippet edit inserting
   `$0</div>` at the cursor. RED: stub returned `None`. GREEN: the whole
   provider — the `ch != ">"` guard, `FindPrecedingToken`, the synthesized-token
   guard, the `IsJsxText`+`IsJsxElement` element branch, `isUnclosedTag`,
   `EntityNameToString`, and the `$0` + `escapeSnippetText` snippet edit.
3. **Fragment branch.** `const x = <> text ;`, cursor in ` text ` → `$0</>`
   (the `IsJsxText`+`IsJsxFragment` branch + `isUnclosedFragment`’s
   `THIS_NODE_HAS_ERROR` check on the missing `</>`). GREEN on arrival (same
   cohesive function).
4. **Guards.** `ch != ">"` → `None`; a closed element (`<div> foo </div>`) →
   `None`; a closed fragment (`<> foo </>`) → `None`; a non-JSX position
   (`const x = 1;` on `x`) → `None`; an unknown file → `None`.
5. **Extra coverage.** `$`-escaping (`<$Foo>` → `$0</\$Foo>`); namespaced tag
   (`<ns:tag>` → `$0</ns:tag>`, the `JsxNamespacedName` arm of both
   `EntityNameToString` and `TagNamesAreEquivalent`); property-access tag
   (`<a.b>` → `$0</a.b>`, the `PropertyAccessExpression` arm); the recursive
   `isUnclosedTag` parent branch (an inner `<div>…</div>` nested in an unclosed
   same-named parent still fires → `$0</div>`); plus direct unit tests for the
   pure `escape_snippet_text` and `is_synthesized_token` helpers.

## Ported vs DEFERRED

**Ported (faithful 1:1 with `autoinsert.go`):**

- `ProvideOnAutoInsert`: the `params.VSCh != ">"` early-out, the
  position → byte offset conversion, `FindPrecedingToken`, the
  element-then-fragment dispatch, the `closingText == ""` → empty-response
  guard, and the snippet `TextEdit` (`$0` + escaped closing text, zero-width
  range at the cursor, `InsertTextFormatSnippet`).
- The **`IsJsxText` element branch** (fully reachable): `element =
  token.Parent`, `isUnclosedTag`, and `closingText = "</" +
  EntityNameToString(openingTagName, GetTextOfNode) + ">"`.
- The **`IsJsxText` fragment branch** (fully reachable): `fragment =
  token.Parent`, `isUnclosedFragment`, `closingText = "</>"`.
- `isUnclosedTag` (opening/closing tag-name mismatch + the recursive
  same-named-unclosed-parent branch) and `isUnclosedFragment`
  (`NodeFlagsThisNodeHasError` on the closing fragment + the recursive
  unclosed-parent branch).
- `ast.EntityNameToString` (Identifier/`this`/QualifiedName/PropertyAccess/
  JsxNamespacedName), `ast.TagNamesAreEquivalent`, and
  `completions.go:escapeSnippetText` — ported as local free helpers with
  `// Go:` anchors (none was public in `tsgo_ast`/`tsgo_ls`).
- The two **`>`-token branches** (`token.Kind == KindGreaterThanToken &&
  IsJsxOpeningElement/Fragment(token.Parent)`) — ported in place but **inert**
  (see DEFERRED).

**DEFERRED:**

- **The two `>`-token branches — ported but inert.** These handle the common
  "cursor sits right after the just-typed `>`" case (`<div>|` → `</div>`,
  `<>|` → `</>`; the entire `autoCloseTag` / `autoCloseFragment` /
  `autoCloseTagsWithTriviaAndComplexNames` "`>`-adjacent" set). Go reaches
  `token.Parent` on a `FindPrecedingToken` result that is the synthesized `>`
  punctuation token; in `tsgo_astnav` such tokens live in a side store with **no
  parent pointer** and are not real arena nodes, so `NodeArena::parent` cannot be
  called on them. This module guards with `is_synthesized_token` (returning
  `None`) before the branch logic, exactly like `linkedediting.rs`'s fragment
  branch and `signaturehelp.rs`'s synthesized-`(`/`,` note, so the two
  `>`-token branches never execute at runtime. The branch code is left in place
  so the port flips on for free once `astnav` grows a parent-carrying
  synthesized-token store. blocked-by: a parent-carrying synthesized-token store
  in `tsgo_astnav`. The reachable behavior (cursor in the JSX **text/children**
  of an unclosed element/fragment) is fully ported and tested.

## Extra behavioral tests (only MORE than Go, never fewer)

`internal/ls/autoinsert.go` has **no Go `*_test.go`** upstream (it is covered by
the `autoCloseTag` / `autoCloseFragment` /
`autoCloseTagsWithTriviaAndComplexNames` fourslash baselines, which are P10), so
all 13 tests here are net-new behavioral coverage, each pinned to a fourslash
ground-truth marker where one exists: unclosed element → `</div>` (autoCloseTag
/5), unclosed fragment → `</>` (autoCloseFragment /5), `$`-escaped tag name
(complex-names /10), namespaced tag (complex-names /2), property-access tag
(complex-names /5), nested-unclosed recursion (autoCloseTag /9), `ch != ">"`,
closed element → None (autoCloseTag /1), closed fragment → None
(autoCloseFragment /1), non-JSX → None, unknown-file → None, plus the pure
`escape_snippet_text` and `is_synthesized_token` unit tests. Plus 2
`tsgo_lsproto` roundtrip tests for the new `VsOnAutoInsertResponseItem` type.

## Test deltas

- `tsgo_ls`: **153 → 166** unit tests (+13), all green (+ 1 doctest unchanged).
- `tsgo_lsproto`: **307 → 309** unit tests (+2), all green (+ 15 doctests
  unchanged).

No existing test weakened or deleted.

## Gates (all GREEN)

```
cargo test  -p tsgo_ls                                    # 166 passed; 0 failed (+ doctest)
cargo test  -p tsgo_lsproto                               # 309 passed; 0 failed (+ doctests)
cargo clippy -p tsgo_ls --all-targets -- -D warnings      # clean
cargo clippy -p tsgo_lsproto --all-targets -- -D warnings # clean
cargo fmt                                                 # applied
cargo fmt -- --check                                      # clean
cargo build --workspace --all-targets                     # ok
```

This round adds the fifth purely-syntactic LS feature (after folding, document
symbols, selection ranges, and linked editing); it reuses the checker-free
AST-walk + `Converters` pattern and adds the new
`lsproto::VsOnAutoInsertResponseItem` result type.

---

# P7/astnav — synthesized-token parent + autoinsert `>`-branch enable

> Follow-up SOLO round. Keystone: give `astnav`'s scanner-synthesized tokens a
> **queryable parent**, then re-enable the highest-value blocked LS branch (the
> autoinsert `>`-token branch, `<div>|` -> `</div>` — the common case) as the
> consumer + validation.

## Step 0 — feasibility verdict

**SMALL accessor addition** (not a big refactor). The synthesized-token side
store (`SynthesizedTokenStore` in `internal/astnav/lib.rs`) already received the
`parent` `NodeId` at synthesis time — it was used as part of the `TokenCacheKey`
(`{parent, pos, end}`) for pointer-equality but **thrown away** afterwards (the
`SynthesizedToken` struct stored only `kind`/`loc`). Go's
`ast.go:SourceFile.GetOrCreateToken` sets `token.Parent = parent`, so a
synthesized token in Go always carries its parent; the LS reads it via
`token.Parent`. The faithful fix is therefore tiny: **store** the `parent` on
the `SynthesizedToken` entry and **expose** it through a new
`NavEngine::parent` accessor that spans both the real arena and the side store.
No structural change, no new deps, fully additive.

## astnav change (the root-cause fix)

| File | What |
|---|---|
| `internal/astnav/lib.rs` | `SynthesizedToken` gains a `parent: NodeId` field (recorded in `SynthesizedTokenStore::get_or_create`, exactly the `parent`/`current`/`containingNode` the scanner was walking — the value Go assigns to `token.Parent`). New **`pub fn NavEngine::parent(&self, id) -> Option<NodeId>`**: for a synthesized id it returns `Some(stored_parent)`; for a real id it returns `self.arena().parent(id)` (so the root yields `None`). `// Go:` anchored to `SourceFile.GetOrCreateToken`. |
| `internal/astnav/lib_test.rs` | `parse_tsx` helper; **2 new unit tests** (see below). |

The accessor is purely additive — every existing `astnav` consumer (checker,
other LS providers) is unaffected; the previous `arena().parent(id)` calls keep
working unchanged.

## LS branch re-enabled vs still DEFERRED

**Re-enabled (this round):** `internal/ls/autoinsert.rs` — the two **`>`-token**
branches (`token.Kind == KindGreaterThanToken &&
IsJsxOpeningElement/Fragment(token.Parent)`). The provider now resolves the
preceding token's kind/parent through `nav.kind(token)` / `nav.parent(token)`
(which span the real arena + the synthesized-token side store) instead of
`arena.kind`/`arena.parent`, and the old `is_synthesized_token(token) -> None`
guard + the local `is_synthesized_token`/`SYNTHESIZED_NODE_TAG` workaround are
**removed** (Go's `autoinsert.go` has no such concept — it just reads
`token.Parent`). The four Go branches are now all reachable; the module doc's
`# DEFER` section became `# Reachable branches`.

**Still DEFERRED (notes only, now unblockable):** `linkedediting.rs`'s
JSX-fragment branch and `signaturehelp.rs`'s synthesized-`(`/`,` branch keep
their own `is_synthesized_token` guards for now. The shared root cause (no
parent on synthesized tokens) is fixed, so those modules can flip on by the
same `nav.parent` swap in a follow-up; left untouched here to keep the diff
surgical and avoid unproven position ground-truth for those providers.

## RED → GREEN slices (one behavior at a time)

1. **astnav — synthesized `>` token reports its `JsxOpeningElement` parent.**
   `const x = <div> text ;`, `find_preceding_token(15)` (cursor right after the
   `>`). RED: a temporary `parent` stub returned `None` for synthesized ids →
   the test failed at "a synthesized token carries its parent" (the `kind ==
   GreaterThanToken` + high-bit-tagged assertions already passed, confirming the
   parse/position). GREEN: added `SynthesizedToken.parent` + returned
   `Some(stored)`.
2. **ls autoinsert — `<div>|` + `>` inserts `$0</div>`.** Same file, cursor at
   byte 15. RED: the existing synthesized-token guard returned `None`. GREEN:
   removed the guard and wired the branch to `nav.kind`/`nav.parent`; the
   `>`-token element branch + `isUnclosedTag` + `EntityNameToString` now fire.
3. **Guards (no regression / no panic).** New astnav unit test:
   `parent` on the root source file is `None` (genuinely-parentless edge) and a
   real identifier's `parent` equals `arena().parent` (no corruption); plus a
   `NavEngine::parent` doctest asserting `nav.parent(nav.root()) == None`. The
   pre-existing astnav suite, the autoinsert `IsJsxText` tests, and the
   `linkedediting` tests all stay green.

## Test deltas

- `tsgo_astnav`: **35 → 37** unit tests (+2:
  `synthesized_greater_than_token_reports_jsx_opening_element_parent`,
  `parent_resolves_real_nodes_and_root_has_none`); **26 → 27** doctests (+1: the
  `NavEngine::parent` example).
- `tsgo_ls`: **166 → 166** unit tests (net 0: **+1**
  `provide_on_auto_insert_greater_than_token_inserts_closing_tag`; **−1** the
  `is_synthesized_token_detects_high_bit` unit test, which pinned a now-deleted
  local workaround helper — its removal tracks the helper, not behavior). All
  autoinsert behavior tests (incl. both `IsJsxText` branches and all guards)
  remain.

No behavioral test weakened; the only removed test covered the deleted
`is_synthesized_token` helper.

## Gates (all GREEN; never `--no-verify`)

```
cargo test  -p tsgo_astnav                                # 37 unit + 27 doctest; 0 failed
cargo test  -p tsgo_ls                                    # 166 unit + 1 doctest; 0 failed
cargo clippy -p tsgo_astnav -p tsgo_ls --all-targets -- -D warnings   # clean
cargo fmt && cargo fmt -- --check                         # clean
cargo build --workspace --all-targets                     # ok (astnav is upstream; parser/checker/compiler/ls/fourslash all rebuilt clean)
```

Tree carries exactly four edited files (`astnav/lib.rs`, `astnav/lib_test.rs`,
`ls/autoinsert.rs`, `ls/autoinsert_test.rs`); `Cargo.lock` unchanged; no new
deps. No downstream regression.
