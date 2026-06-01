//! `tsgo_ls`: the TypeScript language service.
//!
//! Ports Go's `internal/ls` package (the `LanguageService` + `Host` plumbing and
//! the per-feature providers). This is the **LS root** round: it establishes the
//! [`LanguageService`] + [`LanguageServiceHost`] plumbing and the first feature
//! providers — diagnostics ([`diagnostics`]) and quick-info/hover ([`hover`]) —
//! that resolve a token (via [`tsgo_astnav`]) and a type checker (via
//! [`tsgo_compiler`]/[`tsgo_checker`]) for a file and convert positions with
//! [`tsgo_ls_lsconv`].
//!
//! The remaining ~60 ls features (completions, definitions, find-all-references,
//! rename, code fixes, navigation bar, semantic tokens, folding, signature help,
//! call hierarchy, ...) are deferred to later ls rounds and build on this root.

mod host;
mod languageservice;

pub mod diagnostics;
pub mod hover;

pub use host::LanguageServiceHost;
pub use hover::QuickInfo;
pub use languageservice::LanguageService;

#[cfg(test)]
mod test_support;
