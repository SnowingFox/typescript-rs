//! `tsgo_ls`: the TypeScript language service.
//!
//! Ports Go's `internal/ls` package (the `LanguageService` + `LanguageServiceHost`
//! and the per-feature providers: completions, hover/quick-info, definitions,
//! find-all-references, rename, diagnostics, code fixes, navigation, folding,
//! semantic tokens, etc.). Skeleton crate registered ahead of the P7 ls rounds;
//! the service and its features are filled in there.
