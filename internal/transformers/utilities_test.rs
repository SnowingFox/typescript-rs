use super::*;
use tsgo_ast::Kind;
use tsgo_printer::{EmitContext, EmitFlags};

// Go: internal/transformers/utilities.go:IsGeneratedIdentifier
// A factory-produced name is generated; a plain identifier is not.
#[test]
fn generated_identifier_detects_auto_names() {
    let mut ec = EmitContext::new();
    let temp = ec.factory().new_temp_variable();
    assert!(is_generated_identifier(&ec, temp));
    let plain = ec.arena_mut().new_identifier("x");
    assert!(!is_generated_identifier(&ec, plain));
}

// Go: internal/transformers/utilities.go:IsHelperName/IsLocalName/IsExportName
// Each predicate is true only for its own emit flag.
#[test]
fn emit_flag_name_predicates() {
    let mut ec = EmitContext::new();
    let helper = ec.arena_mut().new_identifier("__assign");
    ec.set_emit_flags(helper, EmitFlags::HELPER_NAME);
    let local = ec.arena_mut().new_identifier("C");
    ec.set_emit_flags(local, EmitFlags::LOCAL_NAME);
    let export = ec.arena_mut().new_identifier("C");
    ec.set_emit_flags(export, EmitFlags::EXPORT_NAME);
    let plain = ec.arena_mut().new_identifier("d");

    assert!(is_helper_name(&ec, helper));
    assert!(!is_helper_name(&ec, local));
    assert!(!is_helper_name(&ec, plain));

    assert!(is_local_name(&ec, local));
    assert!(!is_local_name(&ec, helper));
    assert!(!is_local_name(&ec, export));

    assert!(is_export_name(&ec, export));
    assert!(!is_export_name(&ec, local));
    assert!(!is_export_name(&ec, plain));
}

// Go: internal/transformers/utilities.go:GetNonAssignmentOperatorForCompoundAssignment
// Each compound-assignment token maps to its non-assignment operator; a
// non-compound token is returned unchanged.
#[test]
fn non_assignment_operator_maps_compound_tokens() {
    let cases = [
        (Kind::PlusEqualsToken, Kind::PlusToken),
        (Kind::MinusEqualsToken, Kind::MinusToken),
        (Kind::AsteriskEqualsToken, Kind::AsteriskToken),
        (
            Kind::AsteriskAsteriskEqualsToken,
            Kind::AsteriskAsteriskToken,
        ),
        (Kind::SlashEqualsToken, Kind::SlashToken),
        (Kind::PercentEqualsToken, Kind::PercentToken),
        (
            Kind::LessThanLessThanEqualsToken,
            Kind::LessThanLessThanToken,
        ),
        (
            Kind::GreaterThanGreaterThanEqualsToken,
            Kind::GreaterThanGreaterThanToken,
        ),
        (
            Kind::GreaterThanGreaterThanGreaterThanEqualsToken,
            Kind::GreaterThanGreaterThanGreaterThanToken,
        ),
        (Kind::AmpersandEqualsToken, Kind::AmpersandToken),
        (Kind::BarEqualsToken, Kind::BarToken),
        (Kind::CaretEqualsToken, Kind::CaretToken),
        (Kind::BarBarEqualsToken, Kind::BarBarToken),
        (
            Kind::AmpersandAmpersandEqualsToken,
            Kind::AmpersandAmpersandToken,
        ),
        (
            Kind::QuestionQuestionEqualsToken,
            Kind::QuestionQuestionToken,
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(
            get_non_assignment_operator_for_compound_assignment(input),
            expected,
            "mapping for {input:?}"
        );
    }
    // Non-compound tokens pass through unchanged.
    assert_eq!(
        get_non_assignment_operator_for_compound_assignment(Kind::EqualsToken),
        Kind::EqualsToken
    );
    assert_eq!(
        get_non_assignment_operator_for_compound_assignment(Kind::PlusToken),
        Kind::PlusToken
    );
}
