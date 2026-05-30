use super::*;

// Go: internal/pseudochecker/checker.go:NewPseudoChecker
// The constructor stores the two flags verbatim and exposes them for the
// derivation logic (e.g. strict-null-checks-driven parameter optionality).
#[test]
fn new_stores_flags() {
    let ch = PseudoChecker::new(true, false);
    assert!(ch.strict_null_checks());
    assert!(!ch.exact_optional_property_types());

    let ch = PseudoChecker::new(false, true);
    assert!(!ch.strict_null_checks());
    assert!(ch.exact_optional_property_types());
}
