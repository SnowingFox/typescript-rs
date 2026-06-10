//! Type mappers and type instantiation.
//!
//! A [`TypeMapper`] records how type parameters are substituted; `instantiate_*`
//! walks a type/signature and applies the mapper, recreating composite types
//! with substituted constituents. Mirrors Go's `mapper.go` + `instantiateType`.
//!
//! 4d ports the mapper variants and the instantiation dispatch for type
//! parameters, unions, and generic type references. Deep anonymous-object,
//! mapped, conditional, indexed-access, and template-literal instantiation are
//! deferred to later sub-phases.

use super::signatures::SignatureId;
use super::types::{ObjectFlags, TypeFlags, TypeId};
use super::Checker;

/// Go reaches the error type after 100 nested instantiations.
// Go: internal/checker/checker.go:instantiateTypeWithAlias (instantiationDepth == 100)
const MAX_INSTANTIATION_DEPTH: u32 = 100;

/// Go bails after 5M instantiations from one statement.
// Go: internal/checker/checker.go:instantiateTypeWithAlias (instantiationCount >= 5_000_000)
const MAX_INSTANTIATION_COUNT: u32 = 5_000_000;

/// A substitution of type parameters to types.
///
/// Replaces Go's `*TypeMapper` interface graph with a closed enum. The
/// `Function` variant uses a plain `fn` pointer (Go allows arbitrary closures;
/// the checker-bound closures — inference/restrictive mappers — are deferred).
///
/// # Examples
/// ```
/// use tsgo_checker::TypeMapper;
/// use tsgo_checker::TypeId;
/// let m = TypeMapper::Simple { source: TypeId(1), target: TypeId(2) };
/// assert!(matches!(m, TypeMapper::Simple { .. }));
/// ```
///
/// Side effects: none (pure value type).
///
/// Note: not `PartialEq` because the `Function` variant holds a `fn` pointer
/// (pointer equality is not meaningful); compare by matching on the variant.
// Go: internal/checker/mapper.go:TypeMapper
#[derive(Clone, Debug)]
pub enum TypeMapper {
    /// Maps a single `source` type to `target`.
    Simple {
        /// The type to replace.
        source: TypeId,
        /// Its replacement.
        target: TypeId,
    },
    /// Maps `sources[i]` to `targets[i]`.
    Array {
        /// The types to replace.
        sources: Vec<TypeId>,
        /// Their replacements (same length as `sources`).
        targets: Vec<TypeId>,
    },
    /// Applies the first mapper, then the second (`m2(m1(t))`).
    Merged(Box<TypeMapper>, Box<TypeMapper>),
    /// Like [`TypeMapper::Merged`], but re-instantiates when the first mapper
    /// changes the type (Go's `CompositeTypeMapper`).
    Composite(Box<TypeMapper>, Box<TypeMapper>),
    /// Maps via a pure function.
    Function(fn(TypeId) -> TypeId),
}

impl TypeMapper {
    /// Builds the simplest mapper for parallel `sources`/`targets` slices.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{TypeId, TypeMapper};
    /// let m = TypeMapper::new(&[TypeId(1)], &[TypeId(2)]);
    /// assert!(matches!(m, TypeMapper::Simple { source, target } if source == TypeId(1) && target == TypeId(2)));
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/mapper.go:newTypeMapper
    pub fn new(sources: &[TypeId], targets: &[TypeId]) -> TypeMapper {
        if sources.len() == 1 {
            TypeMapper::Simple {
                source: sources[0],
                target: targets[0],
            }
        } else {
            TypeMapper::Array {
                sources: sources.to_vec(),
                targets: targets.to_vec(),
            }
        }
    }

    /// Builds a one-entry mapper `source -> target` (Go's `makeUnaryTypeMapper`
    /// / `newSimpleTypeMapper`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{TypeId, TypeMapper};
    /// let m = TypeMapper::unary(TypeId(1), TypeId(2));
    /// assert!(matches!(m, TypeMapper::Simple { source, target } if source == TypeId(1) && target == TypeId(2)));
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/mapper.go:newSimpleTypeMapper
    pub fn unary(source: TypeId, target: TypeId) -> TypeMapper {
        TypeMapper::Simple { source, target }
    }

    /// Sequences two mappers so the result applies `m1` then `m2`
    /// (`m2(m1(t))`); Go's `mergeTypeMappers`/`newMergedTypeMapper`.
    ///
    /// Unlike [`combine`](TypeMapper::combine), this does not re-instantiate
    /// when `m1` changes the type — it is a plain composition of two
    /// substitutions, used to thread an outer instantiation through an inner one.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, TypeId, TypeMapper};
    /// let mut c = Checker::new();
    /// let a = c.new_type_parameter(None);
    /// let b = c.new_type_parameter(None);
    /// // a -> b, then b -> number.
    /// let m = TypeMapper::merge(
    ///     TypeMapper::unary(a, b),
    ///     TypeMapper::unary(b, c.number_type()),
    /// );
    /// assert_eq!(c.map_type(&m, a), c.number_type());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/mapper.go:newMergedTypeMapper / mergeTypeMappers
    pub fn merge(m1: TypeMapper, m2: TypeMapper) -> TypeMapper {
        TypeMapper::Merged(Box::new(m1), Box::new(m2))
    }

    /// Combines two mappers so the second re-instantiates the (changed) result
    /// of the first; Go's `combineTypeMappers`/`newCompositeTypeMapper`.
    ///
    /// `None` for `m1` yields `m2` unchanged (Go returns `m2` when `m1 == nil`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{TypeId, TypeMapper};
    /// let m2 = TypeMapper::unary(TypeId(1), TypeId(2));
    /// // With no first mapper, the combination is just `m2`.
    /// assert!(matches!(TypeMapper::combine(None, m2), TypeMapper::Simple { .. }));
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/mapper.go:Checker.combineTypeMappers
    pub fn combine(m1: Option<TypeMapper>, m2: TypeMapper) -> TypeMapper {
        match m1 {
            Some(m1) => TypeMapper::Composite(Box::new(m1), Box::new(m2)),
            None => m2,
        }
    }

    /// Appends a `source -> target` mapping after `mapper` (Go's
    /// `appendTypeMapping`): `None` yields the unary mapper, otherwise the unary
    /// mapping is merged after `mapper`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, TypeId, TypeMapper};
    /// let mut c = Checker::new();
    /// let a = c.new_type_parameter(None);
    /// let b = c.new_type_parameter(None);
    /// let m = TypeMapper::append_mapping(None, a, c.string_type());
    /// let m = TypeMapper::append_mapping(Some(m), b, c.number_type());
    /// assert_eq!(c.map_type(&m, a), c.string_type());
    /// assert_eq!(c.map_type(&m, b), c.number_type());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/mapper.go:appendTypeMapping
    pub fn append_mapping(
        mapper: Option<TypeMapper>,
        source: TypeId,
        target: TypeId,
    ) -> TypeMapper {
        match mapper {
            Some(mapper) => TypeMapper::merge(mapper, TypeMapper::unary(source, target)),
            None => TypeMapper::unary(source, target),
        }
    }
}

impl Checker {
    /// Maps a single type through `mapper` (Go's `TypeMapper.Map`).
    ///
    /// Simple/Array/Function look the type up directly; Merged composes; and
    /// Composite re-instantiates through the second mapper when the first one
    /// changes the type.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, TypeMapper};
    /// let mut c = Checker::new();
    /// let tp = c.new_type_parameter(None);
    /// let m = TypeMapper::Simple { source: tp, target: c.string_type() };
    /// assert_eq!(c.map_type(&m, tp), c.string_type());
    /// assert_eq!(c.map_type(&m, c.number_type()), c.number_type());
    /// ```
    ///
    /// Side effects: Composite mapping may allocate via `instantiate_type`.
    // Go: internal/checker/mapper.go:*TypeMapper.Map
    pub fn map_type(&mut self, mapper: &TypeMapper, t: TypeId) -> TypeId {
        match mapper {
            TypeMapper::Simple { source, target } => {
                if t == *source {
                    *target
                } else {
                    t
                }
            }
            TypeMapper::Array { sources, targets } => sources
                .iter()
                .position(|&s| s == t)
                .map(|i| targets[i])
                .unwrap_or(t),
            TypeMapper::Function(f) => f(t),
            TypeMapper::Merged(m1, m2) => {
                let t1 = self.map_type(m1, t);
                self.map_type(m2, t1)
            }
            TypeMapper::Composite(m1, m2) => {
                let t1 = self.map_type(m1, t);
                if t1 != t {
                    self.instantiate_type(t1, m2)
                } else {
                    self.map_type(m2, t)
                }
            }
        }
    }

    /// Instantiates `t` by applying `mapper`, recreating composite types with
    /// substituted constituents.
    ///
    /// Guards against runaway recursion (Go's depth/count limits), yielding the
    /// error type when exceeded.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, TypeMapper};
    /// let mut c = Checker::new();
    /// let tp = c.new_type_parameter(None);
    /// let m = TypeMapper::Simple { source: tp, target: c.number_type() };
    /// // A bare type parameter is substituted.
    /// assert_eq!(c.instantiate_type(tp, &m), c.number_type());
    /// // A type without type variables is returned unchanged.
    /// assert_eq!(c.instantiate_type(c.string_type(), &m), c.string_type());
    /// ```
    ///
    /// Side effects: may allocate new types; bumps the instantiation counters.
    // Go: internal/checker/checker.go:Checker.instantiateType / instantiateTypeWorker
    pub fn instantiate_type(&mut self, t: TypeId, mapper: &TypeMapper) -> TypeId {
        if self.instantiation_depth >= MAX_INSTANTIATION_DEPTH
            || self.instantiation_count >= MAX_INSTANTIATION_COUNT
        {
            // Go: internal/checker/checker.go:instantiateTypeWithAlias — emits
            // TS2589 on `c.currentNode` when the depth/count limit fires.
            self.report_instantiation_depth_error();
            return self.error_type();
        }
        self.instantiation_count += 1;
        self.instantiation_depth += 1;
        let result = self.instantiate_type_worker(t, mapper);
        self.instantiation_depth -= 1;
        result
    }

    /// Emits TS2589 on `current_node` (the node currently being checked), or
    /// silently drops if no program/node is available (test-only checker).
    // Go: internal/checker/checker.go:instantiateTypeWithAlias (c.error(c.currentNode, ...))
    fn report_instantiation_depth_error(&mut self) {
        let Some(node) = self.current_node else {
            return;
        };
        let Some(program) = self.retained_program() else {
            return;
        };
        use crate::core::check::Diagnostic;
        let msg = &tsgo_diagnostics::TYPE_INSTANTIATION_IS_EXCESSIVELY_DEEP_AND_POSSIBLY_INFINITE;
        let loc = program.arena().loc(node);
        let diagnostic = Diagnostic {
            code: msg.code(),
            category: msg.category(),
            message: msg.to_string(),
            start: loc.pos(),
            length: loc.end() - loc.pos(),
            related_information: Vec::new(),
            message_chain: Vec::new(),
        };
        self.diagnostics_by_file
            .entry(program.file_handle())
            .or_default()
            .push(diagnostic);
    }

    // Go: internal/checker/checker.go:Checker.instantiateTypeWorker
    fn instantiate_type_worker(&mut self, t: TypeId, mapper: &TypeMapper) -> TypeId {
        let flags = self.get_type(t).flags();
        if flags.contains(TypeFlags::TYPE_PARAMETER) {
            return self.map_type(mapper, t);
        }
        if flags.contains(TypeFlags::UNION) {
            let members = self.get_type(t).union_types().unwrap_or(&[]).to_vec();
            let instantiated: Vec<TypeId> = members
                .iter()
                .map(|&m| self.instantiate_type(m, mapper))
                .collect();
            if instantiated == members {
                return t;
            }
            return self.get_union_type(&instantiated);
        }
        if flags.contains(TypeFlags::INTERSECTION) {
            let members = self
                .get_type(t)
                .intersection_types()
                .unwrap_or(&[])
                .to_vec();
            let instantiated: Vec<TypeId> = members
                .iter()
                .map(|&m| self.instantiate_type(m, mapper))
                .collect();
            if instantiated == members {
                return t;
            }
            return self.get_intersection_type(&instantiated);
        }
        if flags.contains(TypeFlags::OBJECT) {
            // A mapped type `{ [K in C]: V }` re-resolves under the combined
            // mapper, eagerly producing a concrete anonymous object when its
            // modifiers type becomes concrete (Go's `instantiateMappedType`).
            if self
                .get_type(t)
                .object_flags()
                .contains(ObjectFlags::MAPPED)
            {
                return match self.retained_program() {
                    Some(program) => super::declared_types::instantiate_mapped_type(
                        self,
                        program.as_ref(),
                        t,
                        mapper,
                    ),
                    // No retained program: leave the mapped type deferred.
                    None => t,
                };
            }
            if let Some(obj) = self.get_type(t).as_object() {
                if let Some(target) = obj.target {
                    let args = obj.resolved_type_arguments.clone();
                    let new_args: Vec<TypeId> = args
                        .iter()
                        .map(|&a| self.instantiate_type(a, mapper))
                        .collect();
                    if new_args == args {
                        return t;
                    }
                    return self.create_type_reference(target, new_args);
                }
                if self
                    .get_type(t)
                    .object_flags()
                    .contains(crate::core::types::ObjectFlags::TUPLE)
                {
                    let readonly = obj.readonly;
                    let tuple_fixed_length = obj.tuple_fixed_length;
                    let tuple_min_length = obj.tuple_min_length;
                    let tuple_element_optional = obj.tuple_element_optional.clone();
                    let tuple_element_rest = obj.tuple_element_rest.clone();
                    let tuple_element_variadic = obj.tuple_element_variadic.clone();
                    let args = obj.resolved_type_arguments.clone();
                    let new_args: Vec<TypeId> = args
                        .iter()
                        .map(|&a| self.instantiate_type(a, mapper))
                        .collect();
                    if new_args == args {
                        return t;
                    }
                    return self.create_tuple_type_structured(
                        new_args,
                        readonly,
                        tuple_fixed_length,
                        tuple_min_length,
                        tuple_element_optional,
                        tuple_element_rest,
                        tuple_element_variadic,
                    );
                }
            }
            // DEFER(phase-4-checker-4e): anonymous/mapped object deep
            // instantiation (re-create members with instantiated types).
            // blocked-by: member-type instantiation (`getObjectTypeInstantiation`)
            // needs instantiated symbol types (4e).
            return t;
        }
        // `keyof X`: instantiate the target, then recompute `keyof` over it, so
        // `keyof T` with `T -> { a }` becomes `"a"` (Go's index arm).
        if flags.contains(TypeFlags::INDEX) {
            let target = self.get_type(t).as_index().expect("index type").target;
            let instantiated_target = self.instantiate_type(target, mapper);
            return super::declared_types::get_index_type(self, instantiated_target);
        }
        // `X[Y]`: instantiate both operands, then re-resolve the indexed access,
        // so `T[K]` with `T -> { a: number }, K -> "a"` becomes `number` (Go's
        // indexed-access arm).
        if flags.contains(TypeFlags::INDEXED_ACCESS) {
            let d = self
                .get_type(t)
                .as_indexed_access()
                .expect("indexed access type")
                .clone();
            let object_type = self.instantiate_type(d.object_type, mapper);
            let index_type = self.instantiate_type(d.index_type, mapper);
            // The property resolution path needs the bound program (to resolve
            // member/symbol types); it is the retained program in real checking.
            // A deferred re-form needs no program, so when the access stays
            // generic the result is interned without one.
            return match self.retained_program() {
                Some(program) => super::declared_types::get_indexed_access_type(
                    self,
                    program.as_ref(),
                    object_type,
                    index_type,
                )
                // Go: `getIndexedAccessTypeEx` with no access node yields
                // `unknownType` when nothing resolves.
                .unwrap_or_else(|| self.unknown_type()),
                // No retained program (intrinsic-only checker): only the
                // deferred path is reachable, so re-form the indexed access.
                None => self.new_indexed_access_type(object_type, index_type, d.access_flags),
            };
        }
        // A conditional type `T extends U ? X : Y`: re-resolve it under the
        // combined (own + incoming) mapper, distributing over a union check type
        // and resolving a branch once the check/extends types become concrete
        // (Go's `getConditionalTypeInstantiation`).
        if flags.contains(TypeFlags::CONDITIONAL) {
            let own_mapper = self.conditional_mapper(t);
            let combined = TypeMapper::combine(own_mapper, mapper.clone());
            return match self.retained_program() {
                Some(program) => super::conditional_types::get_conditional_type_instantiation(
                    self,
                    program.as_ref(),
                    t,
                    &combined,
                ),
                // No retained program (intrinsic-only checker): the branch type
                // nodes cannot be read, so leave the conditional deferred.
                None => t,
            };
        }
        // A template literal type `` `a${T}b` ``: instantiate each placeholder,
        // then rebuild via `getTemplateLiteralType` so concrete placeholders fold
        // into a string literal (Go's `instantiateType` template-literal arm).
        if flags.contains(TypeFlags::TEMPLATE_LITERAL) {
            let (texts, types) = {
                let d = self
                    .get_type(t)
                    .as_template_literal()
                    .expect("template literal type");
                (d.texts.clone(), d.types.clone())
            };
            let new_types: Vec<TypeId> = types
                .iter()
                .map(|&ty| self.instantiate_type(ty, mapper))
                .collect();
            if new_types == types {
                return t;
            }
            return super::conditional_types::get_template_literal_type(self, &texts, &new_types);
        }
        // A string-mapping type `Uppercase<S>`: instantiate the target, then
        // re-apply the mapping so a concrete string literal folds to its mapped
        // literal (Go's `instantiateType` string-mapping arm).
        if flags.contains(TypeFlags::STRING_MAPPING) {
            let d = self
                .get_type(t)
                .as_string_mapping()
                .expect("string mapping type")
                .clone();
            let new_target = self.instantiate_type(d.target, mapper);
            if new_target == d.target {
                return t;
            }
            return super::conditional_types::get_string_mapping_type(self, d.kind, new_target);
        }
        if flags.contains(TypeFlags::SUBSTITUTION) {
            let d = self
                .get_type(t)
                .as_substitution()
                .expect("substitution type")
                .clone();
            let new_base = self.instantiate_type(d.base_type, mapper);
            let new_constraint = self.instantiate_type(d.constraint, mapper);
            if new_base == d.base_type && new_constraint == d.constraint {
                return t;
            }
            return self.get_or_create_substitution_type_unchecked(new_base, new_constraint);
        }
        t
    }

    /// Instantiates a signature through `mapper`, returning a new signature id.
    ///
    /// The return type is instantiated eagerly; parameter types are mapped on
    /// read (the new signature stores the composed `mapper` and keeps the base
    /// parameter symbols), so [`Checker::get_type_at_position`] substitutes them
    /// through it. Re-instantiating an already-instantiated signature composes
    /// the two mappers (Go's `instantiateSignatureEx` chains via
    /// `combineTypeMappers` for the fresh-type-parameter case; the
    /// `eraseTypeParameters` path 4e/C-B1 uses simply substitutes, so a plain
    /// `merge` is the observationally-equivalent composition).
    ///
    /// DEFER(phase-4-checker-C-B2+): the fresh-type-parameter signature
    /// (`!eraseTypeParameters`) form, the `this`-parameter instantiation, and
    /// the type-predicate instantiation. blocked-by: nested generic signatures +
    /// `this`-typing + type predicates.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, Signature, SignatureFlags, TypeMapper};
    /// let mut c = Checker::new();
    /// let tp = c.new_type_parameter(None);
    /// let mut sig = Signature::new(SignatureFlags::NONE);
    /// sig.resolved_return_type = Some(tp);
    /// let id = c.new_signature(sig);
    /// let m = TypeMapper::Simple { source: tp, target: c.number_type() };
    /// let inst = c.instantiate_signature(id, &m);
    /// assert_eq!(c.signature(inst).resolved_return_type, Some(c.number_type()));
    /// ```
    ///
    /// Side effects: allocates a new signature; may allocate instantiated types.
    // Go: internal/checker/checker.go:Checker.instantiateSignature / instantiateSignatureEx
    pub fn instantiate_signature(
        &mut self,
        signature: SignatureId,
        mapper: &TypeMapper,
    ) -> SignatureId {
        let mut new_sig = self.signature(signature).clone();
        new_sig.target = Some(signature);
        // Compose with any mapper already on the source signature so a chained
        // instantiation applies both (inner first, then the new outer mapper).
        new_sig.mapper = Some(match new_sig.mapper.take() {
            Some(existing) => TypeMapper::merge(existing, mapper.clone()),
            None => mapper.clone(),
        });
        if let Some(ret) = new_sig.resolved_return_type {
            new_sig.resolved_return_type = Some(self.instantiate_type(ret, mapper));
        }
        self.new_signature(new_sig)
    }
}

#[cfg(test)]
#[path = "mapper_test.rs"]
mod tests;
