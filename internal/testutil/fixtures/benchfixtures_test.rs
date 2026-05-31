use super::*;

// Go: internal/testutil/fixtures/benchfixtures.go:BenchFixtures
// There is no Go `_test.go`; these behavior tests assert the fixture list's
// shape against the Go literals in `benchfixtures.go`.

#[test]
fn bench_fixtures_lists_expected_names_in_order() {
    let fixtures = bench_fixtures();
    let names: Vec<&str> = fixtures.iter().map(|f| f.name()).collect();
    assert_eq!(
        names,
        vec![
            "empty.ts",
            "checker.ts",
            "dom.generated.d.ts",
            "Herebyfile.mjs",
            "jsxComplexSignatureHasApplicabilityError.tsx",
        ]
    );
}

// Go: filefixture.FromString("empty.ts", "empty.ts", "")
#[test]
fn empty_ts_is_string_backed_and_never_skipped() {
    let fixtures = bench_fixtures();
    let empty = &fixtures[0];
    assert_eq!(empty.name(), "empty.ts");
    assert_eq!(empty.path(), "empty.ts");
    assert_eq!(empty.read_file(), "");
    assert!(!empty.should_skip());
}

// Go: filefixture.FromFile(..., filepath.Join(repo.TypeScriptSubmodulePath(), ...))
#[test]
fn file_fixtures_paths_are_rooted_in_the_typescript_submodule() {
    let fixtures = bench_fixtures();
    let submodule = tsgo_repo::typescript_submodule_path();

    let checker = &fixtures[1];
    assert_eq!(
        checker.path(),
        format!("{submodule}/src/compiler/checker.ts")
    );

    let dom = &fixtures[2];
    assert_eq!(
        dom.path(),
        format!("{submodule}/src/lib/dom.generated.d.ts")
    );

    let hereby = &fixtures[3];
    assert_eq!(hereby.path(), format!("{submodule}/Herebyfile.mjs"));

    let jsx = &fixtures[4];
    assert_eq!(
        jsx.path(),
        format!("{submodule}/tests/cases/compiler/jsxComplexSignatureHasApplicabilityError.tsx")
    );
}
