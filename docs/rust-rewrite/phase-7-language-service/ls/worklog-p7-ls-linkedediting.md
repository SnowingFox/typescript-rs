# ls — worklog (P7 — linked editing ranges provider)

> P7 `ls` (root crate `tsgo_ls`). Strict TDD (red→green vertical slices, one
> behavior at a time). SOLO round. Crate-scoped gates plus the touched
> `tsgo_lsproto` crate.

This round ports `internal/ls/linkedediting.go` — the
`textDocument/linkedEditingRange` provider. When the cursor sits inside a JSX
tag name it returns **both** the opening and closing tag-name
[`Range`](tsgo_lsproto::Range)s (so an editor renames the pair together as the
user types) plus a permissive `wordPattern` regex describing valid tag-name
characters. It is **purely syntactic** (no checker): it reads the program's
already-parsed source file (its `NodeArena` + root) through `astnav`'s
shared-borrow `NavSourceFile` and converts byte offsets to UTF-16 LSP positions
with the round-1 `Converters` + the file's `DocumentScript` — the same `&self`
pattern as `selectionranges.rs` / `folding.rs`.

## What landed

| File | Go source | What |
|---|---|---|
| `linkedediting.rs` | `internal/ls/linkedediting.go` | `provide_linked_editing_ranges(file, position) -> Option<lsproto::LinkedEditingRanges>` (the `ProvideLinkedEditingRange` entry); the JSX-**element** branch `linked_editing_for_element` (find the opening/closing element via `find_ancestor`, the well-formedness guard, the within-tag-name guard, the identical-tag-text guard, the two tag-name ranges); the JSX-**fragment** branch `linked_editing_for_fragment` (ported 1:1, inert — see DEFERRED); free helpers `jsx_tag_name`, `find_ancestor`, `is_synthesized_token`; the `JSX_TAG_WORD_PATTERN` const |
| `lib.rs` | — | `pub mod linkedediting;` |
| `linkedediting_test.rs` | — | 9 behavioral unit tests |
| `lsp/lsproto/generated.rs` | `lsp_generated.go:LinkedEditingRanges` | **new** `LinkedEditingRanges { ranges: Vec<Range>, word_pattern: Option<String> }` lsp_object (`reqnn ranges` / `opt word_pattern`, matching Go's `errNull` + `omitzero` shape) |
| `lsp/lsproto/generated_test.rs` | — | 2 roundtrip tests (with / without `wordPattern`) |

Public API is **additive**: one new `pub fn` on `LanguageService`
(`provide_linked_editing_ranges`), one new `pub` lsproto type
(`LinkedEditingRanges`, auto-exported via `pub use generated::*`), and the
`pub mod` registration. No existing public item changed; no test weakened or
deleted.

### lsproto note (deviation from the brief)

The brief said to confirm whether `tsgo_lsproto` already had the
`LinkedEditingRanges` *result* type and to add it 1:1 if not. It did **not** —
only the server-capability `LinkedEditingRangeOptions` /
`LinkedEditingRangeRegistrationOptions` existed, not the result type. So the
result `LinkedEditingRanges` (Go `lsp_generated.go:LinkedEditingRanges`,
`{ ranges []Range, wordPattern *string }`) was added to
`tsgo_lsproto::generated.rs` via the existing `lsp_object!` macro (`ranges` is a
required slice that rejects an explicit `null` → `reqnn`; `wordPattern` is an
optional, `omitzero` pointer → `opt`) plus two roundtrip tests — exactly how
Round 34 added `SelectionRange`. This is the faithful 1:1 choice (the Go
provider returns `lsproto.LinkedEditingRanges`) and is why the changed-file set
includes 2 lsproto files in addition to the new `ls` file + `lib.rs` + this
worklog.

### Word pattern (ported EXACTLY)

Go: `var jsxTagWordPattern = new("[a-zA-Z0-9:\\-\\._$]*")`. The Go source literal
denotes the string `[a-zA-Z0-9:\-\._$]*`. Rust ports it as the byte-for-byte
identical raw string `const JSX_TAG_WORD_PATTERN: &str = r"[a-zA-Z0-9:\-\._$]*";`
(verified by a dedicated unit test asserting the exact returned string).

## RED → GREEN slices (one behavior at a time)

1. **lsproto type.** RED: the two roundtrip tests referenced a non-existent
   `LinkedEditingRanges` (compile error). GREEN: added the `lsp_object!` type;
   `tsgo_lsproto` 305 → 307.
2. **Headline — cursor in the OPENING tag name.** `<div></div>`, position inside
   the opening `div` (byte 2) → `ranges = [0:1-0:4 (opening div), 0:7-0:10
   (closing div)]` + word pattern. RED: stub returned `None`. GREEN: the whole
   element branch — `find_ancestor` to the opening/closing element, the
   well-formedness guard (`tagNameStart == elementStart` / `tagNameEnd ==
   elementEnd`), the within-tag-name guard, the identical-tag-text guard, and the
   two `to_lsp_range` ranges + word pattern.
3. **Cursor in the CLOSING tag name** (byte 8) → the same two ranges (the
   `closeTagNameStart <= position <= closeTagNameEnd` arm). GREEN immediately
   (same element branch).
4. **Guards.** self-closing `<div/>` → `None` (a `JsxSelfClosingElement` is
   neither opening nor closing, so `find_ancestor` returns `None`); a non-JSX
   position (`const x = 1;` on `x`) → `None`; a cursor on element body text
   (`<div>x</div>` on `x`) → `None`; an unknown file → `None`. The body / non-JSX
   guards surfaced the **synthesized-token panic** (see below); fixed by guarding
   with `is_synthesized_token` before touching `arena.parent`.
5. **Extra coverage.** multi-character tag name (`<span>x</span>` → `0:1-0:5` /
   `0:9-0:13`); multi-line element (`<div>\n</div>` → opening `0:1-0:4`, closing
   `1:2-1:5`, exercising cross-line position conversion); the exact word-pattern
   string.

### Synthesized-token panic (fixed)

Go's `FindPrecedingToken` returns a token whose `token.Parent` is always
readable, including for JSX punctuation (`<`, `>`, `/`) and keywords (`const`).
In `tsgo_astnav` those tokens are *synthesized* into a side store, are not real
arena nodes, and `NodeArena::parent` indexes the parsed-node vector directly —
so calling it on a synthesized id **panics** (out-of-bounds), it does not return
`None`. Two guard tests (body text, variable name) initially panicked there. Fix
(mirroring `signaturehelp.rs` / `completions.rs`): guard the
`FindPrecedingToken` result with a local `is_synthesized_token` and return `None`
for a synthesized preceding token. This matches the established LS pattern and is
the cause of the DEFERRED fragment branch below.

## Ported vs DEFERRED

**Ported (faithful 1:1 with `linkedediting.go`):**

- `ProvideLinkedEditingRange`: position → byte offset via `Converters`,
  `FindPrecedingToken`, the `token == nil || token.Parent.Kind == SourceFile`
  early-out, and the fragment-vs-element dispatch on `IsJsxFragment(token.Parent.Parent)`.
- The **element branch** (fully reachable): `FindAncestor` to the nearest
  opening/closing element, `jsxElement := tag.Parent.AsJsxElement()`, the
  opening/closing tag-name `[start, end)` computation, the not-well-formed guard,
  the within-tag-name guard, the identical-tag-text guard
  (`scanner.GetTextOfNode` == the trivia-skipped tag-name source slice), and the
  two tag-name ranges + `jsxTagWordPattern`.
- The **fragment branch** code (`openFragment`/`closeFragment` error-flag check,
  `openPos = start + len("<")`, `closePos = start + len("</")`, the
  `position == openPos || position == closePos` gate, the two zero-width fragment
  ranges + word pattern).

**DEFERRED:**

- **JSX fragment branch (`<>...</>`) — ported but inert.** Its only valid cursor
  positions (`openPos`/`closePos`) sit exactly on the `<` / `</` punctuation,
  which `FindPrecedingToken` resolves to *synthesized* tokens that carry no arena
  parent (the parent is only a synthesized-token cache key, never read back, and
  the token is not a real arena node). Go reaches `token.Parent.Parent` for them;
  here the `is_synthesized_token` guard returns `None` first, so the fragment
  branch is never entered at runtime. The same limitation applies to
  **element-branch boundary positions** that land on punctuation (e.g. the cursor
  exactly between `<` and the tag name) — the common case (cursor strictly inside
  the tag name → the real identifier node) is unaffected. blocked-by: a
  parent-carrying synthesized-token store in `tsgo_astnav` (same root cause as
  `signaturehelp.rs`'s synthesized-`(`/`,` note). The fragment code is left in
  place so the port flips on for free once `astnav` grows that capability.

## Extra behavioral tests (only MORE than Go, never fewer)

`internal/ls/linkedediting.go` has **no Go `*_test.go`** upstream (it is covered
by the `linkedEditing_*` fourslash baselines, which are P10), so all 9 tests here
are net-new behavioral coverage: opening-tag cursor, closing-tag cursor, exact
word pattern, multi-character tag name, multi-line element, self-closing → None,
non-JSX → None, body-text → None, unknown-file → None. Plus 2 `tsgo_lsproto`
roundtrip tests for the new `LinkedEditingRanges` type.

## Test deltas

- `tsgo_ls`: **144 → 153** unit tests (+9), all green (+ 1 doctest unchanged).
- `tsgo_lsproto`: **305 → 307** unit tests (+2), all green (+ 15 doctests
  unchanged).

No existing test weakened or deleted.

## Gates (all GREEN)

```
cargo test  -p tsgo_ls                                    # 153 passed; 0 failed (+ doctest)
cargo test  -p tsgo_lsproto                               # 307 passed; 0 failed (+ doctests)
cargo clippy -p tsgo_ls --all-targets -- -D warnings      # clean
cargo clippy -p tsgo_lsproto --all-targets -- -D warnings # clean
cargo fmt                                                 # applied
cargo fmt -- --check                                      # clean
cargo build --workspace --all-targets                     # ok
```

This round adds the fourth purely-syntactic LS feature (after folding, document
symbols, and selection ranges); it reuses the checker-free AST-walk + `Converters`
pattern and adds the new `lsproto::LinkedEditingRanges` result type.
