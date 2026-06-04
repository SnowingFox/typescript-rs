use super::*;
use crate::file_include::*;
use tsgo_tspath::Path;

fn p(s: &str) -> Path {
    Path(s.to_string())
}

#[test]
fn unknown_reference_roundtrip() {
    let reason = FileIncludeReason::referenced_file(
        FileIncludeKind::TypeReferenceDirective,
        ReferencedFileData {
            file: p("/src/main.ts"),
            index: 0,
            synthetic_text: None,
        },
    );
    let diag = ProcessingDiagnostic::unknown_reference(reason);
    assert_eq!(diag.kind, ProcessingDiagnosticKind::UnknownReference);
    assert!(diag.as_file_include_reason().is_some());
    assert!(diag.as_include_explaining().is_none());
}

#[test]
fn explaining_file_include_roundtrip() {
    let explaining = IncludeExplainingDiagnostic {
        file: p("/src/helper.ts"),
        diagnostic_reason: None,
        message: &tsgo_diagnostics::THE_FILE_IS_IN_THE_PROGRAM_BECAUSE_COLON,
        args: vec!["helper.ts".to_string()],
    };
    let diag = ProcessingDiagnostic::explaining_file_include(explaining);
    assert_eq!(diag.kind, ProcessingDiagnosticKind::ExplainingFileInclude);
    let expl = diag.as_include_explaining().unwrap();
    assert_eq!(expl.file, p("/src/helper.ts"));
    assert!(diag.as_file_include_reason().is_none());
}

#[test]
fn explaining_with_reason() {
    let reason = FileIncludeReason::root_file(5);
    let explaining = IncludeExplainingDiagnostic {
        file: p("/app/index.ts"),
        diagnostic_reason: Some(Box::new(reason)),
        message: &tsgo_diagnostics::THE_FILE_IS_IN_THE_PROGRAM_BECAUSE_COLON,
        args: vec![],
    };
    let diag = ProcessingDiagnostic::explaining_file_include(explaining);
    let inner = diag
        .as_include_explaining()
        .unwrap()
        .diagnostic_reason
        .as_ref()
        .unwrap();
    assert_eq!(inner.kind, FileIncludeKind::RootFile);
}
