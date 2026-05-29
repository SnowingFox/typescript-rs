use super::*;

// Go: internal/core/languagevariant_stringer_generated.go:String
#[test]
fn languagevariant_display() {
    assert_eq!(
        LanguageVariant::Standard.to_string(),
        "LanguageVariantStandard"
    );
    assert_eq!(LanguageVariant::Jsx.to_string(), "LanguageVariantJSX");
}
