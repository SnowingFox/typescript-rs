//! `tsgo_ls_autoimport`: the auto-import export index + module-specifier engine.
//!
//! Ports the reachable core of Go's `internal/ls/autoimport` package: the
//! cross-file export index that powers auto-import completions and the "add
//! missing import" code fix, the module-specifier selection that chooses the
//! import path, and the import-candidate computation that turns an unresolved
//! name into ranked `import { X } from "..."` suggestions.
//!
//! # Reachable subset (read this first)
//!
//! Go's package extracts exports through the **type checker** (symbol tables,
//! alias resolution, ambient-module merging) and tracks them in an incremental,
//! dirty-map-backed `Registry` of project / `node_modules` buckets. The checker
//! (`tsgo_checker`), compiler program (`tsgo_compiler`), and the `ls/change`
//! edit applier are not available to this crate yet, so this round ports the
//! genuinely reachable spine:
//!
//! * [`index`] — the prefix/word index (`Index<T>`) — a verbatim 1:1 port of
//!   `index.go` (depends on nothing but the standard library).
//! * [`util`] — `word_indices`, the camelCase/snake_case word splitter.
//! * [`export`] — the `Export` value types (`ModuleId`, `ExportId`,
//!   `ExportSyntax`, `Export`).
//! * [`extract`] — a reachable **AST-walking** extractor that reads the
//!   top-level `export` declarations of a parsed file. This is the reachable
//!   analog of `extract.go`'s checker-driven `extractFromModule`; the
//!   symbol/alias/ambient-module paths are deferred until `tsgo_checker` lands.
//! * [`registry`] — building an index over several files.
//! * [`specifiers`] — `get_module_specifier`, the reachable tail of
//!   `View.GetModuleSpecifier` that calls `tsgo_modulespecifiers`.
//! * [`view`] — `find_import_candidates`, the reachable analog of
//!   `View.Search` / `View.GetCompletions`.
//!
//! See `docs/rust-rewrite/phase-7-language-service/autoimport/worklog.md` for
//! the full DEFER list and the blocked-by dependencies.

pub mod export;
pub mod extract;
pub mod index;
pub mod registry;
pub mod specifiers;
pub mod util;
pub mod view;

pub use export::{Export, ExportId, ExportSyntax, ModuleId};
pub use extract::extract_top_level_exports;
pub use index::{Index, Named};
pub use registry::{build_index_for_files, FileInput};
pub use specifiers::get_module_specifier;
pub use util::word_indices;
pub use view::{find_import_candidates, search_index, ImportCandidate, QueryKind};
