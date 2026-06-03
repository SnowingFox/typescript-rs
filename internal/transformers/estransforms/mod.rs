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
//! `Object.assign`). Round 6g deepened `objectrestspread` to consume the
//! `__rest` helper for object **rest** bindings in variable declarations
//! (`var { a, ...rest } = o;` → `var { a } = o, rest = __rest(o, ["a"]);`),
//! threading the emit context like `async` (helper request + source-file
//! attach + prologue emit).
//!
//! Deferred (DEFER(P5), see each `blocked-by`):
//!
//! - `objectrestspread` object **rest** binding beyond the 6g variable-
//!   declaration subset: the generic `FlattenDestructuringBinding`
//!   (nested/array binding patterns, default values, computed property keys
//!   needing temp caching, non-simple initializers needing a hoisted temp) =
//!   round-6h `destructuring.go`; rest in parameters / `for-of` / `catch` /
//!   assignment-destructuring patterns (need the parameter / assignment
//!   flatteners + `FlattenDestructuringAssignment`).
//! - `optionalchain` landed a 6d+6h+6i subset: 6d lowered single-`?.` chains
//!   with a simple receiver; 6h deepened it (ec-threaded, var-environment like
//!   `exponentiation`) with **receiver temp-hoisting** (`f()?.b` → `var _a;
//!   (_a = f()) === null || _a === void 0 ? void 0 : _a.b`) and **multiple
//!   `?.`** (`a?.b?.c` → nested guards, one temp per link). 6i wired
//!   **per-scope variable environments**, threading the emit context through
//!   function-like bodies (function declarations / expressions, arrow bodies,
//!   class methods) so a temp-hoisting chain inside such a body lands in *that*
//!   body's leading `var ...;` rather than at module top. DEFER: temp-hoisting
//!   chains nested in *still-unthreaded* positions (control-flow statement
//!   bodies like `if`/`for`/`while`, `switch` cases, object-literal method
//!   shorthands, constructor / accessor bodies), `this`-capture for
//!   parenthesized optional calls (`(a?.b)()`), tagged templates, and
//!   `delete a?.b`. blocked-by: threading the emit context through the
//!   remaining statement/member kinds, and `this`-capture needs the
//!   `SyntheticReferenceExpression` machinery, not yet ported.
//! - `namedevaluation` landed a 6d-2 subset (`var f = <anonymous fn>` →
//!   `__setFunctionName(<fn>, "f")`) as the end-to-end validation of the new
//!   printer emit-helper infrastructure (`request_emit_helper` + unscoped
//!   helper-name + prologue emit, ported in 6d-2). DEFER: the full
//!   `isNamedEvaluation` surface (property/shorthand/parameter/binding-element/
//!   property-declaration/export=, computed-name `__propKey` caching, anonymous
//!   class `static { __setFunctionName(this, …) }` blocks) + `EmitContext`
//!   assigned-name tracking + target/`useDefineForClassFields` gating.
//! - `async` landed a 6d-3 + 6m subset: 6d-3 lowered a top-level **async
//!   function declaration** to the `__awaiter(this, void 0, void 0,
//!   function* () { … })` wrapper with `await X` → `yield X` in the (direct)
//!   body. 6m extended it (via an emit-context-threaded `VisitEachChild`) to
//!   **async function expressions** and **async methods** (same wrapper, lexical
//!   `this` → first arg `this`) and **async arrows** (concise-body arrow
//!   returning the `__awaiter(…)` call directly; at module top there is no
//!   lexical `this`, so the first arg is `void 0`). DEFER: async accessors,
//!   async generators (`__asyncGenerator`), `super` in async methods (needs a
//!   `_super` binding), threading `asyncContextHasLexicalThis` through nested
//!   scopes (an arrow inside an async method should thread `this`), lexical-
//!   `arguments`/`_this` capture, default/rest parameter handling, `for await`,
//!   and top-level `await` — these need the `EmitContext` super-capture +
//!   parameter machinery and the async-generator helpers not yet ported.
//! - `forawait` landed a 6y subset: the ES2018 **async-generator function
//!   declaration** lowering (`async function* g() { ... }` → `function g() {
//!   return __asyncGenerator(this, arguments, function* g_1() { ... }); }`, with
//!   `await x` → `yield __await(x)`, `yield e` → `yield yield __await(e)`,
//!   `yield* e` → `yield __await(yield* __asyncDelegator(__asyncValues(e)))`,
//!   `return e` → `return yield __await(e)`). The `__await`/`__asyncGenerator`/
//!   `__asyncDelegator`/`__asyncValues` helper definitions live in `forawait.rs`
//!   itself (the `tsgo_printer` crate is out of that round's edit scope). 6z
//!   landed the **`for await (x of y)` downlevel** inside an async (non-
//!   generator) function: the full async-iteration scaffold — `__asyncValues`
//!   iterator temp + `result` temp + the C-style `for` with `result = await
//!   iterator.next(), done = result.done, !done` + loop variable bound from
//!   `result.value` + the `try/catch/finally` `iterator.return` cleanup (down-
//!   level `await`), with the `done`/`errorRecord`/`returnMethod`/`value` temps
//!   hoisted into the enclosing function body's variable environment. DEFER:
//!   `for await` with an **identifier source** (`for await (const x of y)`) —
//!   derives the iterator/result names from the source identifier and needs the
//!   printer's resolving `getTextOfNode` for the nested generated name (un-
//!   ported; the non-identifier `gen()` source uses clean `NewTempVariable`
//!   temps); `for await` inside an **async generator** (needs
//!   `createDownlevelAwait`'s `yield __await(...)` form = enclosing-function-
//!   flags threading); **destructuring** loop variables, **top-level** `for
//!   await`, **labeled**/`continue`/`break` interplay, the **nested-loop
//!   `errorRecord` reset**; async-generator **methods** / **function
//!   expressions** / **arrows** (need the `_super` binding plus `hasLexicalThis`
//!   hierarchy-facts threading; a function declaration always has its own
//!   `this`), non-simple parameter lists, the variable-environment merge, and
//!   top-level `await`. blocked-by: the printer's resolving name generation,
//!   enclosing-function-flags threading, the destructuring flattener, and the
//!   `EmitContext` super-capture plus parameter machinery.
//! - `using`. DEFER (parser): the `tsgo_parser` crate does not parse
//!   statement-level `using x = expr;` (reports "';' expected"), so the stage
//!   cannot be exercised through the parse→transform→emit path, and the parser
//!   is out of this round's edit scope. The transform itself (try/finally +
//!   `__addDisposableResource`/`__disposeResources`, both helpers ported) is
//!   portable once the parser supports `using`. blocked-by: parser `using`
//!   declaration support. (`await using` additionally needs async disposal.)
//! - `esdecorator`. blocked-by: checker metadata.
//! - `spread` landed a 6aa subset: the ES2015 **array-literal** spread
//!   (`[...a, b]` → `__spreadArray(__spreadArray([], a, true), [b], false)`, the
//!   nested segment fold with the array-literal `pack=true`/literal `pack=false`
//!   flags) and **call-argument** spread (`f(...args)` → `f.apply(void 0,
//!   args)`, `f(a, ...args)` → `f.apply(void 0, __spreadArray([a], args,
//!   false))` with the argument-list `pack=false` flag and the lone-spread
//!   shortcut; `o.m(...args)` → `o.m.apply(o, args)` capturing a plain
//!   identifier receiver as `this`). The `__spreadArray` helper definition lives
//!   in `spread.rs` (the `tsgo_printer` crate is out of edit scope, mirroring
//!   `forawait.rs`). The Go port has no ES2015 spread transform, so shapes are
//!   verified against `tsc --target es5`. DEFER: `new C(...args)` (construct +
//!   `bind.apply`), `super(...args)`, non-simple member receivers needing a
//!   capture temp, `--downlevelIteration` (`__read`/`__spread`); object spread
//!   is already in `objectrestspread` (6d/6g). blocked-by: the `new`-target bind
//!   form, `super` receiver capture, the `createCallBinding` temp-capture, and
//!   the iteration helpers. Also DEFER wiring into `GetESTransformer` (no
//!   `NewES2015Transformer` chain exists yet) — with the `definitions` port.
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
//!   scopes: 6i wired function declaration bodies (their own variable
//!   environment), so `function f() { a.x **= b; }` now hoists `var _a;` inside
//!   `f`. DEFER: `**=` targets nested in still-unthreaded positions
//!   (control-flow statement bodies, class methods / function expressions /
//!   arrows for this stage, nested classes). blocked-by: threading the emit
//!   context through those remaining kinds in `exponentiation`.
//! - `definitions` (`GetESTransformer` target dispatch + the per-version
//!   transformer chains). blocked-by: depends on every es stage being ported.
//! - `classthis` / `nullishcoalescing` / `logicalassignment` / `optionalcatch`
//!   / `taggedtemplate` / `usestrict`. blocked-by: larger `printer::NodeFactory`
//!   constructor surface (and, for some, helper-emit) not yet ported.

pub mod r#async;
pub mod classfields;
pub mod classthis;
pub mod exponentiation;
pub mod forawait;
pub mod logicalassignment;
pub mod namedevaluation;
pub mod nullishcoalescing;
pub mod objectrestspread;
pub mod optionalcatch;
pub mod optionalchain;
pub mod spread;
pub mod taggedtemplate;
pub mod usestrict;
pub mod using;
