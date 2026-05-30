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
//! - `namedevaluation` landed a 6d-2 subset (`var f = <anonymous fn>` →
//!   `__setFunctionName(<fn>, "f")`) as the end-to-end validation of the new
//!   printer emit-helper infrastructure (`request_emit_helper` + unscoped
//!   helper-name + prologue emit, ported in 6d-2). DEFER: the full
//!   `isNamedEvaluation` surface (property/shorthand/parameter/binding-element/
//!   property-declaration/export=, computed-name `__propKey` caching, anonymous
//!   class `static { __setFunctionName(this, …) }` blocks) + `EmitContext`
//!   assigned-name tracking + target/`useDefineForClassFields` gating.
//! - `async` landed a 6d-3 subset: a top-level **async function declaration**
//!   lowers to the `__awaiter(this, void 0, void 0, function* () { … })` wrapper
//!   with `await X` → `yield X` in the (direct) body. DEFER: async methods/
//!   accessors/arrows, async generators (`__asyncGenerator`), super/lexical-
//!   `this`/`arguments` capture, default/rest parameter handling, and top-level
//!   `await` — these need the `EmitContext` super-capture + parameter/variable-
//!   environment machinery not yet ported.
//! - `forawait`. DEFER (scope): `for await (x of y)` has no minimal tracer — the
//!   faithful lowering always emits the full async-iteration scaffold
//!   (`__asyncValues` iterator + downlevel-`await` of `.next()` +
//!   generated-name iterator/result/value temps + an `iterator.return` cleanup
//!   nested `try/finally`). The `__asyncValues`/`__await` helpers exist, but the
//!   generated-name + `.call`/`convertForOfStatementHead` scaffolding is too
//!   large to land faithfully as a tracer this round; a partial version would
//!   emit broken code. blocked-by: the async-iteration lowering scaffold.
//! - `using`. DEFER (parser): the `tsgo_parser` crate does not parse
//!   statement-level `using x = expr;` (reports "';' expected"), so the stage
//!   cannot be exercised through the parse→transform→emit path, and the parser
//!   is out of this round's edit scope. The transform itself (try/finally +
//!   `__addDisposableResource`/`__disposeResources`, both helpers ported) is
//!   portable once the parser supports `using`. blocked-by: parser `using`
//!   declaration support. (`await using` additionally needs async disposal.)
//! - `esdecorator`. blocked-by: checker metadata.
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

pub mod r#async;
pub mod classfields;
pub mod exponentiation;
pub mod namedevaluation;
pub mod objectrestspread;
pub mod optionalchain;
