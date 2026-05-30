//! Decomposition of Go's `internal/checker/checker.go` (the ~32k-line checker
//! body) into per-subsystem modules (PORTING, section 2).
//!
//! Round 4a populates [`types`] (the `Type` arena and flags) and [`symbols`]
//! (symbol-link scaffolding), and adds the [`Checker`] skeleton with intrinsic
//! type construction. Later sub-phases (4b..4k) add `relations`, `inference`,
//! `instantiation`, `flow`, and the rest.

pub mod check;
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

use std::cell::OnceCell;

use rustc_hash::FxHashMap;

use emit_resolver::EmitResolver;
use relations::RelationCache;
use signatures::{IndexInfo, IndexInfoArena, IndexInfoId, Signature, SignatureArena, SignatureId};
use symbols::{
    DeclaredTypeLinks, SymbolLinks, SymbolReferenceLinks, TypeAliasLinks, ValueSymbolLinks,
};
use types::{
    IntrinsicType, LiteralType, LiteralValue, ObjectFlags, ObjectType, Type, TypeArena, TypeData,
    TypeFlags, TypeId, TypeParameter, UnionType,
};

/// The TypeScript type checker.
///
/// Round 4a builds only the foundation: the [`TypeArena`], the intrinsic type
/// singletons, and the per-symbol link stores. The real entry point
/// `NewChecker(program)` — which binds a whole `Program` and drives checking —
/// is deferred until a `Program`/host exists (Phase 6); [`Checker::new`] here
/// constructs just the intrinsic substrate so type construction and printing
/// can be exercised in isolation.
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
    /// Lazily-built declared types for interface/class/enum symbols.
    declared_type_links: SymbolLinks<DeclaredTypeLinks>,
    /// Lazily-built declared types for type-alias symbols.
    type_alias_links: SymbolLinks<TypeAliasLinks>,
    /// Lazily-computed types of value/property symbols.
    value_symbol_links: SymbolLinks<ValueSymbolLinks>,
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
    /// Diagnostics recorded while checking (Go accumulates into a per-file
    /// `DiagnosticsCollection`; 4g keeps a flat list for the single stub file).
    diagnostics: Vec<check::Diagnostic>,
    /// The `JSX.IntrinsicElements` type, used to resolve intrinsic (lowercase)
    /// JSX tags. Resolved from lib globals in Go; until those land (P6) callers
    /// inject it via [`Checker::set_jsx_intrinsic_elements`].
    jsx_intrinsic_elements: Option<TypeId>,
    /// The cached emit-time query handle (Go's `GetEmitResolver` `sync.Once`).
    emit_resolver: OnceCell<EmitResolver>,

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
            declared_type_links: SymbolLinks::default(),
            type_alias_links: SymbolLinks::default(),
            value_symbol_links: SymbolLinks::default(),
            signatures: SignatureArena::new(),
            index_infos: IndexInfoArena::new(),
            relations: RelationCache::default(),
            instantiation_depth: 0,
            instantiation_count: 0,
            diagnostics: Vec::new(),
            jsx_intrinsic_elements: None,
            emit_resolver: OnceCell::new(),
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

    /// Constructs a checker for a bound `program` (the forward-compatible
    /// `NewChecker(program)` entry point).
    ///
    /// In 4k this only initializes the intrinsic substrate (delegating to
    /// [`Checker::new`]); the real `NewChecker` also binds the program's global
    /// scope and lib types, which is wired once a real host exists.
    ///
    /// blocked-by: `compiler.Program` + lib globals (P6) — global/lib binding,
    /// `getGlobalType` population, and storing the program on the checker.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(p: &P) -> Checker {
    /// Checker::new_checker(p)
    /// # }
    /// ```
    ///
    /// Side effects: allocates the intrinsic types in a fresh arena.
    // Go: internal/checker/checker.go:NewChecker
    pub fn new_checker(program: &dyn program::BoundProgram) -> Self {
        // The program is not yet retained; global/lib binding lands with P6.
        let _ = program;
        Checker::new()
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
