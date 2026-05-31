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
use tsgo_ast::{SymbolFlags, SymbolId};

use super::declared_types::{
    get_applicable_index_info_for_name, get_properties_of_type, get_property_of_type,
    get_type_of_symbol,
};
use super::program::BoundProgram;
use super::types::{ObjectFlags, TypeData, TypeFlags, TypeId};
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
        // Go interns literal types by value (`getStringLiteralType` /
        // `getNumberLiteralType`), so two occurrences of `"a"` are the same
        // `*Type` and identity already relates them. As of 4bc the port interns
        // string/number literals the same way (booleans were already singletons),
        // so equal-valued literals share one id and this identity check covers
        // `"a" === "a"` — the 4bb `literals_equal_by_value` value shim is retired.
        // Go: internal/checker/relater.go:Checker.isTypeRelatedTo (interned literal identity)
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
        // In non-strictNullChecks mode, `undefined` and `null` are assignable to
        // anything except `never`. Since unions and intersections may reduce to
        // `never`, they are excluded here. Under strictNullChecks, `undefined`
        // is only assignable to itself / `void`, and `null` only to itself
        // (`any`/`unknown` are handled by the top-type rules above).
        if s.intersects(TypeFlags::UNDEFINED)
            && ((!self.strict_null_checks() && !t.intersects(TypeFlags::UNION_OR_INTERSECTION))
                || t.intersects(TypeFlags::VOID_LIKE))
        {
            return true;
        }
        if s.intersects(TypeFlags::NULL)
            && ((!self.strict_null_checks() && !t.intersects(TypeFlags::UNION_OR_INTERSECTION))
                || t.intersects(TypeFlags::NULL))
        {
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
            // Target intersection: source must relate to EACH constituent
            // (Go's `typeRelatedToEachType`). Deconstructed after unions, which
            // are always at the top of a normalized type.
            if tf.contains(TypeFlags::INTERSECTION) {
                let members = self
                    .get_type(target)
                    .intersection_types()
                    .unwrap_or(&[])
                    .to_vec();
                return members
                    .iter()
                    .all(|&m| self.is_type_related_to(program, source, m, relation));
            }
            // Source intersection: first try whether SOME constituent relates
            // to the target on its own (Go's `someTypeRelatedToType` for
            // `IntersectionStateSource`). When none does, Go falls back to
            // checking the full intersection viewed as an object — its
            // synthesized properties — against the target. We reach that
            // fallback when the target is an object type (e.g. `A & B` ↔ `AB`).
            if sf.contains(TypeFlags::INTERSECTION) {
                let members = self
                    .get_type(source)
                    .intersection_types()
                    .unwrap_or(&[])
                    .to_vec();
                if members
                    .iter()
                    .any(|&m| self.is_type_related_to(program, m, target, relation))
                {
                    return true;
                }
                if tf.contains(TypeFlags::OBJECT) {
                    return self.properties_related_to(program, source, target, relation);
                }
                return false;
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
    // related. A missing source property is tolerated only when the target
    // property is optional and the relation does not require optional properties
    // (Go's `getUnmatchedProperty` with `requireOptionalProperties`).
    // Go: internal/checker/relater.go:Checker.propertiesRelatedTo (4d subset)
    fn properties_related_to(
        &mut self,
        program: &dyn BoundProgram,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> bool {
        // Go: subtype/strictSubtype relations still require optional members to
        // be matched; assignability/comparability/identity do not.
        let require_optional_properties =
            relation == RelationKind::Subtype || relation == RelationKind::StrictSubtype;
        for (name, target_prop) in get_properties_of_type(self, target) {
            let Some(source_prop) = get_property_of_type(self, source, &name) else {
                if !require_optional_properties && self.symbol_is_optional(program, target_prop) {
                    continue;
                }
                return false;
            };
            let source_type = get_type_of_symbol(self, program, source_prop, None);
            let target_type = get_type_of_symbol(self, program, target_prop, None);
            if !self.is_type_related_to(program, source_type, target_type, relation) {
                return false;
            }
            // An optional source property cannot satisfy a required target
            // (class) member: the source value might lack the property entirely.
            // Comparability passes Go's `skipOptional`, tolerating the mismatch.
            if relation != RelationKind::Comparable
                && self.symbol_is_optional(program, source_prop)
                && self.symbol_is_class_member(program, target_prop)
                && !self.symbol_is_optional(program, target_prop)
            {
                return false;
            }
        }
        true
    }

    // Reports whether a property symbol was declared optional (`a?: T`).
    // Routes synthesized union/intersection property symbols to the checker's
    // transient arena instead of the program (which would panic on a tagged id).
    // Go: internal/checker/relater.go usage of `ast.SymbolFlagsOptional`
    fn symbol_is_optional(&self, program: &dyn BoundProgram, symbol: SymbolId) -> bool {
        self.resolved_symbol_flags(program, symbol)
            .contains(SymbolFlags::OPTIONAL)
    }

    // Reports whether a symbol is a class member (method/accessor/property).
    // Go: internal/ast/symbolflags.go:SymbolFlagsClassMember
    fn symbol_is_class_member(&self, program: &dyn BoundProgram, symbol: SymbolId) -> bool {
        const CLASS_MEMBER: SymbolFlags = SymbolFlags::METHOD
            .union(SymbolFlags::GET_ACCESSOR)
            .union(SymbolFlags::SET_ACCESSOR)
            .union(SymbolFlags::PROPERTY);
        self.resolved_symbol_flags(program, symbol)
            .intersects(CLASS_MEMBER)
    }

    // Reports whether `t` is an object-literal type (Go's `isObjectLiteralType`):
    // an object type carrying the `ObjectLiteral` flag. This (plus the
    // `FreshLiteral` flag) gates excess-property checking.
    // Go: internal/checker/utilities.go:isObjectLiteralType(801)
    pub(crate) fn is_object_literal_type(&self, t: TypeId) -> bool {
        self.get_type(t)
            .object_flags()
            .contains(ObjectFlags::OBJECT_LITERAL)
    }

    // Reports whether `target` is a valid target for excess-property checking
    // (Go's `isExcessPropertyCheckTarget`): a non-pattern object type, the
    // non-primitive `object` type, or a union (some constituent is a target) /
    // intersection (every constituent is a target).
    //
    // DEFER(phase-4-checker-4bg+): the substitution-type arm
    // (`isExcessPropertyCheckTarget(baseType)`). blocked-by: substitution types.
    // Go: internal/checker/relater.go:isExcessPropertyCheckTarget(746)
    pub(crate) fn is_excess_property_check_target(&self, target: TypeId) -> bool {
        let flags = self.get_type(target).flags();
        if flags.intersects(TypeFlags::OBJECT)
            && !self
                .get_type(target)
                .object_flags()
                .contains(ObjectFlags::OBJECT_LITERAL_PATTERN_WITH_COMPUTED_PROPERTIES)
        {
            return true;
        }
        if flags.intersects(TypeFlags::NON_PRIMITIVE) {
            return true;
        }
        if flags.intersects(TypeFlags::UNION) {
            let members = self.get_type(target).union_types().unwrap_or(&[]).to_vec();
            return members
                .iter()
                .any(|&m| self.is_excess_property_check_target(m));
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            let members = self
                .get_type(target)
                .intersection_types()
                .unwrap_or(&[])
                .to_vec();
            return members
                .iter()
                .all(|&m| self.is_excess_property_check_target(m));
        }
        false
    }

    // Reports whether `name` is a known property of `target` (Go's
    // `isKnownProperty`): an object type knows a name when it has that property;
    // a union/intersection target knows it when some constituent does.
    //
    // DEFER(phase-4-checker-4bg+): the late-bound-name string-index exception,
    // the substitution-type arm, and the JSX hyphenated-name allowance.
    // blocked-by: late binding, substitution types, and JSX attribute typing.
    // Go: internal/checker/relater.go:Checker.isKnownProperty(716)
    pub(crate) fn is_known_property(
        &mut self,
        program: &dyn BoundProgram,
        target: TypeId,
        name: &str,
    ) -> bool {
        let flags = self.get_type(target).flags();
        if flags.intersects(TypeFlags::OBJECT)
            && (get_property_of_type(self, target, name).is_some()
                || get_applicable_index_info_for_name(self, program, target, name).is_some())
        {
            return true;
        }
        if flags.intersects(TypeFlags::UNION_OR_INTERSECTION)
            && self.is_excess_property_check_target(target)
        {
            let members = match self.get_type(target).data {
                TypeData::Union(ref u) => u.types.clone(),
                TypeData::Intersection(ref i) => i.types.clone(),
                _ => Vec::new(),
            };
            for m in members {
                if self.is_known_property(program, m, name) {
                    return true;
                }
            }
        }
        false
    }

    // Reports whether `t` is an empty object type (Go's `isEmptyObjectType`):
    // an object type with no properties, no call/construct signatures, and no
    // index signatures, or the non-primitive `object` type. Such a target
    // accepts any property, so excess-property checking is suppressed against it.
    //
    // DEFER(phase-4-checker-4bg+): the generic-mapped-type exclusion and the
    // union (some) / intersection (every) arms. blocked-by: mapped types and the
    // union/intersection excess-check reduction.
    // Go: internal/checker/checker.go:Checker.isEmptyObjectType(26326) / isEmptyResolvedType(26322)
    pub(crate) fn is_empty_object_type(&mut self, t: TypeId) -> bool {
        let flags = self.get_type(t).flags();
        if flags.intersects(TypeFlags::NON_PRIMITIVE) {
            return true;
        }
        if flags.intersects(TypeFlags::OBJECT) {
            if !get_properties_of_type(self, t).is_empty() {
                return false;
            }
            if let Some(obj) = self.get_type(t).as_object() {
                if !obj.call_signatures.is_empty() || !obj.construct_signatures.is_empty() {
                    return false;
                }
            }
            return super::declared_types::get_index_infos_of_type(self, t).is_empty();
        }
        false
    }
}

#[cfg(test)]
#[path = "relations_test.rs"]
mod tests;
