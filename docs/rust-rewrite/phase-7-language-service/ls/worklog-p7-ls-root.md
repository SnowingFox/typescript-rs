# ls — Round 1 worklog (LS root: LanguageService/Host + diagnostics + quick-info/hover)

> P7 `ls` (root crate `tsgo_ls`) round 1. Strict TDD (red→green vertical slices).
> Crate-scoped gates only (`-p tsgo_ls`); a concurrent lane (P10 corpus runner)
> was editing `internal/testrunner/**` + `internal/testutil/harnessutil/**`, so
> this round touched **only** `internal/ls/**` (the root crate files — NOT the
> `lsconv`/`lsutil`/`change`/`autoimport` sub-crates) + this doc. No root
> `Cargo.toml` edit (the crate was already registered); deps were added to
> `internal/ls/Cargo.toml`.

This round establishes the **LS root** that every remaining ls feature round
builds on: the `LanguageService` + `LanguageServiceHost` plumbing, and the first
two feature providers — **diagnostics** and **quick-info/hover** — wired through
the real `tsgo_compiler` program, `tsgo_checker`, `tsgo_astnav` token resolver,
and `tsgo_ls_lsconv` UTF-8↔UTF-16 position conversion.

## What landed

| File | Go source | What |
|---|---|---|
| `host.rs` | `internal/ls/host.go` | `LanguageServiceHost` trait (reachable subset: `use_case_sensitive_file_names`, `read_file`) |
| `languageservice.rs` | `internal/ls/languageservice.go` | `LanguageService` struct + `new` (`NewLanguageService`), `program`/`to_path`/`read_file`/`use_case_sensitive_file_names`, and the **token+checker plumbing** `file_check_context` (the analogue of `tryGetProgramAndFile` + `program.GetTypeCheckerForFile`) |
| `diagnostics.rs` | `internal/ls/diagnostics.go` | `get_syntactic_diagnostics` + `get_semantic_diagnostics` → `tsgo_lsproto::Diagnostic` (the reachable subset of `diagnosticToLSP`) |
| `hover.rs` | `internal/ls/hover.go` | `get_quick_info_at_position` (the `ProvideHover` tracer) + `provide_hover` → `tsgo_lsproto::Hover`; `QuickInfo` |
| `test_support.rs` | — | test-only `MockHost` + `build_service` (program-over-`MapFs`) |

Public API is **additive within `tsgo_ls`** (the crate was a doc-only skeleton):
`LanguageService`, `LanguageServiceHost`, `QuickInfo`, the `diagnostics`/`hover`
feature methods. No other crate's public API changed.

## Architecture note: how a feature reaches a token + a checker

Go's `ProvideHover` does `program.GetTypeCheckerForFile(ctx, file)` then
`c.GetSymbolAtLocation(node)`. The Rust `tsgo_compiler::Program` keeps its
checker pool **internal**, so a feature builds a checker the same way the pool
does (`internal/compiler/checkerpool.rs`):

1. `program.bind_source_files()` (idempotent),
2. join every bound `ParsedFile` into one `MultiFileBoundProgram` (the
   production multi-file `BoundProgram` view, carrying the program's real
   `CompilerOptions`),
3. take the target file's per-file `file_view(handle)` (its own `NodeArena`, the
   merged symbol space + globals),
4. `Checker::new_checker(Rc::clone(&program))`.

The view's `arena()`/`root()` feed `tsgo_astnav::NavSourceFile` (the
**shared-borrow** navigation context), so a byte position resolves to a token
whose `NodeId` is consistent with the checker's symbol/flow side tables. This is
`LanguageService::file_check_context`, returning an owned `FileCheckContext`
(`checker` + `view` (`Rc`) + `text` + `root`) that outlives the `&mut self`
borrow.

Position conversion uses a `tsgo_ls_lsconv::Converters` built **from the
program's own file snapshots** (UTF-16, the LSP default) at `LanguageService::new`
— behaviorally equivalent to Go (where the project builds the host's converters
from the same file set) without the host carrying a second copy of every file's
text. This is the one documented divergence from Go's `host.Converters()` (see
`host.rs` module note).

## RED → GREEN slices (observed symptoms)

1. **`LanguageService` constructs over a one-file program and exposes the
   program/checker.** RED: `LanguageService`/`LanguageServiceHost` absent (the
   crate was a doc-only `lib.rs`) → unresolved-name compile error. GREEN: the
   struct + `new` + `program()` + `file_check_context`; the plumbing test
   navigates to the `x` binding in `const x = 1;` and resolves its symbol
   (`get_symbol_at_location` → name `"x"`).
2. **`get_semantic_diagnostics` on `const x: number = "s";` → one TS2322.** RED:
   method absent. GREEN: drives `program.semantic_diagnostics()` and maps to
   `lsproto::Diagnostic`. First range guess (char 5) was wrong by my reckoning
   (I asserted 6); the failure revealed the checker reports at the
   `VariableDeclaration` node's **full start** (byte 5, the space after `const`),
   ending after the initializer (byte 21) — locked to `{0,5}..{0,21}`,
   severity `Error`, code `2322`, source `"ts"`, message
   `Type 'string' is not assignable to type 'number'.`
3. **`get_syntactic_diagnostics` on `let x = ;` → the parse diagnostic.** RED:
   method absent. GREEN: reads `file.diagnostics()` (parser) and localizes →
   TS1109 `Expression expected.` at the `;` token `{0,8}..{0,9}`, source `"ts"`.
4. **`get_quick_info_at_position` hovering the `x` use in
   `const x: number = 1; x` → type string `number`.** RED: method absent.
   GREEN: `get_touching_property_name` → `Identifier` → `get_symbol_at_location`
   → `get_type_of_symbol` → `type_to_string` = `"number"`, span `[21,22)`. A
   follow-up RED appeared here: hovering the `const` keyword **panicked**
   (`index out of bounds`), because astnav returns a *synthesized*
   (high-bit-tagged) token for keyword/punctuation that is not in the checker's
   arena. GREEN: guard quick-info to real `Identifier`/`PrivateIdentifier` nodes
   (which astnav never synthesizes), so a keyword/whole-file position yields
   `None`.
5. **UTF-16 position conversion correct on a multi-byte line.** RED: would have
   resolved the wrong byte / reported the wrong column with a naive byte==char
   assumption. GREEN: with an astral `𐐷` (U+10437; 4 UTF-8 bytes, 2 UTF-16
   units) earlier on the line, hovering the trailing `x` at UTF-16 `(0,37)`
   converts to byte 39 to resolve the symbol (`number`), and the reported hover
   range converts byte `[39,40)` back to UTF-16 `[37,38)` — exercising both
   `lsconv` directions.

## Go functions mirrored (`// Go:` anchors)

- `languageservice.go:NewLanguageService` / `LanguageService.GetProgram` /
  `LanguageService.toPath` / `LanguageService.ReadFile` /
  `LanguageService.UseCaseSensitiveFileNames` / `tryGetProgramAndFile`
  (+ `program.GetTypeCheckerForFile`).
- `host.go:Host` (reachable subset).
- `diagnostics.go:getAllDiagnostics` (the `GetSyntacticDiagnostics` +
  `GetSemanticDiagnostics` halves) and `lsconv/converters.go:diagnosticToLSP`
  (severity/code/source/message/range mapping; the `category`→severity switch).
- `hover.go:ProvideHover` (the symbol→type→`type_to_string` tracer) and the
  `IsSourceFile`/no-symbol guards.

## Test deltas

Crate started at **0** tests (doc-only skeleton). Now **15** unit tests
(+0 doctests), all green:

- `languageservice_test.rs` — 5 (construct/`GetProgram`, `toPath`, host delegation,
  the token+checker plumbing, unknown-file `None`).
- `diagnostics_test.rs` — 5 (TS2322 + clean + unknown for semantic; TS1109 +
  clean for syntactic).
- `hover_test.rs` — 5 (identifier type, `lsproto::Hover` wrapper, no-symbol
  keyword `None`, unknown-file `None`, UTF-16 multi-byte conversion).

Per rule 5 (only MORE tests than Go, never fewer): no existing test was weakened
or deleted; every public method has at least one behavioral test plus
None/empty-path coverage.

## Gates (crate-scoped, all GREEN)

```
cargo test  -p tsgo_ls                              # 15 passed; 0 failed (+ 0 doctests)
cargo clippy -p tsgo_ls --all-targets -- -D warnings # clean
cargo fmt   -p tsgo_ls -- --check                    # clean
cargo build -p tsgo_ls                               # ok
```

(`--workspace` was intentionally not run — concurrent lane active.)

## DEFER list (blocked-by → future ls rounds)

- **The other ~60 ls features** — completions, definition/`go-to`, find-all-
  references, rename, code fixes (import/missing-member/…), navigation bar,
  semantic tokens, folding, signature help, call hierarchy, document highlights,
  inlay hints, selection ranges, organize imports, linked editing, code lens, …
  → separate ls feature rounds.
  blocked-by: their respective checker/printer/`ls/change`/`ls/autoimport`
  surfaces (this round deliberately stayed on diagnostics + hover, which need
  only `compiler` + `checker` + `astnav` + `lsproto` + `lsconv`).
- **Full hover display parts + documentation** — the `const`/`let`/`function`/
  `(property)`/`(parameter)`/`type`/`interface`/`class`/`enum member`/`alias`
  prefixes, type-parameter + signature rendering, JSDoc documentation, markdown
  code-fence formatting, and verbosity-level expansion.
  blocked-by: the checker's classified display-parts / `nodebuilder` surface,
  the JSDoc reparser, and `VerbosityContext`.
- **`GetTypeAtLocation` fallback** for `this`/`super`/meta-property/expression
  nodes with no resolvable symbol (Go's `shouldGetType`), and property-access /
  qualified-name quick info.
  blocked-by: `get_type_at_location` + a property-access-aware
  `getSymbolAtLocationForQuickInfo`.
- **Un-annotated initializer types** — `const x = 1` hovers as `any` (not `1`),
  because the checker's `get_type_of_symbol` defers initializer inference.
  blocked-by: checker initializer inference / `getWidenedLiteralType`.
- **Suggestion + declaration diagnostics** and **per-file filtering** of a
  multi-file program's semantic diagnostics (the reachable subset is a
  single-user-file program; `Program::semantic_diagnostics` has no per-file
  partition yet).
  blocked-by: `GetSuggestionDiagnostics`/`GetDeclarationDiagnostics` + a per-file
  semantic-diagnostic partition on `tsgo_compiler`.
- **Diagnostic related-information / tags / message-chain flattening** and the
  Visual-Studio `TS<code>` string code (client-capability-gated), plus the push
  (`publishDiagnostics`) path.
  blocked-by: `tsgo_diagnosticwriter` message-chain flattening + the client-
  capability surface.
- **Host facets** beyond the reachable two — `Converters()` (built by the service
  here instead), `GetPreferences`/`UserPreferences`, `GetECMALineInfo`,
  `AutoImportRegistry`, and the `ReadDirectory`/`GetDirectories`/`DirectoryExists`/
  `FileExists` module-specifier-completion trio.
  blocked-by: `tsgo_ls_autoimport`, `tsgo_sourcemap` document-position mapping,
  and the completions round.
- **Cross-file / project host** (multi-file open document set, snapshots).
  blocked-by: P8 `project`.

This establishes the LS root the remaining feature rounds extend; each new
feature reuses `file_check_context` (token + checker) and the `Converters`
position conversion landed here.
