//! `tsgo_ls_autoimport`: the auto-import export index + import-fix engine.
//!
//! Ports Go's `internal/ls/autoimport` package (the cross-file export registry/index
//! that powers auto-import completions and the "add missing import" code fix, plus
//! the module-specifier selection and import-statement insertion helpers). Skeleton
//! crate registered ahead of the P7 autoimport round; filled in there.
