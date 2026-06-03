// Go: internal/project/project_stringer_generated.go + project.go
use super::*;

#[test]
fn kind_display_inferred() {
    // Go: internal/project/project_stringer_generated.go:String (KindInferred)
    assert_eq!(Kind::Inferred.to_string(), "Inferred");
}

#[test]
fn kind_display_configured() {
    // Go: internal/project/project_stringer_generated.go:String (KindConfigured)
    assert_eq!(Kind::Configured.to_string(), "Configured");
}

#[test]
fn program_update_kind_default_is_none() {
    // Go: internal/project/project.go:ProgramUpdateKind (zero value)
    assert_eq!(ProgramUpdateKind::None as i32, 0);
}

#[test]
fn program_update_kind_values() {
    // Go: internal/project/project.go:ProgramUpdateKind iota
    assert_eq!(ProgramUpdateKind::None as i32, 0);
    assert_eq!(ProgramUpdateKind::Cloned as i32, 1);
    assert_eq!(ProgramUpdateKind::SameFileNames as i32, 2);
    assert_eq!(ProgramUpdateKind::NewFiles as i32, 3);
}

#[test]
fn pending_reload_values() {
    // Go: internal/project/project.go:PendingReload iota
    assert_eq!(PendingReload::None as i32, 0);
    assert_eq!(PendingReload::FileNames as i32, 1);
    assert_eq!(PendingReload::Full as i32, 2);
}
