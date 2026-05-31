use std::path::Path;

use tsgo_repo::typescript_submodule_path;
use tsgo_testutil_filefixture::{from_file, from_string, Fixture};

/// Returns the standard set of benchmark fixtures.
///
/// The list mirrors Go's package-level `BenchFixtures` variable: an in-memory
/// `empty.ts` plus four real sources read from the vendored TypeScript
/// submodule (`compiler/checker.ts`, `lib/dom.generated.d.ts`,
/// `Herebyfile.mjs`, and a representative compiler test case).
///
/// DIVERGENCE(port): Go exposes a package-level slice
/// (`var BenchFixtures = []filefixture.Fixture{...}`); Rust has no equivalent
/// non-`const` global without lazy initialization, so this is a function that
/// constructs the list on each call. The file-backed fixtures read lazily, so
/// calling this does not touch the filesystem.
///
/// # Examples
/// ```
/// use tsgo_testutil_fixtures::bench_fixtures;
/// let fixtures = bench_fixtures();
/// assert_eq!(fixtures[0].name(), "empty.ts");
/// ```
///
/// Side effects: none (the returned fixtures read files lazily on demand).
// Go: internal/testutil/fixtures/benchfixtures.go:BenchFixtures
pub fn bench_fixtures() -> Vec<Box<dyn Fixture>> {
    vec![
        from_string("empty.ts", "empty.ts", ""),
        from_file("checker.ts", &submodule_join("src/compiler/checker.ts")),
        from_file(
            "dom.generated.d.ts",
            &submodule_join("src/lib/dom.generated.d.ts"),
        ),
        from_file("Herebyfile.mjs", &submodule_join("Herebyfile.mjs")),
        from_file(
            "jsxComplexSignatureHasApplicabilityError.tsx",
            &submodule_join("tests/cases/compiler/jsxComplexSignatureHasApplicabilityError.tsx"),
        ),
    ]
}

// Joins the submodule root with a relative path the way Go's `filepath.Join`
// does for these clean, forward-slash inputs.
fn submodule_join(rel: &str) -> String {
    Path::new(typescript_submodule_path())
        .join(rel)
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
#[path = "benchfixtures_test.rs"]
mod tests;
