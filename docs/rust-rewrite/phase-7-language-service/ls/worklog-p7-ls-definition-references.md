# ls — Round 2 worklog (LS root: go-to-definition + find-all-references)

> P7 `ls` (root crate `tsgo_ls`) round 2. Strict TDD (red→green vertical slices).
> Crate-scoped gates only (`-p tsgo_ls`). A concurrent lane was editing the
> separate `tsgo_ls_change` crate (`internal/ls/change/**`), disjoint from this
> lane, so this round touched **only** `internal/ls/**` root-crate files (NOT the
> `lsconv`/`lsutil`/`change`/`autoimport` sub-crates) + this doc. No root
> `Cargo.toml` edit and **no new dependency on `tsgo_ls_change`** — definitions /
> references need only `compiler` + `checker` + `astnav` + `ast` + `lsproto` +
> `lsconv`, all already in `internal/ls/Cargo.toml`. No other crate's source was
> touched.

This round adds the next two LS feature providers on top of the round-1 LS root
(`LanguageService` + `Host` + `file_check_context` token→symbol→checker bridge):
**go-to-definition** and **find-all-references**, both the reachable single-file
subset.

## What landed

| File | Go source | What |
|---|---|---|
| `definition.rs` | `internal/ls/definition.go` | `provide_definition` (the `ProvideDefinition`/`getDefinitionAtPosition` reachable subset) → `Vec<lsproto::Location>`; `name_of_declaration` (reachable subset of `GetNameOfDeclaration`) |
| `references.rs` | `internal/ls/findallreferences.go` | `provide_references` (the `ProvideReferences`/`getReferencedSymbolsForNode` single-file subset) → `Vec<lsproto::Location>`; `collect_named_identifiers` (reachable subset of `getPossibleSymbolReferenceNodes`) |
| `definition_test.rs` | — | 5 unit tests |
| `references_test.rs` | — | 6 unit tests |

Public API is **additive within `tsgo_ls`**: two new `pub fn` on
`LanguageService` (`provide_definition`, `provide_references`) and the
`pub mod definition; pub mod references;` registrations. No other crate's public
API changed.

## Architecture: reuse of the round-1 token+checker bridge

Both features follow the same shape as `hover.rs`, reusing
`LanguageService::file_check_context` (the analogue of Go's
`program.GetTypeCheckerForFile` + `tryGetProgramAndFile`):

1. `document_script` + `converters().line_and_character_to_position` to turn the
   LSP `(line, character)` into an internal UTF-8 byte offset (immutable borrows
   first, so the checking context can take `&mut self`),
2. `file_check_context(file)` → owned `FileCheckContext` (`checker` + `view`
   (`Rc`) + `text` + `root`),
3. `NavSourceFile::from_borrowed_arena(view.arena(), root, text)` to resolve the
   touching property name to a node id consistent with the checker's tables,
4. `get_symbol_at_location(&mut checker, view, node, globals)` to resolve the
   symbol,
5. convert internal byte `TextRange`s back to UTF-16 `lsproto::Location`s via
   `Converters::to_lsp_location` (the `file_name_to_document_uri` + `to_lsp_range`
   pair).

**Definition** maps the symbol's `declarations` to the UTF-16 range of each
declaration's *name* (Go's `createDefinitionLocations`:
`core.OrElse(GetNameOfDeclaration(decl), decl)` then `createRangeFromNode` =
`[GetTokenPosOfNode, End)`, which is `astnav::get_start_of_node(..,false)` ..
`nav.end`), deduped by range.

**References** walks every identifier node in the file with the searched symbol's
name (the reachable analogue of Go's `getPossibleSymbolReferencePositions` text
scan + `GetTouchingPropertyName`), resolves each via `get_symbol_at_location`, and
keeps those whose `SymbolId` equals the searched symbol's — i.e. Go's
`getReferencesAtLocation` → `getRelatedSymbol` comparison, collapsed to direct
single-file symbol-identity. Scope-aware name resolution
(`get_symbol_at_location` → `resolve_name`) makes the search shadowing-correct.
The declaration is included (Go's default `IncludeDeclaration`) because its name
identifier resolves to the same symbol.

## RED → GREEN slices (observed symptoms)

1. **Local-var definition: `const x = 1;\nx;`, definition at the line-2 `x` use →
   the line-1 `x` declaration-name range.** RED: `no method named
   provide_definition found for struct LanguageService`. GREEN: resolve the
   touched `Identifier` → its symbol → the `VariableDeclaration`'s name → range
   `[6,7)` = UTF-16 `(0,6)..(0,7)`.
2. **Function definition: `function f(){}\nf();`, definition at the `f` call → the
   `function f` declaration name `(0,9)..(0,10)`.** Exercised the
   `FunctionDeclaration` arm of `name_of_declaration` and value resolution of a
   call target; passed on the first run of the new test (general impl already
   covered it).
3. **Parameter definition: `function f(p){ return p; }`, definition at the `p`
   use → the parameter declaration name `(0,11)..(0,12)`.** Exercised the
   `ParameterDeclaration` arm + scope resolution of a parameter in the function
   body's locals.
4. **Find-all-references: `const x = 1; x; x;`, references at the declaration → 3
   locations (decl + 2 uses).** RED: `no method named provide_references found
   for struct LanguageService`. GREEN: walk identifiers named `x`, keep the 3 that
   resolve to the same symbol → `(0,6)..(0,7)`, `(0,13)..(0,14)`,
   `(0,16)..(0,17)`, in source order.
5. **References respects shadowing: `const x=1; function f(){ const x=2; x; }
   x;`, references at the inner `x` → only the inner decl + inner use
   (`(0,31)..(0,32)`, `(0,36)..(0,37)`), never the shadowed outer `x` at 6/41.**
   Confirms scope-aware `getRelatedSymbol`-equivalent symbol identity.

## Extra behavioral tests (only MORE than Go, never fewer)

- Definition: unknown-file → empty; `const` keyword (non-identifier) → empty
  (the synthesized-token guard, same as hover).
- References: searching from a *use* position returns the full set (not just refs
  after the cursor); a function symbol's references span the declaration name +
  every call site (`function f(){}\nf();\nf();` → 3); unknown-file → empty; `const`
  keyword → empty.

## Go functions mirrored (`// Go:` anchors)

- `definition.go:LanguageService.ProvideDefinition` / `provideDefinitionWorker`
  (the symbol→declarations→name-range tracer), `getDefinitionAtPosition` (body),
  `getDeclarationsFromLocation` (the `GetSymbolAtLocation` → `symbol.Declarations`
  path), `createDefinitionLocations` (the per-declaration name range + dedupe),
  and `ast/utilities.go:GetNameOfDeclaration` (reachable subset).
- `findallreferences.go:LanguageService.ProvideReferences`,
  `getReferencedSymbolsForNode` (single-file body), `getReferencesAtLocation` /
  `getRelatedSymbol` (the same-symbol comparison), `getPossibleSymbolReferenceNodes`
  (reachable subset: walk parsed identifier nodes), and `getRangeOfNode` /
  `createRangeFromNode` (the identifier range).
- `lsconv/converters.go:ToLSPLocation` / `FileNameToDocumentURI` (the Location
  shape), reused from the round-1 `Converters`.

## Test deltas

Crate was at **15** unit tests (round 1). Now **26** unit tests (+0 doctests),
all green:

- `definition_test.rs` — 5 (var/function/parameter definition + unknown-file /
  keyword `None`).
- `references_test.rs` — 6 (decl+uses, shadowing, from-a-use, function call
  sites, unknown-file / keyword empty).

No existing test was weakened or deleted (rule 5); every new `pub fn` has a
behavioral test plus empty/edge coverage.

## Gates (crate-scoped, all GREEN)

```
cargo test  -p tsgo_ls                              # 26 passed; 0 failed (+ 0 doctests)
cargo clippy -p tsgo_ls --all-targets -- -D warnings # clean
cargo fmt   -p tsgo_ls -- --check                    # clean
cargo build -p tsgo_ls                               # ok
```

(`--workspace` was intentionally not run — concurrent `tsgo_ls_change` lane
active.)

## DEFER list (blocked-by → future ls rounds)

- **Cross-file / module-resolution definitions** — Go's `getReferenceAtPosition`
  (triple-slash / module-specifier) path, `ProvideSourceDefinition`,
  `ProvideTypeDefinition`, and the `LocationLink` + `clientSupportsLink` /
  `OriginSelectionRange` / context-range shape.
  blocked-by: a `compiler.Program`-level multi-file symbol/module resolver +
  `GetClientCapabilities`.
- **Definition keyword / special-case targets** — `override` member
  (`getSymbolForOverriddenMember`), jump-statement labels
  (`IsJumpStatementTarget`/`getTargetLabel`), `case`/`default`,
  `return`/`yield`/`await` → enclosing function, the called-signature /
  constructor disambiguation (`tryGetSignatureDeclaration`/`symbolMatchesSignature`),
  shorthand-property & object-literal-element contextual declarations
  (`getDeclarationsFromObjectLiteralElement`), object-binding-pattern property
  declarations, alias resolution (`ResolveAlias`), and index-signature targets
  (`GetIndexSignaturesAtLocation`).
  blocked-by: `GetResolvedSignature` / `GetContextualType` / `ResolveAlias` /
  `GetIndexSignaturesAtLocation` and the keyword-name adjustment helpers.
- **Cross-file references** — Go's `getReferencesInContainerOrFiles` global
  search over every program file (the reachable subset is single-file).
  blocked-by: a `compiler.Program`-level multi-file symbol resolver + symbol
  scope (`getSymbolScope`).
- **Reference special-cases** — string-literal references
  (`getReferencesForStringLiteral`), the triple-slash / module-symbol path,
  label references, import/export specifier references
  (`getReferencesAtExportSpecifier`/`getImportOrExportReferences`),
  constructor / `super` / static-`this` references, shorthand-property
  references, the root-symbol `allSearchSymbols` / `getRelatedSymbol` machinery,
  the read/write-access classification (`IsWriteAccessForReference`, the VS
  reference items), and the `IncludeDeclaration == false` filtering.
  blocked-by: `GetContextualType`, the import/export + alias resolver, and the
  cross-project orchestrator surface.
- **Rename, document highlights, call hierarchy** — separate later rounds that
  build on this same resolve-symbol-then-collect-references core.
  blocked-by: their respective providers.

This round extends the LS root with two more providers; both reuse
`file_check_context` (token + checker) and the round-1 `Converters` position
conversion, and establish the resolve-symbol → declaration-names / same-symbol
references core that rename / highlights / call-hierarchy will build on.
