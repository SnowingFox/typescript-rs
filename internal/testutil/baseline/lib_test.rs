use super::*;

// Go: internal/testutil/baseline/baseline.go:writeComparison
// When actual == reference, no failure is recorded and no local file is left.
#[test]
fn write_comparison_equal_content_no_failure() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("local").join("a.txt");
    let reference = dir.path().join("ref").join("a.txt");
    std::fs::create_dir_all(reference.parent().unwrap()).unwrap();
    std::fs::write(&reference, b"foo\n").unwrap();

    let mut h = Harness::new();
    write_comparison(&mut h, "foo\n", &local, &reference, false);

    assert!(
        h.failures().is_empty(),
        "expected no failures, got {:?}",
        h.failures()
    );
    assert!(
        !local.exists(),
        "local must not be written when content matches"
    );
}

// Go: internal/testutil/baseline/baseline.go:Run (non-submodule, unknown name)
#[test]
fn run_creates_new_local_baseline_for_unknown_name() {
    let subfolder = "ts_rust_port_test_run_new";
    let mut h = Harness::new();
    let opts = Options {
        subfolder: subfolder.to_string(),
        ..Default::default()
    };
    run(&mut h, "run_new.txt", "hello\n", &opts);

    assert!(
        h.failures()
            .iter()
            .any(|f| f.contains("new baseline created")),
        "expected a new-baseline failure, got {:?}",
        h.failures()
    );
    let local = local_root().join(subfolder).join("run_new.txt");
    assert_eq!(std::fs::read_to_string(&local).unwrap(), "hello\n");

    let _ = std::fs::remove_dir_all(local_root().join(subfolder));
}

// Go: internal/testutil/baseline/baseline.go:Run (submodule diff path)
#[test]
fn run_submodule_writes_categorized_diff() {
    let subfolder = "ts_rust_port_test_run_submodule";
    let mut h = Harness::new();
    let opts = Options {
        subfolder: subfolder.to_string(),
        is_submodule: true,
        ..Default::default()
    };
    // The submodule is not checked out, so the upstream baseline reads as
    // NoContent; a non-empty `actual` produces a diff written under
    // local/submodule/<subfolder>/<file>.diff.
    run(&mut h, "run_sm.js", "var x = 1;\n", &opts);

    let diff_local = local_root()
        .join("submodule")
        .join(subfolder)
        .join("run_sm.js.diff");
    assert!(
        diff_local.exists(),
        "expected a categorized submodule diff at {diff_local:?}"
    );

    for root in ["submodule", "submoduleAccepted", "submoduleTriaged"] {
        let _ = std::fs::remove_dir_all(local_root().join(root).join(subfolder));
    }
    let _ = std::fs::remove_dir_all(local_root().join("submodule").join(subfolder));
}

// Go: internal/testutil/baseline/baseline.go:RunAgainstSubmodule
#[test]
fn run_against_submodule_reports_missing_in_submodule() {
    let subfolder = "ts_rust_port_test_ras";
    let mut h = Harness::new();
    let opts = Options {
        subfolder: subfolder.to_string(),
        ..Default::default()
    };
    run_against_submodule(&mut h, "ras.txt", "data\n", &opts);

    assert!(
        h.failures()
            .iter()
            .any(|f| f.contains("does not exist in the TypeScript submodule")),
        "expected a submodule-missing failure, got {:?}",
        h.failures()
    );
    let local = local_root().join(subfolder).join("ras.txt");
    assert_eq!(std::fs::read_to_string(&local).unwrap(), "data\n");

    let _ = std::fs::remove_dir_all(local_root().join(subfolder));
}

// Go: internal/testutil/baseline/baseline_test.go:TestSubmoduleAcceptedFilesExist
// Every entry listed in submoduleAccepted.txt must have a baseline file under
// reference/submoduleAccepted/.
#[test]
fn submodule_accepted_files_exist() {
    for name in submodule_accepted_file_names().keys() {
        let path = reference_root().join("submoduleAccepted").join(name);
        assert!(
            path.exists(),
            "submoduleAccepted.txt references {name:?}, but the baseline file does not exist"
        );
    }
}

// Go: internal/testutil/baseline/baseline_test.go:TestSubmoduleTriagedFilesExist
#[test]
fn submodule_triaged_files_exist() {
    for name in submodule_triaged_file_names().keys() {
        let path = reference_root().join("submoduleTriaged").join(name);
        assert!(
            path.exists(),
            "submoduleTriaged.txt references {name:?}, but the baseline file does not exist"
        );
    }
}

// Go: internal/testutil/baseline/baseline.go:readFileNameSet
#[test]
fn read_file_name_set_skips_blank_and_comments() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("list.txt");
    std::fs::write(
        &path,
        "# a comment\n\n  foo/bar.diff  \nbaz.diff\n   \n# another\nfoo/bar.diff\n",
    )
    .unwrap();

    let set = read_file_name_set(&path);

    assert!(set.has(&"foo/bar.diff".to_string()));
    assert!(set.has(&"baz.diff".to_string()));
    assert!(!set.has(&"# a comment".to_string()));
    // Deduplicated; only the two distinct non-comment, non-blank entries remain.
    assert_eq!(set.len(), 2);
}

// Go: internal/testutil/baseline/baseline.go:DiffText
#[test]
fn diff_text_basic_unified() {
    let d = diff_text("old.x", "new.x", "a\nb\nc\n", "a\nB\nc\n");
    assert!(d.contains("--- old.x"), "missing old header in:\n{d}");
    assert!(d.contains("+++ new.x"), "missing new header in:\n{d}");
    assert!(d.contains("@@"), "missing hunk header in:\n{d}");
    assert!(d.contains("-b"), "missing removed line in:\n{d}");
    assert!(d.contains("+B"), "missing added line in:\n{d}");
}

// Go: internal/testutil/baseline/baseline.go:getBaselineDiff (identical -> NoContent)
#[test]
fn get_baseline_diff_identical_returns_nocontent() {
    let d = get_baseline_diff("same\n", "same\n", "f.txt", None, None);
    assert_eq!(d, NO_CONTENT);
}

// Go: internal/testutil/baseline/baseline.go:getBaselineDiff (fixups make equal -> NoContent)
#[test]
fn get_baseline_diff_fixups_applied() {
    // fixup_new normalizes actual to match expected, so the diff collapses.
    let fixup_new = |s: &str| s.replace("VERSION_X", "VERSION");
    let d = get_baseline_diff(
        "v=VERSION_X\n",
        "v=VERSION\n",
        "f.txt",
        None,
        Some(&fixup_new),
    );
    assert_eq!(d, NO_CONTENT);
}

// Go: internal/testutil/baseline/baseline.go:getBaselineDiff (header line numbers skipped)
#[test]
fn get_baseline_diff_header_line_numbers_skipped() {
    // Build a 12-line file with two well-separated changes so the unified diff
    // splits into two hunks (context radius 3 leaves an unchanged gap).
    let expected: String = (1..=12).map(|i| format!("line{i}\n")).collect();
    let mut actual_lines: Vec<String> = (1..=12).map(|i| format!("line{i}")).collect();
    actual_lines[1] = "CHANGED2".to_string();
    actual_lines[10] = "CHANGED11".to_string();
    let actual: String = actual_lines.iter().map(|l| format!("{l}\n")).collect();

    let d = get_baseline_diff(&actual, &expected, "f.txt", None, None);

    assert!(
        d.contains("@@= skipped -"),
        "expected rewritten hunk headers, got:\n{d}"
    );
    assert!(
        !d.contains("@@ -"),
        "raw unified hunk headers must be rewritten, got:\n{d}"
    );
    assert!(
        d.matches("@@= skipped -").count() >= 2,
        "expected at least two hunks rewritten, got:\n{d}"
    );
}

// Go: internal/testutil/baseline/baseline.go:writeComparison (changed branch)
#[test]
fn write_comparison_mismatch_reports_changed() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("local").join("a.txt");
    let reference = dir.path().join("ref").join("a.txt");
    std::fs::create_dir_all(reference.parent().unwrap()).unwrap();
    std::fs::write(&reference, b"bar\n").unwrap();

    let mut h = Harness::new();
    write_comparison(&mut h, "foo\n", &local, &reference, false);

    assert_eq!(h.failures().len(), 1);
    assert!(
        h.failures()[0].contains("has changed"),
        "got {:?}",
        h.failures()
    );
    assert_eq!(std::fs::read_to_string(&local).unwrap(), "foo\n");
}

// Go: internal/testutil/baseline/baseline.go:writeComparison (new baseline branch)
#[test]
fn write_comparison_missing_reference_reports_new() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("local").join("a.txt");
    let reference = dir.path().join("ref").join("missing.txt");

    let mut h = Harness::new();
    write_comparison(&mut h, "x", &local, &reference, false);

    assert_eq!(h.failures().len(), 1);
    assert!(
        h.failures()[0].contains("new baseline created at"),
        "got {:?}",
        h.failures()
    );
    assert_eq!(std::fs::read_to_string(&local).unwrap(), "x");
}

// Go: internal/testutil/baseline/baseline.go:writeComparison (empty-content panic)
#[test]
#[should_panic(expected = "the generated content was")]
fn write_comparison_empty_actual_panics() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("local").join("a.txt");
    let reference = dir.path().join("ref").join("a.txt");

    let mut h = Harness::new();
    write_comparison(&mut h, "", &local, &reference, false);
}

// Go: internal/testutil/baseline/baseline.go:writeComparison (NoContent -> .delete marker)
#[test]
fn write_comparison_nocontent_writes_delete_marker() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("local").join("a.txt");
    let reference = dir.path().join("ref").join("a.txt");
    std::fs::create_dir_all(reference.parent().unwrap()).unwrap();
    std::fs::write(&reference, b"foo\n").unwrap();

    let mut h = Harness::new();
    write_comparison(&mut h, NO_CONTENT, &local, &reference, false);

    let delete = dir.path().join("local").join("a.txt.delete");
    assert!(
        delete.exists(),
        "expected <local>.delete marker to be written"
    );
    assert!(!local.exists(), "the real local file must not be written");
}

// Go: internal/testutil/baseline/baseline.go:writeComparison (submodule branches)
#[test]
fn write_comparison_submodule_messages() {
    let dir = tempfile::tempdir().unwrap();

    // Reference missing in submodule.
    let local1 = dir.path().join("local").join("a.txt");
    let reference1 = dir.path().join("ref").join("missing.txt");
    let mut h = Harness::new();
    write_comparison(&mut h, "x", &local1, &reference1, true);
    assert_eq!(h.failures().len(), 1);
    assert!(
        h.failures()[0].contains("does not exist in the TypeScript submodule"),
        "got {:?}",
        h.failures()
    );

    // Reference present but differing in submodule.
    let local2 = dir.path().join("local").join("b.txt");
    let reference2 = dir.path().join("ref").join("b.txt");
    std::fs::create_dir_all(reference2.parent().unwrap()).unwrap();
    std::fs::write(&reference2, b"bar\n").unwrap();
    let mut h2 = Harness::new();
    write_comparison(&mut h2, "foo\n", &local2, &reference2, true);
    assert_eq!(h2.failures().len(), 1);
    assert!(
        h2.failures()[0].contains("does not match the reference in the TypeScript submodule"),
        "got {:?}",
        h2.failures()
    );
}
