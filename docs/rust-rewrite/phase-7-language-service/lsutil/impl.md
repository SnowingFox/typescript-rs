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

> Why not reuse `tsgo_astnav::SourceFile`? It owns its own arena and exposes
> neither a mutable arena nor `get_or_create_token`, so token synthesis cannot
> go through it. Conversely, `astnav.FindChildOfKind`/`FindNextToken` require
> `tsgo_astnav::SourceFile`, so the lsutil functions that need *both* token
> synthesis and `astnav` navigation over the *same* arena are deferred (see
> DEFER list) — bridging them needs an `astnav` API change, which is out of this
> lane's edit scope.

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
| `asi.go` | `asi.rs` | kind predicates ported; node-level entry points DEFER |
| `children.go` | `children.rs` | fully ported (+ `SourceFile` context) |
| `utilities.go` | `utilities.rs` | identifier/keyword/quote helpers ported; rest DEFER |
| `userpreferences.go` | `userpreferences.rs` | only `QuotePreference` enum ported; rest DEFER |
| `symbol_display.go` | `symbol_display.rs` | enums + `strings()` + `FILE_EXTENSION_KIND_MODIFIERS` ported; symbol functions DEFER |
| `completednode.go` | — | DEFER (needs `astnav.FindChildOfKind` over a shared arena) |
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
| `completednode.go`: `IsCompletedNode`, `nodeEndsWith`, `hasChildOfKind`, `PositionBelongsToNode` | `hasChildOfKind` needs `astnav.FindChildOfKind`, which requires `tsgo_astnav::SourceFile` (an arena-owning context) and cannot share this crate's owned arena without an `astnav` API change (out of lane scope). `nodeEndsWith` *is* implementable here, but it is private and only used by `IsCompletedNode`, so it is deferred with it. | `ls` root / future astnav-shared-arena API |
| `asi.go`: `NodeIsASICandidate`, `PositionIsASICandidate` | need `astnav.FindNextToken` (same shared-arena issue) and `scanner::GetECMALineOfPosition` (deferred in `tsgo_scanner`). | same |
| `utilities.go`: `ProbablyUsesSemicolons` | `scanner::GetECMALineOfPosition` deferred in `tsgo_scanner` (`GetLastToken` itself is available). | scanner GetECMALine* port |
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

## Gates (crate-scoped)

- `cargo test -p tsgo_ls_lsutil` — **40 unit + 25 doctests, all green**.
- `cargo clippy -p tsgo_ls_lsutil --all-targets -- -D warnings` — **clean**.
- `cargo fmt -p tsgo_ls_lsutil -- --check` — **clean**.
- `cargo build -p tsgo_ls_lsutil` — **ok**.

Public API is additive within the crate; no other crate's source or the root
`Cargo.toml` was touched.
