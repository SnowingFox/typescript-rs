//! Port of Go `internal/transformers/moduletransforms`: the module-format
//! transforms (CommonJS, ESM, System) and their shared import/export analysis.
//!
//! Round 6e lands the shared, structural [`externalmoduleinfo`] analysis
//! ([`collect_external_module_info`](externalmoduleinfo::collect_external_module_info)):
//! it scans a module's top-level statements for external imports, exported
//! names, `export *`, and `export =`. This is the foundation both the CommonJS
//! and ESM transforms consume.
//!
//! Round 6e-2 unblocked the use-site rewrite path: the printer now has an
//! emit-time **node-substitution** hook and `TransformOptions` carries
//! `compiler_options`, so [`commonjsmodule`] lands a validating subset
//! (`import { x } from "m"; x;` → `const m_1 = require("m"); m_1.x;`).
//!
//! Deferred (DEFER(P5), see each `blocked-by`):
//!
//! - `commonjsmodule` remaining surface: the full `__esModule` external-module
//!   gating (import-only modules), function-export hoisting/ordering across
//!   external-helpers imports, the `export import x = require()` /Node16+
//!   `import =` forms, dynamic `import()`, and `export * as ns`. Rounds 6e-2…6x
//!   landed the reachable structural subset (import/export lowering + interop
//!   helpers, `export =`, exported function/class declarations, `import =` →
//!   `const require`, and the `exports.<name> = void 0` export-name init). The
//!   `"use strict"` prologue is a *separate* transform — see
//!   [`crate::estransforms::usestrict`] (6x), matching Go's pipeline. blocked-by:
//!   most remaining items need further factory/helper surface; **scope-correct**
//!   import-use rewriting needs a real `ReferenceResolver` (the ported one is a
//!   no-op placeholder; the 6e-2 validation matches uses by name).
//! - real `ReferenceResolver` (use-site → declaration resolution). blocked-by:
//!   checker `resolveName`/`EmitResolver` — the binder produces declaration
//!   symbols but not scope-aware reference resolution, which is checker work.
//! - `externalmoduleinfo` resolver-dependent classification (function-vs-binding
//!   for `export { x }`, `exportedBindings`/`exportedFunctions`). blocked-by:
//!   the no-op `ReferenceResolver`.

pub mod commonjsmodule;
pub mod esmodule;
pub mod externalmoduleinfo;
pub mod impliedmodule;
pub mod systemmodule;
