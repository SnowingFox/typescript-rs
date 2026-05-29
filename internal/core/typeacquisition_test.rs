//! Behavior tests for type acquisition options (Go has no `_test.go`).

use super::*;
use crate::tristate::Tristate;

// Go: internal/core/typeacquisition.go:Equals (behavior-level; no Go _test.go)
#[test]
fn equals_is_true_for_identical_values() {
    let a = TypeAcquisition {
        enable: Tristate::True,
        include: vec!["jquery".to_string()],
        ..Default::default()
    };
    let b = a.clone();
    assert!(a.equals(&b));
}

// Go: internal/core/typeacquisition.go:Equals (behavior-level; no Go _test.go)
#[test]
fn equals_is_false_when_a_field_differs() {
    let a = TypeAcquisition {
        include: vec!["jquery".to_string()],
        ..Default::default()
    };
    let b = TypeAcquisition {
        include: vec!["lodash".to_string()],
        ..Default::default()
    };
    assert!(!a.equals(&b));
}
