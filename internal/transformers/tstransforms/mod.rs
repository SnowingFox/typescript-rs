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
//! - `runtimesyntax` (enum/namespace/parameter-property/`import=` runtime
//!   lowering). DEFER(P5) blocked-by: needs a large `printer::NodeFactory`
//!   constructor surface (IIFE/assignment/block builders) not yet ported.
//! - `legacydecorators` / `metadata` / `typeserializer`. DEFER(P5) blocked-by:
//!   need `tsgo_checker` type→metadata serialization and the decorator helper
//!   factories.
//! - `importelision`. DEFER(P5) blocked-by: needs checker
//!   `EmitResolver.MarkLinkedReferencesRecursively` (usage-based elision).

pub mod typeeraser;
pub mod utilities;
