use std::fmt;
use std::path::Path;
use std::sync::OnceLock;

use tsgo_json::serde::de::DeserializeOwned;
use tsgo_repo::root_path;
use tsgo_tspath::normalize_path;

/// The default ES-module loader: imports `script.mjs`'s default export, calls
/// it with the process arguments, and writes the JSON-stringified awaited
/// result to stdout.
// Go: internal/testutil/jstest/node.go:loaderScript
const LOADER_SCRIPT: &str = "import script from \"./script.mjs\";\nprocess.stdout.write(JSON.stringify(await script(...process.argv.slice(2))));";

/// An error from evaluating a Node.js script.
///
/// DIVERGENCE(port): Go returns `(T, error)` with `fmt.Errorf`-wrapped causes
/// and uses `t.Fatal` when Node.js is missing. Rust folds the missing-binary
/// case into this enum ([`JsTestError::NodeNotFound`]) so callers handle it as
/// a normal `Result`.
///
/// # Examples
/// ```
/// use tsgo_testutil_jstest::JsTestError;
/// assert_eq!(JsTestError::NodeNotFound.to_string(), "Node.js not found");
/// ```
///
/// Side effects: none (pure value type).
#[derive(Debug)]
pub enum JsTestError {
    /// No `node` executable was found on `PATH`.
    NodeNotFound,
    /// An I/O error writing the script/loader files or spawning the process.
    Io(std::io::Error),
    /// Node.js ran but exited non-zero; carries the combined stdout+stderr.
    Run(String),
    /// The JSON output could not be deserialized into the requested type.
    Unmarshal(String),
}

impl fmt::Display for JsTestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsTestError::NodeNotFound => write!(f, "Node.js not found"),
            JsTestError::Io(e) => write!(f, "{e}"),
            JsTestError::Run(output) => write!(f, "failed to run node:\n{output}"),
            JsTestError::Unmarshal(msg) => write!(f, "failed to unmarshal JSON output: {msg}"),
        }
    }
}

impl std::error::Error for JsTestError {}

/// Returns the absolute path to the `node` executable, or `None` if it is not
/// found on `PATH`. The lookup happens once and is cached.
///
/// Mirrors Go's `sync.OnceValue(exec.LookPath("node"))`.
///
/// # Examples
/// ```
/// // Returns `Some(path)` when Node.js is installed, `None` otherwise.
/// let _maybe = tsgo_testutil_jstest::node_exe();
/// ```
///
/// Side effects: reads the `PATH` environment variable and stats candidate
/// files (first call only).
// Go: internal/testutil/jstest/node.go:getNodeExeOnce
pub fn node_exe() -> Option<&'static str> {
    static EXE: OnceLock<Option<String>> = OnceLock::new();
    EXE.get_or_init(|| look_path("node")).as_deref()
}

/// Reports whether a Node.js-dependent test should be skipped because no `node`
/// executable is available.
///
/// DIVERGENCE(port): Go's `SkipIfNoNodeJS(t)` calls `t.Skip` directly. Rust has
/// no library-level skip primitive, so this returns `true` when the caller
/// should skip; `false` otherwise.
///
/// # Examples
/// ```
/// if tsgo_testutil_jstest::should_skip_no_nodejs() {
///     // caller would `return;` to skip a Node.js-dependent test
/// }
/// ```
///
/// Side effects: none beyond the cached [`node_exe`].
// Go: internal/testutil/jstest/node.go:SkipIfNoNodeJS
pub fn should_skip_no_nodejs() -> bool {
    node_exe().is_none()
}

/// Imports a Node.js script that default-exports a single (optionally async)
/// function, calls it with `args`, and deserializes the JSON-stringified
/// awaited return value into `T`.
///
/// The caller supplies the working directory `dir`; both `script.mjs` and the
/// loader are written there.
///
/// # Examples
/// ```no_run
/// use std::path::Path;
/// let n: i64 = tsgo_testutil_jstest::eval_node_script(
///     "export default async (a, b) => Number(a) + Number(b);",
///     Path::new("/tmp/somedir"),
///     &["2", "3"],
/// ).unwrap();
/// assert_eq!(n, 5);
/// ```
///
/// Side effects: writes `script.mjs` and `loader.mjs` into `dir` and spawns a
/// `node` child process.
// Go: internal/testutil/jstest/node.go:EvalNodeScript
pub fn eval_node_script<T: DeserializeOwned>(
    script: &str,
    dir: &Path,
    args: &[&str],
) -> Result<T, JsTestError> {
    eval_node_script_inner(script, LOADER_SCRIPT, dir, args)
}

/// Like [`eval_node_script`], but the script receives the TypeScript library as
/// its first argument. When `dir` is `None` a fresh temporary directory is
/// created.
///
/// # Examples
/// ```no_run
/// // Requires `node_modules/typescript/lib/typescript.js` to be present.
/// let _v: i32 = tsgo_testutil_jstest::eval_node_script_with_ts(
///     "export default async (ts) => ts.version.length;",
///     None,
///     &[],
/// ).unwrap();
/// ```
///
/// Side effects: writes `script.mjs` and `loader.mjs`, spawns a `node` child
/// process, and (when `dir` is `None`) creates a temporary directory.
// Go: internal/testutil/jstest/node.go:EvalNodeScriptWithTS
pub fn eval_node_script_with_ts<T: DeserializeOwned>(
    script: &str,
    dir: Option<&Path>,
    args: &[&str],
) -> Result<T, JsTestError> {
    let owned_tmp;
    let dir: &Path = match dir {
        Some(d) => d,
        None => {
            owned_tmp = tempfile::tempdir().map_err(JsTestError::Io)?;
            owned_tmp.path()
        }
    };
    let loader = build_ts_loader_script(&typescript_module_url());
    eval_node_script_inner(script, &loader, dir, args)
}

// Builds the TS-aware loader that imports the script plus the TypeScript module
// at `ts_src` and passes `ts` as the first argument.
// Go: internal/testutil/jstest/node.go:EvalNodeScriptWithTS (inline loader)
fn build_ts_loader_script(ts_src: &str) -> String {
    format!(
        "import script from \"./script.mjs\";\n\
         import * as ts from \"{ts_src}\";\n\
         process.stdout.write(JSON.stringify(await script(ts, ...process.argv.slice(2))));"
    )
}

// Computes the `file://` URL of the vendored TypeScript library
// (`<root>/node_modules/typescript/lib/typescript.js`).
// Go: internal/testutil/jstest/node.go:EvalNodeScriptWithTS (tsSrc computation)
fn typescript_module_url() -> String {
    let joined = Path::new(root_path())
        .join("node_modules/typescript/lib/typescript.js")
        .to_string_lossy()
        .into_owned();
    let ts_src = normalize_path(&joined);
    if ts_src.starts_with('/') {
        format!("file://{ts_src}")
    } else {
        format!("file:///{ts_src}")
    }
}

// Writes the script + loader into `dir`, runs node on the loader, and
// deserializes its stdout into `T`.
// Go: internal/testutil/jstest/node.go:evalNodeScript
fn eval_node_script_inner<T: DeserializeOwned>(
    script: &str,
    loader: &str,
    dir: &Path,
    args: &[&str],
) -> Result<T, JsTestError> {
    let exe = node_exe().ok_or(JsTestError::NodeNotFound)?;

    let script_path = dir.join("script.mjs");
    std::fs::write(&script_path, script).map_err(JsTestError::Io)?;
    let loader_path = dir.join("loader.mjs");
    std::fs::write(&loader_path, loader).map_err(JsTestError::Io)?;

    let output = std::process::Command::new(exe)
        .arg(&loader_path)
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(JsTestError::Io)?;

    // DIVERGENCE(port): Go uses `exec.CombinedOutput`, which interleaves stdout
    // and stderr on a single fd. Rust captures them separately, so we
    // concatenate (stdout then stderr); this is identical to Go for the normal
    // case where the script writes only JSON to stdout and nothing to stderr.
    let mut combined = output.stdout;
    combined.extend_from_slice(&output.stderr);

    if !output.status.success() {
        return Err(JsTestError::Run(
            String::from_utf8_lossy(&combined).into_owned(),
        ));
    }

    tsgo_json::unmarshal(&combined).map_err(|e| JsTestError::Unmarshal(e.to_string()))
}

// Resolves the path to the `node` executable, mirroring `exec.LookPath`: walk
// `PATH` and return the first directory containing an executable named `exe`.
fn look_path(exe: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(exe);
        if is_executable_file(&candidate) {
            return Some(candidate.to_string_lossy().into_owned());
        }
        // On Windows, executables carry an extension; check the common ones.
        #[cfg(windows)]
        for ext in ["exe", "cmd", "bat", "com"] {
            let with_ext = dir.join(format!("{exe}.{ext}"));
            if is_executable_file(&with_ext) {
                return Some(with_ext.to_string_lossy().into_owned());
            }
        }
    }
    None
}

// Reports whether `path` is a regular file that is executable by the current
// user (on Unix, any execute bit is set; on other platforms, existence as a
// file is sufficient).
fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
#[path = "node_test.rs"]
mod tests;
