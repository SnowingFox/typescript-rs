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

use super::declared_types::{
    get_constraint_of_type_parameter, get_default_from_type_parameter, get_properties_of_type,
    get_property_of_type, get_type_of_symbol,
};
use super::mapper::TypeMapper;
use super::program::BoundProgram;
use super::signatures::SignatureId;
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
    /// The seed for inference priority tracking — higher than every real
    /// priority, so the first inference of any priority replaces it (Go
    /// `1 << 11`).
    pub const MAX_VALUE: InferencePriority = InferencePriority(1 << 11);
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
/// use tsgo_checker::{InferenceInfo, InferencePriority, TypeId};
/// let info = InferenceInfo::new(TypeId(1));
/// assert_eq!(info.type_parameter, TypeId(1));
/// assert!(info.candidates.is_empty());
/// assert!(!info.is_fixed);
/// assert_eq!(info.priority, InferencePriority::MAX_VALUE);
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
            // Seeded with the max priority (Go's `newInferenceInfo`), so the
            // first inference of any priority replaces it.
            priority: InferencePriority::MAX_VALUE,
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
        self.infer_types_with_priority(
            program,
            inferences,
            source,
            target,
            InferencePriority::NONE,
        );
    }

    /// Like [`infer_types`](Checker::infer_types) but records the candidates with
    /// `priority`. A lower-numbered priority is stronger: when an inference with a
    /// stronger priority reaches a slot, it discards weaker-priority candidates
    /// (Go's `inferWithPriority` -> `inferFromTypes` `priority < inference.priority`
    /// clear). The reachable subset uses [`InferencePriority::NONE`] for
    /// arguments and [`InferencePriority::RETURN_TYPE`] for contextual-return
    /// inference, so argument inferences override return-type inferences.
    ///
    /// Side effects: pushes candidates into `inferences`.
    // Go: internal/checker/inference.go:Checker.inferTypes (priority form)
    pub(crate) fn infer_types_with_priority(
        &mut self,
        program: &dyn BoundProgram,
        inferences: &mut [InferenceInfo],
        source: TypeId,
        target: TypeId,
        priority: InferencePriority,
    ) {
        let mut visited: FxHashSet<(TypeId, TypeId)> = FxHashSet::default();
        self.infer_from_types(program, inferences, &mut visited, source, target, priority);
    }

    // Go: internal/checker/inference.go:Checker.inferFromTypes
    fn infer_from_types(
        &mut self,
        program: &dyn BoundProgram,
        inferences: &mut [InferenceInfo],
        visited: &mut FxHashSet<(TypeId, TypeId)>,
        source: TypeId,
        target: TypeId,
        priority: InferencePriority,
    ) {
        // Target is a type parameter in our inference set: add a candidate,
        // honoring the priority lattice (the reachable subset: a stronger
        // priority clears weaker candidates).
        if self
            .get_type(target)
            .flags()
            .contains(TypeFlags::TYPE_PARAMETER)
        {
            if let Some(idx) = inferences.iter().position(|i| i.type_parameter == target) {
                if !inferences[idx].is_fixed {
                    if priority < inferences[idx].priority {
                        inferences[idx].candidates.clear();
                        inferences[idx].contra_candidates.clear();
                        inferences[idx].top_level = true;
                        inferences[idx].priority = priority;
                        inferences[idx].inferred_type = None;
                    }
                    if priority == inferences[idx].priority
                        && !inferences[idx].candidates.contains(&source)
                    {
                        inferences[idx].candidates.push(source);
                    }
                }
            }
            return;
        }
        // Target union: infer from the source to each constituent.
        if self.get_type(target).flags().contains(TypeFlags::UNION) {
            let members = self.get_type(target).union_types().unwrap_or(&[]).to_vec();
            for m in members {
                self.infer_from_types(program, inferences, visited, source, m, priority);
            }
            return;
        }
        // Source union: infer from each constituent to the target.
        if self.get_type(source).flags().contains(TypeFlags::UNION) {
            let members = self.get_type(source).union_types().unwrap_or(&[]).to_vec();
            for m in members {
                self.infer_from_types(program, inferences, visited, m, target, priority);
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
                    self.infer_from_types(
                        program, inferences, visited, s_args[i], t_args[i], priority,
                    );
                }
                return;
            }
        }
        // Two object types: infer member-by-member on matching property names,
        // then from matching call/construct signatures (Go's `inferFromProperties`
        // + `inferFromSignatures`). The signature arm is the keystone for callback
        // result inference: a callback argument's `(x: number) => R` is matched
        // against the parameter's `(x: T) => U`, inferring `U` from `R` covariantly
        // (and `T` from the parameter contravariantly).
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
                self.infer_from_types(
                    program,
                    inferences,
                    visited,
                    source_type,
                    target_type,
                    priority,
                );
            }
            self.infer_from_signatures(program, inferences, visited, source, target, priority);
        }
        // DEFER(phase-4-checker-C-C): index/indexed-access/conditional/template/
        // mapped/substitution inference and the construct-signature arm.
        // blocked-by: those type constructors + variance land later.
    }

    /// Infers candidates from the call signatures of `source` matched against
    /// those of `target` (Go's `inferFromSignatures`, call-kind, reachable
    /// subset). Source and target signatures are matched bottom-up; with one of
    /// each (the reachable callback case) it infers from the single source
    /// signature to the single target signature.
    ///
    /// DEFER(phase-4-checker-C-C): construct signatures, the multi-signature
    /// bottom-up matrix beyond a single pair, and `getBaseSignature`/
    /// `getErasedSignature` normalization. blocked-by: overload signature lists +
    /// signature erasure.
    // Go: internal/checker/inference.go:Checker.inferFromSignatures
    fn infer_from_signatures(
        &mut self,
        program: &dyn BoundProgram,
        inferences: &mut [InferenceInfo],
        visited: &mut FxHashSet<(TypeId, TypeId)>,
        source: TypeId,
        target: TypeId,
        priority: InferencePriority,
    ) {
        let source_signatures = self.get_signatures_of_type(source);
        let source_len = source_signatures.len();
        if source_len == 0 {
            return;
        }
        let target_signatures = self.get_signatures_of_type(target);
        let target_len = target_signatures.len();
        for (i, &target_signature) in target_signatures.iter().enumerate() {
            // Match source and target signatures bottom-up; when the source has
            // fewer signatures, the first source signature matches the excess
            // target signatures (Go: `max(sourceLen-targetLen+i, 0)`).
            let source_index = (source_len + i)
                .saturating_sub(target_len)
                .min(source_len - 1);
            self.infer_from_signature(
                program,
                inferences,
                visited,
                source_signatures[source_index],
                target_signature,
                priority,
            );
        }
    }

    /// Infers candidates from one source signature to one target signature: the
    /// parameter types (contravariant in Go; the reachable subset re-uses the
    /// covariant walk because the parameter type variables are already fixed when
    /// a callback is processed, so this is a no-op re-inference) and the return
    /// types (covariant — the `U` from a callback's return).
    ///
    /// DEFER(phase-4-checker-C-C): the real contravariant parameter inference
    /// (`inferFromContravariantTypes` into `contra_candidates`), the bivariance
    /// flip for method signatures, the `this`-parameter and rest-parameter
    /// positions, and type-predicate inference. blocked-by: the contravariant
    /// candidate lattice + `this`/rest/tuple types + type predicates.
    // Go: internal/checker/inference.go:Checker.inferFromSignature
    fn infer_from_signature(
        &mut self,
        program: &dyn BoundProgram,
        inferences: &mut [InferenceInfo],
        visited: &mut FxHashSet<(TypeId, TypeId)>,
        source: SignatureId,
        target: SignatureId,
        priority: InferencePriority,
    ) {
        // Parameters (Go: `applyToParameterTypes` with contravariant inference).
        let source_count = self.signature(source).parameters.len();
        let target_count = self.signature(target).parameters.len();
        let param_count = source_count.min(target_count);
        for i in 0..param_count {
            let s = self.get_type_at_position(program, source, i);
            let t = self.get_type_at_position(program, target, i);
            self.infer_from_types(program, inferences, visited, s, t, priority);
        }
        // Return types (Go: `applyToReturnTypes`, covariant): infer from the
        // source return type to the target return type when the latter could
        // contain type variables.
        let target_return = self.signature_return_type(target);
        if self.could_contain_type_variables(target_return) {
            let source_return = self.signature_return_type(source);
            self.infer_from_types(
                program,
                inferences,
                visited,
                source_return,
                target_return,
                priority,
            );
        }
    }

    /// Reports whether `t` could contain type variables that inference should
    /// descend into (Go's `couldContainTypeVariables`, reachable subset): a type
    /// parameter, a union/intersection with such a member, a generic type
    /// reference whose type arguments could, or an anonymous object whose call
    /// signatures' parameter/return types could.
    ///
    /// DEFER(phase-4-checker-C-C): the full object-flags caching, mapped/
    /// conditional/indexed-access/substitution constructors, and property-type
    /// scanning. blocked-by: those constructors + a couldContainTypeVariables
    /// object-flag cache.
    // Go: internal/checker/checker.go:Checker.couldContainTypeVariables
    pub(crate) fn could_contain_type_variables(&self, t: TypeId) -> bool {
        let flags = self.get_type(t).flags();
        // Any instantiable type (a type parameter, a `keyof T` index, a `T[K]`
        // indexed access, and — once ported — conditionals/substitutions/
        // template-literals/string-mappings) could contain type variables.
        if flags.intersects(TypeFlags::INSTANTIABLE) {
            return true;
        }
        if flags.intersects(TypeFlags::UNION | TypeFlags::INTERSECTION) {
            let members = if flags.contains(TypeFlags::UNION) {
                self.get_type(t).union_types().unwrap_or(&[]).to_vec()
            } else {
                self.get_type(t)
                    .intersection_types()
                    .unwrap_or(&[])
                    .to_vec()
            };
            return members
                .iter()
                .any(|&m| self.could_contain_type_variables(m));
        }
        if let Some(obj) = self.get_type(t).as_object() {
            if obj.target.is_some() {
                // A generic type reference could contain type variables when any
                // of its type arguments could (Go's reference arm:
                // `some(typeArguments, couldContainTypeVariables)`).
                return obj
                    .resolved_type_arguments
                    .iter()
                    .any(|&a| self.could_contain_type_variables(a));
            }
            // Anonymous object: scan its call signatures' parameter/return types.
            let signatures = obj.call_signatures.clone();
            for sig in signatures {
                if self.could_contain_type_variables(self.signature_return_type(sig)) {
                    return true;
                }
                let params = self.signature(sig).parameters.clone();
                for p in params {
                    if let Some(ty) = self
                        .value_symbol_links
                        .try_get(&p)
                        .and_then(|l| l.resolved_type)
                    {
                        if self.could_contain_type_variables(ty) {
                            return true;
                        }
                    }
                }
            }
        }
        false
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

    /// Resolves every inference slot of a *call* inference context to its
    /// inferred type, applying the call-resolution rules (Go's `getInferredTypes`
    /// reachable subset, distinct from the eager 4e [`get_inferred_types`]).
    ///
    /// Side effects: caches each slot's inferred type in `context`.
    // Go: internal/checker/inference.go:Checker.getInferredTypes
    pub(crate) fn get_inferred_types_for_call(
        &mut self,
        program: &dyn BoundProgram,
        context: &mut InferenceContext,
    ) -> Vec<TypeId> {
        (0..context.inferences.len())
            .map(|i| self.get_inferred_type_for_call(program, context, i))
            .collect()
    }

    /// Resolves the inferred type for one slot of a call inference context.
    ///
    /// With covariant candidates, returns their covariant inference (Go's
    /// `getCovariantInference`: `getCommonSupertype` then `getWidenedType`). With
    /// none, falls back to the type parameter's default, else `unknown` (Go's
    /// `InferenceFlagsNone` path). Finally, if the inferred type violates the
    /// type parameter's constraint, the constraint is inferred instead (Go's
    /// `getInferredType` constraint branch).
    ///
    /// Side effects: caches the inferred type in `context`.
    // Go: internal/checker/inference.go:Checker.getInferredType (reachable subset)
    pub(crate) fn get_inferred_type_for_call(
        &mut self,
        program: &dyn BoundProgram,
        context: &mut InferenceContext,
        index: usize,
    ) -> TypeId {
        if let Some(cached) = context.inferences[index].inferred_type {
            return cached;
        }
        let candidates = context.inferences[index].candidates.clone();
        let type_parameter = context.inferences[index].type_parameter;
        // Covariant inference, or the default/`unknown` fallback when no
        // inferences were made. Contravariant candidates and the co-/contra
        // preference logic are DEFERRED (no contravariant call inference yet).
        let mut inferred = if !candidates.is_empty() {
            self.get_covariant_inference(program, &candidates)
        } else {
            get_default_from_type_parameter(self, program, type_parameter)
                .unwrap_or(self.unknown_type)
        };
        // Apply the constraint: a covariantly-inferred type that violates the
        // (instantiated) constraint is replaced by the constraint type. The
        // constraint is instantiated through the inferences made so far (Go's
        // `nonFixingMapper`), so a constraint that references another type
        // parameter resolves — e.g. `K extends keyof T` becomes `keyof
        // <inferred T>` (a concrete key union), letting an inferred literal key
        // be checked against it instead of against a deferred `keyof T`.
        //
        // DEFER(phase-4-checker-C-C2): `getTypeWithThisArgument` and the
        // return-type `filteredByConstraint` speculation. blocked-by: `this`-
        // types + return-type inference speculation.
        if let Some(constraint) = get_constraint_of_type_parameter(self, program, type_parameter) {
            let mapper = self.inference_context_mapper(context);
            let instantiated_constraint = self.instantiate_type(constraint, &mapper);
            if !self.is_type_assignable_to(program, inferred, instantiated_constraint) {
                inferred = instantiated_constraint;
            }
        }
        context.inferences[index].inferred_type = Some(inferred);
        inferred
    }

    /// Builds the mapper from each type parameter of `context` to the type
    /// inferred for it so far (or the type parameter itself when not yet
    /// resolved), used to instantiate a type-parameter constraint during
    /// inference (Go's `context.nonFixingMapper`, eager reachable form).
    ///
    /// Side effects: none (reads the context's current inferences).
    // Go: internal/checker/inference.go:Checker.createInferenceContextWorker (nonFixingMapper)
    fn inference_context_mapper(&self, context: &InferenceContext) -> TypeMapper {
        let targets: Vec<TypeId> = context
            .inferences
            .iter()
            .map(|inf| inf.inferred_type.unwrap_or(inf.type_parameter))
            .collect();
        TypeMapper::Array {
            sources: context.type_parameters.clone(),
            targets,
        }
    }

    /// The covariant inference for a non-empty candidate set: the common
    /// supertype of the candidates, widened (Go's `getCovariantInference`
    /// reachable subset — without the `widenLiteralTypes` /
    /// `PriorityImpliesCombination` paths, which the reachable `: T`-return
    /// signatures never trigger).
    // Go: internal/checker/inference.go:Checker.getCovariantInference
    fn get_covariant_inference(
        &mut self,
        program: &dyn BoundProgram,
        candidates: &[TypeId],
    ) -> TypeId {
        let unwidened = self.get_common_supertype(program, candidates);
        self.get_widened_type(unwidened)
    }

    /// The common supertype of `types` (Go's `getCommonSupertype`): a union when
    /// every candidate is a literal of the same base type, otherwise the single
    /// common supertype.
    ///
    /// DEFER(phase-4-checker-C-C): the `strictNullChecks` nullable strip/restore
    /// (`filterType` + `getNullableType`); the reachable call candidates are
    /// non-nullable. blocked-by: `filterType`-over-candidates.
    // Go: internal/checker/inference.go:Checker.getCommonSupertype
    fn get_common_supertype(&mut self, program: &dyn BoundProgram, types: &[TypeId]) -> TypeId {
        if types.len() == 1 {
            return types[0];
        }
        if self.literal_types_with_same_base_type(types) {
            self.get_union_type(types)
        } else {
            self.get_single_common_supertype(program, types)
        }
    }

    /// The leftmost candidate for which no later candidate is a supertype (Go's
    /// `getSingleCommonSupertype`).
    ///
    /// Go first tries strict-subtype, then falls back to subtype; for the
    /// reachable call candidates (literals/primitives, never `any`) the strict
    /// and non-strict relations coincide, so both passes use
    /// [`is_type_subtype_of`](Checker::is_type_subtype_of).
    // Go: internal/checker/inference.go:Checker.getSingleCommonSupertype
    fn get_single_common_supertype(
        &mut self,
        program: &dyn BoundProgram,
        types: &[TypeId],
    ) -> TypeId {
        let candidate = self.find_leftmost_supertype(program, types);
        if types
            .iter()
            .all(|&t| t == candidate || self.is_type_subtype_of(program, t, candidate))
        {
            return candidate;
        }
        self.find_leftmost_supertype(program, types)
    }

    /// The leftmost type for which no later type is a (strict) supertype (Go's
    /// `findLeftmostType` with a subtype predicate): scan left-to-right,
    /// replacing the running candidate whenever it is a subtype of the next type.
    // Go: internal/checker/inference.go:findLeftmostType
    fn find_leftmost_supertype(&mut self, program: &dyn BoundProgram, types: &[TypeId]) -> TypeId {
        let mut candidate = types[0];
        for &t in &types[1..] {
            if self.is_type_subtype_of(program, candidate, t) {
                candidate = t;
            }
        }
        candidate
    }

    /// Reports whether every type in `types` is a literal type and they all
    /// share the same base type (Go's `literalTypesWithSameBaseType`). A
    /// non-literal candidate (whose base type equals itself) makes this false.
    // Go: internal/checker/inference.go:Checker.literalTypesWithSameBaseType
    fn literal_types_with_same_base_type(&self, types: &[TypeId]) -> bool {
        let mut common_base: Option<TypeId> = None;
        for &t in types {
            let base = self.get_base_type_of_literal_type(t);
            let common = *common_base.get_or_insert(base);
            if base == t || base != common {
                return false;
            }
        }
        true
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
