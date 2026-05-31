//! `tsgo_testutil_baseline` — the baseline diff / accept-baseline framework.
//!
//! 1:1 port of Go `internal/testutil/baseline` (`baseline.go` + `testmain.go`).
//! This is the byte-for-byte comparison engine that writes test output to
//! `testdata/baselines/local/...` and compares it against the committed
//! `testdata/baselines/reference/...`, reporting a failure when they diverge so
//! `hereby baseline-accept` can refresh the reference. It is the foundation the
//! rest of the P10 test facility (testrunner / fourslash / conformance) builds
//! on.
//!
//! DIVERGENCE(port): Go threads `*testing.T` through every entry point to
//! accumulate failures, skip, and run sub-tests. Rust has no equivalent
//! library-level testing handle, so failures are accumulated into a lightweight
//! [`Harness`] passed by `&mut` reference. The baseline file names / directory
//! layout are preserved 1:1; only the `*testing.T` call sites become `&mut
//! Harness` methods.

use std::path::Path;

mod testmain;
use testmain::record_baseline;
pub use testmain::track;

/// Sentinel returned by baseline producers when there is nothing to baseline.
///
/// Mirrors Go `baseline.NoContent`. Passing an empty string to a baseline
/// writer is a programming error (it panics); callers must return this constant
/// instead when no baseline output is required.
// Go: internal/testutil/baseline/baseline.go:NoContent
pub const NO_CONTENT: &str = "<no content>";

/// A closure that normalizes baseline text before diffing it against the
/// submodule baseline (mirrors Go's `func(string) string` fixup).
pub type DiffFixup = Box<dyn Fn(&str) -> String>;

/// Options controlling how a baseline is compared and, for submodule baselines,
/// how the diff against the upstream TypeScript baseline is categorized.
///
/// Mirrors Go `baseline.Options`. The `diff_fixup_*` closures normalize the old
/// (submodule) / new (corsa) text before diffing — e.g. to erase version
/// numbers that would otherwise churn the committed `.diff`.
// Go: internal/testutil/baseline/baseline.go:Options
#[derive(Default)]
pub struct Options {
    /// Subdirectory (under the baseline root) the baseline lives in.
    pub subfolder: String,
    /// Whether this baseline is also diffed against the TypeScript submodule.
    pub is_submodule: bool,
    /// Force-classify the submodule diff as accepted.
    pub is_submodule_accepted: bool,
    /// Force-classify the submodule diff as triaged.
    pub is_submodule_triaged: bool,
    /// Normalizes the old (submodule) text before diffing.
    pub diff_fixup_old: Option<DiffFixup>,
    /// Normalizes the new (corsa) text before diffing.
    pub diff_fixup_new: Option<DiffFixup>,
    /// Skip the submodule diff entirely (only write the local baseline).
    pub skip_diff_with_old: bool,
}

/// Accumulates baseline comparison failures in place of Go's `*testing.T`.
///
/// Each `t.Error`/`t.Errorf`/`t.Fatalf` call site in the Go source becomes a
/// pushed failure message here. After running comparisons, a test driver
/// inspects [`Harness::failures`] to decide whether the test passed.
///
/// # Examples
/// ```
/// use tsgo_testutil_baseline::Harness;
/// let mut h = Harness::new();
/// assert!(h.failures().is_empty());
/// ```
#[derive(Debug, Default)]
pub struct Harness {
    failures: Vec<String>,
}

impl Harness {
    /// Creates an empty harness.
    ///
    /// Side effects: none (pure).
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the accumulated failure messages, in the order they occurred.
    ///
    /// Side effects: none (pure).
    pub fn failures(&self) -> &[String] {
        &self.failures
    }

    /// Reports whether any failure has been recorded.
    ///
    /// Side effects: none (pure).
    pub fn has_failures(&self) -> bool {
        !self.failures.is_empty()
    }

    /// Records a failure message (replaces Go `t.Error`/`t.Errorf`).
    ///
    /// Side effects: mutates `self`.
    pub(crate) fn error(&mut self, msg: impl Into<String>) {
        self.failures.push(msg.into());
    }
}

/// Produces a unified diff between `expected` (old) and `actual` (new) with
/// three lines of context and the given `--- old` / `+++ new` headers.
///
/// # Examples
/// ```
/// use tsgo_testutil_baseline::diff_text;
/// let d = diff_text("old.x", "new.x", "a\nb\nc\n", "a\nB\nc\n");
/// assert!(d.contains("--- old.x"));
/// assert!(d.contains("-b"));
/// assert!(d.contains("+B"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/testutil/baseline/baseline.go:DiffText
pub fn diff_text(old_name: &str, new_name: &str, expected: &str, actual: &str) -> String {
    // DIVERGENCE(port): Go uses `patience.Diff(SplitLines(expected),
    // SplitLines(actual))` then `patience.UnifiedDiffTextWithOptions` with 3
    // lines of pre/post context. We use the `similar` crate's line diff with the
    // same context radius and `--- old` / `+++ new` headers. Byte-for-byte
    // parity with `patience` for the committed submodule `.diff` baselines is a
    // P10 conformance concern (verified end-to-end), not asserted here.
    similar::TextDiff::from_lines(expected, actual)
        .unified_diff()
        .context_radius(3)
        .header(old_name, new_name)
        .to_string()
}

/// Computes the submodule diff text between corsa output (`actual`) and the
/// TypeScript submodule baseline (`expected`), after applying optional fixup
/// closures, then strips line numbers from the unified-diff hunk headers.
///
/// Returns [`NO_CONTENT`] when the (fixed-up) inputs are equal or the diff has
/// no hunks. Otherwise each `@@ -a,m +b,n @@` header is rewritten to
/// `@@= skipped -da, +db lines =@@`, where `da`/`db` are the start-line deltas
/// from the previous hunk. This keeps inserted/deleted lines from causing
/// knock-on header churn later in the committed `.diff` baselines.
///
/// Side effects: none (pure).
// Go: internal/testutil/baseline/baseline.go:getBaselineDiff
fn get_baseline_diff(
    actual: &str,
    expected: &str,
    file_name: &str,
    fixup_old: Option<&dyn Fn(&str) -> String>,
    fixup_new: Option<&dyn Fn(&str) -> String>,
) -> String {
    let expected = match fixup_old {
        Some(f) => f(expected),
        None => expected.to_string(),
    };
    let actual = match fixup_new {
        Some(f) => f(actual),
        None => actual.to_string(),
    };
    if actual == expected {
        return NO_CONTENT.to_string();
    }
    let s = diff_text(
        &format!("old.{file_name}"),
        &format!("new.{file_name}"),
        &expected,
        &actual,
    );

    // If the diff is empty (just headers, no hunks), return NoContent.
    if !s.contains("@@") {
        return NO_CONTENT.to_string();
    }

    // Remove line numbers from unified diff headers; this avoids adding/deleting
    // lines in our baselines from causing knock-on header changes later in the
    // diff.
    let re = fix_unified_diff();
    let mut a_cur_line: i64 = 1;
    let mut b_cur_line: i64 = 1;
    let result = re.replace_all(&s, |caps: &regex::Captures| {
        let a_line: i64 = caps[1].parse().expect("unified diff header line number");
        let b_line: i64 = caps[2].parse().expect("unified diff header line number");
        let a_diff = a_line - a_cur_line;
        let b_diff = b_line - b_cur_line;
        a_cur_line = a_line;
        b_cur_line = b_line;
        // Keep surrounded by @@, to make GitHub's grammar happy.
        format!("@@= skipped -{a_diff}, +{b_diff} lines =@@")
    });

    result.into_owned()
}

/// Matches a unified-diff hunk header, capturing the old and new start lines.
///
/// The line counts are optional because some diff emitters omit `,1`; Go's
/// `patience` always includes them, but capturing only the start lines keeps the
/// header rewrite robust either way.
// Go: internal/testutil/baseline/baseline.go:fixUnifiedDiff
fn fix_unified_diff() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@").expect("valid regex")
    })
}

/// Reads a newline-separated list of file names into a [`Set`], trimming each
/// line and skipping blank lines and `#` comments.
///
/// Side effects: reads `path`.
///
/// # Panics
/// Panics if `path` cannot be read (mirrors Go's `panic`).
// Go: internal/testutil/baseline/baseline.go:readFileNameSet
fn read_file_name_set(path: &Path) -> tsgo_collections::Set<String> {
    let mut set = tsgo_collections::Set::default();
    match std::fs::read_to_string(path) {
        Ok(content) => {
            for line in content.split('\n') {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                set.add(line.to_string());
            }
        }
        Err(e) => panic!("failed to read file {}: {e}", path.display()),
    }
    set
}

/// Writes `actual` to the local baseline for `file_name` and compares it to the
/// committed reference; for submodule baselines, also writes a categorized diff
/// (`submodule` / `submoduleAccepted` / `submoduleTriaged`) against the upstream
/// TypeScript baseline.
///
/// Failures (mismatch, new baseline, etc.) are accumulated on `harness`.
///
/// # Examples
/// ```no_run
/// # use tsgo_testutil_baseline::{Harness, Options, run};
/// let mut h = Harness::new();
/// run(&mut h, "my.types", "actual content\n", &Options::default());
/// ```
///
/// Side effects: writes under `<testdata>/baselines/local`; reads reference
/// baselines. Reference baselines: never written.
// Go: internal/testutil/baseline/baseline.go:Run
pub fn run(harness: &mut Harness, file_name: &str, actual: &str, opts: &Options) {
    let orig_subfolder = opts.subfolder.as_str();

    {
        let subfolder = if opts.is_submodule {
            Path::new("submodule").join(&opts.subfolder)
        } else {
            std::path::PathBuf::from(&opts.subfolder)
        };

        let local_path = local_root().join(&subfolder).join(file_name);
        let reference_path = reference_root().join(&subfolder).join(file_name);

        // Record this baseline for tracking unused baselines.
        record_baseline(harness, &subfolder.join(file_name).to_string_lossy());

        write_comparison(harness, actual, &local_path, &reference_path, false);
    }

    if !opts.is_submodule || opts.skip_diff_with_old {
        // Not a submodule, no diffs.
        return;
    }

    let submodule_reference = submodule_reference_root().join(file_name);
    let submodule_expected = read_file_or_no_content(&submodule_reference);

    const SUBMODULE_FOLDER: &str = "submodule";
    const SUBMODULE_ACCEPTED_FOLDER: &str = "submoduleAccepted";
    const SUBMODULE_TRIAGED_FOLDER: &str = "submoduleTriaged";

    let diff_file_name = format!("{file_name}.diff");
    let diff_key = format!("{orig_subfolder}/{diff_file_name}");
    let is_submodule_accepted =
        opts.is_submodule_accepted || submodule_accepted_file_names().has(&diff_key);
    let is_submodule_triaged =
        opts.is_submodule_triaged || submodule_triaged_file_names().has(&diff_key);

    if is_submodule_accepted && is_submodule_triaged {
        harness.error(format!(
            "diff file {orig_subfolder}/{diff_file_name} is in both submoduleAccepted and submoduleTriaged; it should only be in one"
        ));
        return;
    }

    let out_root = if is_submodule_accepted {
        SUBMODULE_ACCEPTED_FOLDER
    } else if is_submodule_triaged {
        SUBMODULE_TRIAGED_FOLDER
    } else {
        SUBMODULE_FOLDER
    };

    let all_roots = [
        SUBMODULE_FOLDER,
        SUBMODULE_ACCEPTED_FOLDER,
        SUBMODULE_TRIAGED_FOLDER,
    ];

    let diff = get_baseline_diff(
        actual,
        &submodule_expected,
        file_name,
        opts.diff_fixup_old.as_deref(),
        opts.diff_fixup_new.as_deref(),
    );

    for root in all_roots {
        let local_path = local_root()
            .join(root)
            .join(orig_subfolder)
            .join(&diff_file_name);
        let reference_path = reference_root()
            .join(root)
            .join(orig_subfolder)
            .join(&diff_file_name);

        // Record this baseline for tracking unused baselines.
        record_baseline(
            harness,
            &Path::new(root)
                .join(orig_subfolder)
                .join(&diff_file_name)
                .to_string_lossy(),
        );

        if root == out_root {
            write_comparison(harness, &diff, &local_path, &reference_path, false);
        } else {
            write_comparison(harness, NO_CONTENT, &local_path, &reference_path, false);
        }
    }
}

/// Compares `actual` directly against the TypeScript submodule baseline (used
/// when corsa output is expected to match upstream byte-for-byte).
///
/// # Examples
/// ```no_run
/// # use tsgo_testutil_baseline::{Harness, Options, run_against_submodule};
/// let mut h = Harness::new();
/// run_against_submodule(&mut h, "f.js", "actual\n", &Options::default());
/// ```
///
/// Side effects: writes under `<testdata>/baselines/local`; reads the submodule
/// reference baseline.
// Go: internal/testutil/baseline/baseline.go:RunAgainstSubmodule
pub fn run_against_submodule(harness: &mut Harness, file_name: &str, actual: &str, opts: &Options) {
    // Record this baseline for tracking unused baselines.
    record_baseline(
        harness,
        &Path::new(&opts.subfolder).join(file_name).to_string_lossy(),
    );

    let local = local_root().join(&opts.subfolder).join(file_name);
    let reference = submodule_reference_root()
        .join(&opts.subfolder)
        .join(file_name);
    write_comparison(harness, actual, &local, &reference, true);
}

/// Reads `file_name`'s contents, or returns [`NO_CONTENT`] if it cannot be read.
///
/// Side effects: reads `file_name`.
// Go: internal/testutil/baseline/baseline.go:readFileOrNoContent
fn read_file_or_no_content(file_name: &Path) -> String {
    match std::fs::read(file_name) {
        Ok(content) => String::from_utf8_lossy(&content).into_owned(),
        Err(_) => NO_CONTENT.to_string(),
    }
}

/// `<testdata>/baselines/local` — where actual test output is written.
// Go: internal/testutil/baseline/baseline.go:localRoot
fn local_root() -> &'static Path {
    static P: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        Path::new(tsgo_repo::test_data_path())
            .join("baselines")
            .join("local")
    })
}

/// `<testdata>/baselines/reference` — the committed reference baselines.
// Go: internal/testutil/baseline/baseline.go:referenceRoot
fn reference_root() -> &'static Path {
    static P: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        Path::new(tsgo_repo::test_data_path())
            .join("baselines")
            .join("reference")
    })
}

/// `<submodule>/tests/baselines/reference` — the upstream TypeScript baselines.
// Go: internal/testutil/baseline/baseline.go:submoduleReferenceRoot
fn submodule_reference_root() -> &'static Path {
    static P: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        Path::new(tsgo_repo::typescript_submodule_path())
            .join("tests")
            .join("baselines")
            .join("reference")
    })
}

/// Set of `<subfolder>/<file>.diff` keys explicitly accepted as deviations from
/// the TypeScript submodule baselines (loaded once from
/// `<testdata>/submoduleAccepted.txt`).
///
/// Side effects: reads the list file on first call.
// Go: internal/testutil/baseline/baseline.go:submoduleAcceptedFileNames
fn submodule_accepted_file_names() -> &'static tsgo_collections::Set<String> {
    static S: std::sync::OnceLock<tsgo_collections::Set<String>> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        read_file_name_set(&Path::new(tsgo_repo::test_data_path()).join("submoduleAccepted.txt"))
    })
}

/// Set of `<subfolder>/<file>.diff` keys triaged (known-but-not-yet-accepted)
/// against the TypeScript submodule baselines (loaded once from
/// `<testdata>/submoduleTriaged.txt`).
///
/// Side effects: reads the list file on first call.
// Go: internal/testutil/baseline/baseline.go:submoduleTriagedFileNames
fn submodule_triaged_file_names() -> &'static tsgo_collections::Set<String> {
    static S: std::sync::OnceLock<tsgo_collections::Set<String>> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        read_file_name_set(&Path::new(tsgo_repo::test_data_path()).join("submoduleTriaged.txt"))
    })
}

/// Appends `suffix` to the final component of `path` (mirrors Go's `local +
/// ".delete"` string concatenation, which is not a path-extension swap).
fn append_suffix(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    std::path::PathBuf::from(s)
}

/// Writes `actual_content` to `local`, compares it to the `reference` file, and
/// records a failure on the harness when they differ (or when a new baseline is
/// created).
///
/// When `actual_content` equals the reference, nothing is written (a stale
/// `local` file is removed). When it is [`NO_CONTENT`] and a reference exists, a
/// `<local>.delete` marker is written instead. An empty `actual_content` is a
/// programming error and panics.
///
/// # Examples
/// ```no_run
/// # use std::path::Path;
/// # use tsgo_testutil_baseline::Harness;
/// // Internal engine; see crate tests for usage with temp directories.
/// ```
///
/// Side effects: creates `local`'s parent directories; writes/removes `local`
/// (or `<local>.delete`); reads `reference`. Reference file: never written.
// Go: internal/testutil/baseline/baseline.go:writeComparison
fn write_comparison(
    harness: &mut Harness,
    actual_content: &str,
    local: &Path,
    reference: &Path,
    comparing_against_submodule: bool,
) {
    if actual_content.is_empty() {
        panic!(
            "the generated content was \"\". Return 'baseline.NoContent' if no baselining is required."
        );
    }

    if let Some(parent) = local.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            harness.error(format!(
                "failed to create directories for the local baseline file {}: {e}",
                local.display()
            ));
            return;
        }
    }

    if local.exists() {
        if let Err(e) = std::fs::remove_file(local) {
            harness.error(format!(
                "failed to remove the local baseline file {}: {e}",
                local.display()
            ));
            return;
        }
    }

    let (expected, found_expected) = match std::fs::read(reference) {
        Ok(content) => (String::from_utf8_lossy(&content).into_owned(), true),
        Err(_) => (NO_CONTENT.to_string(), false),
    };

    if expected != actual_content || (actual_content == NO_CONTENT && found_expected) {
        if actual_content == NO_CONTENT {
            let delete = append_suffix(local, ".delete");
            if let Err(e) = std::fs::write(&delete, b"") {
                harness.error(format!(
                    "failed to write the local baseline file {}: {e}",
                    delete.display()
                ));
                return;
            }
        } else if let Err(e) = std::fs::write(local, actual_content.as_bytes()) {
            harness.error(format!(
                "failed to write the local baseline file {}: {e}",
                local.display()
            ));
            return;
        }

        if !reference.exists() {
            if comparing_against_submodule {
                harness.error(format!(
                    "the baseline file {} does not exist in the TypeScript submodule",
                    reference.display()
                ));
            } else {
                harness.error(format!("new baseline created at {}.", local.display()));
            }
        } else if comparing_against_submodule {
            harness.error(format!(
                "the baseline file {} does not match the reference in the TypeScript submodule",
                reference.display()
            ));
        } else {
            harness.error(format!(
                "the baseline file {} has changed. (Run `hereby baseline-accept` if the new baseline is correct.)",
                reference.display()
            ));
        }
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
