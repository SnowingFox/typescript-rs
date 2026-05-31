//! Port of Go `internal/transformers/tstransforms`: the TypeScript→JavaScript
//! lowering stages (type erasure, enum/namespace runtime syntax, legacy
//! decorators, decorator metadata, type serialization).
//!
//! Round 6b landed `typeeraser`'s type-annotation-stripping subset and
//! `tstransforms/utilities`. Round 6c-prep added the shared lowering primitives
//! (removal-aware [`visit_nodes_removable`](tsgo_ast::NodeArena::visit_nodes_removable)
//! plus the [`NotEmittedStatement`](tsgo_ast::Kind::NotEmittedStatement) /
//! [`PartiallyEmittedExpression`](tsgo_ast::Kind::PartiallyEmittedExpression)
//! node kinds) and used them to complete `typeeraser` elision (type-only
//! declarations, ambient/overload signatures, type-only modifiers,
//! `implements`, `this` parameters, assertions, `import type`).
//!
//! Still deferred:
//!
//! - `typeeraser` per-specifier `import { type x }` / `export { type x }`
//!   elision, namespace instantiation analysis (`IsInstantiatedModule`), and
//!   method/constructor/accessor overload elision. DEFER(P5) blocked-by: the
//!   named-imports/exports rebuild + `verbatimModuleSyntax` option and the
//!   module-instantiation predicate are not yet ported.
//! - `runtimesyntax` landed a checker-free subset in round 6n: `enum` -> IIFE
//!   (auto-numbered / explicit-numeric / string members, plus `const enum`
//!   omission) and instantiated `namespace` -> IIFE (`export const` ->
//!   `N.x = init`, type-only namespaces omitted). Still DEFER(P5): `const enum`
//!   member-reference inlining, non-literal initializer constant-folding,
//!   `E.A`/`N.x` member-reference rewriting, exported/merged/nested namespaces,
//!   `export =`, parameter properties, and `import=` lowering. blocked-by: the
//!   checker constant evaluation and the binder `ReferenceResolver`.
//! - `legacydecorators` / `metadata` / `typeserializer`. DEFER(P5) blocked-by:
//!   need `tsgo_checker` type→metadata serialization and the decorator helper
//!   factories.
//! - `importelision` landed a scope-correct subset in round 6af: unreferenced
//!   *value* import bindings (named / namespace / default) are elided via the
//!   checker's `EmitResolver::is_referenced`, dropping the whole import
//!   declaration when every binding is unreferenced and per-specifier otherwise.
//!   Still DEFER(P5): the export side (`export {}` / `export =` need
//!   `IsValueAliasDeclaration`), `import =` (needs
//!   `IsTopLevelValueImportEqualsWithEntityName`), type-only-position uses
//!   keeping value imports alive, and the `verbatimModuleSyntax` /
//!   `isolatedModules` / `importsNotUsedAsValues` policy variants. blocked-by:
//!   the checker `IsValueAliasDeclaration` / `markLinkedReferences` queries.

pub mod importelision;
pub mod legacydecorators;
pub mod runtimesyntax;
pub mod typeeraser;
pub mod utilities;
