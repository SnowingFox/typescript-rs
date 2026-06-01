# ls — signature help worklog (P7 `tsgo_ls`)

> P7 `ls` (root crate `tsgo_ls`) round: **signature help** (the parameter-hints
> popup shown inside a call's argument list). Strict TDD (red→green vertical
> slices). Crate-scoped gates only (`-p tsgo_ls`). A concurrent lane was editing
> `internal/testrunner/**` (a **disjoint** crate, not on this lane's build
> path), so this round touched **only** `internal/ls/**` root-crate files (NOT
> the `lsconv`/`lsutil`/`change`/`autoimport` sub-crates' source) + this doc.
> **No root `Cargo.toml` edit, no new dependency, no other crate's source
> touched.** Every dependency needed (`tsgo_checker`, `tsgo_ast`, `tsgo_astnav`,
> `tsgo_core`, `tsgo_lsproto`, `tsgo_parser`) was already declared.

This round ports the reachable subset of Go's `internal/ls/signaturehelp.go`
(`ProvideSignatureHelp` / `GetSignatureHelpItems` / `getContainingArgumentInfo` /
`createSignatureHelpItems`), building on the round-1 `Converters` +
`file_check_context` (token + checker bridge) and the `hover` / `completions`
resolution chain (`get_symbol_at_location` → `get_type_of_symbol` →
`type_to_string`).

## What landed

| File | Go source | What |
|---|---|---|
| `signaturehelp.rs` | `internal/ls/signaturehelp.go` | `provide_signature_help` (the `ProvideSignatureHelp` / `GetSignatureHelpItems` entry, reachable subset) → `Option<SignatureHelp>`; `signature_help_at` (the body); `find_enclosing_call` (`getContainingArgumentInfo` / `getImmediatelyContainingArgumentInfo` reachable walk); `call_parts` (`getExpressionFromInvocation` + `getChildListThatStartsWithOpenerToken`); `build_signature_help` (`createSignatureHelpItems`); `render_signature` (`getSignatureHelpItem` / `itemInfoForParameters` / `returnTypeToDisplayParts`); `active_parameter_index` (`getArgumentIndexOrCount`); the local `SignatureHelp` / `SignatureInformation` / `ParameterInformation` data records |
| `signaturehelp_test.rs` | — | 11 unit tests |
| `lib.rs` | `internal/ls/lib.rs` | `pub mod signaturehelp;` + `pub use signaturehelp::{ParameterInformation, SignatureHelp, SignatureInformation};` |

Public API is **additive within `tsgo_ls`**: one new `pub fn` on
`LanguageService` (`provide_signature_help`), three new public types
(`SignatureHelp`, `SignatureInformation`, `ParameterInformation`), and the
`pub mod signaturehelp;` registration + re-export. Every other item in
`signaturehelp.rs` is crate-private (`fn` / `const`), not public API. No existing
public item changed; **no existing test was weakened or deleted** (rule 5).

## Architecture: reuse of the round-1 resolution chain

Go's signature help is: find the call whose argument list spans the cursor, get
its candidate signatures, pick the active signature + active parameter, and
render each signature's label. The reachable subset wires the common case (a
non-overloaded `f(...)` call) through the already-ported pieces:

- **Find the enclosing call** (`find_enclosing_call`) = the reachable subset of
  `getContainingArgumentInfo`. Go walks up `startingToken.Parent`; the Rust `(`
  and `,` tokens are *synthesized* by `astnav` (the AST stores no such child, so
  they have **no arena parent** — the same constraint `completions`' dot recovery
  hit). So the walk anchors on the nearest **real** node at/before the position
  (stepping left off a synthesized `(`/`,` to the callee or previous argument)
  and climbs its arena parents to the nearest `CallExpression`/`NewExpression`
  whose argument-list region (`position > callee.End() && position <= call.End()`)
  contains the cursor. The region guard reproduces Go's `findContainingList`
  returning nil when the cursor is on the call target itself.
- **Resolve the signatures** (`build_signature_help`) = the reachable subset of
  `createSignatureHelpItems` + `getCandidateOrTypeInfo`. It types the callee
  (`get_symbol_at_location` → `get_type_of_symbol`, the same chain `hover` /
  `completions` use) and reads its call signatures via the public
  `Checker::get_signatures_of_type`. The call-target name comes from
  `symbol_to_string` (Go's `c.SymbolToString(callTargetSymbol)`).
- **Render each signature** (`render_signature`) = the reachable subset of
  `getSignatureHelpItem` / `itemInfoForParameters` / `returnTypeToDisplayParts`:
  `name(p1: T1, p2: T2): R`, where each parameter is rendered as `name: type`
  (parameter name from the bound program, type via `type_to_string`) and the
  return type via `type_to_string`. Each parameter is one `ParameterInformation`
  whose label is the `name: type` substring (Go's string-form
  `signatureHelpParameter` from `createSignatureHelpParameterFromLabel`).
- **Active parameter** (`active_parameter_index`) = the reachable subset of
  `getArgumentIndexOrCount`: the count of arguments whose end is before the
  cursor (the cursor is "in" the first argument ending at/after it; a
  trailing-comma empty slot leaves the cursor past every argument → the argument
  count). `active_signature` is `Some(0)` (overload selection deferred).

### One cross-crate constraint handled in this lane

**`lsproto` has no `SignatureHelp` / `SignatureInformation` /
`ParameterInformation`** (only `SignatureHelpOptions`, the server capability),
and this lane may not edit `lsproto`. So the three LSP-shaped types are defined
locally in `signaturehelp.rs` (mirroring `lsproto.SignatureHelp` et al.) — the
same approach `documenthighlights` / `symbols` / `completions` take for their
feature types. `ParameterInformation.label` uses the string form (Go's
`StringOrTuple{String}`); the label-offset tuple form is deferred.

## RED → GREEN slices (observed symptoms)

1. **Basic `f(|)` (cursor in the empty argument list) →
   `f(a: number, b: string): void`, active parameter 0.** RED: the
   `signature_help_at` stub returned `None`; `.expect("signature help for \`f(|)\`")`
   panicked. GREEN: the parent walk anchors on the callee `f`, climbs to the
   `CallExpression`, types `f` (a function symbol → anonymous object type with
   one call signature), and renders the label.
2. **Active parameter advances (`f(1, |)` → 1; `f(1, "x"|)` → 1).** RED:
   `active_parameter` was hard-coded to `Some(0)` in slice 1, so both asserts
   read `Some(0)` (expected `Some(1)`). GREEN: `active_parameter_index` counts
   the completed arguments before the cursor.
3. **Parameter labels (`a: number` / `b: string`).** The per-parameter
   `ParameterInformation` labels were produced as part of slice 1's
   `getSignatureHelpItem` port (the same `name: type` strings the label is built
   from); slice 3 adds the explicit assertion on the `parameters` vector and that
   each label is a substring of the signature label (the LSP string-label
   contract). GREEN on first run.
4. **`new C(|)` on a class → `None` (construct signatures deferred); cursor on
   the call target (`f|(1)`) → `None`.** GREEN guards: the structural detection
   *does* find the `NewExpression` (and the call for the target case), but a
   class value symbol's type is its **instance** type (no call signatures) and
   the only public checker API returns *call* — not construct — signatures, so
   `new C(|)` resolves to `None` without a panic; the call-target case is
   excluded by the `position > callee.End()` region guard. These assert the
   active call-detection produces **no false positive**.
5. **Position outside any call (`const x = 1; x`, cursor on `x`) → `None`.**
   GREEN guard: the parent walk finds no `CallExpression`/`NewExpression`
   ancestor → `None`, no panic.

(Slices 4–5 are negative/guard tests: they passed against the `None` stub *and*
the real implementation, so their value is regression protection against the
now-active call detection producing spurious help, not a fresh red→green.)

## Extra behavioral tests (only MORE than Go, never fewer)

- **No-parameter function** (`function h(): void {}` → `h()`): label `h(): void`,
  empty `parameters`, active parameter 0.
- **Property-access callee** (`o.m(|)` on `interface I { m(a: number): void }`):
  resolves through the `PropertyAccessExpression` callee to the method `m`, label
  `m(a: number): void`, parameter `a: number`.
- **Unknown file** → `None` (no panic).
- **`active_parameter_index` unit** (`f(1, 22, 333)`): a focused unit test
  (parses the snippet, gets the argument list via `call_parts`) asserting the
  index at six positions (inside / at-end of each argument, and a past-all
  trailing slot → the argument count).

## Go functions mirrored (`// Go:` anchors)

- `signaturehelp.go:ProvideSignatureHelp` / `GetSignatureHelpItems` (the entry +
  dispatch body, reachable subset), `getContainingArgumentInfo` /
  `getImmediatelyContainingArgumentInfo` (the enclosing-call walk),
  `getExpressionFromInvocation` + `getChildListThatStartsWithOpenerToken`
  (callee + argument-list extraction), `createSignatureHelpItems` (resolve +
  flatten), `getSignatureHelpItem` / `itemInfoForParameters` /
  `returnTypeToDisplayParts` (label rendering),
  `createSignatureHelpParameterFromLabel` (the string-form parameter label), and
  `getArgumentIndexOrCount` (the active-parameter counting).
- `internal/checker/checker.go:getSymbolAtLocation` / `getTypeOfSymbol` /
  `getSignaturesOfType` / `symbolToString` / `typeToString` (reused from
  `tsgo_checker`).
- `astnav` `FindPrecedingToken` (reused from `tsgo_astnav`).

## Test deltas

Crate was at **69** unit tests. Now **80** unit tests (+0 doctests), all green:

- `signaturehelp_test.rs` — 11 (basic empty arg list; active param after comma;
  active param inside second arg; parameter labels; `new C` → None;
  cursor-on-call-target → None; outside-any-call → None; unknown-file → None;
  no-parameters; property-access callee; the `active_parameter_index` unit).

Every new `pub fn` / resolution path has a behavioral test plus negative/edge
coverage; no existing test was weakened or deleted (rule 5).

## Gates (crate-scoped, all GREEN)

```
cargo test  -p tsgo_ls                               # 80 passed; 0 failed (+ 0 doctests)
cargo clippy -p tsgo_ls --all-targets -- -D warnings # clean
cargo fmt   -p tsgo_ls -- --check                    # clean
cargo build -p tsgo_ls                               # ok
```

(`--workspace` was intentionally not run — a concurrent disjoint
`internal/testrunner/**` lane was active.)

## DEFER list (blocked-by → future ls rounds)

- **Overloaded-call signature selection** — Go's
  `GetResolvedSignatureForSignatureHelp` (overload resolution) +
  `createSignatureHelpItems`' `selectedItemIndex` arity loop (picking the active
  signature among multiple candidates). The reachable subset emits every call
  signature with `activeSignature = 0`. blocked-by: `getResolvedSignature` /
  overload resolution.
- **Generic signature instantiation display** — type parameters (`<T, U>`),
  the `getExpandedParameters` rest-tuple expansion, and instantiated parameter /
  return types. The reachable subset renders the un-instantiated declared
  parameter / return types. blocked-by: generic call-site inference + the
  node-builder parameter-declaration printer.
- **Type-argument signature help (`f<|>`)** — Go's `typeArgsInvocation` /
  `getPossibleGenericSignatures` / `itemInfoForTypeParameters` (the
  `isTypeParameterList` path) and `createTypeHelpItems`. blocked-by: the
  type-argument argument-info case + `GetLocalTypeParametersOfClassOrInterfaceOrTypeAlias`.
- **JSX-attribute / tagged-template signature help** — Go's
  `IsJsxOpeningLikeElement` branch and the `TaggedTemplateExpression` /
  `TemplateSpan` argument-info cases (`getArgumentListInfoForTemplate` /
  `getArgumentIndexForTemplatePiece`). blocked-by: the JSX intrinsic surface +
  tagged-template argument modelling.
- **Contextual signatures for callbacks** — Go's `tryGetParameterInfo` /
  `getContextualSignatureLocationInfo` (showing a callback's signature inside its
  own parameter list, via `GetContextualType` / `GetContextualTypeForObjectLiteralElement`).
  blocked-by: contextual typing (`GetContextualType`).
- **Constructor (`new C(...)`) signatures** — a class value symbol's
  `get_type_of_symbol` is its **instance** type (no call signatures), and the
  only public checker API (`Checker::get_signatures_of_type`) returns *call*, not
  construct, signatures. So `new C(|)` yields no help yet. blocked-by: class
  construct-signature collection + the static-side (`typeof C`) class value type
  + a public construct-signature accessor.
- **Documentation / JSDoc-tag rendering** — Go's `getDocumentationFromDeclaration`
  (the signature / per-parameter `Documentation`) and the VS colorized label
  (`ColorizedRuns` / `displayPartsWriter`). blocked-by: the JSDoc reparser + the
  display-parts writer.
- **Client-capability handling** — Go's per-signature `activeParameter`
  (`computeActiveParameter` with `ActiveParameterSupport` /
  `NoActiveParameterSupport`) and the variadic active-parameter clamping /
  middle-rest null. The reachable subset sets the top-level `active_parameter`
  only. blocked-by: the `GetClientCapabilities` signature-help surface + rest
  parameters / `HasEffectiveRestParameter`.
- **`SignatureHelpContext` / trigger-reason filtering** — Go's
  `signatureHelpTriggerReasonKind` (`onlyUseSyntacticOwners` / the in-string /
  in-comment bail-out, `isSyntacticOwner`). The reachable subset always resolves
  (manual-invoke semantics). blocked-by: `IsInString` / `isInComment` /
  `SignatureHelpContext` wiring.
- **Trailing-comma / whitespace active-parameter edge cases** beyond the
  reachable subset (spread-element counting `getSpreadElementCount`, the
  skip-comma logic of `getArgumentIndexOrCount`). blocked-by: tuple/spread types.
- **The JS named-declaration fallback** — Go's `createJSSignatureHelpItems` /
  `findSignatureHelpFromNamedDeclarations` for untyped JS. blocked-by: a
  multi-file program scan + JS heuristics.
- **`SignatureHelp` / `SignatureInformation` / `ParameterInformation` hoist into
  `lsproto`** — the locally-defined types should move into `tsgo_lsproto` once
  that crate generates them. blocked-by: `tsgo_lsproto` is owned by a different
  crate/lane (not editable here) and has not yet ported them.

This round establishes the signature-help core (enclosing-call detection +
call-signature resolution + `name(params): return` label rendering + active
parameter) that the later overload-selection, generics-display, type-argument,
JSX/tagged-template, contextual-callback, and documentation rounds will build on.
