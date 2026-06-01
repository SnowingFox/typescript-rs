# ls/change — Round 1 worklog (ChangeTracker + trivia-aware deletion)

> P7 `ls/change` round 1. Strict TDD (red→green vertical slices). Crate-scoped
> gates only (`-p tsgo_ls_change`); a concurrent lane was editing the `internal/ls/**`
> ROOT crate (definitions/references) at the same time, so this round touched
> **only** `internal/ls/change/**` + this doc. No root `Cargo.toml` edit (the
> crate was already registered); deps declared in `internal/ls/change/Cargo.toml`.

## Scope decision

`internal/ls/change` is the language-service `ChangeTracker`: it accumulates
insert/delete/replace edits over the AST (tracking leading/trailing trivia so a
deleted node removes its whitespace/comments cleanly) and finally computes the
`[]TextChange` edits that back every code fix and refactor. Go has 3 non-test
files (`tracker.go` / `trackerimpl.go` / `delete.go`) and **no** `*_test.go`, so
every test here is a new behavior-level test (asserting the exact produced edit
span + text, hand-derived from Go's algorithm).

After reading all three Go files **and** the current state of the dependency
crates, two large sub-systems were found to be blocked and were deferred (see
the DEFER list); the genuinely reachable, fully-testable core the task
emphasizes was ported this round:

- the **edit accumulator** (`ChangeTracker`) with `replace_range_with_text` /
  `insert_text` / `delete_range` / `delete_node` / `delete_node_range` /
  `delete`, and `get_changes` (sort by `(pos, end)`, assert non-overlap, emit
  `TextChange`s) — `tracker.go` + `trackerimpl.go:getTextChangesFromChanges`;
- the **trivia-aware range adjustment** `get_adjusted_range` /
  `get_adjusted_start_position` / `get_adjusted_end_position` /
  `get_end_position_of_multiline_trailing_comment` — `trackerimpl.go`;
- the **trivia-aware node deletion** dispatcher `delete_declaration` +
  `delete_variable_declaration` + the core `delete_node` helper +
  `start_position_to_delete_node_in_list` + `positions_are_on_same_line` —
  `delete.go`.

This is the spine every code fix / refactor edit derives from: a fix builds an
edited AST or a literal span, hands it to the tracker, and `get_changes` returns
the sorted, non-overlapping `TextChange` list.

## Deliberate, documented divergences

1. **Edits are byte-offset `core::TextChange`, not `lsproto.TextEdit`.** Go's
   `trackerEdit` embeds an `lsproto.Range` (0-based line / UTF-16 character) and
   converts byte offsets at every boundary via a `lsconv.Converters`; the final
   output is `map[string][]*lsproto.TextEdit`. This port keeps the tracker
   entirely in the compiler's native UTF-8 byte offsets (`core::TextRange`) and
   emits `core::TextChange` (`range` + `new_text`) — exactly the "span + new
   text" the slices assert. The ordering/overlap logic is preserved 1:1 (sort by
   `(pos, end)`; on a tie the shorter range first; panic on a true overlap, i.e.
   `changes[i].end > changes[i+1].pos`). The line/character conversion is a thin
   boundary wrapper deferred to the `ls` root integration. blocked-by: a per-file
   `lsconv::Converters` / line-map plumbed through the program.
2. **Source file = `tsgo_astnav::RcSourceFile`.** Go threads a `*ast.SourceFile`
   through every method; this port uses the committed shared-borrow navigation
   surface. Node ranges/kinds come from the nav context (`pos`/`end`/`kind`/
   `find_child_of_kind`); node payloads (parent, statement list, declaration
   list, file name) from its `arena()`. The deletion queue stores
   `Rc<RcSourceFile>` (standing in for Go's `*ast.SourceFile` pointer) so the
   deferred `finish_delete_declarations` can revisit the file; contained-node
   detection uses `Rc::ptr_eq` for the "same file" check.
3. **`format::GetLineStartPositionForPosition` reimplemented locally.** It is
   `pub` but lives in `tsgo_format`'s private `util` module, so it is reproduced
   over `core::compute_ecma_line_starts` + `scanner::compute_line_of_position`
   (a 2-line helper). The line break test `IsLineBreak(rune(text[newEnd-1]))`
   is reproduced byte-wise (`text.as_bytes()[i] as char`), matching Go's
   `rune(text[i])` (which only recognizes `\n`/`\r` here, not `U+2028/2029`).

## RED → GREEN slices (each asserts the exact produced edit)

| # | Behavior | Observed edit / result | Go anchor |
|---|---|---|---|
| 1 | `replace_range_with_text` of a node span | one `TextChange{(6,7), "y"}` → `const y = 1;` | `tracker.go:ReplaceRangeWithText` |
| 2 | `insert_text` at an offset | `TextChange{(1,1), "X"}` → `aXb` | `tracker.go:InsertText` |
| 3 | `delete_range` | `TextChange{(2,4), ""}` → `abef` | `tracker.go:DeleteRange` |
| 4 | `get_changes` sorts + non-overlap + applies | reorders to `(0,2)(5,5)(8,10)` → `YcdeXfgh`; panics on overlap `(0,5)/(3,8)`; allows touching `(0,3)/(3,6)` | `trackerimpl.go:getTextChangesFromChanges` |
| 5 | `get_adjusted_end_position` Include consumes trailing `\n` | end `12 → 13`; multiline `/* … */` → 13; `delete_node` removes `const b = 2;\n` | `trackerimpl.go:getAdjustedEndPosition` |
| 6 | `get_adjusted_start_position` Exclude / StartLine / IncludeAll | `2` / `13` / IncludeAll pulls to comment line `13` vs StartLine keeps comment `22` | `trackerimpl.go:getAdjustedStartPosition` |
| 7 | `delete_node` of a statement in a list | removes `const b = 2;\n` cleanly | `tracker.go:DeleteNode` |
| 8 | smart `delete()` → `finish_delete_declarations` | delete `b` from `const a=1; const b=2; const c=3;` → `const a = 1; const c = 3;`; preserves a leading comment via StartLine; contained-node skipped | `delete.go:deleteDeclaration` / `deleteVariableDeclaration`; `tracker.go:finishDeleteDeclarations` |

`ExcludeWhitespace` (keeps a trailing comment, drops the newline → end `8`),
`get_adjusted_range` composition, `positions_are_on_same_line`, multi-file edit
keying, and `start_position_to_delete_node_in_list` (skips leading whitespace →
`2`) have dedicated tests too.

## Go functions mirrored (`// Go:` anchors)

- `tracker.go`: `NewTracker`, `GetChanges`, `ReplaceRangeWithText`, `InsertText`,
  `DeleteRange`, `DeleteNode`, `DeleteNodeRange`, `Delete`,
  `finishDeleteDeclarations`, `rangeContainsRangeExclusive`, `LeadingTriviaOption`,
  `TrailingTriviaOption`, `trackerEdit`, `deletedNode`.
- `trackerimpl.go`: `getTextChangesFromChanges`, `GetAdjustedRange`,
  `getAdjustedStartPosition`, `getAdjustedEndPosition`,
  `getEndPositionOfMultilineTrailingComment`.
- `delete.go`: `deleteDeclaration`, `deleteVariableDeclaration`, `deleteNode`,
  `startPositionToDeleteNodeInList`, `positionsAreOnSameLine`, `hasJSDocNodes`.

## Test delta

Crate started at 0 tests. Now: **27 unit tests + 1 doctest**, all green
(`tracker_test.rs` 10, `trackerimpl_test.rs` 10, `delete_test.rs` 7). Every
reachable `pub`/`pub(crate)` function has at least one behavior-level test.

## Gate results (crate-scoped; concurrent lane active, so no `--workspace`)

- `cargo test -p tsgo_ls_change` → **27 passed; 0 failed** + doc-test **1 passed**.
- `cargo clippy -p tsgo_ls_change --all-targets -- -D warnings` → clean.
- `cargo fmt -p tsgo_ls_change -- --check` → clean.
- `cargo build -p tsgo_ls_change` → ok.

## Public API (additive, within `tsgo_ls_change`)

`ChangeTracker` (`new`, `new_line`, `replace_range_with_text`, `insert_text`,
`delete_range`, `delete_node`, `delete_node_range`, `delete`, `get_changes`) +
the `LeadingTriviaOption` / `TrailingTriviaOption` enums. No other crate's source
was touched; the root `Cargo.toml` was not edited.

## DEFER list (blocked-by)

- **Format-on-insert (the whole node-insertion / node-replacement surface).**
  `ReplaceNode`, `ReplaceNodeWithNodes`, `ReplaceRange`/`ReplaceRangeWithNodes`
  with a node, `InsertNodeAt`/`Before`/`After`/`InsertNodesAt`/`After`,
  `InsertNodeInListAfter`, `InsertImportSpecifierAtIndex`, `InsertAtTopOfFile`,
  `InsertMemberAtStart`, `insertNodeAtStartWorker`, `TryInsertTypeAnnotation`,
  `ParenthesizeArrowParameters`, `InsertModifierBefore`,
  `finishNodesWithInsertionsAtStart`, and `computeNewText` /
  `getFormattedTextOfNode` / `getNonformattedText` / `getFormatCodeSettingsForWriting`.
  They reformat the inserted node text through `printer.ChangeTrackerWriter` +
  `format.FormatNodeGivenIndentation`. blocked-by: `printer.ChangeTrackerWriter`
  / `format::format_node_given_indentation` (not ported). The node-bearing
  `trackerEditKind` variants and `NodeOptions.indentation/delta/joiner` are not
  represented; only text edits are produced.
- **`deleteNodeInList` + trailing-comma fixup.** They call
  `format.GetContainingList`, which the `tsgo_format` port stubs to `None` (its
  list detection — `getListByRange` — is deferred). So comma-separated list
  deletion (parameters, type parameters, import specifiers, binding elements,
  multi-declaration variable lists, call arguments) and the
  last-in-list trailing-comma cleanup in `finishDeleteDeclarations` are deferred;
  those `deleteDeclaration` arms collapse into the generic whole-node deletion.
  `start_position_to_delete_node_in_list` / `positions_are_on_same_line` are
  ported (and tested) but currently reached only by these deferred paths.
  blocked-by: `format::indent::get_containing_list`.
- **Import-specific deletion.** `deleteImportBinding`, `deleteDefaultImport`,
  `sourceFile.Imports()`, and the `ImportDeclaration`/`ImportEquals`/
  `NamespaceImport`/`ImportSpecifier`/`TypeKeyword`-in-import arms need `ast`
  import-clause accessors not yet ported. blocked-by: `ast` import accessors.
- **`for-of`/`for-in` variable rebinding.** `deleteVariableDeclaration` replaces
  the binding with `{}` via the deferred node-insertion path; this port deletes
  the declaration list instead. blocked-by: node-insertion (above).
- **JSDoc-aware leading trivia.** The parser has not reparsed JSDoc, so
  `has_jsdoc_nodes` is always `false` and `LeadingTriviaOption::JSDoc` /
  `getAdjustedStartPosition`'s JSDoc branch are inert. blocked-by: JSDoc reparser
  (`tsgo_parser`).
- **`endPosForInsertNodeAfter` / `needSemicolonBetween` / option helpers**
  (`getInsertNodeAfterOptions`, `getOptionsForInsertNodeBefore`,
  `getInsertNodeAtStartInsertOptions`, `getInsertionPositionAtSourceFileTop`,
  indentation helpers `tryComputeIndentation*` / `findIndentationColumn`). These
  feed the deferred node-insertion path and/or need statement predicates
  (`ast.IsStatement`, `ast.IsPrologueDirective`, …) not yet ported. blocked-by:
  node-insertion + `ast` statement predicates.
- **`isContained`/checker-program ops** such as autoimport insertion live in
  `ls/autoimport`, out of scope here.
