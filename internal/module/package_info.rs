//! `package.json` info loading and caching, package-id derivation, peer
//! dependency reading, and field validation.
//!
//! Split from Go `internal/module/resolver.go` (PORTING.md §2).
//!
//! # Race semantics (TS issue #3526 / PR #50740)
//! [`ResolutionState::get_package_json_info`] mirrors Go: a cache-miss store
//! returns the *winner's* stored directory (via `LoadOrStore`), while a cache
//! hit reports the *requested* directory. Combined with the candidate
//! normalization in
//! [`load_module_from_specific_node_modules_directory`](ResolutionState::load_module_from_specific_node_modules_directory),
//! `pkg` and `pkg/` produce identical directories so the
//! `loadNodeModuleFromDirectoryWorker` `ComparePaths` guard still matches.

use std::sync::Arc;

use tsgo_packagejson::{Expected, InfoCacheEntry, PackageJson, TypeValidatedField};
use tsgo_tspath as tspath;

use tsgo_diagnostics::{
    EXPECTED_TYPE_OF_0_FIELD_IN_PACKAGE_JSON_TO_BE_1_GOT_2, FAILED_TO_FIND_PEERDEPENDENCY_0,
    FILE_0_DOES_NOT_EXIST, FILE_0_DOES_NOT_EXIST_ACCORDING_TO_EARLIER_CACHED_LOOKUPS,
    FILE_0_EXISTS_ACCORDING_TO_EARLIER_CACHED_LOOKUPS, FOUND_PACKAGE_JSON_AT_0,
    FOUND_PEERDEPENDENCY_0_WITH_1_VERSION, X_PACKAGE_JSON_DOES_NOT_HAVE_A_0_FIELD,
    X_PACKAGE_JSON_HAD_A_FALSY_0_FIELD, X_PACKAGE_JSON_HAS_0_FIELD_1_THAT_REFERENCES_2,
    X_PACKAGE_JSON_HAS_A_PEERDEPENDENCIES_FIELD,
};

use crate::state::{PackageJsonInfo, ResolutionState};
use crate::{write_trace, PackageId};

impl ResolutionState<'_> {
    // Go: internal/module/resolver.go:resolutionState.getPackageJsonInfo
    pub(crate) fn get_package_json_info(
        &mut self,
        package_directory: &str,
    ) -> Option<PackageJsonInfo> {
        let package_json_path = tspath::combine_paths(package_directory, &["package.json"]);

        if let Some(existing) = self
            .resolver
            .caches
            .package_json_info_cache
            .get(&package_json_path)
        {
            if existing.contents.is_some() {
                write_trace(
                    &mut self.tracer,
                    &FILE_0_EXISTS_ACCORDING_TO_EARLIER_CACHED_LOOKUPS,
                    &[&package_json_path],
                );
                // Both branches report the requested directory: an exact match
                // returns the entry as-is, and a mismatch is TS PR #50740 where
                // the same canonical `package.json` is reached via a different
                // spelling.
                return Some(PackageJsonInfo::new(
                    existing,
                    package_directory.to_string(),
                ));
            }
            if existing.directory_exists {
                write_trace(
                    &mut self.tracer,
                    &FILE_0_DOES_NOT_EXIST_ACCORDING_TO_EARLIER_CACHED_LOOKUPS,
                    &[&package_json_path],
                );
            }
            return None;
        }

        let directory_exists = self.resolver.host.fs().directory_exists(package_directory);
        if directory_exists && self.resolver.host.fs().file_exists(&package_json_path) {
            let contents = self
                .resolver
                .host
                .fs()
                .read_file(&package_json_path)
                .unwrap_or_default();
            let (fields, parseable) = match tsgo_packagejson::parse(contents.as_bytes()) {
                Ok(fields) => (fields, true),
                Err(_) => (Default::default(), false),
            };
            write_trace(
                &mut self.tracer,
                &FOUND_PACKAGE_JSON_AT_0,
                &[&package_json_path],
            );
            let entry = Arc::new(InfoCacheEntry {
                package_directory: package_directory.to_string(),
                directory_exists: true,
                contents: Some(PackageJson::new(fields, parseable)),
            });
            let stored = self
                .resolver
                .caches
                .package_json_info_cache
                .set(&package_json_path, entry);
            // The winner's stored directory wins the race (Go `LoadOrStore`).
            let dir = stored.package_directory.clone();
            return Some(PackageJsonInfo::new(stored, dir));
        }

        if directory_exists {
            write_trace(
                &mut self.tracer,
                &FILE_0_DOES_NOT_EXIST,
                &[&package_json_path],
            );
        }
        let entry = Arc::new(InfoCacheEntry {
            package_directory: package_directory.to_string(),
            directory_exists,
            contents: None,
        });
        let _ = self
            .resolver
            .caches
            .package_json_info_cache
            .set(&package_json_path, entry);
        None
    }

    // Go: internal/module/resolver.go:resolutionState.getPackageId
    pub(crate) fn get_package_id(
        &mut self,
        resolved_file_name: &str,
        package_info: Option<&PackageJsonInfo>,
    ) -> PackageId {
        if let Some(info) = package_info {
            if info.exists() {
                let contents = info.contents();
                let (name, name_ok) = contents.fields().header.name.get_value();
                if name_ok {
                    let (version, version_ok) = contents.fields().header.version.get_value();
                    if version_ok {
                        let name = name.clone();
                        let version = version.clone();
                        let mut sub_module_name = String::new();
                        if resolved_file_name.len() > info.package_directory().len() {
                            sub_module_name = resolved_file_name
                                [info.package_directory().len() + 1..]
                                .to_string();
                        }
                        let peer_dependencies = self.read_package_json_peer_dependencies(info);
                        return PackageId {
                            name,
                            version,
                            sub_module_name,
                            peer_dependencies,
                        };
                    }
                }
            }
        }
        PackageId::default()
    }

    // Go: internal/module/resolver.go:resolutionState.readPackageJsonPeerDependencies
    pub(crate) fn read_package_json_peer_dependencies(
        &mut self,
        package_json_info: &PackageJsonInfo,
    ) -> String {
        let contents = package_json_info.contents();
        let peer_dependencies = &contents.fields().deps.peer_dependencies;
        let ok = self.validate_package_json_field("peerDependencies", peer_dependencies);
        let (peer_value, _) = peer_dependencies.get_value();
        if !ok || peer_value.is_empty() {
            return String::new();
        }
        write_trace(
            &mut self.tracer,
            &X_PACKAGE_JSON_HAS_A_PEERDEPENDENCIES_FIELD,
            &[],
        );
        let mut names: Vec<String> = peer_value.keys().cloned().collect();
        names.sort();

        let package_directory = self.real_path(package_json_info.package_directory());
        let Some(node_modules_index) = package_directory.rfind("/node_modules") else {
            return String::new();
        };
        let node_modules = format!(
            "{}/",
            &package_directory[..node_modules_index + "/node_modules".len()]
        );

        let mut builder = String::new();
        for name in &names {
            match self.get_package_json_info(&format!("{node_modules}{name}")) {
                Some(peer) => {
                    let (version, _) = peer.contents().fields().header.version.get_value();
                    let version = version.clone();
                    builder.push('+');
                    builder.push_str(name);
                    builder.push('@');
                    builder.push_str(&version);
                    write_trace(
                        &mut self.tracer,
                        &FOUND_PEERDEPENDENCY_0_WITH_1_VERSION,
                        &[name, &version],
                    );
                }
                None => {
                    write_trace(&mut self.tracer, &FAILED_TO_FIND_PEERDEPENDENCY_0, &[name]);
                }
            }
        }
        builder
    }

    // Go: internal/module/resolver.go:resolutionState.validatePackageJSONField
    pub(crate) fn validate_package_json_field(
        &mut self,
        field_name: &str,
        field: &dyn TypeValidatedField,
    ) -> bool {
        if field.is_present() {
            if field.is_valid() {
                return true;
            }
            if self.tracer.is_some() {
                let expected = field.expected_json_type();
                let actual = field.actual_json_type().to_string();
                write_trace(
                    &mut self.tracer,
                    &EXPECTED_TYPE_OF_0_FIELD_IN_PACKAGE_JSON_TO_BE_1_GOT_2,
                    &[field_name, expected, &actual],
                );
            }
        }
        write_trace(
            &mut self.tracer,
            &X_PACKAGE_JSON_DOES_NOT_HAVE_A_0_FIELD,
            &[field_name],
        );
        false
    }

    // Go: internal/module/resolver.go:resolutionState.getPackageJSONPathField
    pub(crate) fn get_package_json_path_field(
        &mut self,
        field_name: &str,
        field: &Expected<String>,
        directory: &str,
    ) -> Option<String> {
        if !self.validate_package_json_field(field_name, field) {
            return None;
        }
        let (value, _) = field.get_value();
        if value.is_empty() {
            write_trace(
                &mut self.tracer,
                &X_PACKAGE_JSON_HAD_A_FALSY_0_FIELD,
                &[field_name],
            );
            return None;
        }
        let path = tspath::normalize_path(&tspath::combine_paths(directory, &[value.as_str()]));
        write_trace(
            &mut self.tracer,
            &X_PACKAGE_JSON_HAS_0_FIELD_1_THAT_REFERENCES_2,
            &[field_name, value, &path],
        );
        Some(path)
    }
}

#[cfg(test)]
#[path = "package_info_test.rs"]
mod tests;
