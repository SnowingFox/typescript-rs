//! `tsgo_checker` — 1:1 Rust port of Go `internal/checker`, the TypeScript type
//! checker (the largest module in the compiler).
//!
//! # Status
//!
//! This crate is being ported over multiple rounds (sub-phases 4a..4k; see
//! `docs/rust-rewrite/phase-4-checker/checker/impl.md`). Round 4a establishes
//! the type/symbol ownership foundation only; the vast majority of checking
//! behavior is not yet ported.
//!
//! # Mega-file split (PORTING, section 2)
//!
//! Go's `checker.go` is a single ~32k-line file. Per the porting contract it is
//! decomposed into the [`core`] subdirectory by subsystem rather than mapped to
//! one giant `lib.rs`:
//!
//! - [`core::types`] — the `Type` representation (a [`TypeId`]-indexed arena,
//!   `TypeFlags`/`ObjectFlags`, and the type-data variants).
//! - [`core::symbols`] — symbol-resolution scaffolding layered on
//!   `tsgo_ast`'s `SymbolId` and the binder's symbol tables.
//! - [`core`] — the `Checker` struct skeleton and its construction entry.
//! - [`tracer`] — the per-checker trace recorder (arg-injection core).
//!
//! # Ownership model (read this first)
//!
//! Go's `Type`/`Symbol` graphs are arenas of heap pointers (`*Type`/`*Symbol`)
//! interned through dozens of maps, and each `Type` carries a `checker
//! *Checker` back-pointer. That cyclic, aliased shape cannot be expressed in
//! safe Rust with `&`/`Box`. Following PORTING section 5 (and the existing
//! `tsgo_ast` node arena), this crate instead:
//!
//! - owns every `Type` in a single arena ([`core::types::TypeArena`]) addressed
//!   by a [`TypeId`] (`u32`) handle;
//! - replaces every `*Type` reference (interning keys, union members, ...) with
//!   a `TypeId`;
//! - **drops** Go's `Type.checker` back-pointer — type operations are methods on
//!   the [`Checker`], which owns the arena.
//!
//! This is a deliberate, structure-preserving deviation that keeps the crate
//! 100% safe Rust (zero `unsafe`).
//!
//! ## Retained program (sub-phase 4l)
//!
//! Go's `NewChecker(program)` stores `c.program = program` — a shared,
//! non-owning pointer into GC'd memory — and every checker in the pool shares
//! that one `*Program`. The zero-`unsafe` analog (PORTING section 3: a shared
//! non-null pointer maps to `Rc<T>`) is to retain the program behind an
//! `Rc<dyn BoundProgram>`: [`Checker::new_checker`] stores it, and cloning the
//! handle is how a pool seeds its K checkers from one program. This keeps
//! [`Checker`] free of a lifetime parameter, so the intrinsic-only constructor
//! [`Checker::new`] and the ~200 call sites that take `&mut Checker` are
//! unaffected.
//!
//! Known divergences from Go for the retained program:
//! - `Rc`, not a raw/GC pointer. The shared trait object is `'static`
//!   (`Rc<dyn BoundProgram>`), so the program must own its data; the in-crate
//!   test stub qualifies, but a *borrowing* program view (e.g. the compiler's
//!   `BoundFile<'a>`) must first be made owned/`Rc`-shared. blocked-by:
//!   `compiler.Program` (P6).
//! - `Rc`, not `Arc`. The pool drives checkers sequentially today
//!   (parallel-over-checkers is DEFER per PORTING section 6); switch the field
//!   to `Arc<dyn BoundProgram + Send + Sync>` when that lands.

pub mod core;
pub mod tracer;

pub use core::check::{Diagnostic, DiagnosticMessageChain};
pub use core::declared_types::{
    fill_missing_type_arguments, get_apparent_type, get_constraint_of_type_parameter,
    get_declared_type_of_symbol, get_default_from_type_parameter, get_global_type,
    get_index_infos_of_type, get_index_type, get_indexed_access_type, get_min_type_argument_count,
    get_properties_of_type, get_property_of_type, get_type_from_type_node,
    get_type_of_property_of_type, get_type_of_symbol, resolve_structured_type_members,
};
pub use core::emit_resolver::{EmitResolver, SerializedTypeNode, TypeReferenceSerializationKind};
pub use core::inference::{InferenceContext, InferenceInfo, InferencePriority};
pub use core::mapper::TypeMapper;
pub use core::nodebuilder::{symbol_to_string, type_to_string};
pub use core::program::BoundProgram;
pub use core::relations::RelationKind;
pub use core::signatures::{
    IndexInfo, IndexInfoArena, IndexInfoId, Signature, SignatureArena, SignatureFlags, SignatureId,
};
pub use core::symbols::{
    resolve_name, skip_alias, AliasSymbolLinks, DeclaredTypeLinks, MergedSymbols,
    ModuleSymbolLinks, SymbolLinks, SymbolReferenceLinks, TypeAliasLinks, ValueSymbolLinks,
};
pub use core::symbols_query::{get_symbol_at_location, get_symbol_of_declaration};
pub use core::type_facts::TypeFacts;
pub use core::types::{
    format_type_flags, AccessFlags, IndexFlags, IndexType, IndexedAccessType, IntersectionType,
    IntrinsicType, LiteralType, LiteralValue, ObjectFlags, ObjectType, Type, TypeArena, TypeData,
    TypeFlags, TypeId, TypeParameter, UnionType,
};
pub use core::Checker;
pub use tracer::Tracer;
pub use tsgo_diagnostics::Category;
