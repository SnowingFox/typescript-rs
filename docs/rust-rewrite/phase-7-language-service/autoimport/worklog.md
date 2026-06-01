# ls/autoimport — Round 1 worklog (export index + module-specifier + candidates)

> P7 `ls/autoimport` round 1. Strict TDD (red→green vertical slices). Crate-scoped
> gates only (`-p tsgo_ls_autoimport`); a concurrent lane was editing the
> `internal/ls/**` ROOT crate (rename + document-highlights) at the same time, so
> this round touched **only** `internal/ls/autoimport/**` + this doc. No root
> `Cargo.toml` edit (the crate was already registered as a workspace member);
> deps are declared in `internal/ls/autoimport/Cargo.toml`.

## Scope decision (the reachable core)

`internal/ls/autoimport` maintains a cross-file index of every exported symbol in
the program so that, when the user types an unresolved name, it can offer
`import { X } from "..."` candidates (with the module specifier computed via
`modulespecifiers`) and back the "add missing import" code fix.

Go drives the whole thing through the **type checker**: `extract.go` walks
`file.Symbol.Exports`, resolves aliases (`tryResolveSymbol`/`GetAliasedSymbol`),
follows `export *` through `GetExportsOfModule`, and merges ambient modules;
`registry.go` tracks the results in an incremental, dirty-map-backed `Registry`
of project / `node_modules` buckets (`tsgo_project_dirty`), with a three-phase
`node_modules` discovery/extraction/build pipeline; `fix.go`/`import_adder.go`
turn a chosen export into a text edit via `ls/change`. None of `tsgo_checker`,
`tsgo_compiler`, the full `Registry`, or `ls/change` is available to this crate
yet, so this round ports the genuinely reachable spine the task emphasises:

1. **`index.rs`** — the prefix/word `Index<T>` (verbatim 1:1 of `index.go`).
2. **`util.rs`** — `word_indices` (the camelCase/snake_case splitter, `util.go`).
3. **`export.rs`** — the `Export` value types (`ModuleId`/`ExportId`/
   `ExportSyntax`+stringer/`Export`+methods) (`export.go` data types).
4. **`extract.rs`** — a reachable **AST-walking** extractor (the reachable analog
   of `extract.go`'s checker-driven `extractFromModule`).
5. **`registry.rs`** — `build_index_for_files`, building one index over several
   files (reachable analog of `registryBuilder.buildProjectBucket`), plus a
   single level of cross-file `export *` resolution.
6. **`specifiers.rs`** — `get_module_specifier`, the reachable tail of
   `View.GetModuleSpecifier` calling `tsgo_modulespecifiers`.
7. **`view.rs`** — `search_index` + `find_import_candidates`, the reachable
   analog of `View.search` / `View.GetCompletions`.

## Deliberate, documented divergences

1. **AST-walking extraction instead of checker symbols.** Go extracts from the
   binder/checker symbol graph; without `tsgo_checker` this port walks the
   parsed file's top-level `export` statements directly (`extract.rs`). It covers
   the syntactic forms the task lists — `export const/let/var`, `export function`,
   `export class`, `export interface/type/enum/namespace`, `export { x }` /
   `export { x as y }`, `export default …`, `export = …`, and recognises
   `export * from "…"`. The `ScriptElementKind`/`SymbolFlags` are mapped from the
   declaration's AST kind (variable→`VariableElement`/`*_SCOPED_VARIABLE`,
   function→`FunctionElement`/`FUNCTION`, class→`ClassElement`/`CLASS`, …) rather
   than from a resolved symbol. Re-export specifiers (`export { x }`) are recorded
   with `ExportSyntax::Named` + `SymbolFlags::ALIAS` and `ScriptElementKind::Unknown`
   because the real target/kind needs the checker.
2. **`Export` embeds `ExportId` as a field `id`** (Go promotes the embedded
   `ExportID`, so `e.ModuleID`/`e.ExportName` become `e.id.module_id`/
   `e.id.export_name`). The unexported Go fields `localName`/`through` map to
   `pub(crate)` (the crate is the analog of Go's package); `through` is exposed
   read-only via `Export::through()`.
3. **Anonymous `export default <expr>`** with no derivable identifier falls back
   to a file-name-derived identifier via `lsutil::module_specifier_to_valid_identifier`
   (Go's last resort in `createExport`). `SkipOuterExpressions` unwrapping of the
   assigned expression is simplified to a bare-identifier check.
4. **Reachable single-level `export *` resolution.** `build_index_for_files`
   resolves a relative `export * from "./b"` against the supplied file set (it has
   every parsed AST) and re-exports `b`'s direct names *through* the re-exporting
   file: the re-export keeps the name, sets `module_id` to the re-exporter,
   `syntax = Star`, `through = "\u{FE}export"` (`INTERNAL_SYMBOL_NAME_EXPORT_STAR`),
   and `target` pointing back at `b`'s `ExportId` — mirroring Go's
   `extractFromSymbol` star arm. It is intentionally non-recursive and relative-
   only; the cross-package / recursive enumeration is Go's checker
   `GetExportsOfModule` (deferred).
5. **`get_module_specifier` is host-generic, program-free.** Go's
   `View.GetModuleSpecifier` reads `v.program`/`v.registry.entrypoints`/the
   per-file specifier cache; this port takes the `modulespecifiers`
   `ModuleSpecifierGenerationHost` + `SourceFileForSpecifierGeneration` traits and
   `CompilerOptions` directly, preserving the ambient-module short-circuit and the
   "first non-`/node_modules/` candidate wins" loop verbatim.

## RED → GREEN slices (each observed failing before implementing)

| # | Slice | RED symptom (observed) | GREEN |
|---|---|---|---|
| 0 | `util::word_indices` (`util_test.go:TestWordIndices`, 12 cases) | all 12 `assert_eq!` failed: `word_indices(..) == []` vs expected words | byte-faithful splitter (prev/next-rune lowercase checks) |
| 1 | `index::clone_filtered` (`index_test.go:TestIndexClone`) | `clone_filters_entries_by_package`: cloned `entries.len() == 0` vs `2` (other index behaviors already green from real `find`/`search`/`insert`) | remap old→new positions, rebuild word map |
| 2 | `export::Export::name` | `name_prefers_local_name`: `"default"` vs `"MyComponent"`; `name_export_equals_uses_target`: `"export="` vs `"realName"` | local-name precedence, then `export=`→target, then plain |
| 3 | `extract` slice 1 (const/function/class) | `extracts_const_function_class`: names `[]` vs `["a","b","C"]` | walk `VariableStatement`/`FunctionDeclaration`/`ClassDeclaration` with `export` modifier |
| 4 | `extract` slice 2 (`{x}`/default/`=`/iface/type/enum/ns) | 8 tests, e.g. `extracts_default_function`: no export named `"foo"` | `ExportDeclaration`/`ExportAssignment`/default-modifier + type-decl arms |
| 5 | `registry::build_index_for_files` slice 3 | `multi_file_index_maps_name_to_file`: `find("a").len() == 0` vs `1` | parse+extract each file into one index; resolve cross-file `export *` |
| 6 | `specifiers::get_module_specifier` slice 4 | `relative_specifier_for_sibling_directory`: `""` vs `"./lib/b"` | ambient short-circuit + `get_module_specifiers_for_file_with_info` + node_modules filter |
| 7 | `view::find_import_candidates` slice 5 | `candidate_for_unresolved_function`: `candidates.len() == 0` vs `1` | `search_index` (self-import skip) + per-hit specifier → `ImportCandidate` |

## Go functions mirrored (`// Go:` anchors in source)

- `index.go`: `Index`, `Named`, `Index.Find`, `Index.SearchWordPrefix`,
  `Index.insertAsWords`, `Index.Clone`, `containsCharsInOrder`.
- `util.go`: `wordIndices`.
- `export.go`: `ModuleID`, `ExportID`, `ExportSyntax`, `Export`, `Export.Name`,
  `Export.IsRenameable`, `Export.AmbientModuleName`, `Export.IsUnresolvedAlias`,
  `Export.through`; `export_stringer_generated.go:ExportSyntax.String`.
- `extract.go`: `exportExtractor.extractFromFile` (AST analog), `getSyntax`
  (declaration→`ExportSyntax` mapping), `isUnusableName`; the
  `InternalSymbolNameExportStar` arm (→ `collect_star_reexport_specifiers`).
- `registry.go`: `registryBuilder.buildProjectBucket` (AST analog).
- `specifiers.go`: `View.GetModuleSpecifier`.
- `view.go`: `QueryKind`, `View.search`, `View.GetCompletions` (search +
  candidate half).

## Test deltas (crate starts at 0)

- **55 unit tests + 6 doctests** added (crate had none before).
  - `util_test.rs`: 12 (1:1 with Go `TestWordIndices` sub-cases) + 1 doctest.
  - `index_test.rs`: 8 (Go `TestIndexClone`'s 3 reachable sub-cases + 5 new
    behavior tests for `find`/`search_word_prefix`/`contains_chars_in_order`/
    empty-name panic) + 2 doctests.
  - `export_test.rs`: 10 (all new — Go has no `export_test.go`) + 3 doctests.
  - `extract_test.rs`: 13 (all new behavior tests for the AST extractor).
  - `registry_test.rs`: 4 (all new).
  - `specifiers_test.rs`: 4 (all new).
  - `view_test.rs`: 4 (all new).
- Go `*_test.go` ported 1:1 where reachable: `index_test.go:TestIndexClone`
  (nil-receiver sub-case documented as N/A — Rust references are non-null),
  `util_test.go:TestWordIndices` (all 12 cases). `registry_test.go` and the
  vfs-backed `util_test.go:TestGetPackageRealpathFuncs_*` are session/vfs/program
  integration tests → deferred with the registry pipeline (see DEFER).
- No existing test weakened or deleted (the crate had none).

## Gate results (crate-scoped, all GREEN)

- `cargo test -p tsgo_ls_autoimport` → `55 passed` (unit) + `6 passed` (doctest).
- `cargo clippy -p tsgo_ls_autoimport --all-targets -- -D warnings` → clean.
- `cargo fmt -p tsgo_ls_autoimport -- --check` → clean.
- `cargo build -p tsgo_ls_autoimport` → ok.
- `--workspace` intentionally not run (concurrent lane active).

## Public API (additive only, within `tsgo_ls_autoimport`)

- `index`: `Index<T>` (`find`, `search_word_prefix`, `insert_as_words`,
  `clone_filtered`, `default`), `Named`, `contains_chars_in_order`.
- `util`: `word_indices`.
- `export`: `ModuleId`, `ExportId`, `ExportSyntax` (+ `Display`), `Export`
  (+ `name`/`is_renameable`/`ambient_module_name`/`is_unresolved_alias`/`through`),
  `is_unusable_name`.
- `extract`: `extract_top_level_exports`, `collect_star_reexport_specifiers`.
- `registry`: `FileInput`, `build_index_for_files`.
- `specifiers`: `get_module_specifier`.
- `view`: `QueryKind`, `ImportCandidate`, `search_index`, `find_import_candidates`.

## DEFER list (with blocked-by)

- **`fix.go` / `import_adder.go`** — turning a chosen `Export` into the actual
  import text edit (and de-duplicating against existing imports). blocked-by:
  `ls/change` (`tsgo_ls_change` edit applier) + `tsgo_checker` + the `ls` root.
- **`aliasresolver.go` + checker-driven extraction** — real alias-target/kind
  resolution for `export { x }`, cross-module/recursive `export *` enumeration,
  ambient-module merging, and the `SymbolToExport`/`extractFirstExport` entry
  points. blocked-by: `tsgo_checker` (`GetExportsOfModule`/`GetAliasedSymbol`/
  symbol graph).
- **The incremental `Registry`** — dirty-map project/`node_modules` buckets
  (`Clone`/`BucketState`/`markBucketsDirty`/`updateIndexes`), the three-phase
  `node_modules` discovery→extraction→build pipeline, `package.json` dependency
  scanning, project-reference output redirects, the specifier cache, and
  `GetCacheStats`. blocked-by: `tsgo_compiler` program + `RegistryCloneHost`
  (`tsgo_project_dirty` is available, but the host/program are not).
- **`View.GetModuleSpecifier` `PackageName != ""` branch** — resolving a
  `node_modules` entrypoint via `registry.entrypoints` + the program's resolution
  conditions. blocked-by: the full `Registry` + `tsgo_compiler` program.
- **`GetCompletions` ranking/grouping** — `CompareFixesForRanking` /
  `CompareFixesForSorting`, the per-`node_modules`-bucket shadowing walk, and the
  `package.json`-dependency allow-list. blocked-by: `Registry` buckets + program.
- **`util.go` vfs/checker helpers** — `getResolvedPackageNames`,
  `getPackageNamesInNodeModules`, `getPackageRealpathFuncs`, `createCheckerPool`,
  `addPackageJsonDependencies`, `getModuleResolver`. blocked-by: the program /
  module resolver / checker host (the vfs-backed `util_test.go` cases go with
  them).
