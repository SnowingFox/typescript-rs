use super::*;

// A `Runner` implementation exposes its enumerated test files.
// Go: internal/testrunner/runner.go:Runner
#[test]
fn runner_enumerates_its_files() {
    struct FixedRunner {
        files: Vec<String>,
    }
    impl Runner for FixedRunner {
        fn enumerate_test_files(&self) -> Vec<String> {
            self.files.clone()
        }
    }

    let runner = FixedRunner {
        files: vec!["a.ts".to_string(), "b.ts".to_string()],
    };
    assert_eq!(runner.enumerate_test_files(), vec!["a.ts", "b.ts"]);
}
