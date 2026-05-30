use super::*;

// Go: internal/modulespecifiers/types.go:ImportModuleSpecifierPreference
#[test]
fn import_module_specifier_preference_round_trip() {
    for (variant, text) in [
        (ImportModuleSpecifierPreference::None, ""),
        (ImportModuleSpecifierPreference::Shortest, "shortest"),
        (
            ImportModuleSpecifierPreference::ProjectRelative,
            "project-relative",
        ),
        (ImportModuleSpecifierPreference::Relative, "relative"),
        (ImportModuleSpecifierPreference::NonRelative, "non-relative"),
    ] {
        assert_eq!(variant.as_str(), text);
        assert_eq!(ImportModuleSpecifierPreference::from_str(text), variant);
    }
    // Unknown strings fall back to `None`.
    assert_eq!(
        ImportModuleSpecifierPreference::from_str("bogus"),
        ImportModuleSpecifierPreference::None
    );
}

// Go: internal/modulespecifiers/types.go:ImportModuleSpecifierEndingPreference
#[test]
fn import_module_specifier_ending_preference_round_trip() {
    for (variant, text) in [
        (ImportModuleSpecifierEndingPreference::None, ""),
        (ImportModuleSpecifierEndingPreference::Auto, "auto"),
        (ImportModuleSpecifierEndingPreference::Minimal, "minimal"),
        (ImportModuleSpecifierEndingPreference::Index, "index"),
        (ImportModuleSpecifierEndingPreference::Js, "js"),
    ] {
        assert_eq!(variant.as_str(), text);
        assert_eq!(
            ImportModuleSpecifierEndingPreference::from_str(text),
            variant
        );
    }
    assert_eq!(
        ImportModuleSpecifierEndingPreference::from_str("bogus"),
        ImportModuleSpecifierEndingPreference::None
    );
}

// Go: internal/modulespecifiers/types.go:ResultKind / ModuleSpecifierEnding (iota order)
#[test]
fn enum_discriminants_match_go_iota() {
    assert_eq!(ResultKind::None as u8, 0);
    assert_eq!(ResultKind::NodeModules as u8, 1);
    assert_eq!(ResultKind::Paths as u8, 2);
    assert_eq!(ResultKind::Redirect as u8, 3);
    assert_eq!(ResultKind::Relative as u8, 4);
    assert_eq!(ResultKind::Ambient as u8, 5);

    assert_eq!(ModuleSpecifierEnding::Minimal as u8, 0);
    assert_eq!(ModuleSpecifierEnding::Index as u8, 1);
    assert_eq!(ModuleSpecifierEnding::JsExtension as u8, 2);
    assert_eq!(ModuleSpecifierEnding::TsExtension as u8, 3);

    assert_eq!(RelativePreferenceKind::Relative as u8, 0);
    assert_eq!(RelativePreferenceKind::ExternalNonRelative as u8, 3);

    assert_eq!(MatchingMode::Exact as u8, 0);
    assert_eq!(MatchingMode::Pattern as u8, 2);
}
