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
use super::types::{TypeFlags, TypeId};
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
            // DEFER(phase-4-checker-4g): Go also reports
            // `Type_instantiation_is_excessively_deep_and_possibly_infinite`.
            // blocked-by: diagnostics emission lands in 4g.
            return self.error_type();
        }
        self.instantiation_count += 1;
        self.instantiation_depth += 1;
        let result = self.instantiate_type_worker(t, mapper);
        self.instantiation_depth -= 1;
        result
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
        if flags.contains(TypeFlags::OBJECT) {
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
            }
            // DEFER(phase-4-checker-4e): anonymous/mapped object deep
            // instantiation (re-create members with instantiated types).
            // blocked-by: member-type instantiation (`getObjectTypeInstantiation`)
            // needs instantiated symbol types (4e).
            return t;
        }
        // DEFER(phase-4-checker-4e+): index/indexed-access/conditional/
        // template-literal/substitution instantiation.
        // blocked-by: those type constructors land across 4e+.
        t
    }

    /// Instantiates a signature's return type through `mapper`, returning a new
    /// signature id.
    ///
    /// DEFER(phase-4-checker-4e): instantiate the signature's own type
    /// parameters and parameter-symbol types (`instantiateSignature` full form).
    /// blocked-by: instantiated symbol types (4e).
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
    // Go: internal/checker/checker.go:Checker.instantiateSignature
    pub fn instantiate_signature(
        &mut self,
        signature: SignatureId,
        mapper: &TypeMapper,
    ) -> SignatureId {
        let mut new_sig = self.signature(signature).clone();
        new_sig.target = Some(signature);
        if let Some(ret) = new_sig.resolved_return_type {
            new_sig.resolved_return_type = Some(self.instantiate_type(ret, mapper));
        }
        self.new_signature(new_sig)
    }
}

#[cfg(test)]
#[path = "mapper_test.rs"]
mod tests;
