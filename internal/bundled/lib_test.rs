use super::*;

// Go: internal/bundled/bundled_test.go:TestTestingLibPath
#[test]
fn test_testing_lib_path() {
    let p = testing_lib_path();

    assert!(
        std::fs::metadata(&p).is_ok(),
        "testing lib dir should exist: {p}"
    );

    let lib_dts = std::path::Path::new(&p).join("lib.d.ts");
    assert!(
        std::fs::metadata(&lib_dts).is_ok(),
        "lib.d.ts should exist under {p}"
    );
}
