use super::*;
use crate::export::{Export, ExportId, ModuleId};
use tsgo_core::compileroptions::{
    CompilerOptions, ModuleResolutionKind, ResolutionMode, RESOLUTION_MODE_NONE,
};
use tsgo_modulespecifiers::{
    HasFileName, ModuleSpecifierGenerationHost, SourceFileForSpecifierGeneration,
    SourceOutputAndProjectReference,
};
use tsgo_packagejson::InfoCacheEntry;
use tsgo_symlinks::{new_known_symlink, KnownSymlinks};
use tsgo_tspath::{to_path, Path};

/// Minimal host (mirrors `modulespecifiers`'s `MockHost`): empty everything
/// except cwd / case-sensitivity / an empty symlink cache.
struct TestHost {
    current_dir: String,
    symlink_cache: KnownSymlinks,
}

impl TestHost {
    fn new(current_dir: &str) -> TestHost {
        TestHost {
            current_dir: current_dir.to_string(),
            symlink_cache: new_known_symlink(current_dir, true),
        }
    }
}

impl ModuleSpecifierGenerationHost for TestHost {
    fn get_symlink_cache(&self) -> Option<&KnownSymlinks> {
        Some(&self.symlink_cache)
    }
    fn common_source_directory(&self) -> String {
        self.current_dir.clone()
    }
    fn get_global_typings_cache_location(&self) -> String {
        String::new()
    }
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }
    fn get_current_directory(&self) -> String {
        self.current_dir.clone()
    }
    fn get_project_reference_from_source(
        &self,
        _path: &Path,
    ) -> Option<SourceOutputAndProjectReference> {
        None
    }
    fn get_redirect_targets(&self, _path: &Path) -> Vec<String> {
        Vec::new()
    }
    fn get_source_of_project_reference_if_output_included(&self, file: &dyn HasFileName) -> String {
        file.file_name()
    }
    fn file_exists(&self, _path: &str) -> bool {
        true
    }
    fn get_nearest_ancestor_directory_with_package_json(&self, _dirname: &str) -> String {
        String::new()
    }
    fn get_package_json_info(&self, _pkg_json_path: &str) -> Option<&InfoCacheEntry> {
        None
    }
    fn get_default_resolution_mode_for_file(&self, _file: &dyn HasFileName) -> ResolutionMode {
        RESOLUTION_MODE_NONE
    }
}

/// The importing source file.
struct TestSourceFile {
    file_name: String,
}

impl HasFileName for TestSourceFile {
    fn file_name(&self) -> String {
        self.file_name.clone()
    }
    fn path(&self) -> Path {
        to_path(&self.file_name, "/", true)
    }
}

impl SourceFileForSpecifierGeneration for TestSourceFile {
    fn imports(&self) -> Vec<String> {
        Vec::new()
    }
    fn is_js(&self) -> bool {
        false
    }
}

fn bundler_opts() -> CompilerOptions {
    CompilerOptions {
        module_resolution: ModuleResolutionKind::Bundler,
        ..Default::default()
    }
}

fn export_in(module_file_name: &str) -> Export {
    Export {
        id: ExportId {
            module_id: ModuleId::new(to_path(module_file_name, "/", true).0),
            export_name: "b".to_string(),
        },
        module_file_name: module_file_name.to_string(),
        ..Default::default()
    }
}

// Go: internal/ls/autoimport/specifiers.go:View.GetModuleSpecifier (relative)
// Task TDD slice 4: /src/a.ts importing from /src/lib/b.ts -> "./lib/b".
#[test]
fn relative_specifier_for_sibling_directory() {
    let host = TestHost::new("/");
    let importing = TestSourceFile {
        file_name: "/src/a.ts".to_string(),
    };
    let export = export_in("/src/lib/b.ts");
    let (specifier, kind) = get_module_specifier(
        &export,
        &importing,
        &bundler_opts(),
        &host,
        &UserPreferences::default(),
    );
    assert_eq!(specifier, "./lib/b");
    assert_eq!(kind, ResultKind::Relative);
}

// Importing from a parent directory yields a `../`-relative specifier.
#[test]
fn relative_specifier_for_parent_directory() {
    let host = TestHost::new("/");
    let importing = TestSourceFile {
        file_name: "/src/sub/a.ts".to_string(),
    };
    let export = export_in("/src/lib/b.ts");
    let (specifier, kind) = get_module_specifier(
        &export,
        &importing,
        &bundler_opts(),
        &host,
        &UserPreferences::default(),
    );
    assert_eq!(specifier, "../lib/b");
    assert_eq!(kind, ResultKind::Relative);
}

// Go: internal/ls/autoimport/specifiers.go:View.GetModuleSpecifier (ambient module)
#[test]
fn bare_module_id_returned_as_ambient() {
    let host = TestHost::new("/");
    let importing = TestSourceFile {
        file_name: "/src/a.ts".to_string(),
    };
    // A bare module id (`react`) is an ambient module specifier.
    let export = Export {
        id: ExportId {
            module_id: ModuleId::new("react"),
            export_name: "useState".to_string(),
        },
        module_file_name: "/node_modules/react/index.d.ts".to_string(),
        ..Default::default()
    };
    let (specifier, kind) = get_module_specifier(
        &export,
        &importing,
        &bundler_opts(),
        &host,
        &UserPreferences::default(),
    );
    assert_eq!(specifier, "react");
    assert_eq!(kind, ResultKind::Ambient);
}

// A bare module id excluded by the user's regexes yields no specifier.
#[test]
fn excluded_ambient_returns_none() {
    let host = TestHost::new("/");
    let importing = TestSourceFile {
        file_name: "/src/a.ts".to_string(),
    };
    let export = Export {
        id: ExportId {
            module_id: ModuleId::new("react"),
            export_name: "useState".to_string(),
        },
        ..Default::default()
    };
    let prefs = UserPreferences {
        auto_import_specifier_exclude_regexes: vec!["^react$".to_string()],
        ..Default::default()
    };
    let (specifier, kind) =
        get_module_specifier(&export, &importing, &bundler_opts(), &host, &prefs);
    assert_eq!(specifier, "");
    assert_eq!(kind, ResultKind::None);
}
