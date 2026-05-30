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
//! static fields); 6c-4 closed out the reachable `classfields` surface —
//! **private instance fields** (`#x`, direct `WeakMap` `.get`/`.set` form) and
//! **computed instance-field names** (key cached in a hoisted temp). Round 6d
//! landed the two **helper-free** es stages: `optionalchain` (`a?.b` →
//! not-null-guarded conditional) and `objectrestspread` (object spread →
//! `Object.assign`).
//!
//! Deferred (DEFER(P5), see each `blocked-by`):
//!
//! - `objectrestspread` object **rest** binding (`const { a, ...rest } = o` →
//!   `__rest`), rest in parameters / `for-of` / `catch` / assignment patterns.
//!   blocked-by: the `__rest` helper-library emit + the destructuring
//!   transformer, not yet ported.
//! - `optionalchain` chains needing a hoisted temp (non-simple receiver like
//!   `f()?.b`, multiple `?.` like `a?.b?.c`), `this`-capture for parenthesized
//!   optional calls (`(a?.b)()`), tagged templates, and `delete a?.b`.
//!   blocked-by: receiver temp hoisting must be threaded through the visit
//!   (var-environment like `exponentiation`), and `this`-capture needs the
//!   `SyntheticReferenceExpression` machinery, not yet ported.
//! - `namedevaluation`, `using`, `forawait`, `async` (and `esdecorator`).
//!   blocked-by: **the helper-library emit infrastructure is not ported** —
//!   `printer::EmitContext::RequestEmitHelper`, unscoped helper-name nodes
//!   (`NewUnscopedHelperName`), and the helper-definition prologue emit. Every
//!   output path of these stages references a TS runtime helper
//!   (`__setFunctionName`/`__propKey`; `__addDisposableResource`/
//!   `__disposeResources`; `__asyncValues`/`__await`; `__awaiter`/`__generator`),
//!   so none has a helper-free tracer. These need a dedicated printer
//!   helper-emit-infrastructure round first; `async`/`forawait` additionally
//!   need the await→yield generator-body state machine and `esdecorator` needs
//!   checker metadata.
//!
//! - `classfields` named-helper private form (`__classPrivateFieldGet/Set`),
//!   private static fields, private methods/accessors (`WeakSet`), `accessor`
//!   fields, class expressions, parameter properties, prologue directives,
//!   anonymous-class members, name-generator-backed temp/brand uniqueness, and
//!   `--target`/`useDefineForClassFields` gating. blocked-by: the named-helper
//!   form needs helper-library import emit; `accessor` fields need the
//!   emit-context name generator (generated backing private name) + get/set
//!   redirector synthesis + a second-pass result visitor; class expressions
//!   need IIFE/comma-sequence statement hoisting; parameter properties are a
//!   `tstransforms` concern; collision-free temps/brands need the name
//!   generator; the rest need checker info not yet ported.
//! - `exponentiation` `**=` temp-hoisting targets nested in non-top-level
//!   scopes. blocked-by: scope-level variable-environment nesting (function
//!   bodies) is not yet wired through the visit.
//! - `definitions` (`GetESTransformer` target dispatch + the per-version
//!   transformer chains). blocked-by: depends on every es stage being ported.
//! - `classthis` / `nullishcoalescing` / `logicalassignment` / `optionalcatch`
//!   / `taggedtemplate` / `usestrict`. blocked-by: larger `printer::NodeFactory`
//!   constructor surface (and, for some, helper-emit) not yet ported.

pub mod classfields;
pub mod exponentiation;
pub mod objectrestspread;
pub mod optionalchain;
