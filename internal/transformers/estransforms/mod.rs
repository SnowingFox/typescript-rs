//! Port of Go `internal/transformers/estransforms`: the ECMAScript
//! down-leveling stages selected by `GetESTransformer` per `--target`
//! (exponentiation, class fields, decorators, optional chaining, nullish
//! coalescing, object rest/spread, `for await`, logical assignment, `using`,
//! tagged templates, optional catch, `use strict`).
//!
//! Round 6c-1 landed the `exponentiation` tracer and the simplest `classfields`
//! lowering; 6c-2 completed the `classfields` constructor-insertion family;
//! 6c-3 added the two reusable infra tracks — the `EmitContext` variable
//! environment (temp hoisting, consumed by `exponentiation` `**=` element /
//! property targets) and the `SyntaxList` node kind (consumed by `classfields`
//! static fields).
//!
//! Deferred (DEFER(P5), see each `blocked-by`):
//!
//! - `classfields` private names (`#x`), `accessor` fields, computed property
//!   names, class expressions, parameter properties, prologue directives,
//!   anonymous-class static fields, and `--target`/`useDefineForClassFields`
//!   gating. blocked-by: private names need a private-environment map + private
//!   access expression rewriting + WeakMap brand naming (and the
//!   `__classPrivateFieldGet/Set` helper-library import for the full form); the
//!   rest need checker info or further factory surface not yet ported.
//! - `exponentiation` `**=` temp-hoisting targets nested in non-top-level
//!   scopes. blocked-by: scope-level variable-environment nesting (function
//!   bodies) is not yet wired through the visit.
//! - `definitions` (`GetESTransformer` target dispatch + the per-version
//!   transformer chains). blocked-by: depends on every es stage being ported.
//! - `classthis` / `namedevaluation` / `async` / `forawait` / `using` /
//!   `optionalchain` / `nullishcoalescing` / `objectrestspread` /
//!   `logicalassignment` / `optionalcatch` / `taggedtemplate` / `usestrict` /
//!   `esdecorator`. blocked-by: larger `printer::NodeFactory` constructor and
//!   helper-emit surface (and, for `esdecorator`, checker metadata) not yet
//!   ported.

pub mod classfields;
pub mod exponentiation;
