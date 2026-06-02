use super::*;

// Go: internal/ls/lsutil/userpreferences.go:QuotePreference constants
#[test]
fn quote_preference_wire_values() {
    assert_eq!(QuotePreference::Unknown.as_str(), "");
    assert_eq!(QuotePreference::Auto.as_str(), "auto");
    assert_eq!(QuotePreference::Double.as_str(), "double");
    assert_eq!(QuotePreference::Single.as_str(), "single");
}

// Go: internal/ls/lsutil/userpreferences.go:QuotePreference (zero value is Unknown)
#[test]
fn quote_preference_default_is_unknown() {
    assert_eq!(QuotePreference::default(), QuotePreference::Unknown);
}

// Go: internal/ls/lsutil/userpreferences.go:IncludeInlayParameterNameHints constants
#[test]
fn include_inlay_parameter_name_hints_wire_values() {
    assert_eq!(IncludeInlayParameterNameHints::None.as_str(), "");
    assert_eq!(IncludeInlayParameterNameHints::All.as_str(), "all");
    assert_eq!(
        IncludeInlayParameterNameHints::Literals.as_str(),
        "literals"
    );
}

// Go: internal/ls/lsutil/userpreferences.go:IncludeInlayParameterNameHints
// (the empty string / zero value is `None`).
#[test]
fn include_inlay_parameter_name_hints_default_is_none() {
    assert_eq!(
        IncludeInlayParameterNameHints::default(),
        IncludeInlayParameterNameHints::None
    );
}

// Go: internal/ls/lsutil/userpreferences.go:InlayHintsPreferences (the zero value
// has every gate off — `None` for parameter-name hints, `Unknown` (false-ish)
// for each tristate gate).
#[test]
fn inlay_hints_preferences_default_has_every_gate_off() {
    let prefs = InlayHintsPreferences::default();
    assert_eq!(
        prefs.include_inlay_parameter_name_hints,
        IncludeInlayParameterNameHints::None
    );
    assert!(!prefs
        .include_inlay_parameter_name_hints_when_argument_matches_name
        .is_true());
    assert!(!prefs.include_inlay_function_parameter_type_hints.is_true());
    assert!(!prefs.include_inlay_variable_type_hints.is_true());
    assert!(!prefs
        .include_inlay_variable_type_hints_when_type_matches_name
        .is_true());
    assert!(!prefs
        .include_inlay_property_declaration_type_hints
        .is_true());
    assert!(!prefs
        .include_inlay_function_like_return_type_hints
        .is_true());
    assert!(!prefs.include_inlay_enum_member_value_hints.is_true());
}
