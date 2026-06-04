use super::*;
use tsgo_tspath::Path;

fn p(s: &str) -> Path {
    Path(s.to_string())
}

#[test]
fn kind_is_referenced_file_variants() {
    assert!(FileIncludeKind::Import.is_referenced_file());
    assert!(FileIncludeKind::ReferenceFile.is_referenced_file());
    assert!(FileIncludeKind::TypeReferenceDirective.is_referenced_file());
    assert!(FileIncludeKind::LibReferenceDirective.is_referenced_file());
    assert!(!FileIncludeKind::RootFile.is_referenced_file());
    assert!(!FileIncludeKind::LibFile.is_referenced_file());
    assert!(!FileIncludeKind::AutomaticTypeDirectiveFile.is_referenced_file());
}

#[test]
fn package_id_display() {
    let pid = PackageId {
        name: "foo".into(),
        sub_module_name: String::new(),
        version: "1.0.0".into(),
    };
    assert_eq!(pid.to_string(), "foo@1.0.0");
    assert!(!pid.is_empty());

    let pid2 = PackageId {
        name: "bar".into(),
        sub_module_name: "utils".into(),
        version: "2.0.0".into(),
    };
    assert_eq!(pid2.to_string(), "bar@2.0.0/utils");
    assert!(PackageId::default().is_empty());
}

#[test]
fn root_file_reason() {
    let r = FileIncludeReason::root_file(42);
    assert_eq!(r.kind, FileIncludeKind::RootFile);
    assert_eq!(r.as_index(), Some(42));
    assert!(!r.is_referenced_file());
}

#[test]
fn lib_file_reason_with_index() {
    let r = FileIncludeReason::lib_file(3);
    assert_eq!(r.as_lib_file_index(), Some(3));
}

#[test]
fn default_lib_has_no_index() {
    let r = FileIncludeReason::default_lib();
    assert!(r.as_lib_file_index().is_none());
    assert!(r.as_index().is_none());
}

#[test]
fn referenced_file_import() {
    let data = ReferencedFileData {
        file: p("/src/a.ts"),
        index: 0,
        synthetic_text: None,
    };
    let r = FileIncludeReason::referenced_file(FileIncludeKind::Import, data);
    assert!(r.is_referenced_file());
    let rf = r.as_referenced_file_data().unwrap();
    assert_eq!(rf.file, p("/src/a.ts"));
    assert_eq!(rf.index, 0);
}

#[test]
fn automatic_type_directive() {
    let data = AutomaticTypeDirectiveData {
        type_reference: "node".into(),
        package_id: Some(PackageId {
            name: "@types/node".into(),
            sub_module_name: String::new(),
            version: "18.0.0".into(),
        }),
    };
    let r = FileIncludeReason::automatic_type_directive(data);
    assert_eq!(r.kind, FileIncludeKind::AutomaticTypeDirectiveFile);
    let atd = r.as_automatic_type_directive_data().unwrap();
    assert_eq!(atd.type_reference, "node");
}
