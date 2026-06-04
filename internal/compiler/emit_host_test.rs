use super::*;
use std::sync::Arc;
use tsgo_core::compileroptions::{CompilerOptions, ModuleKind};
use tsgo_tspath::Path;

struct StubEmitHost {
    options: CompilerOptions,
    cwd: String,
    common: String,
}

impl EmitHost for StubEmitHost {
    fn options(&self) -> &CompilerOptions {
        &self.options
    }
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }
    fn get_current_directory(&self) -> &str {
        &self.cwd
    }
    fn common_source_directory(&self) -> &str {
        &self.common
    }
    fn is_emit_blocked(&self, _file: &str) -> bool {
        false
    }
    fn file_exists(&self, _path: &str) -> bool {
        true
    }
    fn write_file(&self, _file_name: &str, _text: &str) -> std::io::Result<()> {
        Ok(())
    }
    fn get_emit_module_format_of_file(&self, _path: &Path) -> ModuleKind {
        ModuleKind::EsNext
    }
}

#[test]
fn emit_host_is_object_safe_and_send_sync() {
    let host = StubEmitHost {
        options: CompilerOptions::default(),
        cwd: "/project".into(),
        common: "/project/src".into(),
    };
    let arc: Arc<dyn EmitHost> = Arc::new(host);
    assert_eq!(arc.get_current_directory(), "/project");
    assert_eq!(arc.common_source_directory(), "/project/src");
    assert!(arc.use_case_sensitive_file_names());
    assert!(!arc.is_emit_blocked("/a.ts"));
}

#[test]
fn emit_host_write_file() {
    let host = StubEmitHost {
        options: CompilerOptions::default(),
        cwd: "/p".into(),
        common: "/p/src".into(),
    };
    assert!(host.write_file("/p/out/a.js", "console.log(1);").is_ok());
}
