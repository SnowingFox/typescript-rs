use super::*;
use std::sync::Arc;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

fn sys() -> VfsSystem {
    let fs: Arc<dyn Fs + Send + Sync> = Arc::new(MapFs::from_map([("/p/a.ts", "")], true));
    VfsSystem::new(fs, "/p", "/lib")
}

#[test]
fn write_accumulates_into_output() {
    let s = sys();
    s.write("hello ");
    s.write("world");
    assert_eq!(s.output(), "hello world");
}

#[test]
fn defaults_are_non_tty_and_empty_env() {
    let s = sys();
    assert!(!s.write_output_is_tty());
    assert_eq!(s.get_environment_variable("NO_COLOR"), "");
    assert_eq!(s.get_current_directory(), "/p");
    assert_eq!(s.default_library_path(), "/lib");
}

#[test]
fn environment_and_tty_are_configurable() {
    let mut s = sys();
    s.set_environment_variable("NO_COLOR", "1");
    s.set_write_output_is_tty(true);
    assert_eq!(s.get_environment_variable("NO_COLOR"), "1");
    assert!(s.write_output_is_tty());
}
