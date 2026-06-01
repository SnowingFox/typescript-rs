//! `tsgo_ls_change`: the language-service text-change tracker.
//!
//! 1:1 Rust port of the reachable subset of Go `internal/ls/change` (the
//! `ChangeTracker` that records insert/delete/replace edits over the AST — with
//! trivia-aware node deletion — and produces edits for code fixes and
//! refactors). Source files: [`tracker`] (`tracker.go`), [`trackerimpl`]
//! (`trackerimpl.go`), [`delete`] (`delete.go`).
//!
//! # What is `ChangeTracker`
//!
//! [`ChangeTracker`] accumulates edits ([`ChangeTracker::replace_range_with_text`],
//! [`ChangeTracker::insert_text`], [`ChangeTracker::delete_range`],
//! [`ChangeTracker::delete_node`], [`ChangeTracker::delete`]), tracking the
//! leading/trailing trivia of a deleted node so its surrounding
//! whitespace/comments are removed cleanly, and finally computes a sorted,
//! non-overlapping list of edits ([`ChangeTracker::get_changes`]).
//!
//! # Divergence from Go (deliberate, documented)
//!
//! - **Edits are byte-offset [`tsgo_core::textchange::TextChange`], not
//!   `lsproto.TextEdit`.** Go's `trackerEdit` embeds an `lsproto.Range`
//!   (0-based line / UTF-16 character) and converts byte offsets at every
//!   boundary via a `lsconv.Converters`; the final output is
//!   `map[string][]*lsproto.TextEdit`. This port keeps the tracker entirely in
//!   the compiler's native UTF-8 byte offsets ([`tsgo_core::text::TextRange`])
//!   and emits [`tsgo_core::textchange::TextChange`] (`range` + `new_text`),
//!   exactly the "span + new text" the slices assert. The line/character
//!   conversion is a thin boundary wrapper deferred to the `ls` root
//!   integration (blocked-by: a per-file `lsconv::Converters`/line-map plumbed
//!   through the program). The ordering/overlap logic is preserved 1:1 (sort by
//!   `(pos, end)`, panic on a true overlap).
//! - **The source file is a [`tsgo_astnav::RcSourceFile`]** (the committed
//!   shared-borrow navigation surface) rather than a `*ast.SourceFile`. Node
//!   ranges/kinds come from the nav context; node payloads (parent, statement
//!   list, declaration list) from its [`tsgo_astnav::NavEngine::arena`].
//!
//! # Deferred (blocked-by)
//!
//! - **Format-on-insert.** Every node-insertion / node-replacement operation
//!   (`ReplaceNode`, `ReplaceRange` with a node, `InsertNodeAt`/`Before`/`After`,
//!   `InsertAtTopOfFile`, `InsertMemberAtStart`, `TryInsertTypeAnnotation`,
//!   `InsertNodeInListAfter`, ...) reformats the inserted node text through
//!   `printer.ChangeTrackerWriter` + `format.FormatNodeGivenIndentation`. Those
//!   printer APIs are not ported, so the node-bearing `trackerEdit` kinds and
//!   `computeNewText`/`getFormattedTextOfNode`/`getNonformattedText` are
//!   deferred. blocked-by: `printer.ChangeTrackerWriter` /
//!   `format::format_node_given_indentation`.
//! - **`deleteNodeInList` + trailing-comma cleanup.** These call
//!   `format.GetContainingList`, which the `tsgo_format` port stubs to `None`
//!   (its list-detection is deferred). So comma-separated list deletion
//!   (parameters, type parameters, import specifiers, binding elements, call
//!   arguments) and the trailing-comma fixup are deferred. blocked-by:
//!   `format::indent::get_containing_list`.
//! - **Import-specific deletion** (`deleteImportBinding` / `deleteDefaultImport`)
//!   and the **insert-at-start / insert-after option** helpers, which need
//!   `ast` import-clause accessors and statement predicates not yet ported.
//!   blocked-by: `ast` import/statement accessors.
//! - **JSDoc-aware leading trivia.** The parser has not reparsed JSDoc, so
//!   `has_jsdoc_nodes` is always `false` and the `LeadingTriviaOption::JSDoc`
//!   branch is inert. blocked-by: JSDoc reparser (`tsgo_parser`).
//!
//! See the phase-7 worklog under `docs/rust-rewrite/` for the full list.

mod delete;
mod tracker;
mod trackerimpl;

pub use tracker::{ChangeTracker, LeadingTriviaOption, TrailingTriviaOption};
