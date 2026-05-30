use super::*;

// Go: internal/tsoptions/diagnostics.go:CompilerOptionsDidYouMeanDiagnostics
#[test]
fn compiler_diagnostics_shape() {
    let d = &*COMPILER_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS;
    assert_eq!(d.did_you_mean.unknown_option_diagnostic.code(), 5023);
    assert!(d.did_you_mean.alternate_mode.is_some());
    assert!(!d.did_you_mean.option_declarations.is_empty());
}

// Go: internal/tsoptions/diagnostics.go:buildOptionsDidYouMeanDiagnostics
#[test]
fn build_diagnostics_shape() {
    let d = &*BUILD_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS;
    let alt = d.did_you_mean.alternate_mode.as_ref().unwrap();
    // Alternate mode points at the compiler name map (the "other" mode).
    assert_eq!(
        alt.options_name_map.get("strict").map(|o| o.name),
        Some("strict")
    );
}

// Go: internal/tsoptions/diagnostics.go:watchOptionsDidYouMeanDiagnostics
#[test]
fn watch_diagnostics_have_no_alternate_mode() {
    let d = &*WATCH_OPTIONS_DID_YOU_MEAN_DIAGNOSTICS;
    assert!(d.did_you_mean.alternate_mode.is_none());
}
