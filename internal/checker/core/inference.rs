//! Type inference for generic signatures.
//!
//! Ports the reachable core of Go's `inference.go`: the [`InferenceInfo`] /
//! [`InferenceContext`] state, `infer_types` (walk matched source/target
//! structures collecting candidates into type-parameter slots), and
//! `get_inferred_type`/`get_inferred_types` (resolve each slot to a best-common
//! type). Plus the generic-call helper `infer_type_arguments` that closes the
//! loop with 4d's `instantiate_signature`.
//!
//! 4e covers the common cases (bare type parameters, generic type references,
//! unions, and object members). Contravariant inference, the priority lattice,
//! mapped/conditional/template inference, and the lazy inference `TypeMapper`
//! are deferred; 4e builds an eager `Array` mapper from the inferred types.

use rustc_hash::FxHashSet;

use super::declared_types::{get_properties_of_type, get_property_of_type, get_type_of_symbol};
use super::mapper::TypeMapper;
use super::program::BoundProgram;
use super::types::{TypeFlags, TypeId};
use super::Checker;

/// The priority of an inference candidate set (Go's `InferencePriority`).
///
/// Lower is stronger. 4e only uses [`InferencePriority::NONE`]; the full
/// priority lattice (return-type, mapped-type, keyof, ...) is deferred.
///
/// # Examples
/// ```
/// use tsgo_checker::InferencePriority;
/// assert_eq!(InferencePriority::default(), InferencePriority::NONE);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/checker.go:InferencePriority
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct InferencePriority(pub i32);

impl InferencePriority {
    /// No special priority (the default for 4e inferences).
    pub const NONE: InferencePriority = InferencePriority(0);
    /// A naked type variable in a union/intersection (Go `1 << 0`).
    pub const NAKED_TYPE_VARIABLE: InferencePriority = InferencePriority(1 << 0);
    /// An inference from a generic function's return type (Go `1 << 7`).
    pub const RETURN_TYPE: InferencePriority = InferencePriority(1 << 7);
    /// Inference circularity (less than all other priorities).
    pub const CIRCULARITY: InferencePriority = InferencePriority(-1);
}

impl Default for InferencePriority {
    fn default() -> Self {
        InferencePriority::NONE
    }
}

/// Inference candidates for one type parameter (Go's `InferenceInfo`).
///
/// # Examples
/// ```
/// use tsgo_checker::{InferenceInfo, TypeId};
/// let info = InferenceInfo::new(TypeId(1));
/// assert_eq!(info.type_parameter, TypeId(1));
/// assert!(info.candidates.is_empty());
/// assert!(!info.is_fixed);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/checker.go:InferenceInfo
#[derive(Clone, Debug)]
pub struct InferenceInfo {
    /// The type parameter inferences are being made for.
    pub type_parameter: TypeId,
    /// Candidates from covariant positions.
    pub candidates: Vec<TypeId>,
    /// Candidates from contravariant positions.
    pub contra_candidates: Vec<TypeId>,
    /// Cached resolved inferred type.
    pub inferred_type: Option<TypeId>,
    /// Priority of the current candidate set.
    pub priority: InferencePriority,
    /// Whether all inferences are to top-level occurrences.
    pub top_level: bool,
    /// Whether inferences are fixed (no further candidates accepted).
    pub is_fixed: bool,
}

impl InferenceInfo {
    /// Creates an empty inference slot for `type_parameter`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{InferenceInfo, TypeId};
    /// assert!(InferenceInfo::new(TypeId(2)).candidates.is_empty());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/checker.go:Checker.createInferenceInfo
    pub fn new(type_parameter: TypeId) -> InferenceInfo {
        InferenceInfo {
            type_parameter,
            candidates: Vec::new(),
            contra_candidates: Vec::new(),
            inferred_type: None,
            priority: InferencePriority::NONE,
            top_level: true,
            is_fixed: false,
        }
    }
}

/// The inference state for a generic signature (Go's `InferenceContext`).
///
/// # Examples
/// ```
/// use tsgo_checker::{InferenceContext, TypeId};
/// let ctx = InferenceContext::new(&[TypeId(1), TypeId(2)]);
/// assert_eq!(ctx.inferences.len(), 2);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/checker.go:InferenceContext
#[derive(Clone, Debug)]
pub struct InferenceContext {
    /// One inference slot per type parameter.
    pub inferences: Vec<InferenceInfo>,
    /// The type parameters being inferred, in order.
    pub type_parameters: Vec<TypeId>,
}

impl InferenceContext {
    /// Creates a context with an empty inference slot per type parameter.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{InferenceContext, TypeId};
    /// let ctx = InferenceContext::new(&[TypeId(1)]);
    /// assert_eq!(ctx.type_parameters, vec![TypeId(1)]);
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/checker.go:Checker.createInferenceContext
    pub fn new(type_parameters: &[TypeId]) -> InferenceContext {
        InferenceContext {
            inferences: type_parameters
                .iter()
                .map(|&tp| InferenceInfo::new(tp))
                .collect(),
            type_parameters: type_parameters.to_vec(),
        }
    }
}

impl Checker {
    /// Collects inference candidates from `source` into the slots in
    /// `inferences` by structurally matching it against `target`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker, InferenceContext};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, tp: tsgo_checker::TypeId) {
    /// let mut ctx = InferenceContext::new(&[tp]);
    /// let n = c.number_type();
    /// c.infer_types(p, &mut ctx.inferences, n, tp);
    /// # }
    /// ```
    ///
    /// Side effects: pushes candidates into `inferences`.
    // Go: internal/checker/inference.go:Checker.inferTypes
    pub fn infer_types(
        &mut self,
        program: &dyn BoundProgram,
        inferences: &mut [InferenceInfo],
        source: TypeId,
        target: TypeId,
    ) {
        let mut visited: FxHashSet<(TypeId, TypeId)> = FxHashSet::default();
        self.infer_from_types(program, inferences, &mut visited, source, target);
    }

    // Go: internal/checker/inference.go:Checker.inferFromTypes
    fn infer_from_types(
        &mut self,
        program: &dyn BoundProgram,
        inferences: &mut [InferenceInfo],
        visited: &mut FxHashSet<(TypeId, TypeId)>,
        source: TypeId,
        target: TypeId,
    ) {
        // Target is a type parameter in our inference set: add a candidate.
        if self
            .get_type(target)
            .flags()
            .contains(TypeFlags::TYPE_PARAMETER)
        {
            if let Some(idx) = inferences.iter().position(|i| i.type_parameter == target) {
                if !inferences[idx].is_fixed && !inferences[idx].candidates.contains(&source) {
                    inferences[idx].candidates.push(source);
                }
            }
            return;
        }
        // Target union: infer from the source to each constituent.
        if self.get_type(target).flags().contains(TypeFlags::UNION) {
            let members = self.get_type(target).union_types().unwrap_or(&[]).to_vec();
            for m in members {
                self.infer_from_types(program, inferences, visited, source, m);
            }
            return;
        }
        // Source union: infer from each constituent to the target.
        if self.get_type(source).flags().contains(TypeFlags::UNION) {
            let members = self.get_type(source).union_types().unwrap_or(&[]).to_vec();
            for m in members {
                self.infer_from_types(program, inferences, visited, m, target);
            }
            return;
        }
        // Generic type references to the same target: infer from type arguments.
        let source_ref = self
            .get_type(source)
            .as_object()
            .and_then(|o| o.target.map(|t| (t, o.resolved_type_arguments.clone())));
        let target_ref = self
            .get_type(target)
            .as_object()
            .and_then(|o| o.target.map(|t| (t, o.resolved_type_arguments.clone())));
        if let (Some((s_target, s_args)), Some((t_target, t_args))) = (&source_ref, &target_ref) {
            if s_target == t_target {
                if !visited.insert((source, target)) {
                    return;
                }
                for i in 0..s_args.len().min(t_args.len()) {
                    self.infer_from_types(program, inferences, visited, s_args[i], t_args[i]);
                }
                return;
            }
        }
        // Two object types: infer member-by-member on matching property names.
        if self.get_type(source).flags().contains(TypeFlags::OBJECT)
            && self.get_type(target).flags().contains(TypeFlags::OBJECT)
        {
            if !visited.insert((source, target)) {
                return;
            }
            for (name, _) in get_properties_of_type(self, target) {
                let (Some(source_prop), Some(target_prop)) = (
                    get_property_of_type(self, source, &name),
                    get_property_of_type(self, target, &name),
                ) else {
                    continue;
                };
                let source_type = get_type_of_symbol(self, program, source_prop, None);
                let target_type = get_type_of_symbol(self, program, target_prop, None);
                self.infer_from_types(program, inferences, visited, source_type, target_type);
            }
        }
        // DEFER(phase-4-checker-4f+): index/indexed-access/conditional/template/
        // mapped/substitution inference and contravariant positions.
        // blocked-by: those type constructors + variance land later.
    }

    /// Resolves the inferred type for slot `index`, caching the result.
    ///
    /// With candidates, returns their best common type; with none, returns the
    /// `unknown` default (Go infers the constraint/default/`unknown`).
    ///
    /// DEFER(phase-4-checker-4f): type-parameter defaults and constraints, and
    /// the co-/contra-variant preference logic.
    /// blocked-by: constraint/default resolution.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker, InferenceContext};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, tp: tsgo_checker::TypeId) {
    /// let mut ctx = InferenceContext::new(&[tp]);
    /// let _ = c.get_inferred_type(p, &mut ctx, 0);
    /// # }
    /// ```
    ///
    /// Side effects: caches the inferred type in `context`.
    // Go: internal/checker/inference.go:Checker.getInferredType
    pub fn get_inferred_type(
        &mut self,
        program: &dyn BoundProgram,
        context: &mut InferenceContext,
        index: usize,
    ) -> TypeId {
        if let Some(cached) = context.inferences[index].inferred_type {
            return cached;
        }
        let candidates = context.inferences[index].candidates.clone();
        let inferred = if candidates.is_empty() {
            self.unknown_type()
        } else {
            self.get_best_common_type(program, &candidates)
        };
        context.inferences[index].inferred_type = Some(inferred);
        inferred
    }

    /// Resolves every inference slot to its inferred type.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker, InferenceContext};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, tp: tsgo_checker::TypeId) {
    /// let mut ctx = InferenceContext::new(&[tp]);
    /// let _ = c.get_inferred_types(p, &mut ctx);
    /// # }
    /// ```
    ///
    /// Side effects: caches inferred types in `context`.
    // Go: internal/checker/inference.go:Checker.getInferredTypes
    pub fn get_inferred_types(
        &mut self,
        program: &dyn BoundProgram,
        context: &mut InferenceContext,
    ) -> Vec<TypeId> {
        (0..context.inferences.len())
            .map(|i| self.get_inferred_type(program, context, i))
            .collect()
    }

    /// Infers the type arguments for `type_parameters` from parallel
    /// `sources`/`targets` (argument types vs parameter types).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, tp: tsgo_checker::TypeId) {
    /// // f<T>(x: T): infer T from a number argument.
    /// let n = c.number_type();
    /// let inferred = c.infer_type_arguments(p, &[tp], &[n], &[tp]);
    /// assert_eq!(inferred, vec![n]);
    /// # }
    /// ```
    ///
    /// Side effects: builds a temporary inference context.
    // Go: internal/checker/inference.go (createInferenceContext + inferTypes + getInferredTypes)
    pub fn infer_type_arguments(
        &mut self,
        program: &dyn BoundProgram,
        type_parameters: &[TypeId],
        sources: &[TypeId],
        targets: &[TypeId],
    ) -> Vec<TypeId> {
        let mut context = InferenceContext::new(type_parameters);
        for i in 0..sources.len().min(targets.len()) {
            self.infer_types(program, &mut context.inferences, sources[i], targets[i]);
        }
        self.get_inferred_types(program, &mut context)
    }

    /// Builds the fixing inference `TypeMapper` (`type parameters -> inferred`).
    ///
    /// DEFER(phase-4-checker-4f): Go's lazy `InferenceTypeMapper` (which can
    /// trigger further inference on access); 4e uses an eager `Array` mapper.
    /// blocked-by: a context-capturing mapper variant.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker, InferenceContext};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, tp: tsgo_checker::TypeId) {
    /// let mut ctx = InferenceContext::new(&[tp]);
    /// let _ = c.get_inference_mapper(p, &mut ctx);
    /// # }
    /// ```
    ///
    /// Side effects: resolves the context's inferred types.
    // Go: internal/checker/inference.go:Checker.getMapperFromContext (eager form)
    pub fn get_inference_mapper(
        &mut self,
        program: &dyn BoundProgram,
        context: &mut InferenceContext,
    ) -> TypeMapper {
        let inferred = self.get_inferred_types(program, context);
        TypeMapper::Array {
            sources: context.type_parameters.clone(),
            targets: inferred,
        }
    }

    /// Returns the best common type of `types`: a member that all others are
    /// subtypes of, or their union if none dominates.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P) {
    /// let n = c.number_type();
    /// assert_eq!(c.get_best_common_type(p, &[n, n]), n);
    /// # }
    /// ```
    ///
    /// Side effects: populates the relation cache.
    // Go: internal/checker/checker.go:Checker.getCommonSupertype (4e subset)
    pub fn get_best_common_type(&mut self, program: &dyn BoundProgram, types: &[TypeId]) -> TypeId {
        for &candidate in types {
            let mut dominates = true;
            for &t in types {
                if !self.is_type_subtype_of(program, t, candidate) {
                    dominates = false;
                    break;
                }
            }
            if dominates {
                return candidate;
            }
        }
        self.get_union_type(types)
    }

    /// Removes union members that are subtypes of another (distinct) member.
    ///
    /// For mutually-related members the one with the smaller [`TypeId`] is kept.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P) {
    /// let n = c.number_type();
    /// let s = c.string_type();
    /// assert_eq!(c.subtype_reduce(p, &[n, s]).len(), 2); // disjoint -> kept
    /// # }
    /// ```
    ///
    /// Side effects: populates the relation cache.
    // Go: internal/checker/checker.go:Checker.removeSubtypes (4e subset)
    pub fn subtype_reduce(&mut self, program: &dyn BoundProgram, types: &[TypeId]) -> Vec<TypeId> {
        let mut result = Vec::new();
        for (i, &t) in types.iter().enumerate() {
            let mut subsumed = false;
            for (j, &u) in types.iter().enumerate() {
                if i == j || u == t {
                    continue;
                }
                if self.is_type_subtype_of(program, t, u) {
                    let mutual = self.is_type_subtype_of(program, u, t);
                    if !mutual || u < t {
                        subsumed = true;
                        break;
                    }
                }
            }
            if !subsumed {
                result.push(t);
            }
        }
        result
    }
}

#[cfg(test)]
#[path = "inference_test.rs"]
mod tests;
