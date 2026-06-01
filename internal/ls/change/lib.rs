//! `tsgo_ls_change`: the language-service text-change tracker.
//!
//! Ports Go's `internal/ls/change` package (the `ChangeTracker` that records
//! insert/delete/replace edits over the AST — with trivia-aware node deletion and
//! formatting — and produces `TextChange` edits for code fixes and refactors).
//! Skeleton crate registered ahead of the P7 ls/change round; filled in there.
