use super::*;

// Go: internal/core/tristate.go:IsTrue/IsTrueOrUnknown/IsFalse
#[test]
fn tristate_predicates() {
    assert!(Tristate::True.is_true());
    assert!(!Tristate::False.is_true());
    assert!(Tristate::True.is_true_or_unknown());
    assert!(Tristate::Unknown.is_true_or_unknown());
    assert!(!Tristate::False.is_true_or_unknown());
    assert!(Tristate::False.is_false());
    assert!(!Tristate::True.is_false());
    assert!(Tristate::False.is_false_or_unknown());
    assert!(Tristate::Unknown.is_false_or_unknown());
    assert!(!Tristate::True.is_false_or_unknown());
    assert!(Tristate::Unknown.is_unknown());
    assert!(!Tristate::True.is_unknown());
}

// Go: internal/core/tristate.go:DefaultIfUnknown
#[test]
fn tristate_default_if_unknown() {
    assert_eq!(
        Tristate::Unknown.default_if_unknown(Tristate::True),
        Tristate::True
    );
    assert_eq!(
        Tristate::False.default_if_unknown(Tristate::True),
        Tristate::False
    );
    assert_eq!(
        Tristate::True.default_if_unknown(Tristate::False),
        Tristate::True
    );
}

// Go: internal/core/tristate.go:BoolToTristate
#[test]
fn bool_to_tristate_maps() {
    assert_eq!(bool_to_tristate(true), Tristate::True);
    assert_eq!(bool_to_tristate(false), Tristate::False);
}

// Go: internal/core/tristate.go:MarshalJSON/UnmarshalJSON
#[test]
fn tristate_json() {
    assert_eq!(serde_json::to_string(&Tristate::True).unwrap(), "true");
    assert_eq!(serde_json::to_string(&Tristate::False).unwrap(), "false");
    assert_eq!(serde_json::to_string(&Tristate::Unknown).unwrap(), "null");

    assert_eq!(
        serde_json::from_str::<Tristate>("true").unwrap(),
        Tristate::True
    );
    assert_eq!(
        serde_json::from_str::<Tristate>("false").unwrap(),
        Tristate::False
    );
    assert_eq!(
        serde_json::from_str::<Tristate>("null").unwrap(),
        Tristate::Unknown
    );
    // Any other JSON value maps to Unknown (mirrors Go's default branch).
    assert_eq!(
        serde_json::from_str::<Tristate>("\"x\"").unwrap(),
        Tristate::Unknown
    );
}

// Go: internal/core/tristate_stringer_generated.go:String
#[test]
fn tristate_display() {
    assert_eq!(Tristate::Unknown.to_string(), "TSUnknown");
    assert_eq!(Tristate::False.to_string(), "TSFalse");
    assert_eq!(Tristate::True.to_string(), "TSTrue");
}
