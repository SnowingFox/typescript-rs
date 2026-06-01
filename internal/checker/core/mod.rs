//! Decomposition of Go's `internal/checker/checker.go` (the ~32k-line checker
//! body) into per-subsystem modules (PORTING, section 2).
//!
//! Round 4a populates [`types`] (the `Type` arena and flags) and [`symbols`]
//! (symbol-link scaffolding), and adds the [`Checker`] skeleton with intrinsic
//! type construction. Later sub-phases (4b..4k) add `relations`, `inference`,
//! `instantiation`, `flow`, and the rest.

pub mod check;
pub mod contextual;
pub mod declared_types;
pub mod emit_resolver;
pub mod flow;
pub mod grammar;
pub mod inference;
pub mod jsx;
pub mod mapper;
pub mod nodebuilder;
pub mod program;
pub mod relations;
pub mod signatures;
pub mod symbols;
pub mod symbols_query;
pub mod type_facts;
pub mod types;

#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};
use tsgo_ast::{CheckFlags, NodeId};
use tsgo_core::compileroptions::CompilerOptions;
use tsgo_core::tristate::Tristate;

use emit_resolver::EmitResolver;
use program::{default_compiler_options, BoundProgram};
use relations::RelationCache;
use signatures::{IndexInfo, IndexInfoArena, IndexInfoId, Signature, SignatureArena, SignatureId};
use symbols::{
    DeclaredTypeLinks, SymbolLinks, SymbolReferenceLinks, TypeAliasLinks, ValueSymbolLinks,
};
use types::{
    IntersectionType, IntrinsicType, LiteralType, LiteralValue, ObjectFlags, ObjectType, Type,
    TypeArena, TypeData, TypeFlags, TypeId, TypeParameter, UnionType,
};

/// The bound program a checker retains (Go's `c.program` pointer field).
///
/// A thin wrapper around an `Rc<dyn BoundProgram>` whose only job is to give the
/// shared, non-`Debug` trait object a `Debug` impl so [`Checker`] can keep
/// `#[derive(Debug)]`.
// Go: internal/checker/checker.go:Checker.program
struct RetainedProgram(Rc<dyn BoundProgram>);

impl std::fmt::Debug for RetainedProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BoundProgram")
    }
}

/// High bit set on the [`SymbolId`] of a checker-synthesized symbol, so its id
/// space never collides with the program's binder symbol ids (those index a
/// `Vec` and are always small). A program symbol id is recognized by this bit
/// being clear.
///
/// DIVERGENCE(port): Go mints transient symbols from a per-checker `symbolArena`
/// of `*ast.Symbol` pointers (`newSymbol`), so identity is the pointer and there
/// is no shared id space with binder symbols. Rust addresses both binder and
/// synthesized symbols by `SymbolId`; tagging the high bit keeps the two spaces
/// disjoint while letting one `SymbolId` type flow through the checker.
// Go: internal/checker/checker.go:Checker.symbolArena / newSymbol
const SYNTHESIZED_SYMBOL_TAG: u32 = 1 << 31;

/// Reports whether `id` addresses a checker-synthesized (transient) symbol
/// rather than a binder/program symbol.
// Go: internal/checker/checker.go:Checker.newSymbol (SymbolFlagsTransient marker)
pub(crate) fn is_synthesized_symbol(id: symbols::SymbolId) -> bool {
    id.0 & SYNTHESIZED_SYMBOL_TAG != 0
}

/// A checker-minted transient property symbol (Go's `newSymbolEx` result plus
/// the slice of its `valueSymbolLinks` that union/intersection property
/// synthesis populates: `containingType` and the combined `resolvedType`).
///
/// DIVERGENCE(port): Go computes the combined property type *eagerly* inside
/// `createUnionOrIntersectionProperty` (it has `*Checker`). Here the minting
/// entry point [`get_property_of_type`](crate::get_property_of_type) is a
/// `&Checker`, so the symbol is minted via interior mutability and its type is
/// resolved lazily on the first `get_type_of_symbol` (which has `&mut Checker`
/// and the program). The result is identical and keyed by the symbol id.
#[derive(Debug)]
struct SynthesizedSymbol {
    /// Symbol meaning flags (always carries `SymbolFlags::TRANSIENT`).
    flags: symbols::SymbolFlags,
    /// Checker-time flags (e.g. `CheckFlags::SYNTHETIC_PROPERTY` /
    /// `CheckFlags::READONLY`).
    check_flags: CheckFlags,
    /// The property name.
    name: String,
    /// The union/intersection type this property was synthesized from (Go's
    /// `valueSymbolLinks.containingType`).
    containing_type: TypeId,
    /// The combined property type, computed lazily on first resolution.
    resolved_type: Option<TypeId>,
}

/// The TypeScript type checker.
///
/// Round 4a builds only the foundation: the [`TypeArena`], the intrinsic type
/// singletons, and the per-symbol link stores. From sub-phase 4l on, the
/// program-taking entry point [`Checker::new_checker`] retains its
/// [`BoundProgram`] (Go's `c.program`), so the per-file driving surface
/// ([`Checker::check_source_file`]/[`Checker::get_diagnostics`]) works off the
/// retained program — the shape a multi-checker pool drives.
///
/// [`Checker::new`] still constructs just the intrinsic substrate (no program)
/// so type construction and printing can be exercised in isolation.
///
/// Go's `Checker` has ~300 fields; only those needed by 4a are present. Each
/// later sub-phase (4b..4k) adds the fields and methods its subsystem needs.
///
/// # Examples
/// ```
/// use tsgo_checker::Checker;
/// let c = Checker::new();
/// assert_eq!(c.type_to_string(c.string_type()), "string");
/// assert_eq!(c.type_to_string(c.number_type()), "number");
/// ```
///
/// Side effects: none (the constructor allocates only in-memory arenas).
// Go: internal/checker/checker.go:Checker
#[derive(Debug)]
pub struct Checker {
    /// Owns every [`Type`] this checker creates.
    types: TypeArena,
    /// Per-symbol "reference kinds" links (one of Go's ~30 link stores).
    symbol_reference_links: SymbolLinks<SymbolReferenceLinks>,
    /// Resolved global types by name; populated when declared types are built
    /// (sub-phase 4c). Empty in 4a.
    global_types: FxHashMap<String, TypeId>,
    /// Interned union types, keyed by their sorted constituent ids (Go uses a
    /// hashed `CacheHashKey`; the sorted id vector is an equivalent stable key).
    union_types: FxHashMap<Vec<TypeId>, TypeId>,
    /// Interned intersection types, keyed by their sorted constituent ids (Go
    /// uses a hashed `getIntersectionKey`; the sorted id vector is an equivalent
    /// stable key, mirroring the union intern map).
    intersection_types: FxHashMap<Vec<TypeId>, TypeId>,
    /// Interned string-literal types, keyed by the literal value (Go's
    /// `stringLiteralTypes map[string]*Type`), so every `"a"` is one [`TypeId`].
    string_literal_types: FxHashMap<String, TypeId>,
    /// Interned number-literal types, keyed by the literal value's canonical
    /// bit pattern (Go's `numberLiteralTypes map[jsnum.Number]*Type`); `NaN` and
    /// `+0`/`-0` are canonicalized so all `NaN`s share one type and `0`/`-0`
    /// collapse, matching Go's float map-key semantics + separate `nanType`.
    number_literal_types: FxHashMap<u64, TypeId>,
    /// Lazily-built declared types for interface/class/enum symbols.
    declared_type_links: SymbolLinks<DeclaredTypeLinks>,
    /// Lazily-built declared types for type-alias symbols.
    type_alias_links: SymbolLinks<TypeAliasLinks>,
    /// Lazily-computed types of value/property symbols.
    value_symbol_links: SymbolLinks<ValueSymbolLinks>,
    /// Checker-owned arena of synthesized (transient) symbols minted during
    /// union/intersection property synthesis (Go's `symbolArena` + `newSymbol`).
    /// Wrapped in `RefCell` so the `&Checker` `get_property_of_type` entry point
    /// can mint without changing its (compiler-relied-upon) shared signature.
    synthesized_symbols: RefCell<Vec<SynthesizedSymbol>>,
    /// Per-`(containing type, name)` cache of synthesized properties, so a
    /// repeated lookup returns the same symbol id (Go's
    /// `unionOrIntersectionType.propertyCache`). `None` records a definitively
    /// absent property.
    synthesized_property_cache: RefCell<FxHashMap<(TypeId, String), Option<symbols::SymbolId>>>,
    /// Owns every [`Signature`] this checker creates.
    signatures: SignatureArena,
    /// Owns every [`IndexInfo`] this checker creates.
    index_infos: IndexInfoArena,
    /// Per-relation result cache (Go's `identityRelation`/`assignableRelation`/...).
    relations: RelationCache,
    /// Current recursive `instantiate_type` depth (Go's `instantiationDepth`).
    instantiation_depth: u32,
    /// Total `instantiate_type` calls for the current statement (Go's `instantiationCount`).
    instantiation_count: u32,
    /// Diagnostics recorded while checking, partitioned by the source-file
    /// handle (`BoundProgram::file_handle`) they were produced for, so
    /// [`Checker::get_diagnostics`] returns only one file's diagnostics (Go's
    /// per-file `DiagnosticsCollection` / `collection.GetDiagnosticsForFile`).
    /// Each file's `Vec` preserves production order.
    diagnostics_by_file: FxHashMap<NodeId, Vec<check::Diagnostic>>,
    /// The `JSX.IntrinsicElements` type, used to resolve intrinsic (lowercase)
    /// JSX tags. Resolved from lib globals in Go; until those land (P6) callers
    /// inject it via [`Checker::set_jsx_intrinsic_elements`].
    jsx_intrinsic_elements: Option<TypeId>,
    /// The cached emit-time query handle (Go's `GetEmitResolver` `sync.Once`).
    emit_resolver: OnceCell<EmitResolver>,
    /// The bound program this checker was constructed over (Go's `c.program`),
    /// or `None` for an intrinsic-only checker built via [`Checker::new`].
    /// Retained so the per-file driving surface needs no per-call program.
    program: Option<RetainedProgram>,
    /// Files already type-checked, so re-checking is a no-op (Go's per-file
    /// `sourceFileLinks.typeChecked`). Keeps diagnostics from doubling when a
    /// file is checked then re-requested through [`Checker::get_diagnostics`].
    checked_files: FxHashSet<NodeId>,

    // Intrinsic type singletons (Go: the `c.xxxType` fields set in NewChecker).
    any_type: TypeId,
    auto_type: TypeId,
    error_type: TypeId,
    unknown_type: TypeId,
    undefined_type: TypeId,
    null_type: TypeId,
    string_type: TypeId,
    number_type: TypeId,
    bigint_type: TypeId,
    regular_false_type: TypeId,
    false_type: TypeId,
    regular_true_type: TypeId,
    true_type: TypeId,
    boolean_type: TypeId,
    es_symbol_type: TypeId,
    void_type: TypeId,
    never_type: TypeId,
    silent_never_type: TypeId,
    non_primitive_type: TypeId,
    string_or_number_type: TypeId,
    number_or_bigint_type: TypeId,
}

impl Default for Checker {
    fn default() -> Self {
        Checker::new()
    }
}

impl Checker {
    /// Creates a checker with its intrinsic types initialized.
    ///
    /// The intrinsic types are allocated in the same order as Go's
    /// `NewChecker`, so their ids match (modulo the types 4a does not yet
    /// construct, e.g. the boolean literal/union types).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let c = Checker::new();
    /// assert_eq!(c.type_to_string(c.any_type()), "any");
    /// ```
    ///
    /// Side effects: allocates the intrinsic types in a fresh arena.
    // Go: internal/checker/checker.go:NewChecker (intrinsic init block)
    pub fn new() -> Self {
        let mut types = TypeArena::new();
        let any_type = new_intrinsic(&mut types, TypeFlags::ANY, "any", ObjectFlags::empty());
        let auto_type = new_intrinsic(
            &mut types,
            TypeFlags::ANY,
            "any",
            ObjectFlags::NON_INFERRABLE_TYPE,
        );
        let error_type = new_intrinsic(&mut types, TypeFlags::ANY, "error", ObjectFlags::empty());
        let unknown_type = new_intrinsic(
            &mut types,
            TypeFlags::UNKNOWN,
            "unknown",
            ObjectFlags::empty(),
        );
        let undefined_type = new_intrinsic(
            &mut types,
            TypeFlags::UNDEFINED,
            "undefined",
            ObjectFlags::empty(),
        );
        let null_type = new_intrinsic(&mut types, TypeFlags::NULL, "null", ObjectFlags::empty());
        let string_type = new_intrinsic(
            &mut types,
            TypeFlags::STRING,
            "string",
            ObjectFlags::empty(),
        );
        let number_type = new_intrinsic(
            &mut types,
            TypeFlags::NUMBER,
            "number",
            ObjectFlags::empty(),
        );
        let bigint_type = new_intrinsic(
            &mut types,
            TypeFlags::BIG_INT,
            "bigint",
            ObjectFlags::empty(),
        );
        // Boolean is a union of the `false` and `true` literal types; each
        // literal has a fresh/regular pair (Go links them in NewChecker).
        let regular_false_type = new_literal_type_in(
            &mut types,
            TypeFlags::BOOLEAN_LITERAL,
            LiteralValue::Boolean(false),
            None,
        );
        let false_type = new_literal_type_in(
            &mut types,
            TypeFlags::BOOLEAN_LITERAL,
            LiteralValue::Boolean(false),
            Some(regular_false_type),
        );
        set_literal_fresh_type(&mut types, regular_false_type, false_type);
        set_literal_fresh_type(&mut types, false_type, false_type);
        let regular_true_type = new_literal_type_in(
            &mut types,
            TypeFlags::BOOLEAN_LITERAL,
            LiteralValue::Boolean(true),
            None,
        );
        let true_type = new_literal_type_in(
            &mut types,
            TypeFlags::BOOLEAN_LITERAL,
            LiteralValue::Boolean(true),
            Some(regular_true_type),
        );
        set_literal_fresh_type(&mut types, regular_true_type, true_type);
        set_literal_fresh_type(&mut types, true_type, true_type);
        let mut union_types: FxHashMap<Vec<TypeId>, TypeId> = FxHashMap::default();
        let boolean_type = intern_union(
            &mut types,
            &mut union_types,
            vec![regular_false_type, regular_true_type],
        )
        .expect("boolean union has two members");
        let es_symbol_type = new_intrinsic(
            &mut types,
            TypeFlags::ES_SYMBOL,
            "symbol",
            ObjectFlags::empty(),
        );
        let void_type = new_intrinsic(&mut types, TypeFlags::VOID, "void", ObjectFlags::empty());
        let never_type = new_intrinsic(&mut types, TypeFlags::NEVER, "never", ObjectFlags::empty());
        let silent_never_type = new_intrinsic(
            &mut types,
            TypeFlags::NEVER,
            "never",
            ObjectFlags::NON_INFERRABLE_TYPE,
        );
        let non_primitive_type = new_intrinsic(
            &mut types,
            TypeFlags::NON_PRIMITIVE,
            "object",
            ObjectFlags::empty(),
        );
        let string_or_number_type =
            intern_union(&mut types, &mut union_types, vec![string_type, number_type])
                .expect("string|number union has two members");
        let number_or_bigint_type =
            intern_union(&mut types, &mut union_types, vec![number_type, bigint_type])
                .expect("number|bigint union has two members");

        Checker {
            types,
            symbol_reference_links: SymbolLinks::default(),
            global_types: FxHashMap::default(),
            union_types,
            intersection_types: FxHashMap::default(),
            string_literal_types: FxHashMap::default(),
            number_literal_types: FxHashMap::default(),
            declared_type_links: SymbolLinks::default(),
            type_alias_links: SymbolLinks::default(),
            value_symbol_links: SymbolLinks::default(),
            synthesized_symbols: RefCell::new(Vec::new()),
            synthesized_property_cache: RefCell::new(FxHashMap::default()),
            signatures: SignatureArena::new(),
            index_infos: IndexInfoArena::new(),
            relations: RelationCache::default(),
            instantiation_depth: 0,
            instantiation_count: 0,
            diagnostics_by_file: FxHashMap::default(),
            jsx_intrinsic_elements: None,
            emit_resolver: OnceCell::new(),
            program: None,
            checked_files: FxHashSet::default(),
            any_type,
            auto_type,
            error_type,
            unknown_type,
            undefined_type,
            null_type,
            string_type,
            number_type,
            bigint_type,
            regular_false_type,
            false_type,
            regular_true_type,
            true_type,
            boolean_type,
            es_symbol_type,
            void_type,
            never_type,
            silent_never_type,
            non_primitive_type,
            string_or_number_type,
            number_or_bigint_type,
        }
    }

    /// Constructs a checker over a bound `program`, retaining it (Go's
    /// `NewChecker(program)`, which stores `c.program = program`).
    ///
    /// The program is shared (`Rc`), mirroring Go where one `*Program` is shared
    /// by every checker in the pool; cloning the handle is how a pool seeds its K
    /// checkers from one program. After construction the per-file driving surface
    /// — [`Checker::check_source_file`] and [`Checker::get_diagnostics`] — works
    /// off the retained program with no per-call program argument.
    ///
    /// blocked-by: `compiler.Program` + lib globals (P6) — the real `NewChecker`
    /// additionally binds the program's global scope and lib types and populates
    /// `getGlobalType`; 4l only retains the program and initializes intrinsics.
    ///
    /// # Examples
    /// ```
    /// use std::rc::Rc;
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo(p: Rc<dyn BoundProgram>) -> Checker {
    /// Checker::new_checker(p)
    /// # }
    /// ```
    ///
    /// Side effects: retains the program and allocates the intrinsic types.
    // Go: internal/checker/checker.go:NewChecker
    pub fn new_checker(program: Rc<dyn BoundProgram>) -> Self {
        // Retain the program (Go's `c.program = program`); global/lib binding
        // and `getGlobalType` population still land with P6.
        let mut checker = Checker::new();
        checker.program = Some(RetainedProgram(program));
        checker
    }

    /// Returns the program this checker was constructed over (Go's `c.program`),
    /// or `None` for an intrinsic-only checker built via [`Checker::new`].
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// // An intrinsic-only checker has no program.
    /// assert!(Checker::new().program().is_none());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/checker.go:Checker.program
    pub fn program(&self) -> Option<&dyn BoundProgram> {
        self.program.as_ref().map(|p| p.0.as_ref())
    }

    /// Returns the compiler options the checker was constructed with (Go's
    /// `c.compilerOptions`, read from `program.Options()` in `NewChecker`).
    ///
    /// An intrinsic-only checker (built via [`Checker::new`], no program) and a
    /// program that does not carry options both report the all-defaults
    /// [`CompilerOptions`].
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// use tsgo_core::tristate::Tristate;
    /// // An intrinsic-only checker reports all-defaults options.
    /// assert_eq!(Checker::new().compiler_options().strict_null_checks, Tristate::Unknown);
    /// ```
    ///
    /// Side effects: none (a read-only view).
    // Go: internal/checker/checker.go:NewChecker (c.compilerOptions = program.Options())
    pub fn compiler_options(&self) -> &CompilerOptions {
        match self.program() {
            Some(program) => program.compiler_options(),
            None => default_compiler_options(),
        }
    }

    /// Resolves a `strict`-family option: an explicit per-option tri-state wins,
    /// otherwise the option is enabled iff `strict` is not explicitly false
    /// (Go's `compilerOptions.GetStrictOptionValue`, the rule `NewChecker` uses
    /// to derive `c.strictNullChecks`/`c.strictFunctionTypes`/...).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// use tsgo_core::tristate::Tristate;
    /// let c = Checker::new();
    /// // An explicit per-option `false` wins even when `strict` is unset.
    /// assert!(!c.get_strict_option_value(Tristate::False));
    /// // An explicit per-option `true` wins.
    /// assert!(c.get_strict_option_value(Tristate::True));
    /// // With `strict` unset, an unset value follows `strict != false` -> enabled.
    /// assert!(c.get_strict_option_value(Tristate::Unknown));
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/core/compileroptions.go:GetStrictOptionValue
    pub fn get_strict_option_value(&self, value: Tristate) -> bool {
        self.compiler_options().get_strict_option_value(value)
    }

    /// Reports whether `strictNullChecks` is in effect (Go's `c.strictNullChecks`,
    /// `= compilerOptions.GetStrictOptionValue(compilerOptions.StrictNullChecks)`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// // With all-defaults options, the `strict != false` rule enables it
    /// // (faithful to Go's `GetStrictOptionValue`); a real program resolves
    /// // `strict`/`strictNullChecks` explicitly.
    /// assert!(Checker::new().strict_null_checks());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/checker.go:NewChecker (c.strictNullChecks)
    pub fn strict_null_checks(&self) -> bool {
        self.get_strict_option_value(self.compiler_options().strict_null_checks)
    }

    /// Reports whether `strictFunctionTypes` is in effect (Go's
    /// `c.strictFunctionTypes`, `= compilerOptions.GetStrictOptionValue(
    /// compilerOptions.StrictFunctionTypes)`).
    ///
    /// When on, function/property call-signature parameters relate strictly
    /// contravariantly; when off they relate bivariantly. Method-declared
    /// parameters are always bivariant regardless of this flag.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// // With all-defaults options, the `strict != false` rule enables it.
    /// assert!(Checker::new().strict_function_types());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/checker.go:NewChecker (c.strictFunctionTypes)
    pub fn strict_function_types(&self) -> bool {
        self.get_strict_option_value(self.compiler_options().strict_function_types)
    }

    /// Clones the shared handle to the retained program, if any.
    ///
    /// Returning an owned `Rc` lets a `&mut self` driver (e.g.
    /// [`Checker::check_source_file`]) walk the program while mutating the
    /// checker, without holding a borrow of `self`.
    // Go: internal/checker/checker.go:Checker.program (shared pointer)
    pub(crate) fn retained_program(&self) -> Option<Rc<dyn BoundProgram>> {
        self.program.as_ref().map(|p| Rc::clone(&p.0))
    }

    /// Records that `file` has been type-checked, returning `true` the first
    /// time (the caller should check it) and `false` afterwards.
    // Go: internal/checker/checker.go:Checker.checkSourceFile (links.typeChecked)
    pub(crate) fn mark_file_checked(&mut self, file: NodeId) -> bool {
        self.checked_files.insert(file)
    }

    /// Resolves `name` in the retained program's global scope, keeping only a
    /// symbol whose flags intersect `meaning`.
    ///
    /// This is the global-only resolution Go performs as
    /// `getGlobalSymbol(name, meaning)` → `resolveName(nil, name, meaning, ...)`:
    /// with no `location`, the scope-chain walk is skipped and only the merged
    /// global table (`c.globals`, exposed here by [`BoundProgram::globals`]) is
    /// consulted. Returns `None` when there is no retained program, the program
    /// exposes no globals, the name is absent, or its meaning does not match.
    ///
    /// blocked-by: lib.d.ts loading + cross-file global merge (P6). Until then
    /// the globals come from a script source file's top-level declarations
    /// (synthetic globals driven through the test harness).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// use tsgo_ast::SymbolFlags;
    /// // An intrinsic-only checker has no program, hence no globals.
    /// assert!(Checker::new().get_global_symbol("g", SymbolFlags::VALUE).is_none());
    /// ```
    ///
    /// Side effects: none (a read-only lookup over the bound program).
    // Go: internal/checker/checker.go:Checker.getGlobalSymbol
    pub fn get_global_symbol(
        &self,
        name: &str,
        meaning: symbols::SymbolFlags,
    ) -> Option<symbols::SymbolId> {
        let program = self.program()?;
        let globals = program.globals()?;
        let &symbol = globals.get(name)?;
        if program.symbol(symbol).flags.intersects(meaning) {
            Some(symbol)
        } else {
            None
        }
    }

    /// Resolves the global TYPE named `name` from the retained program's globals
    /// and builds (and caches) its declared type.
    ///
    /// This is the convenience entry standing in for Go's
    /// `getGlobalType(name, arity, reportErrors)` driven off the retained
    /// program: it looks `name` up among the program globals (a type-meaning
    /// symbol) and delegates to [`declared_types::get_global_type`] to build the
    /// declared type. Returns `None` when there is no program, no globals, or
    /// the name is not a global type.
    ///
    /// blocked-by: lib.d.ts loading (P6). The real `getGlobalType` also performs
    /// type-parameter arity checking and reports `Cannot_find_global_type_0`,
    /// returning `emptyObjectType`/`emptyGenericType` fallbacks; those need the
    /// empty-object/generic types and diagnostics wiring of the full checker.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// // An intrinsic-only checker has no program, hence no global types.
    /// assert!(Checker::new().get_global_type("Array").is_none());
    /// ```
    ///
    /// Side effects: may build a declared type and populate the global-type
    /// cache.
    // Go: internal/checker/checker.go:Checker.getGlobalType
    pub fn get_global_type(&mut self, name: &str) -> Option<TypeId> {
        if let Some(&cached) = self.global_types.get(name) {
            return Some(cached);
        }
        let program = self.retained_program()?;
        // Resolve the global symbol, then build its declared type against the
        // view of the file that DECLARES it (a multi-file program may declare
        // `String` in a lib file other than the one being checked). For a
        // single-file program the owning view is the program itself.
        let symbol = *program.globals()?.get(name)?;
        let view = program
            .view_for_symbol(symbol)
            .unwrap_or_else(|| Rc::clone(&program));
        let t = {
            let globals = view.globals()?;
            declared_types::get_global_type(self, view.as_ref(), name, globals)?
        };
        // Warm the wrapper's member types against the OWNING view, so a later
        // cross-file property access (resolved while checking a *different*
        // file's view) hits the per-symbol type cache instead of reading the
        // member's declaration node through the wrong file's arena.
        for (_, prop) in declared_types::get_properties_of_type(self, t) {
            let globals = view.globals();
            let _ = declared_types::get_type_of_symbol(self, view.as_ref(), prop, globals);
        }
        Some(t)
    }

    /// Allocates a new type, clearing the cache-only object flags that Go's
    /// `newType` strips, and returns its [`TypeId`].
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, IntrinsicType, ObjectFlags, TypeData, TypeFlags};
    /// let mut c = Checker::new();
    /// let before = c.type_count();
    /// let id = c.new_type(
    ///     TypeFlags::STRING_LITERAL,
    ///     ObjectFlags::empty(),
    ///     TypeData::Intrinsic(IntrinsicType { intrinsic_name: "x".to_string() }),
    /// );
    /// assert_eq!(c.type_count(), before + 1);
    /// assert_eq!(c.get_type(id).flags(), TypeFlags::STRING_LITERAL);
    /// ```
    ///
    /// Side effects: mutates the checker's type arena.
    // Go: internal/checker/checker.go:Checker.newType
    pub fn new_type(
        &mut self,
        flags: TypeFlags,
        object_flags: ObjectFlags,
        data: TypeData,
    ) -> TypeId {
        let cleared = object_flags & !ObjectFlags::FRESH_ALLOCATION_CLEARED;
        self.types.alloc(flags, cleared, None, data)
    }

    /// Returns the number of types this checker has allocated (Go's
    /// `Checker.TypeCount`).
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/checker.go:Checker.TypeCount
    pub fn type_count(&self) -> usize {
        self.types.len()
    }

    /// Returns the [`Type`] for `id`.
    ///
    /// # Panics
    /// Panics if `id` was not produced by this checker.
    ///
    /// Side effects: none (pure).
    pub fn get_type(&self, id: TypeId) -> &Type {
        self.types.get(id)
    }

    /// Returns the printed form of a type.
    ///
    /// For the intrinsic types built in 4a this is just the intrinsic name
    /// (`"string"`, `"any"`, ...), which matches Go's `typeToString` for those
    /// types.
    ///
    /// For intrinsics this is the intrinsic name; for literals, the literal's
    /// printed value (`true`/`false`/`"s"`/number); for unions, the constituent
    /// strings joined by `" | "`. These match Go's `typeToString` for the same
    /// shapes.
    ///
    /// DEFER(phase-4-checker-4j): the full `typeToString` path runs through the
    /// node builder and a printer (object types, alias names, the special
    /// `false | true` => `boolean` collapse, JS-canonical number formatting,
    /// quote-style selection, ...); that is ported in sub-phase 4j.
    /// blocked-by: node builder (`nodebuilderimpl.go`) and `printer.go` are not
    /// ported until 4j; object/alias types are not constructed until 4c+.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let c = Checker::new();
    /// assert_eq!(c.type_to_string(c.never_type()), "never");
    /// assert_eq!(c.type_to_string(c.non_primitive_type()), "object");
    /// assert_eq!(c.type_to_string(c.false_type()), "false");
    /// assert_eq!(c.type_to_string(c.string_or_number_type()), "string | number");
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/printer.go:Checker.TypeToString
    pub fn type_to_string(&self, id: TypeId) -> String {
        match &self.types.get(id).data {
            TypeData::Intrinsic(d) => d.intrinsic_name.clone(),
            TypeData::Literal(d) => literal_value_to_string(&d.value),
            TypeData::Union(d) => d
                .types
                .iter()
                .map(|&member| self.type_to_string(member))
                .collect::<Vec<_>>()
                .join(" | "),
            TypeData::Intersection(d) => d
                .types
                .iter()
                .map(|&member| self.type_to_string(member))
                .collect::<Vec<_>>()
                .join(" & "),
            // DEFER(phase-4-checker-4j): named/structural object and type-parameter
            // printing needs the node builder plus access to the symbol's name
            // (which lives in the program, not the checker). 4c/4d emit
            // placeholders.
            // blocked-by: node builder (`nodebuilderimpl.go`) ships in 4j.
            TypeData::Object(_) => "{ ... }".to_string(),
            TypeData::TypeParameter(_) => "T".to_string(),
        }
    }

    /// Allocates a type parameter for `symbol` (or an anonymous one).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let mut c = Checker::new();
    /// let tp = c.new_type_parameter(None);
    /// assert!(c.get_type(tp).as_type_parameter().is_some());
    /// ```
    ///
    /// Side effects: mutates the checker's type arena.
    // Go: internal/checker/checker.go:Checker.newTypeParameter
    pub fn new_type_parameter(&mut self, symbol: Option<symbols::SymbolId>) -> TypeId {
        self.types.alloc(
            TypeFlags::TYPE_PARAMETER,
            ObjectFlags::empty(),
            symbol,
            TypeData::TypeParameter(TypeParameter {
                symbol,
                constraint: None,
                is_this_type: false,
            }),
        )
    }

    /// Mints a synthesized (transient) property symbol carrying `name`,
    /// `flags`, and `check_flags`, recording the union/intersection
    /// `containing_type` it was synthesized from, and returns its tagged
    /// [`SymbolId`].
    ///
    /// Callable through a shared `&Checker` (interior mutability) so the
    /// `get_property_of_type` entry point can mint without a `&mut` signature —
    /// the analog of Go's `newSymbolEx`, which mutates the checker's symbol
    /// arena from within `createUnionOrIntersectionProperty`.
    ///
    /// Side effects: pushes a symbol into the synthesized-symbol arena.
    // Go: internal/checker/checker.go:Checker.newSymbolEx
    pub(crate) fn new_synthesized_property(
        &self,
        name: &str,
        flags: symbols::SymbolFlags,
        check_flags: CheckFlags,
        containing_type: TypeId,
    ) -> symbols::SymbolId {
        let mut arena = self.synthesized_symbols.borrow_mut();
        let index = arena.len() as u32;
        arena.push(SynthesizedSymbol {
            flags: flags | symbols::SymbolFlags::TRANSIENT,
            check_flags,
            name: name.to_string(),
            containing_type,
            resolved_type: None,
        });
        symbols::SymbolId(SYNTHESIZED_SYMBOL_TAG | index)
    }

    /// Mints a synthesized (transient) property symbol for an object-literal
    /// member named `name`, carrying `flags`, `check_flags`, and the
    /// already-computed member `member_type` as its resolved type.
    ///
    /// `check_flags` carries the checker-time adornments Go's `newSymbolEx`
    /// receives — notably `CheckFlags::READONLY` for an `as const` (const
    /// context) object-literal property.
    ///
    /// Unlike [`new_synthesized_property`](Checker::new_synthesized_property)
    /// (whose type is lazily combined from a union/intersection
    /// `containing_type`), an object-literal property's type is known eagerly
    /// from its initializer, so it is stored directly via
    /// [`set_synthesized_symbol_resolved_type`](Checker::set_synthesized_symbol_resolved_type).
    /// The containing-type slot is irrelevant for such a symbol (the resolved
    /// type short-circuits the union/intersection combine path), so the member
    /// type itself is recorded there as a harmless valid placeholder.
    ///
    /// Side effects: pushes a symbol into the synthesized-symbol arena and
    /// records its resolved type.
    // Go: internal/checker/checker.go:Checker.checkObjectLiteral (newSymbolEx + links.resolvedType = t)
    pub(crate) fn new_object_literal_property(
        &mut self,
        name: &str,
        flags: symbols::SymbolFlags,
        check_flags: CheckFlags,
        member_type: TypeId,
    ) -> symbols::SymbolId {
        let prop = self.new_synthesized_property(name, flags, check_flags, member_type);
        self.set_synthesized_symbol_resolved_type(prop, member_type);
        prop
    }

    // Returns the arena index encoded in a synthesized symbol id.
    fn synthesized_index(id: symbols::SymbolId) -> usize {
        (id.0 & !SYNTHESIZED_SYMBOL_TAG) as usize
    }

    /// Returns the cached synthesized property for `(containing, name)`:
    /// `Some(Some(id))` for a hit, `Some(None)` for a known-absent property, and
    /// `None` when nothing has been computed yet.
    // Go: internal/checker/checker.go:Checker.getUnionOrIntersectionProperty (cache read)
    pub(crate) fn cached_synthesized_property(
        &self,
        containing: TypeId,
        name: &str,
    ) -> Option<Option<symbols::SymbolId>> {
        self.synthesized_property_cache
            .borrow()
            .get(&(containing, name.to_string()))
            .copied()
    }

    /// Records the synthesized-property lookup result for `(containing, name)`.
    // Go: internal/checker/checker.go:Checker.getUnionOrIntersectionProperty (cache write)
    pub(crate) fn cache_synthesized_property(
        &self,
        containing: TypeId,
        name: &str,
        prop: Option<symbols::SymbolId>,
    ) {
        self.synthesized_property_cache
            .borrow_mut()
            .insert((containing, name.to_string()), prop);
    }

    /// Returns a synthesized symbol's meaning flags.
    // Go: internal/ast/symbol.go:Symbol.Flags
    pub(crate) fn synthesized_symbol_flags(&self, id: symbols::SymbolId) -> symbols::SymbolFlags {
        self.synthesized_symbols.borrow()[Self::synthesized_index(id)].flags
    }

    /// Returns a synthesized symbol's checker-time flags (e.g.
    /// `CheckFlags::READONLY` on an `as const` object-literal property). This is
    /// the synthesized-symbol analog of `getCheckFlags`.
    // Go: internal/checker/checker.go:Checker.getCheckFlags
    pub(crate) fn synthesized_symbol_check_flags(&self, id: symbols::SymbolId) -> CheckFlags {
        self.synthesized_symbols.borrow()[Self::synthesized_index(id)].check_flags
    }

    /// Returns a synthesized property's name.
    pub(crate) fn synthesized_symbol_name(&self, id: symbols::SymbolId) -> String {
        self.synthesized_symbols.borrow()[Self::synthesized_index(id)]
            .name
            .clone()
    }

    /// Returns the containing union/intersection type a synthesized property was
    /// minted from (Go's `valueSymbolLinks.containingType`).
    pub(crate) fn synthesized_symbol_containing_type(&self, id: symbols::SymbolId) -> TypeId {
        self.synthesized_symbols.borrow()[Self::synthesized_index(id)].containing_type
    }

    /// Returns a synthesized property's cached combined type, if resolved.
    pub(crate) fn synthesized_symbol_resolved_type(&self, id: symbols::SymbolId) -> Option<TypeId> {
        self.synthesized_symbols.borrow()[Self::synthesized_index(id)].resolved_type
    }

    /// Caches a synthesized property's combined type after lazy resolution.
    pub(crate) fn set_synthesized_symbol_resolved_type(
        &mut self,
        id: symbols::SymbolId,
        t: TypeId,
    ) {
        self.synthesized_symbols.borrow_mut()[Self::synthesized_index(id)].resolved_type = Some(t);
    }

    /// Returns the meaning flags of `id`, routing synthesized ids to the
    /// checker's transient arena and program ids to the bound program. This is
    /// the synthesized-aware analog of `program.symbol(id).flags`.
    // Go: internal/ast/symbol.go:Symbol.Flags (transient symbols live on the checker)
    pub(crate) fn resolved_symbol_flags(
        &self,
        program: &dyn BoundProgram,
        id: symbols::SymbolId,
    ) -> symbols::SymbolFlags {
        if is_synthesized_symbol(id) {
            self.synthesized_symbol_flags(id)
        } else {
            program.symbol(id).flags
        }
    }

    /// Creates a generic type reference: the instantiation `target<args>`.
    ///
    /// The reference shares `target`'s members (property *symbols* are the
    /// same); per-property *type* instantiation is deferred.
    ///
    /// DEFER(phase-4-checker-4e): instantiate each member's type through the
    /// reference's type-argument mapper (`getTypeOfPropertyOfSymbol`).
    /// blocked-by: member-type instantiation needs `get_type_of_symbol` over
    /// instantiated symbols (4e wiring).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let mut c = Checker::new();
    /// let target = c.new_object_type(tsgo_checker::ObjectFlags::INTERFACE, None, Default::default());
    /// let r = c.create_type_reference(target, vec![c.string_type()]);
    /// let obj = c.get_type(r).as_object().unwrap();
    /// assert_eq!(obj.target, Some(target));
    /// assert_eq!(obj.resolved_type_arguments, vec![c.string_type()]);
    /// ```
    ///
    /// Side effects: mutates the checker's type arena.
    // Go: internal/checker/checker.go:Checker.createTypeReference / createNormalizedTypeReference
    pub fn create_type_reference(&mut self, target: TypeId, type_arguments: Vec<TypeId>) -> TypeId {
        let object = ObjectType {
            target: Some(target),
            resolved_type_arguments: type_arguments,
            ..Default::default()
        };
        self.new_object_type(ObjectFlags::REFERENCE, None, object)
    }

    /// Creates a fixed-arity tuple type (`[A, B]`) carrying its element types by
    /// position.
    ///
    /// Go represents a tuple as a type reference to a generated/global tuple
    /// target whose type arguments are the element types; the fixed-arity subset
    /// here stores the positional element types directly on a `TUPLE`-flagged
    /// object type (`resolved_type_arguments`), which supports element access by
    /// a literal index without the full generated-target machinery.
    ///
    /// DEFER(phase-4-checker-4ae+): the generated tuple target with
    /// `TupleElementInfo` (variadic/optional/labeled/rest), `length`/`[number]`
    /// members, and tuple-to-array assignability.
    /// blocked-by: `createNormalizedTupleType` + `getTupleTargetType` + tuple
    /// element flags.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, ObjectFlags};
    /// let mut c = Checker::new();
    /// let t = c.create_tuple_type(vec![c.string_type(), c.number_type()]);
    /// let obj = c.get_type(t).as_object().unwrap();
    /// assert_eq!(obj.resolved_type_arguments, vec![c.string_type(), c.number_type()]);
    /// assert!(c.get_type(t).object_flags().contains(ObjectFlags::TUPLE));
    /// ```
    ///
    /// Side effects: mutates the checker's type arena.
    // Go: internal/checker/checker.go:Checker.createTupleType / createNormalizedTupleType
    pub fn create_tuple_type(&mut self, element_types: Vec<TypeId>) -> TypeId {
        self.create_tuple_type_ex(element_types, false)
    }

    // Creates a fixed-arity tuple type carrying its element types by position,
    // marking it `readonly` when `readonly` is set. This is the const-context
    // form of [`create_tuple_type`](Checker::create_tuple_type): an
    // `[...] as const` array literal produces a readonly tuple
    // (Go's `createTupleTypeEx(elementTypes, elementInfos, readonly)` with
    // `readonly = inConstContext`).
    //
    // Side effects: mutates the checker's type arena.
    // Go: internal/checker/checker.go:Checker.createTupleTypeEx / createNormalizedTupleType (readonly)
    pub(crate) fn create_tuple_type_ex(
        &mut self,
        element_types: Vec<TypeId>,
        readonly: bool,
    ) -> TypeId {
        let object = ObjectType {
            resolved_type_arguments: element_types,
            readonly,
            ..Default::default()
        };
        self.new_object_type(ObjectFlags::TUPLE, None, object)
    }

    /// Allocates an object/interface/class type with the given object flags and
    /// optional declaring symbol, returning its id.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, ObjectFlags, ObjectType};
    /// let mut c = Checker::new();
    /// let id = c.new_object_type(ObjectFlags::INTERFACE, None, ObjectType::default());
    /// assert!(c.get_type(id).as_object().is_some());
    /// ```
    ///
    /// Side effects: mutates the checker's type arena.
    // Go: internal/checker/checker.go:Checker.newObjectType
    pub fn new_object_type(
        &mut self,
        object_flags: ObjectFlags,
        symbol: Option<symbols::SymbolId>,
        object: ObjectType,
    ) -> TypeId {
        let cleared = object_flags & !ObjectFlags::FRESH_ALLOCATION_CLEARED;
        self.types
            .alloc(TypeFlags::OBJECT, cleared, symbol, TypeData::Object(object))
    }

    /// Allocates a [`Signature`], returning its handle.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, Signature, SignatureFlags};
    /// let mut c = Checker::new();
    /// let id = c.new_signature(Signature::new(SignatureFlags::CONSTRUCT));
    /// assert!(c.signature(id).flags.contains(SignatureFlags::CONSTRUCT));
    /// ```
    ///
    /// Side effects: mutates the checker's signature arena.
    // Go: internal/checker/checker.go:Checker.newSignature
    pub fn new_signature(&mut self, signature: Signature) -> SignatureId {
        self.signatures.alloc(signature)
    }

    /// Returns the [`Signature`] for `id`.
    ///
    /// Side effects: none (pure).
    pub fn signature(&self, id: SignatureId) -> &Signature {
        self.signatures.get(id)
    }

    /// Allocates an [`IndexInfo`], returning its handle.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, IndexInfo};
    /// let mut c = Checker::new();
    /// let key = c.string_type();
    /// let val = c.number_type();
    /// let id = c.new_index_info(IndexInfo::new(key, val, false));
    /// assert_eq!(c.index_info(id).value_type, val);
    /// ```
    ///
    /// Side effects: mutates the checker's index-info arena.
    // Go: internal/checker/checker.go:Checker.newIndexInfo
    pub fn new_index_info(&mut self, info: IndexInfo) -> IndexInfoId {
        self.index_infos.alloc(info)
    }

    /// Returns the [`IndexInfo`] for `id`.
    ///
    /// Side effects: none (pure).
    pub fn index_info(&self, id: IndexInfoId) -> &IndexInfo {
        self.index_infos.get(id)
    }

    /// Creates a literal type, linking its regular counterpart (or itself when
    /// `regular_type` is `None`), and returns its id.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, LiteralValue, TypeFlags};
    /// let mut c = Checker::new();
    /// let id = c.new_literal_type(TypeFlags::STRING_LITERAL, LiteralValue::String("a".into()), None);
    /// assert_eq!(c.type_to_string(id), "\"a\"");
    /// ```
    ///
    /// Side effects: mutates the checker's type arena.
    // Go: internal/checker/checker.go:Checker.newLiteralType
    pub fn new_literal_type(
        &mut self,
        flags: TypeFlags,
        value: LiteralValue,
        regular_type: Option<TypeId>,
    ) -> TypeId {
        new_literal_type_in(&mut self.types, flags, value, regular_type)
    }

    // Returns the interned string-literal type for `value`, allocating it once
    // and caching it by value so every `"a"` shares one `TypeId`. This is Go's
    // `getStringLiteralType`: a value-keyed cache giving equal literals id
    // identity, which the union/relation/discriminant machinery relies on for
    // dedup and uniformity.
    //
    // Side effects: allocates a literal type and updates the intern cache on a
    // first-seen value.
    // Go: internal/checker/checker.go:Checker.getStringLiteralType(25164)
    pub(crate) fn get_string_literal_type(&mut self, value: &str) -> TypeId {
        if let Some(&id) = self.string_literal_types.get(value) {
            return id;
        }
        let id = new_literal_type_in(
            &mut self.types,
            TypeFlags::STRING_LITERAL,
            LiteralValue::String(value.to_string()),
            None,
        );
        self.string_literal_types.insert(value.to_string(), id);
        id
    }

    // Returns the interned number-literal type for `value`, allocating it once
    // and caching it by value so every `1` shares one `TypeId`. This is Go's
    // `getNumberLiteralType`: a value-keyed cache. `NaN` and the two signed
    // zeros are canonicalized (Go caches `NaN` separately and its float map-key
    // treats `0`/`-0` as equal), so all `NaN`s share one type and `0`/`-0`
    // collapse.
    //
    // Side effects: allocates a literal type and updates the intern cache on a
    // first-seen value.
    // Go: internal/checker/checker.go:Checker.getNumberLiteralType(25173)
    pub(crate) fn get_number_literal_type(&mut self, value: tsgo_jsnum::Number) -> TypeId {
        let key = number_literal_key(value);
        if let Some(&id) = self.number_literal_types.get(&key) {
            return id;
        }
        let id = new_literal_type_in(
            &mut self.types,
            TypeFlags::NUMBER_LITERAL,
            LiteralValue::Number(value),
            None,
        );
        self.number_literal_types.insert(key, id);
        id
    }

    /// Returns the union of `members`, interned so equal unions share an id.
    ///
    /// 4b implements the structural core: constituents are deduplicated and
    /// sorted by id, an empty union is [`never`](Checker::never_type), and a
    /// single member collapses to that member.
    ///
    /// DEFER(phase-4-checker-4d): subtype/literal reduction, boolean collapse,
    /// `ObjectFlags` aggregation (e.g. `PrimitiveUnion`), and the union-of-union
    /// fast paths are not yet ported.
    /// blocked-by: subtype relations (`relater.go`) and apparent-type machinery
    /// land in sub-phase 4d.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let mut c = Checker::new();
    /// let u = c.get_union_type(&[c.string_type(), c.number_type()]);
    /// assert_eq!(u, c.string_or_number_type());
    /// assert_eq!(c.get_union_type(&[c.string_type()]), c.string_type());
    /// assert_eq!(c.get_union_type(&[]), c.never_type());
    /// ```
    ///
    /// Side effects: may allocate a new union type and update the intern cache.
    // Go: internal/checker/checker.go:Checker.getUnionType
    pub fn get_union_type(&mut self, members: &[TypeId]) -> TypeId {
        intern_union(&mut self.types, &mut self.union_types, members.to_vec())
            .unwrap_or(self.never_type)
    }

    /// Returns the intersection of `members`, interned so equal intersections
    /// share an id.
    ///
    /// 4v implements the reachable core of Go's `getIntersectionType`: nested
    /// intersections are flattened, fresh literals normalized to regular, and
    /// constituents deduplicated (`addTypeToIntersection`); then the basic
    /// reductions apply — `never` short-circuits (`A & never` => `never`),
    /// `unknown` is dropped, `any` short-circuits to `any`, an empty set is
    /// [`unknown`](Checker::unknown_type), and a single member collapses to
    /// itself. Otherwise the result is an interned [`TypeData::Intersection`].
    ///
    /// A union constituent triggers distribution: `X & (A | B)` normalizes to
    /// `(X & A) | (X & B)` via the cross-product of all constituents (Go's
    /// `getCrossProductIntersections`).
    ///
    /// Disjoint-domain constituents reduce to [`never`](Checker::never_type)
    /// (e.g. `string & number`, `object & string`) via the non-strict subset of
    /// Go's `TypeFlagsDisjointDomains` guard.
    ///
    /// DEFER(phase-4-checker-later): the unit-type reduction (two distinct unit
    /// types => `never`), supertype reduction, the type-variable constraint
    /// reduction, and the strictNullChecks undefined/null + `Nullable & Object`
    /// and divide-and-conquer fast paths of the distribution are not yet ported.
    /// blocked-by: those need apparent-type/constraint machinery, unit/literal
    /// type construction, and the strictNullChecks-specific reductions beyond
    /// this round's reach.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let mut c = Checker::new();
    /// let a = c.new_type_parameter(None);
    /// let b = c.new_type_parameter(None);
    /// // The same constituents intern to one id.
    /// let ab = c.get_intersection_type(&[a, b]);
    /// assert_eq!(c.get_intersection_type(&[a, b]), ab);
    /// // A single member collapses; an empty intersection is `unknown`.
    /// assert_eq!(c.get_intersection_type(&[a]), a);
    /// assert_eq!(c.get_intersection_type(&[]), c.unknown_type());
    /// ```
    ///
    /// Side effects: may allocate a new intersection type and update the intern
    /// cache.
    // Go: internal/checker/checker.go:Checker.getIntersectionType
    pub fn get_intersection_type(&mut self, members: &[TypeId]) -> TypeId {
        let mut type_set: Vec<TypeId> = Vec::new();
        let mut includes = TypeFlags::empty();
        for &t in members {
            includes = self.add_type_to_intersection(&mut type_set, includes, t);
        }
        // Go: an intersection that includes `never` is the empty intersection.
        if includes.contains(TypeFlags::NEVER) {
            if type_set.contains(&self.silent_never_type) {
                return self.silent_never_type;
            }
            return self.never_type;
        }
        // Go: an intersection spanning two disjoint domains is empty (`never`).
        // A non-primitive (`object`), string-like, number-like, bigint-like,
        // ES-symbol-like, or void-like type intersected with a member from any
        // *other* disjoint domain cannot have a value, e.g. `string & number`.
        // (The strictNullChecks `Nullable & Object` clause and the unit-type
        // reduction are DEFER'd — see `add_type_to_intersection`.)
        if is_disjoint_domain_intersection(includes) {
            return self.never_type;
        }
        // Go: `any` short-circuits. The `wildcard` sub-case is unreachable
        // (no wildcard type is constructed yet); `error` is preserved.
        if includes.contains(TypeFlags::ANY) {
            if includes.contains(TypeFlags::INCLUDES_ERROR) {
                return self.error_type;
            }
            return self.any_type;
        }
        // Go: an empty intersection is `unknown`; a single member collapses.
        if type_set.is_empty() {
            return self.unknown_type;
        }
        if type_set.len() == 1 {
            return type_set[0];
        }
        // Go: union distribution. When a constituent is a union, normalize
        // `X & (A | B)` into `(X & A) | (X & B)` by intersecting every
        // combination of the union members (the cross-product) and unioning the
        // non-`never` results. The strictNullChecks undefined/null fast paths
        // and the divide-and-conquer optimization for 3+ constituents are
        // DEFER'd; this is the core `default` cross-product branch.
        if includes.contains(TypeFlags::UNION) {
            let constituents = self.get_cross_product_intersections(&type_set);
            return self.get_union_type(&constituents);
        }
        intern_intersection(&mut self.types, &mut self.intersection_types, type_set)
    }

    // Builds the cross-product of intersections for a constituent set in which
    // at least one member is a union: each combination picks one member from
    // every union constituent (and keeps the non-union ones), then intersects
    // that combination. `never` combinations are dropped.
    // Go: internal/checker/checker.go:Checker.getCrossProductIntersections
    fn get_cross_product_intersections(&mut self, types: &[TypeId]) -> Vec<TypeId> {
        let count = self.get_cross_product_union_size(types);
        let mut intersections: Vec<TypeId> = Vec::new();
        for i in 0..count {
            let mut constituents = types.to_vec();
            let mut n = i;
            for j in (0..types.len()).rev() {
                if self.get_type(types[j]).flags().contains(TypeFlags::UNION) {
                    let source_types = self
                        .get_type(types[j])
                        .union_types()
                        .unwrap_or(&[])
                        .to_vec();
                    let length = source_types.len();
                    constituents[j] = source_types[n % length];
                    n /= length;
                }
            }
            let t = self.get_intersection_type(&constituents);
            if !self.get_type(t).flags().contains(TypeFlags::NEVER) {
                intersections.push(t);
            }
        }
        intersections
    }

    // Returns the number of constituents the cross-product union would have:
    // the product of every union constituent's member count, or 0 if any
    // constituent is `never`.
    // Go: internal/checker/checker.go:Checker.getCrossProductUnionSize
    fn get_cross_product_union_size(&self, types: &[TypeId]) -> usize {
        let mut size = 1usize;
        for &t in types {
            let flags = self.get_type(t).flags();
            if flags.contains(TypeFlags::UNION) {
                size *= self.get_type(t).union_types().map_or(1, |u| u.len());
            } else if flags.contains(TypeFlags::NEVER) {
                return 0;
            }
        }
        size
    }

    // Adds `t` to an intersection's constituent set, flattening nested
    // intersections, normalizing fresh literals to their regular form,
    // dropping duplicates, and accumulating the `includes` flags used by the
    // reductions in [`Checker::get_intersection_type`].
    //
    // DEFER(phase-4-checker-later): the empty-anonymous-object special case,
    // the `missingType` rewrite, and the distinct-unit-types reduction (Go ORs
    // in `NonPrimitive` to force an empty intersection).
    // blocked-by: empty-object/missing-type construction and the disjoint-domain
    // reductions are out of this round's reach.
    // Go: internal/checker/checker.go:Checker.addTypeToIntersection
    fn add_type_to_intersection(
        &self,
        type_set: &mut Vec<TypeId>,
        mut includes: TypeFlags,
        t: TypeId,
    ) -> TypeFlags {
        // Go: getRegularTypeOfLiteralType — a fresh literal joins as its regular
        // counterpart so `"a" & T` interns identically to a declared `"a"`.
        let t = self.regular_type_of_literal_type(t);
        let flags = self.get_type(t).flags();
        if flags.contains(TypeFlags::INTERSECTION) {
            let nested = self
                .get_type(t)
                .intersection_types()
                .unwrap_or(&[])
                .to_vec();
            for n in nested {
                includes = self.add_type_to_intersection(type_set, includes, n);
            }
            return includes;
        }
        if flags.intersects(TypeFlags::ANY_OR_UNKNOWN) {
            // `any`/`unknown` are not added to the set: `unknown` is the
            // intersection identity and drops out; `any` short-circuits later.
            if t == self.error_type {
                includes |= TypeFlags::INCLUDES_ERROR;
            }
        } else if !flags.intersects(TypeFlags::NULLABLE) {
            // strictNullChecks is not yet wired; under non-strict semantics a
            // nullable constituent is dropped (Go's `flags&Nullable == 0`).
            if !type_set.contains(&t) {
                type_set.push(t);
            }
        }
        includes |= flags & TypeFlags::INCLUDES_MASK;
        includes
    }

    // Normalizes a fresh literal type to its regular counterpart (Go's
    // `getRegularTypeOfLiteralType` for the fresh/regular literal pair).
    // Go: internal/checker/checker.go:Checker.getRegularTypeOfLiteralType
    pub(crate) fn regular_type_of_literal_type(&self, t: TypeId) -> TypeId {
        if let TypeData::Literal(d) = &self.get_type(t).data {
            if d.fresh_type == Some(t) {
                return d.regular_type.unwrap_or(t);
            }
        }
        t
    }

    // Returns the fresh form of a freshable (literal/enum) type, allocating and
    // linking it on first use. A literal expression carries the FRESH literal
    // type, paired to the interned regular one via `freshType`/`regularType`:
    // the fresh's `regularType` is the source `t` and its `freshType` is itself,
    // while `t`'s `freshType` is the new fresh type, so `regularType` of the
    // fresh resolves back to `t`. Non-freshable types are returned unchanged.
    //
    // Side effects: on first use for `t`, allocates the fresh literal type and
    // links the fresh/regular pair.
    // Go: internal/checker/checker.go:Checker.getFreshTypeOfLiteralType(25146)
    pub(crate) fn get_fresh_type_of_literal_type(&mut self, t: TypeId) -> TypeId {
        let (value, flags, symbol, existing_fresh) = {
            let ty = self.get_type(t);
            if !ty.flags().intersects(TypeFlags::FRESHABLE) {
                return t;
            }
            match &ty.data {
                TypeData::Literal(d) => (d.value.clone(), ty.flags, ty.symbol, d.fresh_type),
                _ => return t,
            }
        };
        if let Some(fresh) = existing_fresh {
            return fresh;
        }
        let fresh = self.new_literal_type(flags, value, Some(t));
        self.types.get_mut(fresh).symbol = symbol;
        set_literal_fresh_type(&mut self.types, fresh, fresh);
        set_literal_fresh_type(&mut self.types, t, fresh);
        fresh
    }

    // Reports whether `t` is a fresh literal type (Go's `isFreshLiteralType`): a
    // freshable type whose `freshType` link points back at itself.
    // Go: internal/checker/checker.go:isFreshLiteralType(25160)
    pub(crate) fn is_fresh_literal_type(&self, t: TypeId) -> bool {
        let ty = self.get_type(t);
        ty.flags().intersects(TypeFlags::FRESHABLE)
            && matches!(&ty.data, TypeData::Literal(d) if d.fresh_type == Some(t))
    }

    // Widens a fresh literal type to its base primitive: a fresh string literal
    // (`"a"`) widens to `string`. A regular (non-fresh) literal, or a non-literal
    // type, is returned unchanged, so a literal in a `const`/readonly position
    // (which carries the regular form) keeps its literal type.
    //
    // DEFER(phase-4-checker-later): the bigint fresh-literal arm (no bigint
    // literal expression is typed yet), the enum-like base type, and the union
    // `mapType` arm are deferred. blocked-by: bigint literal typing, enum base
    // types, and union `mapType` over `getWidenedLiteralType`.
    // Go: internal/checker/checker.go:Checker.getWidenedLiteralType(25346)
    pub(crate) fn get_widened_literal_type(&mut self, t: TypeId) -> TypeId {
        let flags = self.get_type(t).flags();
        if flags.intersects(TypeFlags::STRING_LITERAL) && self.is_fresh_literal_type(t) {
            return self.string_type;
        }
        if flags.intersects(TypeFlags::NUMBER_LITERAL) && self.is_fresh_literal_type(t) {
            return self.number_type;
        }
        if flags.intersects(TypeFlags::BOOLEAN_LITERAL) && self.is_fresh_literal_type(t) {
            return self.boolean_type;
        }
        t
    }

    /// Sets the `JSX.IntrinsicElements` type used to resolve intrinsic JSX tags.
    ///
    /// This is the injection point standing in for lib-global resolution until
    /// the real `JSX` namespace is available (P6).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, ObjectFlags, ObjectType};
    /// let mut c = Checker::new();
    /// let t = c.new_object_type(ObjectFlags::INTERFACE, None, ObjectType::default());
    /// c.set_jsx_intrinsic_elements(t);
    /// ```
    ///
    /// Side effects: stores the type id on the checker.
    // Go: internal/checker/jsx.go:Checker.getJsxType(JsxNames.IntrinsicElements) (injected)
    // blocked-by: lib globals (P6) — the real `JSX.IntrinsicElements` resolution.
    pub fn set_jsx_intrinsic_elements(&mut self, t: TypeId) {
        self.jsx_intrinsic_elements = Some(t);
    }

    /// Returns the checker's emit-time query handle, constructing it once
    /// (Go's `GetEmitResolver`, cached behind a `sync.Once`).
    ///
    /// The returned [`EmitResolver`] is a lightweight value handle; its query
    /// methods take the program (and checker, for type-backed queries).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let c = Checker::new();
    /// let _resolver = c.get_emit_resolver();
    /// ```
    ///
    /// Side effects: initializes the cached resolver on first call.
    // Go: internal/checker/checker.go:Checker.GetEmitResolver(31832)
    pub fn get_emit_resolver(&self) -> EmitResolver {
        *self.emit_resolver.get_or_init(EmitResolver::default)
    }

    /// Records the meaning(s) under which `symbol` was referenced.
    ///
    /// Demonstrates the per-symbol link-store wiring that 4b's name resolver
    /// builds on; the flags accumulate across calls.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// use tsgo_ast::{SymbolFlags, SymbolId};
    /// let mut c = Checker::new();
    /// let s = SymbolId(1);
    /// c.mark_symbol_referenced(s, SymbolFlags::VALUE);
    /// c.mark_symbol_referenced(s, SymbolFlags::TYPE);
    /// assert_eq!(c.symbol_reference_kinds(s), SymbolFlags::VALUE | SymbolFlags::TYPE);
    /// ```
    ///
    /// Side effects: mutates the checker's symbol-reference link store.
    ///
    /// Note: a 4a scaffolding helper exercising the link-store wiring; Go
    /// accumulates `referenceKinds` inline in its resolve paths (ported in 4b).
    // Go: internal/checker/types.go:SymbolReferenceLinks (referenceKinds)
    pub fn mark_symbol_referenced(
        &mut self,
        symbol: symbols::SymbolId,
        meaning: symbols::SymbolFlags,
    ) {
        let links = self.symbol_reference_links.get(symbol);
        links.reference_kinds |= meaning;
    }

    /// Returns the accumulated reference-kind flags for `symbol`.
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/types.go:SymbolReferenceLinks.referenceKinds
    pub fn symbol_reference_kinds(&self, symbol: symbols::SymbolId) -> symbols::SymbolFlags {
        self.symbol_reference_links
            .try_get(&symbol)
            .map(|l| l.reference_kinds)
            .unwrap_or_else(symbols::SymbolFlags::empty)
    }

    /// The intrinsic `any` type.
    ///
    /// Side effects: none (pure).
    pub fn any_type(&self) -> TypeId {
        self.any_type
    }

    /// The `any` type used where inference must be suppressed (`autoType`).
    ///
    /// Side effects: none (pure).
    pub fn auto_type(&self) -> TypeId {
        self.auto_type
    }

    /// The `error` (any-like) type produced on type errors.
    ///
    /// Side effects: none (pure).
    pub fn error_type(&self) -> TypeId {
        self.error_type
    }

    /// The intrinsic `unknown` type.
    ///
    /// Side effects: none (pure).
    pub fn unknown_type(&self) -> TypeId {
        self.unknown_type
    }

    /// The intrinsic `undefined` type.
    ///
    /// Side effects: none (pure).
    pub fn undefined_type(&self) -> TypeId {
        self.undefined_type
    }

    /// The intrinsic `null` type.
    ///
    /// Side effects: none (pure).
    pub fn null_type(&self) -> TypeId {
        self.null_type
    }

    /// The intrinsic `string` type.
    ///
    /// Side effects: none (pure).
    pub fn string_type(&self) -> TypeId {
        self.string_type
    }

    /// The intrinsic `number` type.
    ///
    /// Side effects: none (pure).
    pub fn number_type(&self) -> TypeId {
        self.number_type
    }

    /// The intrinsic `bigint` type.
    ///
    /// Side effects: none (pure).
    pub fn bigint_type(&self) -> TypeId {
        self.bigint_type
    }

    /// The intrinsic ES `symbol` type.
    ///
    /// Side effects: none (pure).
    pub fn es_symbol_type(&self) -> TypeId {
        self.es_symbol_type
    }

    /// The intrinsic `void` type.
    ///
    /// Side effects: none (pure).
    pub fn void_type(&self) -> TypeId {
        self.void_type
    }

    /// The intrinsic `never` type.
    ///
    /// Side effects: none (pure).
    pub fn never_type(&self) -> TypeId {
        self.never_type
    }

    /// The `never` type flagged non-inferrable (`silentNeverType`).
    ///
    /// Side effects: none (pure).
    pub fn silent_never_type(&self) -> TypeId {
        self.silent_never_type
    }

    /// The intrinsic non-primitive `object` type.
    ///
    /// Side effects: none (pure).
    pub fn non_primitive_type(&self) -> TypeId {
        self.non_primitive_type
    }

    /// The regular (non-fresh) `false` literal type.
    ///
    /// Side effects: none (pure).
    pub fn regular_false_type(&self) -> TypeId {
        self.regular_false_type
    }

    /// The fresh `false` literal type.
    ///
    /// Side effects: none (pure).
    pub fn false_type(&self) -> TypeId {
        self.false_type
    }

    /// The regular (non-fresh) `true` literal type.
    ///
    /// Side effects: none (pure).
    pub fn regular_true_type(&self) -> TypeId {
        self.regular_true_type
    }

    /// The fresh `true` literal type.
    ///
    /// Side effects: none (pure).
    pub fn true_type(&self) -> TypeId {
        self.true_type
    }

    /// The `boolean` type (the `false | true` union).
    ///
    /// Side effects: none (pure).
    pub fn boolean_type(&self) -> TypeId {
        self.boolean_type
    }

    /// The `string | number` union type.
    ///
    /// Side effects: none (pure).
    pub fn string_or_number_type(&self) -> TypeId {
        self.string_or_number_type
    }

    /// The `number | bigint` union type.
    ///
    /// Side effects: none (pure).
    pub fn number_or_bigint_type(&self) -> TypeId {
        self.number_or_bigint_type
    }
}

/// Allocates an intrinsic type into `types`, clearing the cache-only object
/// flags exactly as Go's `newType` does.
// Go: internal/checker/checker.go:Checker.newIntrinsicTypeEx
fn new_intrinsic(
    types: &mut TypeArena,
    flags: TypeFlags,
    name: &str,
    object_flags: ObjectFlags,
) -> TypeId {
    let cleared = object_flags & !ObjectFlags::FRESH_ALLOCATION_CLEARED;
    types.alloc(
        flags,
        cleared,
        None,
        TypeData::Intrinsic(IntrinsicType {
            intrinsic_name: name.to_string(),
        }),
    )
}

/// Allocates a literal type, linking its regular counterpart to `regular_type`
/// (or to itself when `None`), mirroring Go's `newLiteralType`.
// Go: internal/checker/checker.go:Checker.newLiteralType
fn new_literal_type_in(
    types: &mut TypeArena,
    flags: TypeFlags,
    value: LiteralValue,
    regular_type: Option<TypeId>,
) -> TypeId {
    let id = types.alloc(
        flags,
        ObjectFlags::empty(),
        None,
        TypeData::Literal(LiteralType {
            value,
            fresh_type: None,
            regular_type: None,
        }),
    );
    let regular = regular_type.unwrap_or(id);
    if let TypeData::Literal(d) = &mut types.get_mut(id).data {
        d.regular_type = Some(regular);
    }
    id
}

/// Returns the canonical map key for a number-literal value.
///
/// `NaN` maps to a single fixed key (Go caches `NaN` in a separate `nanType`
/// because `NaN != NaN` makes a float map-key always miss) and `+0`/`-0` both
/// map to `0` (Go's float map-key treats them as equal), so the resulting cache
/// has Go-identical value-uniqueness for the reachable literal values.
// Go: internal/checker/checker.go:Checker.getNumberLiteralType (NaN special-case + map key)
fn number_literal_key(value: tsgo_jsnum::Number) -> u64 {
    let f = f64::from(value);
    if f.is_nan() {
        f64::NAN.to_bits()
    } else if f == 0.0 {
        // Collapse +0.0 and -0.0 to one key.
        0
    } else {
        f.to_bits()
    }
}

/// Sets the `fresh_type` link of a literal type (a no-op for non-literals).
// Go: internal/checker/checker.go:NewChecker (regularXType.freshType = xType)
fn set_literal_fresh_type(types: &mut TypeArena, id: TypeId, fresh: TypeId) {
    if let TypeData::Literal(d) = &mut types.get_mut(id).data {
        d.fresh_type = Some(fresh);
    }
}

/// Interns the union of `members`: deduplicates and id-sorts them, returns
/// `None` for the empty union (the caller substitutes `never`), the lone member
/// for a singleton, and an interned [`TypeData::Union`] otherwise.
// Go: internal/checker/checker.go:Checker.getUnionType (structural core)
fn intern_union(
    types: &mut TypeArena,
    cache: &mut FxHashMap<Vec<TypeId>, TypeId>,
    mut members: Vec<TypeId>,
) -> Option<TypeId> {
    members.sort();
    members.dedup();
    match members.len() {
        0 => None,
        1 => Some(members[0]),
        _ => {
            if let Some(&id) = cache.get(&members) {
                return Some(id);
            }
            let id = types.alloc(
                TypeFlags::UNION,
                ObjectFlags::empty(),
                None,
                TypeData::Union(UnionType {
                    types: members.clone(),
                }),
            );
            cache.insert(members, id);
            Some(id)
        }
    }
}

// Reports whether an intersection's accumulated `includes` flags span two
// disjoint domains, which makes the intersection empty (`never`). Mirrors the
// non-strictNullChecks subset of Go's guard in `getIntersectionTypeEx`: for
// each disjoint domain present, if any *other* disjoint domain is also present
// the intersection is empty (e.g. `string & number`, `object & string`).
//
// The strictNullChecks-only `Nullable & (Object | NonPrimitive)` clause is not
// included (strictNullChecks is not wired), and the unit-type reduction is
// handled separately (DEFER'd) via `add_type_to_intersection`.
// Go: internal/checker/checker.go:Checker.getIntersectionTypeEx (TypeFlagsDisjointDomains guard)
fn is_disjoint_domain_intersection(includes: TypeFlags) -> bool {
    use TypeFlags as F;
    let dd = F::DISJOINT_DOMAINS;
    let spans =
        |domain: F| includes.intersects(domain) && includes.intersects(dd.difference(domain));
    spans(F::NON_PRIMITIVE)
        || spans(F::STRING_LIKE)
        || spans(F::NUMBER_LIKE)
        || spans(F::BIG_INT_LIKE)
        || spans(F::ES_SYMBOL_LIKE)
        || spans(F::VOID_LIKE)
}

/// Interns a multi-member intersection: id-sorts the constituents (for a stable
/// key, mirroring the union sibling) and returns the cached id when present,
/// else allocates a fresh [`TypeData::Intersection`].
///
/// The caller ([`Checker::get_intersection_type`]) has already flattened,
/// deduplicated, and applied the trivial reductions, so `members` here always
/// has at least two distinct constituents.
// Go: internal/checker/checker.go:Checker.getIntersectionType (intern + newIntersectionType)
fn intern_intersection(
    types: &mut TypeArena,
    cache: &mut FxHashMap<Vec<TypeId>, TypeId>,
    mut members: Vec<TypeId>,
) -> TypeId {
    members.sort();
    if let Some(&id) = cache.get(&members) {
        return id;
    }
    let id = types.alloc(
        TypeFlags::INTERSECTION,
        ObjectFlags::empty(),
        None,
        TypeData::Intersection(IntersectionType {
            types: members.clone(),
        }),
    );
    cache.insert(members, id);
    id
}

/// Renders a literal value as Go's `typeToString` would for its literal type.
///
/// DEFER(phase-4-checker-4j): the JS-canonical number-to-string algorithm and
/// quote-style selection are refined with the printer in 4j; 4b uses Rust's
/// default `f64` formatting (no number literals are constructed yet).
// Go: internal/checker/printer.go (literal rendering within typeToString)
fn literal_value_to_string(value: &LiteralValue) -> String {
    match value {
        LiteralValue::Boolean(true) => "true".to_string(),
        LiteralValue::Boolean(false) => "false".to_string(),
        LiteralValue::String(s) => format!("\"{s}\""),
        LiteralValue::Number(n) => f64::from(*n).to_string(),
    }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
