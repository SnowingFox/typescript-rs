use super::*;
use crate::file_include::*;
use crate::processing_diagnostic::*;
use tsgo_tspath::Path;

fn p(s: &str) -> Path {
    Path(s.to_string())
}

#[test]
fn new_processor_is_empty() {
    let proc = IncludeProcessor::new();
    assert_eq!(proc.file_count(), 0);
    assert_eq!(proc.processing_diagnostic_count(), 0);
}

#[test]
fn add_and_retrieve_reason() {
    let mut proc = IncludeProcessor::new();
    let path = p("/src/a.ts");
    proc.add_file_include_reason(path.clone(), FileIncludeReason::root_file(0));
    assert_eq!(proc.file_count(), 1);
    let reasons = proc.reasons_for(&path).unwrap();
    assert_eq!(reasons.len(), 1);
    assert_eq!(reasons[0].kind, FileIncludeKind::RootFile);
}

#[test]
fn multiple_reasons_for_same_file() {
    let mut proc = IncludeProcessor::new();
    let path = p("/src/shared.ts");
    proc.add_file_include_reason(path.clone(), FileIncludeReason::root_file(1));
    proc.add_file_include_reason(
        path.clone(),
        FileIncludeReason::referenced_file(
            FileIncludeKind::Import,
            ReferencedFileData {
                file: p("/src/main.ts"),
                index: 0,
                synthetic_text: None,
            },
        ),
    );
    assert_eq!(proc.reasons_for(&path).unwrap().len(), 2);
}

#[test]
fn missing_file_returns_none() {
    let proc = IncludeProcessor::new();
    assert!(proc.reasons_for(&p("/nope")).is_none());
}

#[test]
fn add_processing_diagnostic() {
    let mut proc = IncludeProcessor::new();
    proc.add_processing_diagnostic(ProcessingDiagnostic::unknown_reference(
        FileIncludeReason::referenced_file(
            FileIncludeKind::TypeReferenceDirective,
            ReferencedFileData {
                file: p("/types.ts"),
                index: 0,
                synthetic_text: None,
            },
        ),
    ));
    assert_eq!(proc.processing_diagnostic_count(), 1);
}

#[test]
fn file_casing_with_referenced_reason() {
    let mut proc = IncludeProcessor::new();
    let path = p("/src/Utils.ts");
    proc.add_file_include_reason(
        path.clone(),
        FileIncludeReason::referenced_file(
            FileIncludeKind::Import,
            ReferencedFileData {
                file: p("/src/main.ts"),
                index: 0,
                synthetic_text: None,
            },
        ),
    );
    proc.add_file_casing_diagnostic(
        path,
        "Utils.ts",
        "utils.ts",
        FileIncludeReason::root_file(0),
    );
    assert_eq!(proc.processing_diagnostic_count(), 1);
    let expl = proc.processing_diagnostics()[0]
        .as_include_explaining()
        .unwrap();
    assert_eq!(expl.args, vec!["Utils.ts", "utils.ts"]);
}

#[test]
fn updated_from_preserves_data() {
    let mut proc = IncludeProcessor::new();
    proc.add_file_include_reason(p("/a.ts"), FileIncludeReason::root_file(0));
    proc.add_processing_diagnostic(ProcessingDiagnostic::unknown_reference(
        FileIncludeReason::referenced_file(
            FileIncludeKind::LibReferenceDirective,
            ReferencedFileData {
                file: p("/lib.d.ts"),
                index: 0,
                synthetic_text: None,
            },
        ),
    ));
    let updated = IncludeProcessor::updated_from(proc);
    assert_eq!(updated.file_count(), 1);
    assert_eq!(updated.processing_diagnostic_count(), 1);
}
