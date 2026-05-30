//! Shared test doubles mirroring `specifiers_test.go`'s
//! `mockModuleSpecifierGenerationHost`.

use tsgo_core::compileroptions::{ResolutionMode, RESOLUTION_MODE_NONE};
use tsgo_packagejson::InfoCacheEntry;
use tsgo_symlinks::KnownSymlinks;
use tsgo_tspath::{to_path, Path};

use crate::types::{
    HasFileName, ModuleSpecifierGenerationHost, SourceFileForSpecifierGeneration,
    SourceOutputAndProjectReference,
};

/// Test host mirroring Go's `mockModuleSpecifierGenerationHost`: every method
/// returns an empty/default value except the ones the tests configure.
pub(crate) struct MockHost {
    pub current_dir: String,
    pub use_case_sensitive_file_names: bool,
    pub symlink_cache: Option<KnownSymlinks>,
    pub file_exists: bool,
    pub default_resolution_mode: ResolutionMode,
}

impl Default for MockHost {
    fn default() -> Self {
        MockHost {
            current_dir: String::new(),
            use_case_sensitive_file_names: false,
            symlink_cache: None,
            file_exists: true,
            default_resolution_mode: RESOLUTION_MODE_NONE,
        }
    }
}

impl MockHost {
    /// Builds the host used by `TestGetEachFileNameOfModule`: cwd `/project`,
    /// case-sensitive, with an empty symlink cache.
    pub(crate) fn project() -> Self {
        MockHost {
            current_dir: "/project".to_string(),
            use_case_sensitive_file_names: true,
            symlink_cache: Some(tsgo_symlinks::new_known_symlink("/project", true)),
            ..Default::default()
        }
    }
}

impl ModuleSpecifierGenerationHost for MockHost {
    fn get_symlink_cache(&self) -> Option<&KnownSymlinks> {
        self.symlink_cache.as_ref()
    }
    fn common_source_directory(&self) -> String {
        self.current_dir.clone()
    }
    fn get_global_typings_cache_location(&self) -> String {
        String::new()
    }
    fn use_case_sensitive_file_names(&self) -> bool {
        self.use_case_sensitive_file_names
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
        self.file_exists
    }
    fn get_nearest_ancestor_directory_with_package_json(&self, _dirname: &str) -> String {
        String::new()
    }
    fn get_package_json_info(&self, _pkg_json_path: &str) -> Option<&InfoCacheEntry> {
        None
    }
    fn get_default_resolution_mode_for_file(&self, _file: &dyn HasFileName) -> ResolutionMode {
        self.default_resolution_mode
    }
}

/// A configurable [`SourceFileForSpecifierGeneration`] for preference tests.
pub(crate) struct MockSourceFile {
    pub file_name: String,
    pub use_case_sensitive_file_names: bool,
    pub imports: Vec<String>,
    pub is_js: bool,
}

impl Default for MockSourceFile {
    fn default() -> Self {
        MockSourceFile {
            file_name: "/project/src/main.ts".to_string(),
            use_case_sensitive_file_names: true,
            imports: Vec::new(),
            is_js: false,
        }
    }
}

impl HasFileName for MockSourceFile {
    fn file_name(&self) -> String {
        self.file_name.clone()
    }
    fn path(&self) -> Path {
        to_path(&self.file_name, "/", self.use_case_sensitive_file_names)
    }
}

impl SourceFileForSpecifierGeneration for MockSourceFile {
    fn imports(&self) -> Vec<String> {
        self.imports.clone()
    }
    fn is_js(&self) -> bool {
        self.is_js
    }
}
