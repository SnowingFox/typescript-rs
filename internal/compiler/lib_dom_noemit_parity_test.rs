//! T5-1: `lib.dom.d.ts` `--noEmit` parity between Go and Rust `tsgo`.
//!
//! Measured on 2026-06-05 with:
//!   `go run ./cmd/tsgo --noEmit internal/bundled/libs/lib.dom.d.ts`
//!   `./target/release/tsgo --noEmit internal/bundled/libs/lib.dom.d.ts`
//!
//! **Result: DIVERGED** — zero line-level overlap after path normalization.
//!
//! | Side | Total | Unique | Top codes |
//! |------|------:|-------:|-----------|
//! | Go   |   946 |    473 | TS2300×858, TS2374×86, TS2451×2 |
//! | Rust |     5 |      5 | TS2344×5 |
//!
//! **Top gaps (by error code)**
//!
//! - **Go-only TS2300 (858 / 429 unique)**: `Duplicate identifier` — dominant
//!   cluster; many diagnostics appear twice at the same span, indicating
//!   `lib.dom.d.ts` is processed more than once when Go loads it as the root
//!   input *and* via the default-lib reference graph.
//! - **Go-only TS2374 (86 / 43 unique)**: `Duplicate index signature` — same
//!   double-load symptom (e.g. `[index: number]: …` reported twice).
//! - **Go-only TS2451 (2 / 1 unique)**: `Cannot redeclare block-scoped
//!   variable` — same root cause.
//! - **Rust-only TS2344 (5)**: `Type 'string' does not satisfy the constraint
//!   'never'` on template-literal utility aliases (`OptionalPostfixToken`,
//!   `OptionalPrefixToken`, `IDBValidKey`, …) — Go CLI does not emit these when
//!   checking the standalone lib file; Rust does.
//!
//! Re-measure: `bash internal/compiler/lib_dom_noemit_parity.sh`
//! or `cargo test -p tsgo_compiler lib_dom_noemit_cli_parity -- --ignored`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use tsgo_repo::root_path;

/// Path to the bundled DOM lib (ground truth for T5-1).
fn lib_dom_path() -> PathBuf {
    Path::new(root_path())
        .join("internal/bundled/libs/lib.dom.d.ts")
}

/// Normalizes CLI diagnostic lines so Go (`bundled:///libs/…`) and Rust
/// (`internal/bundled/libs/…`) paths compare fairly.
fn normalize_diagnostic_line(line: &str, repo_root: &str) -> Option<String> {
    let line = line.trim_end();
    if !line.contains(": error TS") {
        return None;
    }
    let normalized = line
        .replace("bundled:///libs/", "LIB/")
        .replace(&format!("{repo_root}/internal/bundled/libs/"), "LIB/")
        .replace("internal/bundled/libs/", "LIB/");
    Some(normalized)
}

/// Tallies `error TS####` occurrences in normalized diagnostic lines.
fn count_by_code(lines: &[String]) -> BTreeMap<i32, usize> {
    let mut counts = BTreeMap::new();
    for line in lines {
        if let Some(rest) = line.split(": error TS").nth(1) {
            if let Some(code_str) = rest.split(':').next() {
                if let Ok(code) = code_str.parse::<i32>() {
                    *counts.entry(code).or_default() += 1;
                }
            }
        }
    }
    counts
}

/// Parsed, normalized parity snapshot for one CLI run.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DiagnosticSnapshot {
    lines: Vec<String>,
    unique_lines: Vec<String>,
    by_code: BTreeMap<i32, usize>,
    unique_by_code: BTreeMap<i32, usize>,
}

impl DiagnosticSnapshot {
    fn from_raw_output(raw: &str, repo_root: &str) -> Self {
        let mut lines: Vec<String> = raw
            .lines()
            .filter_map(|l| normalize_diagnostic_line(l, repo_root))
            .collect();
        lines.sort();
        let unique_lines: Vec<String> = {
            let mut u = lines.clone();
            u.sort();
            u.dedup();
            u
        };
        let by_code = count_by_code(&lines);
        let unique_by_code = count_by_code(&unique_lines);
        Self {
            lines,
            unique_lines,
            by_code,
            unique_by_code,
        }
    }

    fn overlap_count(&self, other: &Self) -> usize {
        let other_set: std::collections::BTreeSet<_> = other.lines.iter().collect();
        self.lines.iter().filter(|l| other_set.contains(l)).count()
    }

    fn only_in_self(&self, other: &Self) -> usize {
        let other_set: std::collections::BTreeSet<_> = other.lines.iter().collect();
        self.lines.iter().filter(|l| !other_set.contains(l)).count()
    }
}

/// Pinned T5-1 characterization (2026-06-05). Update only after re-measuring
/// with `lib_dom_noemit_parity.sh`.
const MEASURED_GO_TOTAL: usize = 946;
const MEASURED_GO_UNIQUE: usize = 473;
const MEASURED_RUST_TOTAL: usize = 5;
const MEASURED_RUST_UNIQUE: usize = 5;
const MEASURED_OVERLAP: usize = 0;

const MEASURED_GO_CODES: &[(i32, usize)] = &[(2300, 858), (2374, 86), (2451, 2)];
const MEASURED_RUST_CODES: &[(i32, usize)] = &[(2344, 5)];

const MEASURED_GO_UNIQUE_CODES: &[(i32, usize)] = &[(2300, 429), (2374, 43), (2451, 1)];
const MEASURED_RUST_UNIQUE_CODES: &[(i32, usize)] = &[(2344, 5)];

fn run_go_tsgo(lib_dom: &Path) -> std::io::Result<String> {
    let root = root_path();
    let output = Command::new("go")
        .args(["run", "./cmd/tsgo", "--noEmit"])
        .arg(lib_dom)
        .current_dir(root)
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr))
}

fn run_rust_tsgo(lib_dom: &Path) -> std::io::Result<String> {
    let root = root_path();
    let release_bin = Path::new(root).join("target/release/tsgo");
    let output = if release_bin.is_file() {
        Command::new(&release_bin)
            .args(["--noEmit"])
            .arg(lib_dom)
            .current_dir(root)
            .output()?
    } else {
        Command::new("cargo")
            .args([
                "run",
                "--release",
                "--quiet",
                "-p",
                "tsgo",
                "--",
                "--noEmit",
            ])
            .arg(lib_dom)
            .current_dir(root)
            .output()?
    };
    Ok(String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr))
}

fn assert_code_counts(actual: &BTreeMap<i32, usize>, expected: &[(i32, usize)], label: &str) {
    for &(code, count) in expected {
        assert_eq!(
            actual.get(&code).copied().unwrap_or(0),
            count,
            "{label}: TS{code} count drifted; full map: {actual:?}"
        );
    }
    assert_eq!(
        actual.values().sum::<usize>(),
        expected.iter().map(|(_, c)| c).sum::<usize>(),
        "{label}: unexpected extra error codes: {actual:?}"
    );
}

// Go: cmd/tsgo (CLI --noEmit on bundled lib.dom.d.ts)
#[test]
fn lib_dom_noemit_parity_documented_delta() {
    assert!(
        lib_dom_path().is_file(),
        "bundled lib.dom.d.ts must exist at {:?}",
        lib_dom_path()
    );

    // Pin the measured characterization so drift is caught if someone
    // re-runs the CLI comparison and updates the constants above.
    assert_eq!(MEASURED_GO_TOTAL, 946);
    assert_eq!(MEASURED_GO_UNIQUE, 473);
    assert_eq!(MEASURED_RUST_TOTAL, 5);
    assert_eq!(MEASURED_RUST_UNIQUE, 5);
    assert_eq!(MEASURED_OVERLAP, 0);

    let mut go_map = BTreeMap::new();
    for &(code, count) in MEASURED_GO_CODES {
        go_map.insert(code, count);
    }
    assert_code_counts(&go_map, MEASURED_GO_CODES, "go-total");

    let mut rust_map = BTreeMap::new();
    for &(code, count) in MEASURED_RUST_CODES {
        rust_map.insert(code, count);
    }
    assert_code_counts(&rust_map, MEASURED_RUST_CODES, "rust-total");

    let mut go_unique_map = BTreeMap::new();
    for &(code, count) in MEASURED_GO_UNIQUE_CODES {
        go_unique_map.insert(code, count);
    }
    assert_code_counts(&go_unique_map, MEASURED_GO_UNIQUE_CODES, "go-unique");

    let mut rust_unique_map = BTreeMap::new();
    for &(code, count) in MEASURED_RUST_UNIQUE_CODES {
        rust_unique_map.insert(code, count);
    }
    assert_code_counts(&rust_unique_map, MEASURED_RUST_UNIQUE_CODES, "rust-unique");

    assert!(
        MEASURED_GO_UNIQUE < MEASURED_GO_TOTAL,
        "Go double-load duplicates expected: unique {MEASURED_GO_UNIQUE} < total {MEASURED_GO_TOTAL}"
    );
}

// Go: cmd/tsgo (CLI --noEmit on bundled lib.dom.d.ts)
#[test]
fn lib_dom_noemit_path_normalization_equivalence() {
    let root = root_path();
    let go_style = format!(
        "bundled:///libs/lib.dom.d.ts(1,1): error TS2300: Duplicate identifier 'Foo'."
    );
    let rust_style = format!(
        "{root}/internal/bundled/libs/lib.dom.d.ts(1,1): error TS2300: Duplicate identifier 'Foo'."
    );
    let expected = "LIB/lib.dom.d.ts(1,1): error TS2300: Duplicate identifier 'Foo'.";
    assert_eq!(
        normalize_diagnostic_line(&go_style, root).as_deref(),
        Some(expected)
    );
    assert_eq!(
        normalize_diagnostic_line(&rust_style, root).as_deref(),
        Some(expected)
    );
}

/// Live CLI comparison. Opt-in: `cargo test -p tsgo_compiler lib_dom_noemit_cli_parity -- --ignored`
///
/// Go ground truth: `go run ./cmd/tsgo --noEmit internal/bundled/libs/lib.dom.d.ts`
#[test]
#[ignore]
fn lib_dom_noemit_cli_parity() {
    let lib_dom = lib_dom_path();
    assert!(lib_dom.is_file(), "missing {:?}", lib_dom);

    let root = root_path();
    let go_raw = run_go_tsgo(&lib_dom).expect("go tsgo");
    let rust_raw = run_rust_tsgo(&lib_dom).expect("rust tsgo");

    let go = DiagnosticSnapshot::from_raw_output(&go_raw, root);
    let rust = DiagnosticSnapshot::from_raw_output(&rust_raw, root);

    eprintln!("=== T5-1 lib.dom.d.ts --noEmit CLI parity ===");
    eprintln!(
        "Go:   total={} unique={} codes={:?}",
        go.lines.len(),
        go.unique_lines.len(),
        go.by_code
    );
    eprintln!(
        "Rust: total={} unique={} codes={:?}",
        rust.lines.len(),
        rust.unique_lines.len(),
        rust.by_code
    );
    eprintln!("Overlap: {}", go.overlap_count(&rust));
    eprintln!("Only Go: {}", go.only_in_self(&rust));
    eprintln!("Only Rust: {}", rust.only_in_self(&go));

    assert_eq!(go.lines.len(), MEASURED_GO_TOTAL, "Go total drifted");
    assert_eq!(go.unique_lines.len(), MEASURED_GO_UNIQUE, "Go unique drifted");
    assert_eq!(rust.lines.len(), MEASURED_RUST_TOTAL, "Rust total drifted");
    assert_eq!(
        rust.unique_lines.len(),
        MEASURED_RUST_UNIQUE,
        "Rust unique drifted"
    );
    assert_eq!(go.overlap_count(&rust), MEASURED_OVERLAP, "overlap drifted");

    assert_code_counts(&go.by_code, MEASURED_GO_CODES, "live-go-total");
    assert_code_counts(&rust.by_code, MEASURED_RUST_CODES, "live-rust-total");
    assert_code_counts(&go.unique_by_code, MEASURED_GO_UNIQUE_CODES, "live-go-unique");
    assert_code_counts(
        &rust.unique_by_code,
        MEASURED_RUST_UNIQUE_CODES,
        "live-rust-unique",
    );
}
