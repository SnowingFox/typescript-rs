use super::*;

// There is no Go `_test.go` for this package; these behavior tests assert the
// helpers against the Go source in `node.go`. Tests that actually execute
// Node.js skip themselves when `node` is not on `PATH`, mirroring Go's
// `SkipIfNoNodeJS`.

// Go: internal/testutil/jstest/node.go:getNodeExeOnce / look_path
#[test]
fn node_exe_resolves_an_existing_executable_or_none() {
    match node_exe() {
        Some(path) => {
            assert!(
                Path::new(path).is_file(),
                "resolved node path should exist: {path}"
            );
            let base = Path::new(path).file_name().unwrap().to_string_lossy();
            assert!(
                base == "node" || base == "node.exe",
                "unexpected base {base}"
            );
        }
        None => {
            // No node on PATH: callers must skip node-dependent work.
            assert!(should_skip_no_nodejs());
        }
    }
}

// Go: internal/testutil/jstest/node.go:loaderScript
#[test]
fn loader_script_matches_go_literal() {
    assert_eq!(
        LOADER_SCRIPT,
        "import script from \"./script.mjs\";\n\
         process.stdout.write(JSON.stringify(await script(...process.argv.slice(2))));"
    );
}

// Go: internal/testutil/jstest/node.go:EvalNodeScriptWithTS (inline loader)
#[test]
fn build_ts_loader_script_embeds_the_ts_module_url() {
    let got = build_ts_loader_script("file:///abs/typescript.js");
    assert_eq!(
        got,
        "import script from \"./script.mjs\";\n\
         import * as ts from \"file:///abs/typescript.js\";\n\
         process.stdout.write(JSON.stringify(await script(ts, ...process.argv.slice(2))));"
    );
}

// Go: internal/testutil/jstest/node.go:EvalNodeScriptWithTS (tsSrc computation)
#[test]
fn typescript_module_url_is_a_file_url_to_the_vendored_lib() {
    let url = typescript_module_url();
    assert!(url.starts_with("file://"), "expected file URL, got {url}");
    assert!(
        url.ends_with("node_modules/typescript/lib/typescript.js"),
        "unexpected url {url}"
    );
}

// Go: internal/testutil/jstest/node.go:EvalNodeScript
#[test]
fn eval_node_script_sums_numeric_arguments() {
    if should_skip_no_nodejs() {
        return; // mirrors SkipIfNoNodeJS
    }
    let dir = tempfile::tempdir().unwrap();
    let result: i64 = eval_node_script(
        "export default async (a, b) => Number(a) + Number(b);",
        dir.path(),
        &["2", "3"],
    )
    .unwrap();
    assert_eq!(result, 5);
}

// Go: internal/testutil/jstest/node.go:EvalNodeScript (object result)
#[test]
fn eval_node_script_deserializes_object_result() {
    if should_skip_no_nodejs() {
        return;
    }
    #[derive(tsgo_json::serde::Deserialize, PartialEq, Debug)]
    #[serde(crate = "tsgo_json::serde")]
    struct Point {
        x: i32,
        y: i32,
    }
    let dir = tempfile::tempdir().unwrap();
    let result: Point = eval_node_script(
        "export default async () => ({ x: 1, y: 2 });",
        dir.path(),
        &[],
    )
    .unwrap();
    assert_eq!(result, Point { x: 1, y: 2 });
}

// Go: internal/testutil/jstest/node.go:evalNodeScript (run error path)
#[test]
fn eval_node_script_reports_run_failure() {
    if should_skip_no_nodejs() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let result: Result<i32, JsTestError> = eval_node_script(
        "export default async () => { throw new Error('boom'); };",
        dir.path(),
        &[],
    );
    match result {
        Err(JsTestError::Run(output)) => assert!(output.contains("boom"), "output: {output}"),
        other => panic!("expected Run error, got {other:?}"),
    }
}
