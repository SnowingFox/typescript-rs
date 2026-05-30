//! Type relations: identity, subtype, assignability, and comparability.
//!
//! Ports the reachable core of Go's `relater.go`: `isTypeRelatedTo` +
//! `isSimpleTypeRelatedTo` (the primitive/literal/any/unknown/never rules) and a
//! structural `checkTypeRelatedTo` (union source/target rules + object property
//! comparison) with a per-relation result cache.
//!
//! 4d covers hand-buildable shapes; the full relater (variance, intersections,
//! signatures/index-signature comparison, conditional/mapped types, detailed
//! error reporting, and the `Ternary` recursion machinery) is deferred.

use rustc_hash::FxHashMap;

use super::declared_types::{get_properties_of_type, get_property_of_type, get_type_of_symbol};
use super::program::BoundProgram;
use super::types::{TypeData, TypeFlags, TypeId};
use super::Checker;

/// Which relation is being checked (Go's distinct `*Relation` singletons).
///
/// # Examples
/// ```
/// use tsgo_checker::RelationKind;
/// assert_ne!(RelationKind::Identity, RelationKind::Assignable);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/checker.go:Checker.{identity,subtype,strictSubtype,assignable,comparable}Relation
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RelationKind {
    /// Mutual structural identity.
    Identity,
    /// Subtype (used for inference / best-common-type).
    Subtype,
    /// Subtype that treats `any` more strictly.
    StrictSubtype,
    /// Assignability (the everyday `=`/argument relation).
    Assignable,
    /// Comparability (used by `===`/`switch`).
    Comparable,
}

/// A per-relation cache of comparison results, keyed by `(kind, source, target)`.
///
/// Replaces Go's per-`*Relation` `results map[CacheHashKey]RelationComparisonResult`.
///
/// # Examples
/// ```
/// use tsgo_checker::{RelationKind, TypeId};
/// // The cache is internal to the checker; this just shows the key shape.
/// let key = (RelationKind::Assignable, TypeId(1), TypeId(2));
/// assert_eq!(key.0, RelationKind::Assignable);
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/checker/relater.go:Relation
#[derive(Clone, Debug, Default)]
pub struct RelationCache {
    results: FxHashMap<(RelationKind, TypeId, TypeId), bool>,
}

impl RelationCache {
    /// Returns the cached result for a comparison, if present.
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/relater.go:Relation.get
    pub fn get(&self, kind: RelationKind, source: TypeId, target: TypeId) -> Option<bool> {
        self.results.get(&(kind, source, target)).copied()
    }

    /// Records a comparison result.
    ///
    /// Side effects: mutates the cache.
    // Go: internal/checker/relater.go:Relation.set
    pub fn set(&mut self, kind: RelationKind, source: TypeId, target: TypeId, result: bool) {
        self.results.insert((kind, source, target), result);
    }

    /// Returns the number of cached comparisons.
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/relater.go:Relation.size
    pub fn len(&self) -> usize {
        self.results.len()
    }

    /// Reports whether the cache is empty.
    ///
    /// Side effects: none (pure).
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }
}

impl Checker {
    /// Reports whether `source` is assignable to `target`.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// use tsgo_checker::{BoundProgram, RelationKind};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P) {
    /// let _ = c.is_type_assignable_to(p, c.string_type(), c.string_type());
    /// # }
    /// ```
    ///
    /// Side effects: populates the relation cache; may build property types.
    // Go: internal/checker/relater.go:Checker.isTypeAssignableTo
    pub fn is_type_assignable_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        self.is_type_related_to(program, source, target, RelationKind::Assignable)
    }

    /// Reports whether `source` and `target` are identical.
    ///
    /// Side effects: populates the relation cache.
    // Go: internal/checker/relater.go:Checker.isTypeIdenticalTo
    pub fn is_type_identical_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        self.is_type_related_to(program, source, target, RelationKind::Identity)
    }

    /// Reports whether `source` is a subtype of `target`.
    ///
    /// Side effects: populates the relation cache.
    // Go: internal/checker/relater.go:Checker.isTypeSubtypeOf
    pub fn is_type_subtype_of(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        self.is_type_related_to(program, source, target, RelationKind::Subtype)
    }

    /// Reports whether `source` is comparable to `target`.
    ///
    /// Side effects: populates the relation cache.
    // Go: internal/checker/relater.go:Checker.isTypeComparableTo
    pub fn is_type_comparable_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        self.is_type_related_to(program, source, target, RelationKind::Comparable)
    }

    /// The relation entry point: normalizes fresh literals, applies the simple
    /// (primitive/literal) rules, and falls back to structural comparison.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{Checker, BoundProgram, RelationKind};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P) {
    /// let any = c.any_type();
    /// let s = c.string_type();
    /// assert!(c.is_type_related_to(p, s, any, RelationKind::Assignable));
    /// # }
    /// ```
    ///
    /// Side effects: populates the relation cache; may build property types.
    // Go: internal/checker/relater.go:Checker.isTypeRelatedTo
    pub fn is_type_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        let source = self.regular_literal_type(source);
        let target = self.regular_literal_type(target);
        if source == target {
            return true;
        }
        if relation != RelationKind::Identity {
            if self.is_simple_type_related_to(source, target, relation) {
                return true;
            }
            if relation == RelationKind::Comparable
                && !self.get_type(target).flags().contains(TypeFlags::NEVER)
                && self.is_simple_type_related_to(target, source, relation)
            {
                return true;
            }
        } else {
            let sf = self.get_type(source).flags();
            let tf = self.get_type(target).flags();
            if !(sf | tf).intersects(TypeFlags::UNION_OR_INTERSECTION) {
                if sf != tf {
                    return false;
                }
                if sf.intersects(TypeFlags::SINGLETON) {
                    return true;
                }
            }
        }
        let sf = self.get_type(source).flags();
        let tf = self.get_type(target).flags();
        if sf.intersects(TypeFlags::STRUCTURED_OR_INSTANTIABLE)
            || tf.intersects(TypeFlags::STRUCTURED_OR_INSTANTIABLE)
        {
            return self.check_type_related_to(program, source, target, relation);
        }
        false
    }

    // Normalizes a fresh literal type to its regular counterpart.
    // Go: internal/checker/relater.go:isFreshLiteralType usage
    fn regular_literal_type(&self, t: TypeId) -> TypeId {
        if let TypeData::Literal(d) = &self.get_type(t).data {
            if d.fresh_type == Some(t) {
                return d.regular_type.unwrap_or(t);
            }
        }
        t
    }

    // The primitive/literal/any/unknown/never rules (no structural recursion).
    // Go: internal/checker/relater.go:Checker.isSimpleTypeRelatedTo (4d subset)
    fn is_simple_type_related_to(
        &self,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        let s = self.get_type(source).flags();
        let t = self.get_type(target).flags();
        if t.intersects(TypeFlags::ANY) || s.intersects(TypeFlags::NEVER) {
            return true;
        }
        if t.intersects(TypeFlags::UNKNOWN)
            && !(relation == RelationKind::StrictSubtype && s.intersects(TypeFlags::ANY))
        {
            return true;
        }
        if t.intersects(TypeFlags::NEVER) {
            return false;
        }
        if s.intersects(TypeFlags::STRING_LIKE) && t.intersects(TypeFlags::STRING) {
            return true;
        }
        if s.intersects(TypeFlags::NUMBER_LIKE) && t.intersects(TypeFlags::NUMBER) {
            return true;
        }
        if s.intersects(TypeFlags::BIG_INT_LIKE) && t.intersects(TypeFlags::BIG_INT) {
            return true;
        }
        if s.intersects(TypeFlags::BOOLEAN_LIKE) && t.intersects(TypeFlags::BOOLEAN) {
            return true;
        }
        if s.intersects(TypeFlags::ES_SYMBOL_LIKE) && t.intersects(TypeFlags::ES_SYMBOL) {
            return true;
        }
        // strictNullChecks-independent nullable rules (the non-strict "assignable
        // to anything" rule is DEFER'd until compiler options are wired).
        if s.intersects(TypeFlags::UNDEFINED) && t.intersects(TypeFlags::VOID_LIKE) {
            return true;
        }
        if s.intersects(TypeFlags::NULL) && t.intersects(TypeFlags::NULL) {
            return true;
        }
        if s.intersects(TypeFlags::OBJECT) && t.intersects(TypeFlags::NON_PRIMITIVE) {
            return true;
        }
        if (relation == RelationKind::Assignable || relation == RelationKind::Comparable)
            && s.intersects(TypeFlags::ANY)
        {
            return true;
        }
        false
    }

    // The cached structural relation check (Go's checkTypeRelatedTo core).
    //
    // DEFER(phase-4-checker-4e): the `Ternary` (Maybe) recursion machinery and
    // detailed error reporting. 4d optimistically caches `true` while recursing
    // to terminate on co-recursive structural types.
    // blocked-by: the full relater recursion model lands incrementally.
    // Go: internal/checker/relater.go:Checker.checkTypeRelatedTo
    fn check_type_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        if let Some(cached) = self.relations.get(relation, source, target) {
            return cached;
        }
        // Assume related while recursing to break cycles (e.g. `interface A { a: A }`).
        self.relations.set(relation, source, target, true);
        let result = self.structured_type_related_to(program, source, target, relation);
        self.relations.set(relation, source, target, result);
        result
    }

    // Go: internal/checker/relater.go:Checker.structuredTypeRelatedTo (4d subset)
    fn structured_type_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        let sf = self.get_type(source).flags();
        let tf = self.get_type(target).flags();
        if relation != RelationKind::Identity {
            // Target union: source must relate to some constituent.
            if tf.contains(TypeFlags::UNION) {
                let members = self.get_type(target).union_types().unwrap_or(&[]).to_vec();
                return members
                    .iter()
                    .any(|&m| self.is_type_related_to(program, source, m, relation));
            }
            // Source union: every constituent must relate to the target.
            if sf.contains(TypeFlags::UNION) {
                let members = self.get_type(source).union_types().unwrap_or(&[]).to_vec();
                return members
                    .iter()
                    .all(|&m| self.is_type_related_to(program, m, target, relation));
            }
        }
        if sf.contains(TypeFlags::OBJECT) && tf.contains(TypeFlags::OBJECT) {
            if relation == RelationKind::Identity {
                return self.properties_related_to(program, source, target, relation)
                    && self.properties_related_to(program, target, source, relation);
            }
            return self.properties_related_to(program, source, target, relation);
        }
        false
    }

    // For each property of `target`, `source` must have a property whose type is
    // related. (Optional-property handling is deferred.)
    // Go: internal/checker/relater.go:Checker.propertiesRelatedTo (4d subset)
    fn properties_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        for (name, target_prop) in get_properties_of_type(self, target) {
            let Some(source_prop) = get_property_of_type(self, source, &name) else {
                // DEFER(phase-4-checker-4e): optional properties may be missing.
                // blocked-by: optionality + `exactOptionalPropertyTypes` (4e).
                return false;
            };
            let source_type = get_type_of_symbol(self, program, source_prop, None);
            let target_type = get_type_of_symbol(self, program, target_prop, None);
            if !self.is_type_related_to(program, source_type, target_type, relation) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
#[path = "relations_test.rs"]
mod tests;
