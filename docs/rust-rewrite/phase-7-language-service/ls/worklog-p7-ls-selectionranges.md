# ls — worklog (P7 — selection ranges provider)

> P7 `ls` (root crate `tsgo_ls`). Strict TDD (red→green vertical slices, one
> behavior at a time). SOLO round. Crate-scoped gates plus the touched
> `tsgo_lsproto` crate.

This round ports `internal/ls/selectionranges.go` — the
`textDocument/selectionRange` provider. Given a set of positions it returns one
nested expand/shrink [`SelectionRange`] chain per position: the deepest AST node
containing the position, with `parent` pointers walking outward to the whole
source file. It is **purely syntactic** (no checker): it reads the program's
already-parsed source file (its `NodeArena` + root) through `astnav`'s
shared-borrow `NavSourceFile` and converts byte ranges to UTF-16 LSP positions
with the round-1 `Converters` + the file's `DocumentScript` — the same pattern
as `folding.rs` / `symbols.rs`, taking `&self` (not `&mut self`).

## What landed

| File | Go source | What |
|---|---|---|
| `selectionranges.rs` | `internal/ls/selectionranges.go` | `provide_selection_ranges(file, positions) -> Vec<lsproto::SelectionRange>` (the `ProvideSelectionRanges` entry) + `Walk` (the `getSmartSelectionRange` traversal): `run`, `visit` (the `Visit` hook), `push_child_list_spans` (the `VisitNodes` hook), `push`/`push_comment` (`pushSelectionRange`/`pushSelectionCommentRange`), `node_contains_position`, `positions_are_on_same_line`, `should_skip_node`; free helpers `is_function_like_declaration`, `for_each_child_list` (+ `emit_list`/`emit_opt_list`) |
| `lib.rs` | — | `pub mod selectionranges;` |
| `selectionranges_test.rs` | — | 11 behavioral unit tests |
| `lsp/lsproto/generated.rs` | `lsp_generated.go:SelectionRange` | **new** `SelectionRange { range, parent: Option<Box<SelectionRange>> }` lsp_object (recursive, boxed parent) |
| `lsp/lsproto/generated_test.rs` | — | 2 roundtrip tests (nested chain; `parent` omitted when absent) |

Public API is **additive**: one new `pub fn` on `LanguageService`
(`provide_selection_ranges`), one new `pub` lsproto type (`SelectionRange`,
auto-exported via `pub use generated::*`), and the `pub mod` registration. No
existing public item changed; no test weakened or deleted.

### lsproto note (deviation from the brief)

The brief assumed `tsgo_lsproto` already had the `SelectionRange` type and said
to "reuse it". It did **not** — only the server-capability `SelectionRangeOptions`
/ `SelectionRangeRegistrationOptions` existed, not the result type. So the
result `SelectionRange` (Go `lsp_generated.go:SelectionRange`, `{ range, parent
}`) was added to `tsgo_lsproto::generated.rs` via the existing `lsp_object!`
macro (the `parent` is `Box`ed because the type is self-referential) plus two
roundtrip tests. This is the faithful 1:1 choice (the Go provider returns
`lsproto.SelectionRange`) and is why the changed-file set includes 2 lsproto
files in addition to the new `ls` file + `lib.rs` + this worklog.

## Architecture: two passes over one child stream

Go drives `current.VisitEachChild` with an `ast.NodeVisitor` carrying a per-node
`Visit` hook **and** a per-list `VisitNodes` hook (the latter pushes a stop
spanning each child node list's first→last element). `tsgo_ast`'s shared-borrow
surface exposes `for_each_child` (a flat, source-ordered child stream) but no
list-aware visitor that runs with only a shared borrow (`visitor.rs`'s
`visit_each_child` needs `&mut` to rebuild nodes). So this port splits the one
interleaved pass into two over the same children:

1. **`push_child_list_spans`** reproduces the `VisitNodes` hook via
   `for_each_child_list` — a read-only, list-aware mirror of `for_each_child`
   that emits only the node-list children (the ones Go routes through
   `v.visitNodes` / `v.visitParameters` / `v.visitTopLevelStatements`, all of
   which fall through to the `VisitNodes` hook). **Modifier lists are
   intentionally omitted**: Go routes them through `v.visitModifiers`, which
   does *not* call the `VisitNodes` hook (verified in `ast/visitor.go`).
2. A **`for_each_child`** pass reproduces the per-node `Visit` (the flat stream
   includes modifier elements + list elements, so `Visit` still fires for them).

Sibling nodes and list spans are disjoint, and a list span is always pushed
before the element it contains (Pass A before Pass B), so the resulting parent
chain is **identical** to Go's interleaved traversal (the only ordering
constraint that affects the chain is "list span before its element", which is
preserved).

## RED → GREEN slices (one behavior at a time)

1. **Headline — nested identifier chain.** `function f() {\n  return a + b;\n}`,
   position on `a` → `identifier (1,9)-(1,10)` → `binary expr (1,9)-(1,14)` →
   `return stmt (1,2)-(1,15)` → `function body block (0,13)-(2,1)` →
   `source file (0,0)-(2,1)`. RED: stub returned `[]`. GREEN: the core walk loop
   + `visit` (containing check, multi-line function-body block stop,
   `should_skip_node`, node stop, set next) + `push`/dedup +
   `positions_are_on_same_line`.
2. **String-literal inner vs outer.** `const s = "abc";`, position on the `b` →
   `inner content (0,11)-(0,14)` → `whole literal (0,10)-(0,15)` →
   `source file (0,0)-(0,16)` (the variable-statement/list/declaration stops are
   deduped/skipped). RED: chain stopped at the whole literal. GREEN: the
   `IsStringLiteral || TemplateExpression || NoSubstitutionTemplateLiteral`
   inner-content `[start+1, end-1)` stop.
3. **Guards (no panic).** empty file → the full-file `(0,0)-(0,0)` range; a
   position at the end boundary of `x;` → the full-file range; an unknown file →
   `[]`; no positions → `[]`. (Already green once the loop terminated safely —
   asserts no panic and the always-present full-file range.)

Then, to complete the faithful port (the remaining non-blocked special cases),
each as its own RED→GREEN slice / coverage test:

4. **Trailing single-line comment.** `const x = 1; // hi`, position in the
   comment → `comment content (0,15)-(0,18)` → `whole comment (0,13)-(0,18)` →
   `source file`. GREEN: `push_comment` (the whole comment + the content after
   the `//`) via `tsgo_scanner::get_trailing_comment_ranges`, run before the
   containing-position check (Go's `next == nil` order).
5. **Template-span `${ ... }` synthesized stop.** `` `a${b}c`; ``, position on
   `b` → `b (0,4)-(0,5)` → `${b} (0,2)-(0,6)` → `template inner (0,1)-(0,7)` →
   `whole template (0,0)-(0,8)` → `source file (0,0)-(0,9)`. GREEN: the
   `IsTemplateSpan(parent)` synthesis (`node.pos-2` .. `literal start +1`).
6. **List-span coverage** (the `VisitNodes` hook): parameter list
   (`function f(a, b) {}`, position on `a` → `param/ident (0,11)-(0,12)` →
   `param list (0,11)-(0,15)` → file) and import-specifier list
   (`import { a, b } from "m";`, position on `a` → `specifier (0,9)-(0,10)` →
   `specifier list (0,9)-(0,13)` → `import clause (0,7)-(0,15)` → file).
7. **One chain per position** (the public method maps over `positions`).

## Ported vs DEFERRED

**Ported (faithful 1:1 with `selectionranges.go`):**

- `ProvideSelectionRanges` (position → byte offset via `Converters`, one chain
  per position, `nil sourceFile` → empty).
- `getSmartSelectionRange`: the full-file initial range, the `current → next`
  ancestor descent, the `Visit` hook (`nodeContainsPosition`, multi-line
  function-body block stop, the `${ ... }` template-span synthesis, the
  `shouldSkipNode` gate, the node stop, the string/template inner-content stop,
  the trailing single-line-comment stops), the `VisitNodes` hook (per-list span,
  skipped under `VariableDeclarationList` / `TemplateExpression`),
  `pushSelectionRange` (empty-span / not-containing / equal-range-parent dedup),
  `pushSelectionCommentRange`, `positionsAreOnSameLine`, and `shouldSkipNode`
  (block / template head·middle·tail·span / `VariableDeclarationList` under a
  `VariableStatement` / lone `VariableDeclaration` under a single-declaration
  list).

**DEFERRED:**

- **JSDoc stops** — Go visits `current.JSDoc(sourceFile)` first and
  `shouldSkipNode` skips `JSDocTypeExpression` / `JSDocSignature` /
  `JSDocTypeLiteral`. Ported **structurally** (the `shouldSkipNode` JSDoc arm is
  present) but **inert**: the parser has not reparsed JSDoc, so no node carries
  cached JSDoc and the tree contains no JSDoc-kind nodes. The JSDoc-visit loop is
  a documented no-op. blocked-by: JSDoc reparser (`tsgo_parser`) — same
  `// DEFER(phase-3)` as `astnav`'s `node_jsdoc`.

## Extra behavioral tests (only MORE than Go, never fewer)

`internal/ls/selectionranges.go` has **no Go `*_test.go`** upstream (it is
covered by the `smartSelection_*` fourslash baselines, which are P10), so all 11
tests here are net-new behavioral coverage: nested-identifier chain, string
inner/outer, template-span synthesis, trailing comment, parameter-list span,
import-specifier-list span, multi-position, empty file, end boundary, unknown
file, and no-positions. Plus 2 `tsgo_lsproto` roundtrip tests for the new
`SelectionRange` type.

## Test deltas

- `tsgo_ls`: **133 → 144** unit tests (+11), all green (+ 1 doctest unchanged).
- `tsgo_lsproto`: **+2** roundtrip tests (`SelectionRange` nested / no-parent).

No existing test weakened or deleted.

## Gates (all GREEN)

```
cargo test  -p tsgo_ls                                   # 144 passed; 0 failed (+ doctest)
cargo test  -p tsgo_lsproto                              # all passed (incl. +2 SelectionRange)
cargo clippy -p tsgo_ls --all-targets -- -D warnings     # clean
cargo clippy -p tsgo_lsproto --all-targets -- -D warnings# clean
cargo fmt                                                # applied
cargo fmt -- --check                                     # clean
cargo build --workspace --all-targets                    # ok
```

This round adds the third purely-syntactic LS feature (after folding + document
symbols); it reuses the checker-free AST-walk + `Converters` pattern and the new
recursive `lsproto::SelectionRange` result type.
