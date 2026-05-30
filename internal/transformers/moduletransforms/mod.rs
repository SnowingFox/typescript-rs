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
//! - `commonjsmodule` full surface (default/namespace import interop,
//!   `export` lowering, `__esModule` marker / `"use strict"` / hoisting,
//!   `export =`, dynamic `import()`, re-exports) and the whole `esmodule`
//!   transform. blocked-by: most need further factory/helper surface; **scope-
//!   correct** import-use rewriting needs a real `ReferenceResolver` (the ported
//!   one is a no-op placeholder; the 6e-2 validation matches uses by name).
//! - real `ReferenceResolver` (use-site → declaration resolution). blocked-by:
//!   checker `resolveName`/`EmitResolver` — the binder produces declaration
//!   symbols but not scope-aware reference resolution, which is checker work.
//! - `externalmoduleinfo` resolver-dependent classification (function-vs-binding
//!   for `export { x }`, `exportedBindings`/`exportedFunctions`). blocked-by:
//!   the no-op `ReferenceResolver`.

pub mod commonjsmodule;
pub mod externalmoduleinfo;
