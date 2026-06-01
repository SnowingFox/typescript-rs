# ls — Round 3 worklog (LS root: rename + document highlights)

> P7 `ls` (root crate `tsgo_ls`) round 3. Strict TDD (red→green vertical slices).
> Crate-scoped gates only (`-p tsgo_ls`). A concurrent lane was editing the
> separate `tsgo_ls_change` crate (`internal/ls/change/**`), disjoint from this
> lane, so this round touched **only** `internal/ls/**` root-crate files (NOT the
> `lsconv`/`lsutil`/`change`/`autoimport` sub-crates) + this doc. No root
> `Cargo.toml` edit, no `internal/ls/Cargo.toml` edit, and **no new dependency on
> `tsgo_ls_change`** — rename / document-highlights need only `ast` + `astnav` +
> `checker` + `core` + `diagnostics` + `locale` + `lsproto` + `lsconv`, all
> already in `internal/ls/Cargo.toml`. No other crate's source was touched.

This round adds the next two LS feature providers on top of round 2's
find-all-references core (`same_symbol_reference_nodes`): **rename**
(`provide_rename_locations` + `get_rename_info`) and **document highlights**
(`provide_document_highlights`), both the reachable single-file subset. Both
build directly on the references symbol-walk.

## What landed

| File | Go source | What |
|---|---|---|
| `rename.rs` | `internal/ls/rename.go` | `provide_rename_locations` (the `ProvideRename`/`symbolAndEntriesToRename`/`getRenameLocations` single-file subset) → `Vec<lsproto::Location>`; `get_rename_info` (the `GetRenameInfo`/`getRenameInfoForNode` reachable subset) → `RenameInfo`; `node_is_eligible_for_rename` (reachable subset of `nodeIsEligibleForRename`); `rename_info_error` (`getRenameInfoError`); the `RenameInfo` data record (`ls.RenameInfo`) |
| `documenthighlights.rs` | `internal/ls/documenthighlights.go` | `provide_document_highlights` (the `ProvideDocumentHighlights`/`getSemanticDocumentHighlights`/`toDocumentHighlight` single-file subset) → `Vec<DocumentHighlight>`; the reachable subset of `ast.IsWriteAccessForReference` / `GetDeclarationFromName` / `declarationIsWriteAccess` / `IsWriteAccess` / `accessKind`; the `DocumentHighlight` + `DocumentHighlightKind` LSP-shaped types |
| `references.rs` | `internal/ls/findallreferences.go` | refactor: extracted `same_symbol_reference_nodes` (`pub(crate)`) — the shared same-symbol identifier-node walk — and made `reference_ranges` `pub(crate)`; `provide_references` output is unchanged |
| `definition.rs` | `internal/ls/definition.go` | `name_of_declaration` made `pub(crate)` for reuse by `GetDeclarationFromName` |
| `rename_test.rs` | — | 9 unit tests |
| `documenthighlights_test.rs` | — | 7 unit tests |

Public API is **additive within `tsgo_ls`**: three new `pub fn` on
`LanguageService` (`provide_rename_locations`, `get_rename_info`,
`provide_document_highlights`), three new public types (`RenameInfo`,
`DocumentHighlight`, `DocumentHighlightKind`), and the
`pub mod rename; pub mod documenthighlights;` registrations with their re-exports.
`reference_ranges` / `same_symbol_reference_nodes` / `name_of_declaration` are
crate-internal (`pub(crate)`), not public API. No existing public item changed.

## Architecture: reuse of the round-2 references walk

Both features reuse round 2's `same_symbol_reference_nodes(ctx, position)` — the
walk that resolves the touched identifier to a symbol and collects every
same-symbol identifier node in the file (in source order, deduped by range).
`reference_ranges` is now a thin range-mapping wrapper over it, so
`provide_references` is unchanged.

- **Rename** = "the same-symbol references, gated on rename eligibility". Go's
  `symbolAndEntriesToRename` only emits edits when `nodeIsEligibleForRename` and
  `getRenameInfoForNode().CanRename`; `provide_rename_locations` mirrors that:
  it returns `[]` unless `rename_target` (the eligibility + symbol-with-a-
  declaration check) succeeds, then returns `reference_ranges` mapped to
  `lsproto::Location`. `get_rename_info` shares `rename_target` and builds the
  trigger span from the node (Go's `getRenameInfoSuccess`:
  `[GetStartOfNode(node,false), node.End())`) plus the symbol's printed name
  (`symbol_to_string`).
- **Document highlights** = "the same-symbol occurrences in the file, each tagged
  Read/Write". `provide_document_highlights` maps each node from
  `same_symbol_reference_nodes` to a `DocumentHighlight { range, kind }`, where
  `kind` is `Write` when the reachable subset of `IsWriteAccessForReference` holds
  (a write-access declaration name via `GetDeclarationFromName` +
  `declarationIsWriteAccess`, or a syntactic write via `accessKind`), else `Read`.

## RED → GREEN slices (observed symptoms)

1. **Rename locations: `const x = 1; x; x;`, rename at the declaration → 3
   locations (decl + 2 uses).** RED (tracer): stub returned `[]`; assertion left
   `[]` vs right 3 `Location`s. GREEN: gate on `rename_target`, then map
   `reference_ranges` → `(0,6)..(0,7)`, `(0,13)..(0,14)`, `(0,16)..(0,17)`.
2. **Rename respects shadowing: `const x=1; function f(){ const x=2; x; } x;`,
   rename at the inner `x` → only the inner decl + inner use
   (`(0,31)..(0,32)`, `(0,36)..(0,37)`).** Inherited from the scope-aware
   references walk; passed on first run of the new test.
3. **`get_rename_info`: keyword position (`const` at char 0) → cannot-rename with
   the localized "You cannot rename this element." message; identifier position →
   `can_rename` + trigger span `(0,6)..(0,7)` + display name `"x"`.** Shipped with
   `provide_rename_locations` (they share `rename_target`); the keyword case
   exercises `node_is_eligible_for_rename` rejecting a non-identifier and
   `rename_info_error`'s `Message::localize`, the identifier case exercises
   `getRenameInfoSuccess` (span + `symbol_to_string`).
4. **Document highlights: `let x = 1; x = 2; x;`, highlight at the declaration →
   `Write(0,4..5)`, `Write(0,11..12)`, `Read(0,18..19)`.** RED (tracer): stub
   returned `[]`; assertion left `[]` vs right 3 `DocumentHighlight`s. GREEN: the
   decl is a write-access `VariableDeclaration` (has an initializer), `x = 2` is a
   `=` assignment target (`accessKind` → `Write`), and the trailing `x;` is a
   `Read`.
5. **Highlights are single-file + same-symbol only: `const x=1; function f(){
   const x=2; x; } x;`, highlight at the inner `x` → only the inner decl
   (`Write(0,31..32)`) + inner use (`Read(0,36..37)`), never the outer `x`.**
   Confirms the scope-aware walk plus per-node read/write classification.

## Extra behavioral tests (only MORE than Go, never fewer)

- Rename: function across call sites (`function f(){}\nf();\nf();` → 3); rename on
  a keyword → empty; rename on an unknown file → empty; `get_rename_info` from a
  *use* position spans the use token; `get_rename_info` on an unknown file →
  cannot-rename.
- Document highlights: querying from a *use* still marks the declaration `Write`;
  a compound assignment (`x += 1`) target is `Write` (Go's `accessKind` →
  `ReadWrite`, and `IsWriteAccess` is `!= Read`); a postfix increment (`x++`) is
  `Write`; highlight on a keyword → empty; highlight on an unknown file → empty.

## Go functions mirrored (`// Go:` anchors)

- `rename.go:LanguageService.ProvideRename` / `symbolAndEntriesToRename` (the
  eligibility-gated reference→edit mapping), `getRenameLocations` (flat-map of
  `getReferencedSymbolsForNode`), `GetRenameInfo` / `getRenameInfoForNode`
  (symbol + declaration check), `nodeIsEligibleForRename`, `getRenameInfoSuccess`
  (trigger span + `symbolToString`), and `getRenameInfoError` (`Message.Localize`).
- `documenthighlights.go:LanguageService.ProvideDocumentHighlights` /
  `provideDocumentHighlightsWorker`, `getSemanticDocumentHighlights` (the
  references-driven per-file grouping), and `toDocumentHighlight` (the
  `IsWriteAccessForReference` → `Write`/`Read` tagging).
- `ast/ast.go:IsWriteAccessForReference`, `GetDeclarationFromName`,
  `declarationIsWriteAccess`, `IsWriteAccess`, `accessKind` (reachable subsets,
  ported locally into `documenthighlights.rs` because `tsgo_ast` is owned by a
  different crate/lane), and `ast_generated.go:IsAssignmentOperator`
  (reused from `tsgo_ast::utilities`).
- `lsp_generated.go:DocumentHighlight` / `DocumentHighlightKind` (the LSP shape +
  the `Text=1`/`Read=2`/`Write=3` wire values), defined locally in
  `documenthighlights.rs` (see DEFER below).
- `findallreferences.go:getReferencedSymbolsForNode` (the shared
  `same_symbol_reference_nodes` walk), reused from round 2.

## Test deltas

Crate was at **26** unit tests (round 2). Now **42** unit tests (+0 doctests),
all green:

- `rename_test.rs` — 9 (decl+uses, shadowing, function call sites, keyword empty,
  unknown-file empty; `get_rename_info` accept/from-use/reject-keyword/
  unknown-file).
- `documenthighlights_test.rs` — 7 (write/read classification, shadowing,
  from-a-use, compound `+=`, postfix `++`, keyword empty, unknown-file empty).

No existing test was weakened or deleted (rule 5); every new `pub fn` has a
behavioral test plus empty/edge coverage.

## Gates (crate-scoped, all GREEN)

```
cargo test  -p tsgo_ls                               # 42 passed; 0 failed (+ 0 doctests)
cargo clippy -p tsgo_ls --all-targets -- -D warnings # clean
cargo fmt   -p tsgo_ls -- --check                    # clean
cargo build -p tsgo_ls                               # ok
```

(`--workspace` was intentionally not run — concurrent `tsgo_ls_change` lane
active.)

## DEFER list (blocked-by → future ls rounds)

- **`lsproto::WorkspaceEdit` / `TextEdit` assembly** — Go's `ProvideRename`
  returns a `lsproto.WorkspaceEditOrNull` whose `Changes` map groups one
  `TextEdit{Range, NewText}` per reference by document URI, with the replacement
  text computed by `getTextForRename`. The reachable subset returns the **location
  list** instead. blocked-by: `tsgo_lsproto` does not yet carry a generated
  `WorkspaceEdit` (only the `WorkspaceEdit` *client-capability* types exist), and
  this lane may not edit the `lsproto` crate.
- **Local `DocumentHighlight` / `DocumentHighlightKind`** — defined in
  `documenthighlights.rs` (matching `lsproto.DocumentHighlightKind`'s wire values
  `Text=1`/`Read=2`/`Write=3`). They should be hoisted into `tsgo_lsproto` once
  that crate gains the generated types. blocked-by: `tsgo_lsproto` is owned by a
  different crate/lane (not editable here) and has not yet ported these types.
- **Rename text edits / prefix-suffix rename** — `getTextForRename`
  (shorthand-property `name: new`, import/export `name as new`, numeric index
  quoting) and the quote-preference surface. blocked-by: `UserPreferences` /
  `GetQuotePreference` and the object-literal / import-export reference kinds.
- **Cross-file rename + multi-document highlights** — Go's cross-project
  orchestrator (`handleCrossProject`) + the program-wide reference search, and
  `ProvideMultiDocumentHighlights` over `filesToSearch`. blocked-by: a
  `compiler.Program`-level multi-file symbol resolver + symbol scope.
- **Rename-blocked reasons** — `renameBlockedReason` (library-file
  `isDefinedInLibraryFile`, `default`-import, `node_modules`
  `wouldRenameInOtherNodeModules`) and `getRenameInfoForModule` (module-specifier
  / file rename, `FileToRename`/`NewFileName`). Not reachable single-file without
  a default lib; blocked-by: `IsSourceFileDefaultLibrary` + the alias/module
  resolver + `ClientSupportsRenameResourceOperations`.
- **String-literal / `this` / numeric-property rename + `getAdjustedLocation`** —
  the non-identifier eligibility kinds and the keyword/modifier trigger
  adjustment (so a rename on `const`/`function`/a modifier retargets the name).
  blocked-by: `GetContextualType` (string-literal types) and the
  `getAdjustedLocation*` helper family.
- **Syntactic document highlights + JSX tags** — `getSyntacticDocumentHighlights`
  (`if`/`else`, `return`/`throw`, `try`/`catch`/`finally`, loop `break`/`continue`,
  `switch` `case`/`default`, accessors, `async`/`await`/`yield`, modifier
  occurrences) and the JSX opening/closing-tag pairing. blocked-by: the keyword
  occurrence aggregators (`getReturnOccurrences` etc.) and JSX element navigation.
- **Read/Write classification fine cases + `Text` kind** — the deferred
  `accessKind` arms (parenthesized, property-access/assignment,
  shorthand-property, array-literal destructuring, for-in/of initializer),
  `declarationIsWriteAccess` for accessors / properties / binding elements / enum
  members / modules / ambient, and the `DocumentHighlightKind::Text` range-entry
  kind (string-literal references). blocked-by: the destructuring-pattern
  classifier and the string-literal reference machinery.

This round extends the LS root with rename + document highlights; both reuse the
round-2 `same_symbol_reference_nodes` symbol-walk and the round-1 `Converters`
position conversion, establishing the eligibility-gated rename + per-occurrence
read/write classification that later cross-file rename / multi-document highlight
rounds will build on.
