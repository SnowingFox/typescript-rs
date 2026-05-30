use super::*;

// Go: internal/tsoptions/enummaps.go:GetLibFileName (no dedicated Go test;
// behavior-level per PORTING.md §8.6, expected from the LibMap literals).
#[test]
fn get_lib_file_name_maps_short_name() {
    assert_eq!(get_lib_file_name("es6").as_deref(), Some("lib.es2015.d.ts"));
    assert_eq!(get_lib_file_name("es5").as_deref(), Some("lib.es5.d.ts"));
    assert_eq!(
        get_lib_file_name("es2015.symbol.wellknown").as_deref(),
        Some("lib.es2015.symbol.wellknown.d.ts")
    );
}

#[test]
fn get_lib_file_name_passes_through_file_name() {
    // A known file name is accepted as-is.
    assert_eq!(
        get_lib_file_name("lib.es5.d.ts").as_deref(),
        Some("lib.es5.d.ts")
    );
}

#[test]
fn get_lib_file_name_lowercases_input() {
    assert_eq!(get_lib_file_name("ES6").as_deref(), Some("lib.es2015.d.ts"));
}

#[test]
fn get_lib_file_name_unknown_is_none() {
    assert_eq!(get_lib_file_name("not-a-lib"), None);
}

// Go: internal/tsoptions/enummaps.go:GetDefaultLibFileName
#[test]
fn get_default_lib_file_name_for_targets() {
    use tsgo_core::compileroptions::ScriptTarget;
    let lib_for = |target: ScriptTarget| {
        get_default_lib_file_name(&CompilerOptions {
            target,
            ..Default::default()
        })
    };
    assert_eq!(lib_for(ScriptTarget::Es2020), "lib.es2020.full.d.ts");
    assert_eq!(lib_for(ScriptTarget::EsNext), "lib.esnext.full.d.ts");
    // ES2015 uses lib.es6.d.ts (breaking-change carve-out).
    assert_eq!(lib_for(ScriptTarget::Es2015), "lib.es6.d.ts");
}

#[test]
fn get_default_lib_file_name_unset_uses_latest_standard() {
    // Unset target -> GetEmitScriptTarget == ES2025 -> lib.es2025.full.d.ts.
    let o = CompilerOptions::default();
    assert_eq!(get_default_lib_file_name(&o), "lib.es2025.full.d.ts");
}
