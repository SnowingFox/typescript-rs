//! Substitution types and the `NoInfer<T>` utility type.
//!
//! Ports the reachable subset of Go's `getNoInferType` / `isNoInferType` /
//! `getSubstitutionType` machinery used by inference blocking and assignability.

use super::conditional_types::is_pattern_literal_type;
use super::types::{TypeFlags, TypeId};
use super::Checker;

/// Reports whether `t` is a `NoInfer<...>` substitution type.
///
/// Side effects: none (pure).
// Go: internal/checker/checker.go:Checker.isNoInferType
pub(crate) fn is_no_infer_type(checker: &Checker, t: TypeId) -> bool {
    checker
        .get_type(t)
        .flags()
        .contains(TypeFlags::SUBSTITUTION)
        && checker
            .get_type(
                checker
                    .get_type(t)
                    .as_substitution()
                    .expect("substitution")
                    .constraint,
            )
            .flags()
            .contains(TypeFlags::UNKNOWN)
}

/// Returns `NoInfer<t>` when `t` should block inference, otherwise `t`.
///
/// Side effects: may allocate substitution types.
// Go: internal/checker/checker.go:Checker.getNoInferType
pub(crate) fn get_no_infer_type(checker: &mut Checker, t: TypeId) -> TypeId {
    if is_no_infer_target_type(checker, t) {
        // Go's `getNoInferType` calls `getOrCreateSubstitutionType` directly
        // (not `getSubstitutionType`), so the `unknown` constraint is kept.
        checker.get_or_create_substitution_type_unchecked(t, checker.unknown_type())
    } else {
        t
    }
}

/// The type used when relating `source` to a substitution target.
///
/// For `NoInfer<T>` this is `T`; otherwise it is `constraint & base`.
///
/// Side effects: may allocate intersection types.
// Go: internal/checker/checker.go:Checker.getSubstitutionIntersection
pub(crate) fn get_substitution_intersection(checker: &mut Checker, t: TypeId) -> TypeId {
    if is_no_infer_type(checker, t) {
        return checker
            .get_type(t)
            .as_substitution()
            .expect("substitution")
            .base_type;
    }
    let sub = checker.get_type(t).as_substitution().expect("substitution");
    checker.get_intersection_type(&[sub.constraint, sub.base_type])
}

/// Unwraps indexed-access/substitution shells to the underlying type variable.
///
/// Side effects: none (pure).
// Go: internal/checker/checker.go:Checker.getActualTypeVariable
pub(crate) fn get_actual_type_variable(checker: &Checker, t: TypeId) -> TypeId {
    if is_no_infer_type(checker, t) {
        return checker
            .get_type(t)
            .as_substitution()
            .expect("substitution")
            .base_type;
    }
    if checker.get_type(t).flags().contains(TypeFlags::SUBSTITUTION) {
        return checker
            .get_type(t)
            .as_substitution()
            .expect("substitution")
            .base_type;
    }
    t
}

fn is_no_infer_target_type(checker: &Checker, t: TypeId) -> bool {
    let flags = checker.get_type(t).flags();
    if flags.intersects(TypeFlags::UNION | TypeFlags::INTERSECTION) {
        let members = if flags.contains(TypeFlags::UNION) {
            checker.get_type(t).union_types().unwrap_or(&[]).to_vec()
        } else {
            checker
                .get_type(t)
                .intersection_types()
                .unwrap_or(&[])
                .to_vec()
        };
        return members
            .iter()
            .any(|&m| is_no_infer_target_type(checker, m));
    }
    if flags.contains(TypeFlags::SUBSTITUTION) {
        if is_no_infer_type(checker, t) {
            return false;
        }
        return is_no_infer_target_type(
            checker,
            checker
                .get_type(t)
                .as_substitution()
                .expect("substitution")
                .base_type,
        );
    }
    if flags.contains(TypeFlags::OBJECT) {
        return !is_empty_anonymous_object_type(checker, t);
    }
    flags.intersects(TypeFlags::INSTANTIABLE & !TypeFlags::SUBSTITUTION)
        && !is_pattern_literal_type(checker, t)
}

fn is_empty_anonymous_object_type(checker: &Checker, t: TypeId) -> bool {
    let flags = checker.get_type(t).flags();
    if !flags.contains(TypeFlags::OBJECT) {
        return false;
    }
    let Some(obj) = checker.get_type(t).as_object() else {
        return false;
    };
    if obj.target.is_some() {
        return false;
    }
    obj.call_signatures.is_empty()
        && obj.construct_signatures.is_empty()
        && obj.properties.is_empty()
        && obj.index_infos.is_empty()
}
