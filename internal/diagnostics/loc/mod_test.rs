use super::*;

// Go: internal/diagnostics/loc_generated.go:loadLocaleData
#[test]
fn load_locale_data_decompresses_known_key() {
    let de = de_de();
    assert_eq!(
        de.get("_0_expected_1005").map(String::as_str),
        Some("\"{0}\" wurde erwartet.")
    );
}

// Go: internal/diagnostics/loc_generated.go:matcher
#[test]
fn matcher_table_aligns_with_loaders() {
    assert_eq!(MATCHER_TAGS.len(), LOCALE_FUNCS.len());
    assert_eq!(MATCHER_TAGS[0], "en");
    assert!(LOCALE_FUNCS[0].is_none());
    assert!(LOCALE_FUNCS[1..].iter().all(Option::is_some));
}
