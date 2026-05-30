//! Port of Go `internal/transformers/moduletransforms`: the module-format
//! transforms (CommonJS, ESM, System) and their shared import/export analysis.
//!
//! Round 6e lands the shared, structural [`externalmoduleinfo`] analysis
//! ([`collect_external_module_info`](externalmoduleinfo::collect_external_module_info)):
//! it scans a module's top-level statements for external imports, exported
//! names, `export *`, and `export =`. This is the foundation both the CommonJS
//! and ESM transforms consume.
//!
//! Deferred (DEFER(P5), see each `blocked-by`):
//!
//! - `commonjsmodule` / `esmodule` transforms (`import`→`require`,
//!   `export`→`exports.x`, interop helpers). blocked-by: the **emit
//!   substitution** infrastructure (`onSubstituteNode`, not ported) needed to
//!   rewrite import *uses* (`x` → `m_1.x`); a real `ReferenceResolver` (the
//!   ported one is a no-op placeholder); `compilerOptions` threading through
//!   `TransformOptions` (currently only carries the emit context) for module
//!   kind / `esModuleInterop`; and the `GetExternalHelpersModuleName` /
//!   external-module-indicator surface. Without these, only trivial,
//!   binding-free cases would be correct, so the transforms are deferred.
//! - `externalmoduleinfo` resolver-dependent classification (function-vs-binding
//!   for `export { x }`, `exportedBindings`/`exportedFunctions` from the
//!   resolver). blocked-by: the no-op `ReferenceResolver`.

pub mod externalmoduleinfo;
