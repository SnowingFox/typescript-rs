# ls — worklog (syntactic structural LS: folding ranges + document symbols/navbar)

> P7 `ls` (root crate `tsgo_ls`). Strict TDD (red→green vertical slices).
> Crate-scoped gates only (`-p tsgo_ls`). A concurrent lane was editing
> `internal/execute/**` (watch mode), disjoint from this lane, so this round
> touched **only** `internal/ls/**` root-crate files (NOT the
> `lsconv`/`lsutil`/`change`/`autoimport` sub-crates) + this doc + the
> `internal/ls/Cargo.toml` dep list. No root `Cargo.toml` edit, no other crate's
> source touched.

This round adds the **syntactic structural** LS features — they walk the AST and
need no checker, only the program's already-parsed source file (its `NodeArena` +
root):

- **Folding ranges** (`folding.rs`) — `textDocument/foldingRange`.
- **Document symbols / navbar** (`symbols.rs`) — `textDocument/documentSymbol`,
  the hierarchical navigation tree.

> Naming note: the task brief referred to `navbar.go` / `outliningelements.go`,
> but upstream `microsoft/typescript-go` has **no such files** — the document
> symbol tree lives in `internal/ls/symbols.go` (`ProvideDocumentSymbols`) and the
> outlining/folding logic lives in `internal/ls/folding.go`. Per the 1:1
> file-naming discipline (PORTING §2), the Rust ports are named `symbols.rs` and
> `folding.rs` to match their Go sources.

## What landed

| File | Go source | What |
|---|---|---|
| `folding.rs` | `internal/ls/folding.go` | `provide_folding_ranges` (the `ProvideFoldingRange`/`addNodeOutliningSpans`/`visitNode`/`getOutliningSpanForNode` reachable subset) → `Vec<lsproto::FoldingRange>`; span helpers `function_span`/`try_get_function_open_token`/`is_node_array_multi_line`/`span_for_node`/`span_for_node_array`/`range_between_tokens`; comment folds (`add_leading_comments_for_node`/`add_leading_comments_for_pos`/`combine_single_line_comments`/`is_region_delimiter`); predicates `is_comment_owner`/`is_function_like`/`is_declaration_kind`/`parameters_of`; `positions_are_on_same_line` via `tsgo_core::compute_ecma_line_starts` + `tsgo_scanner::compute_line_of_position` |
| `symbols.rs` | `internal/ls/symbols.go` | `provide_document_symbols` (the `ProvideDocumentSymbols`/`getDocumentSymbolsForChildren`/`visit`/`newDocumentSymbol`/`getSymbolKindFromNode`/`mergeExpandos` reachable subset) → `Vec<DocumentSymbol>`; the local `DocumentSymbol` LSP-shaped type; helpers `get_symbols_for_children`/`add_symbol_for_node`/`merge_expandos`/`merge_children`/`compare_ranges`/`is_anonymous_name`/`get_interior_module`/`get_module_name`/`is_ambient_module`/`get_text_of_name`/`get_unnamed_node_label`/`truncate_by_runes`/`name_of_declaration`/`node_name`/`initializer_of`/`body_of`/`parameters_of`/`is_parameter_property`/`has_default_modifier` |
| `lib.rs` | `internal/ls/folding.go`, `symbols.go` | `pub mod folding; pub mod symbols;` + `pub use symbols::DocumentSymbol;` |
| `Cargo.toml` | — | added `tsgo_scanner` dep (comment ranges + line-of-position + skip-trivia) |
| `folding_test.rs` | — | 9 unit tests |
| `symbols_test.rs` | — | 10 unit tests |

Public API is **additive within `tsgo_ls`**: two new `pub fn` on `LanguageService`
(`provide_folding_ranges`, `provide_document_symbols`), one new public type
(`DocumentSymbol`), and the `pub mod folding; pub mod symbols;` registrations with
the `DocumentSymbol` re-export. No existing public item changed; no existing test
weakened or deleted.

## Architecture: syntactic, checker-free

Both features take `&self` (not `&mut self`): unlike hover/definition/references/
rename/highlights (which build a checker via `file_check_context`), folding and
document symbols only need the **raw parsed file**. They reach it through
`self.program().get_source_file(file_name)` → `ParsedFile::{arena, node, text}`
(parent pointers are set by the parser's `override_parent_in_immediate_children`,
so block-parent / literal-parent dispatch works on the unbound arena), build a
shared-borrow `NavSourceFile`, walk it, and convert byte ranges to UTF-16 LSP
positions with the round-1 `Converters` + the file's `DocumentScript`.

## RED → GREEN slices (observed symptoms)

**Folding** (`provide_folding_ranges`):

1. **Function block: `function f() {\n  return 1;\n}` → one range covering the
   body.** RED (tracer): `collect_outlining_spans` stub returned `[]`; assertion
   left `[]` vs right `[FoldingRange{ start (0,12), end (2,1) }]`. GREEN: implement
   the statement walk + `visit_node` + `getOutliningSpanForNode`(Block→`function_span`)
   → `{` full-start (byte 12) .. `}` end (byte 28 = line 2, char 1).
2. **Object literal: `const o = {\n a: 1,\n b: 2\n};` → object body fold
   `(0,9)-(3,1)`.** Added the `ObjectLiteralExpression` case (`span_for_node`,
   `useFullStart = !parent_is_array_or_call`); passed on first run.
3. **Array literal `const a = [\n 1,\n 2\n];` → `(0,9)-(3,1)`**; **class +
   method `class C {\n  m() {}\n}` → `[(0,7)-(2,1), (1,5)-(1,8)]`** (class body +
   nested method body, sorted by start). Exercises bracket spans + the class-body
   span + nested `function_span` + the `(startLine, startChar)` sort.
4. **Multi-line comment `/* a\n b */` → one `comment`-kind fold `(0,0)-(1,5)`**
   (the EOF token's leading trivia); **two consecutive `//` comments →
   one combined `comment` fold `(0,0)-(1,4)`** before a function body fold;
   **a single `//` is NOT folded**. Exercises `add_leading_comments_for_pos`
   (multi-line + the `count > 1` single-line combine).
5. **Empty file → `[]`; unknown file → `[]`** (no panic).

**Document symbols** (`provide_document_symbols`):

1. **`function f(){}\nclass C { m(){} x = 1; }` → `f`(Function), `C`(Class) with
   children `m`(Method), `x`(Property).** RED (tracer):
   `get_document_symbols_for_children` stub returned `[]`; assertion left `[]` vs
   right the 4-node pre-order outline. GREEN: implement `visit` (class / function /
   method / property cases) + `get_symbols_for_children` + `new_document_symbol` +
   `get_symbol_kind_from_node` + `merge_expandos`.
2. **`function foo() {}` → range = whole decl `(0,0)-(0,17)`, selectionRange =
   the name `foo` `(0,9)-(0,12)`.** Exercises `newDocumentSymbol`'s `node_start_pos`
   (`skip_trivia`) vs name-pos arithmetic.
3. **Empty file → `[]`; unknown file → `[]`** (no panic).

(`merge_children` initially failed to compile — `lsproto::Range` is not `Copy`, so
`compare_ranges` was changed to take `&Range`; a true red→green compile fix.)

## Extra behavioral tests (only MORE than Go, never fewer)

`internal/ls/symbols.go` / `folding.go` carry **no Go `*_test.go`** in upstream, so
every test here is net-new behavioral coverage:

- Folding: array literal, class+method (multi-fold + sort), multi-line comment,
  consecutive single-line combine, single-comment-not-folded, empty, unknown-file.
- Document symbols: top-level variables; enum + enum members; interface + method/
  property signatures; namespace + nested function; **two same-name namespaces
  merging into one** (exercises `merge_expandos` + `merge_children`); object-literal
  initializer producing nested property + method children; range-vs-selectionRange;
  empty; unknown-file.

## Go functions mirrored (`// Go:` anchors)

- `folding.go:ProvideFoldingRange` (entry + `(startLine, startCharacter)` sort),
  `addNodeOutliningSpans` (statement walk + EOF visit), `visitNode`,
  `getOutliningSpanForNode` (Block→`functionSpan`/attached/standalone, ModuleBlock,
  class/interface/enum/CaseBlock/TypeLiteral/ObjectBindingPattern, object/array
  literal, ArrayBindingPattern, case/default clause), `functionSpan`,
  `tryGetFunctionOpenToken`, `isNodeArrayMultiLine`, `spanForNode`,
  `spanForNodeArray`, `rangeBetweenTokens`, `createLspRangeFromNode`,
  `addOutliningForLeadingCommentsForNode`/`ForPos`,
  `combineAndAddMultipleSingleLineComments`, `parseRegionDelimiter` (recognition
  only).
- `symbols.go:ProvideDocumentSymbols` / `getDocumentSymbolsForChildren` (the
  `visit` dispatch), `newDocumentSymbol`, `getSymbolKindFromNode`, `mergeExpandos`
  / `mergeChildren` / `isAnonymousName`, `getInteriorModule`, `getModuleName`,
  `getTextOfName`, `getUnnamedNodeLabel`.
- `ast/utilities.go:IsFunctionLike` / `IsDeclarationKind` / `IsBindingPattern` /
  `IsParameterPropertyDeclaration` / `GetNameOfDeclaration` / `IsAmbientModule`
  (reachable subsets, ported locally because `tsgo_ast` is owned by a different
  crate/lane), `printer/utilities.go:PositionsAreOnSameLine`,
  `stringutil:TruncateByRunes`, `lsproto/util.go:CompareRanges`.
- `lsp_generated.go:DocumentSymbol` — defined locally in `symbols.rs` (see DEFER).

## Test deltas

Crate was at **50** unit tests. Now **69** unit tests (+0 doctests), all green:

- `folding_test.rs` — 9.
- `symbols_test.rs` — 10.

No existing test was weakened or deleted (rule 5); every new `pub fn` has a
behavioral test plus empty/edge coverage.

## Gates (crate-scoped, all GREEN)

```
cargo test  -p tsgo_ls                                # 69 passed; 0 failed (+ 0 doctests)
cargo clippy -p tsgo_ls --all-targets -- -D warnings  # clean
cargo fmt   -p tsgo_ls -- --check                     # clean
cargo build -p tsgo_ls                                # ok
```

(`--workspace` was intentionally not run — concurrent `internal/execute/**` lane
active.)

## DEFER list (blocked-by → future ls rounds)

- **`lsproto::DocumentSymbol`** — `tsgo_lsproto` has only the
  `DocumentSymbolOptions` server-capability type, not the result `DocumentSymbol`.
  The LSP shape is defined locally in `symbols.rs` (name / kind / range /
  selectionRange / children) and should be hoisted into `tsgo_lsproto` once that
  crate gains it. blocked-by: `tsgo_lsproto` owned by a different crate/lane (not
  editable here); also `detail`/`tags`/`deprecated` fields are omitted.
- **Flat `SymbolInformation` fallback** — `getDocumentSymbolInformations` (for
  clients without `HierarchicalDocumentSymbolSupport`). blocked-by:
  `GetClientCapabilities` + a generated `lsproto::SymbolInformation`.
- **Folding `lineFoldingOnly` / `collapsedText`** — `adjustFoldingEnd`
  (subtract 1 from `endLine` on a closing pair when the client signals
  `lineFoldingOnly`) and `supportsCollapsedText` (region/JSX banner text).
  blocked-by: the `GetClientCapabilities` folding-capability surface.
- **`//#region` / `//#endregion` named regions** — `addRegionOutliningSpans`
  (`parseRegionDelimiter` is ported for the comment-skip guard, but the region
  *fold* itself is deferred). blocked-by: nothing structural — a focused follow-up
  (needs `scanner::GetECMALineStarts` + `isInComment` + region collapse text).
- **Import-group fold** — the consecutive-`IsAnyImportSyntax` run in
  `addNodeOutliningSpans` that emits one `imports` fold. blocked-by: `IsAnyImportSyntax`
  + `FindChildOfKind(ImportKeyword)` plumbing (a small follow-up).
- **JSX / template-literal / call / parenthesized / named-import-or-export folds**
  — `spanForJSXElement`/`spanForJSXAttributes`/`spanForTemplateLiteral`/
  `spanForCallExpression`/`spanForParenthesizedExpression`/`spanForArrowFunction`/
  `spanForImportExportElements`. blocked-by: the JSX element/template span helpers
  (some need `positions_are_on_same_line`, already available).
- **Document-symbol JS expando + assignment declarations** — the
  `getAssignmentDeclarationKind` arms in `visit` (`A.b = ...`, `A.prototype.b`,
  `Object.defineProperty`, `module.exports`, `exports.x`) and the expando-property
  merge in `mergeExpandos` (folding `Property` symbols into a same-name
  Class/Function/Variable). blocked-by: the JS assignment-declaration machinery
  (`getAssignmentDeclarationKind`, `expandoTargets`).
- **Import-clause / `import =` / export-specifier / `export =` / `export default`
  symbols + JSDoc `@typedef`/`@callback`** — the remaining `visit` arms. blocked-by:
  import/export name plumbing and the JSDoc reparser.
- **Call-expression callback labels** — `getUnnamedNodeLabel`'s
  `name(args) callback` form (`getCallExpressionName`/`getCallExpressionLiteralArgs`/
  `cleanCallbackText`). blocked-by: nothing structural (a small follow-up); the
  static labels (`<function>`/`<class>`/`constructor`/`()`/`new()`/`[]`/`default`)
  are ported.
- **nav-to / workspace symbols** — `ProvideWorkspaceSymbols`
  (`getMatchScore`/`compareDeclarationInfos`/`GetDeclarationMap`). blocked-by: a
  multi-file `compiler.Program` view across all programs.
- **Semantic tokens + inlay hints** — NOT in this round: they are **not purely
  syntactic**. blocked-by: the checker's semantic classifier
  (`semantictokens.go`) and the checker's inferred-type surface
  (`inlay_hints.go`).

This round extends the LS root with the two purely-syntactic structural features;
both read the raw parsed file (no checker) and the round-1 `Converters`,
establishing the AST-walk + span/symbol-tree pattern that the deferred region /
JSX / JS-expando / nav-to rounds will build on.
