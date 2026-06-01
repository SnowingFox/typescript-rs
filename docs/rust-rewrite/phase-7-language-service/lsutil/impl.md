# ls/lsutil: implementation worklog (impl.md)

> Built during execution: the repo had no lsutil impl.md/tests.md, so this was
> ported directly from the Go source + PORTING/tdd and the progress/divergences
> are recorded here.
> Language note: this worklog is written in English (per the P7 lsutil task
> instruction). All `.rs` comments are English (PORTING §7).
> TDD: every helper landed as a red→green vertical slice (test fails first, then
> minimal implementation); see "TDD ordering" below.

**crate**: `tsgo_ls_lsutil`
**goal**: the shared, mostly-syntactic helper layer the language service builds
on — AST/token position helpers, node-kind predicates, identifier/name helpers,
and ASI syntax classification. (NOT the LS features themselves: those live in
`format`, `ls/change`, `ls/autoimport`, the `ls` root.)
**deps (crate)**: `tsgo_ast`, `tsgo_core`, `tsgo_scanner`, `tsgo_stringutil`,
`tsgo_tspath`, `bitflags`. **dev-dep**: `tsgo_parser` (builds AST inputs for tests).
**Go source**: `internal/ls/lsutil/` (8 non-test files: `asi.go`, `children.go`,
`completednode.go`, `formatcodeoptions.go`, `organizeimports.go`,
`symbol_display.go`, `userpreferences.go`, `utilities.go`).

## What this package is

`lsutil` is the small shared-helper package the language service builds on. The
reachable, syntactic subset is the heart of it: "find the last/first token of a
node", "is this kind terminated by `;`/`,`/a block?", "turn a module specifier
into a valid identifier", plus the `ScriptElementKind`/`...Modifier` value types.

## Ownership model (key decision)

Go threads `(node *ast.Node, sourceFile *ast.SourceFile)`. The Rust `tsgo_ast`
`SourceFile` *node* stores neither the source text nor the synthesized-token
cache, so — exactly like `tsgo_astnav` — this crate models the navigation
context as a dedicated [`SourceFile`] struct owning the `NodeArena`, the root id,
the text, the language variant, and the token cache.

`tsgo_scanner` defers the `ast.SourceFile`-based `GetScannerForSourceFile`, and
`tsgo_astnav` keeps `GetOrCreateToken` private. To stay inside this lane's edit
boundary (only `internal/ls/lsutil/**`), the scanner factory (`SourceFile::scanner_at`)
and the token cache (`SourceFile::get_or_create_token`) are replicated locally
over the owned arena. Behavior is identical to Go's
`scanner.GetScannerForSourceFile` + `sourceFile.GetOrCreateToken`.

## Enablement round (P7 keystone): `astnav` shared-borrow surface lands

`tsgo_astnav` now exposes a **shared-borrow** navigation surface
(`NavSourceFile<'a> = NavEngine<&NodeArena>` and `RcSourceFile`), whose queries
take `&self` and synthesize on-demand tokens into a side store behind a
`RefCell` (interior mutability), with tagged ids so they never collide with real
arena ids. That unblocks the lsutil helpers that need *both* a navigation over
*this crate's* arena and token synthesis:

- `completednode.rs` (`IsCompletedNode`/`nodeEndsWith`/`hasChildOfKind`/
  `PositionBelongsToNode`) — fully `&self`. `hasChildOfKind` builds a
  `tsgo_astnav::NavSourceFile` that **borrows** this crate's arena and calls
  `find_child_of_kind` (shared access, no arena mutation). `nodeEndsWith` only
  needs token *kinds*, so it tracks them with a scanner directly instead of
  synthesizing nodes (a documented, behavior-identical divergence from Go's
  `GetOrCreateToken`).
- `asi.rs` node-level (`NodeIsASICandidate`/`PositionIsASICandidate`) — uses the
  existing `&mut` `GetLastToken`/`GetLastChild` for the trailing-token checks,
  then a borrowed `NavSourceFile` for `FindNextToken`/`GetStartOfNode`, plus the
  additive `scanner::get_ecma_line_of_position(text, pos)` and local
  `FindAncestor`/`FindAncestorOrQuit`/`IsFunctionBlock`/`IsModuleBlock`/
  `IsFunctionLikeKind` ports (kept local to avoid touching `tsgo_ast`).

`tsgo_astnav` was added to `[dependencies]`; `SourceFile::scanner_at` was made
`pub(crate)` so `completednode.rs` can scan. Both are additive.

## Type mapping (this package)

| Go construct | Rust representation | Notes |
|---|---|---|
| `*ast.SourceFile` (navigation context) | `SourceFile { arena, root, text, language_variant, token_cache }` | Mirrors `tsgo_astnav::SourceFile`. |
| `sourceFile.GetOrCreateToken(kind, fullStart, end, parent, flags)` | `SourceFile::get_or_create_token(kind, pos, end, parent)` (private) | Token flags unused by the ported consumers (they read `.kind`/`.pos`/`.end`); cache key is `(parent, pos, end)`. |
| `scanner.GetScannerForSourceFile(file, pos)` | `SourceFile::scanner_at(pos)` (private) | Owns a copy of the text (`PERF(port)`), mirroring `tsgo_astnav`. |
| `ast.IsContextualKeyword` | local `is_contextual_keyword` in `utilities.rs` | `tsgo_ast` has not ported it; mirrored via `Kind::{FIRST,LAST}_CONTEXTUAL_KEYWORD`. |
| `ast.PositionIsSynthesized` | local `position_is_synthesized` in `children.rs` | `pos < 0`; not ported in `tsgo_ast`. |
| `ast.IsJSDocSingleCommentNode` | local `is_jsdoc_single_comment_node` → `false` | Inert: parser has not ported JSDoc reparse (matches `tsgo_astnav`). |
| `unicode.ToUpper(rune)` | local `to_upper_simple(char)` | `char::to_uppercase().next()`; full-vs-simple casing never differs for identifier chars. |
| `QuotePreference string` | `enum QuotePreference` + `as_str()` returning Go wire values | |
| `ScriptElementKind int` (iota) | `#[repr(i32)] enum ScriptElementKind` | Discriminants match iota 0..38. |
| `ScriptElementKindModifier uint32` (iota bits) | `bitflags! ScriptElementKindModifier: u32` | `Public` is bit 1 (value 2); bit 0 unused, matching Go iota. |
| `(m).Strings() collections.Set[string]` | `strings(self) -> Vec<&'static str>` | Returns names in the fixed table order (deterministic; LS joins them). |

## File list → Rust modules

| Go file | Rust file | Status |
|---|---|---|
| `asi.go` | `asi.rs` | kind predicates + node-level entry points ported |
| `children.go` | `children.rs` | fully ported (+ `SourceFile` context) |
| `utilities.go` | `utilities.rs` | identifier/keyword/quote helpers ported; rest DEFER |
| `userpreferences.go` | `userpreferences.rs` | only `QuotePreference` enum ported; rest DEFER |
| `symbol_display.go` | `symbol_display.rs` | enums + `strings()` + `FILE_EXTENSION_KIND_MODIFIERS` ported; symbol functions DEFER |
| `completednode.go` | `completednode.rs` | fully ported (uses `astnav` shared-borrow surface) |
| `organizeimports.go` | — | DEFER (UserPreferences + ICU collation + modulespecifiers) |
| `formatcodeoptions.go` | — | DEFER (lsproto + printer) |
| — (crate root) | `lib.rs` | `mod` declarations + re-exports |

## Ported functions (with `// Go:` anchors)

### `asi.rs` (kind predicates — pure)
- [x] `syntax_requires_trailing_comma_or_semicolon_or_asi` — `asi.go:SyntaxRequiresTrailingCommaOrSemicolonOrASI`
- [x] `syntax_requires_trailing_function_block_or_semicolon_or_asi` — `:SyntaxRequiresTrailingFunctionBlockOrSemicolonOrASI`
- [x] `syntax_requires_trailing_module_block_or_semicolon_or_asi` — `:SyntaxRequiresTrailingModuleBlockOrSemicolonOrASI`
- [x] `syntax_requires_trailing_semicolon_or_asi` — `:SyntaxRequiresTrailingSemicolonOrASI`
- [x] `syntax_may_be_asi_candidate` — `:SyntaxMayBeASICandidate`
- [x] `node_is_asi_candidate` — `:NodeIsASICandidate` (via `astnav.FindNextToken`/`GetStartOfNode` + `scanner::get_ecma_line_of_position`)
- [x] `position_is_asi_candidate` — `:PositionIsASICandidate`
- [x] local `find_ancestor`/`find_ancestor_or_quit`/`is_function_block`/`is_module_block`/`is_function_like_kind` (ported here to avoid touching `tsgo_ast`)

### `completednode.rs` (is-completed / position-belongs)
- [x] `is_completed_node` — `completednode.go:IsCompletedNode` (full kind switch)
- [x] `position_belongs_to_node` — `completednode.go:PositionBelongsToNode`
- [x] `node_ends_with` — `completednode.go:nodeEndsWith` (tracks token kinds, no node synthesis)
- [x] `has_child_of_kind` — `completednode.go:hasChildOfKind` (borrowed `astnav.FindChildOfKind`)
- [x] local node accessors `node_body`/`node_type`/`node_expression`/`node_statement`/`node_module_specifier` + single-kind field reads (mirror Go `Node.Body()`/`Type()`/`Expression()`/`Statement()`/`ModuleSpecifier()`)

### `children.rs` (token/child navigation)
- [x] `SourceFile` (context) + `new`/`root`/`text`/`arena` + private `get_or_create_token`/`scanner_at`
- [x] `assert_has_real_position` — `children.go:AssertHasRealPosition`
- [x] `get_last_visited_child` — `children.go:GetLastVisitedChild`
- [x] `get_last_child` — `children.go:GetLastChild`
- [x] `get_last_token` — `children.go:GetLastToken`
- [x] `get_first_token` — `children.go:GetFirstToken`

### `utilities.rs`
- [x] `is_non_contextual_keyword` — `utilities.go:IsNonContextualKeyword`
- [x] `quote_preference_from_string` — `utilities.go:QuotePreferenceFromString`
- [x] `module_specifier_to_valid_identifier` — `utilities.go:ModuleSpecifierToValidIdentifier`
- [x] `module_symbol_to_valid_identifier` — `utilities.go:ModuleSymbolToValidIdentifier`

### `userpreferences.rs`
- [x] `QuotePreference` enum + `as_str()` — `userpreferences.go:QuotePreference`

### `symbol_display.rs`
- [x] `ScriptElementKind` enum — `symbol_display.go:ScriptElementKind`
- [x] `ScriptElementKindModifier` bitflags + `strings()` — `:ScriptElementKindModifier` / `:ScriptElementKindModifier.Strings`
- [x] `FILE_EXTENSION_KIND_MODIFIERS` — `:FileExtensionKindModifiers`

## DEFER list (with blocked-by)

| Go function(s) | blocked-by | target |
|---|---|---|
| ~~`completednode.go`: `IsCompletedNode`, `nodeEndsWith`, `hasChildOfKind`, `PositionBelongsToNode`~~ | **DONE** — re-enabled via `astnav`'s shared-borrow surface (`NavSourceFile` borrows this crate's arena; `&self` queries). | — |
| ~~`asi.go`: `NodeIsASICandidate`, `PositionIsASICandidate`~~ | **DONE** — `astnav.FindNextToken`/`GetStartOfNode` over a borrowed arena + additive `scanner::get_ecma_line_of_position`. | — |
| `utilities.go`: `ProbablyUsesSemicolons` | needs `astnav.FindPrecedingToken` (now available via shared surface) wired against the LS program; left for the dedicated utilities/`ls` round. `scanner::get_ecma_line_of_position` is now available. | `ls` root |
| `utilities.go`: `ShouldUseUriStyleNodeCoreModules` | `*compiler.Program` (`UsesUriStyleNodeCoreModules`, `Imports()`), `core.NodeCoreModules`. | `ls` root (P6/P7) |
| `utilities.go`: `GetQuotePreference` | `UserPreferences` (full port). | with userpreferences.go |
| `symbol_display.go`: `GetSymbolKind`, `GetSymbolModifiers` + helpers | `*checker.Checker` (`GetRootSymbols`, `GetTypeOfSymbolAtLocation`, `GetCallSignatures`, `GetAliasedSymbol`, `IsDeprecatedDeclaration`, ...). | `ls` root |
| `organizeimports.go`: all | `UserPreferences`, ICU collation (`golang.org/x/text/collate`), `modulespecifiers`, `stringutil` ESLint comparers, `locale`. | with userpreferences.go |
| `userpreferences.go`: `UserPreferences` struct + reflection (un)marshaling, `ParseUserPreferences`, `WithOverrides`, ... | Go reflection-based config marshaling has no 1:1 Rust analog (would need a serde-reflection design); `modulespecifiers`, `vfsmatch`, `internal/json` deps. | dedicated userpreferences round |
| `formatcodeoptions.go`: `FormatCodeSettings`/`EditorSettings` + `FromLSFormatOptions`/`ToLSFormatOptions`/`GetDefaultFormatCodeSettings` | `lsproto.FormattingOptions`, `printer.GetDefaultIndentSize`. | with userpreferences.go |

## TDD ordering (red→green evidence)

Each slice: write the behavior test → run `cargo test -p tsgo_ls_lsutil` and see
it fail (missing fn / `todo!()` panic) → minimal implementation → green.

1. `asi.rs` predicates — RED: 9 unit tests panic on `todo!()` → GREEN.
2. `userpreferences::QuotePreference::as_str` — RED → GREEN.
3. `utilities.rs` (4 fns) — RED: 11 unit tests panic → GREEN.
4. `children.rs` (`SourceFile` + 4 fns) — RED: 9 unit tests panic → GREEN
   (token synthesis driven by `tsgo_scanner` + local token cache).
5. `symbol_display.rs` — data defs land first; `strings()` is the tracer
   (RED on 4 tests) → GREEN.
6. (enablement round) `completednode.rs` — RED: 10 tests panic on `todo!()` →
   GREEN. Then `asi.rs` node-level — RED: 6 tests panic on `todo!()` → GREEN.

## Gates (crate-scoped)

- `cargo test -p tsgo_ls_lsutil` — **56 unit + 29 doctests, all green** (was
  40 + 25; the enablement round added 10 completednode + 6 asi unit tests and
  4 doctests).
- `cargo clippy -p tsgo_ls_lsutil --all-targets -- -D warnings` — **clean**.
- `cargo fmt -p tsgo_ls_lsutil -- --check` — **clean**.
- `cargo build --workspace --all-targets` — **ok** (astnav's shared surface
  introduces no downstream break in checker/transformers/compiler/execute).

Public API is additive. This round added `tsgo_astnav` to `[dependencies]` and
made `SourceFile::scanner_at` `pub(crate)` (both additive); the additive
`scanner::get_ecma_line_of_position` helper was added to `tsgo_scanner`.
