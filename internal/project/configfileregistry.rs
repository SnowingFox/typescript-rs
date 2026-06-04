//! Config file registry — tsconfig parsing/caching/extends chain.
//!
//! 1:1 port of Go `internal/project/configfileregistry.go` and
//! `internal/project/configfileregistrybuilder.go`.
//!
//! The [`ConfigFileRegistry`] caches parsed tsconfig files and their extends
//! chains, tracking which projects and open files retain each entry.
//! [`ConfigFileRegistryBuilder`] is the mutable builder that produces a
//! new registry snapshot.

use std::collections::HashMap;

use tsgo_tspath::Path;

/// Cached information about a file's nearest ancestor config file names.
// Go: internal/project/configfileregistry.go:configFileNames
#[derive(Debug, Clone)]
pub struct ConfigFileNames {
    /// File name of the nearest ancestor config file.
    pub nearest_config_file_name: String,
    /// Map from one ancestor config path to the next higher one.
    pub ancestors: HashMap<String, String>,
}

impl ConfigFileNames {
    /// Creates a deep clone.
    // Go: internal/project/configfileregistry.go:configFileNames.Clone
    pub fn deep_clone(&self) -> Self {
        Self {
            nearest_config_file_name: self.nearest_config_file_name.clone(),
            ancestors: self.ancestors.clone(),
        }
    }
}

/// Pending reload state for a config file entry.
// Go: internal/project/project.go:PendingReload (re-exported from kind.rs)
pub use crate::kind::PendingReload;

/// A cached config file entry in the registry.
// Go: internal/project/configfileregistry.go:configFileEntry
#[derive(Debug, Clone)]
pub struct ConfigFileEntry {
    /// The absolute file name of the config.
    pub file_name: String,
    /// Whether this entry needs to be reloaded.
    pub pending_reload: PendingReload,
    // DEFER(phase-8): commandLine: *tsoptions.ParsedCommandLine
    /// Set of project paths that have acquired this config.
    pub retaining_projects: HashMap<Path, ()>,
    /// Set of open file paths that caused this config to load.
    pub retaining_open_files: HashMap<Path, ()>,
    /// Set of config file paths that extend this config.
    pub retaining_configs: HashMap<Path, ()>,
}

impl ConfigFileEntry {
    /// Creates a deep clone.
    // Go: internal/project/configfileregistry.go:configFileEntry.Clone
    pub fn deep_clone(&self) -> Self {
        Self {
            file_name: self.file_name.clone(),
            pending_reload: self.pending_reload,
            retaining_projects: self.retaining_projects.clone(),
            retaining_open_files: self.retaining_open_files.clone(),
            retaining_configs: self.retaining_configs.clone(),
        }
    }
}

/// The config file registry caches parsed tsconfig files and tracks
/// which projects and open files retain each entry.
// Go: internal/project/configfileregistry.go:ConfigFileRegistry
#[derive(Debug, Clone)]
pub struct ConfigFileRegistry {
    /// Map of config file paths to their entries.
    configs: HashMap<Path, ConfigFileEntry>,
    /// Map of open file paths to their ancestor config file names.
    config_file_names: HashMap<Path, ConfigFileNames>,
    /// Custom config file name preference.
    pub(crate) custom_config_file_name: String,
}

impl ConfigFileRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
            config_file_names: HashMap::new(),
            custom_config_file_name: String::new(),
        }
    }

    /// Returns the config file name for the given path, if cached.
    // Go: internal/project/configfileregistry.go:ConfigFileRegistry.GetConfigFileName
    pub fn get_config_file_name(&self, path: &Path) -> &str {
        self.config_file_names
            .get(path)
            .map(|e| e.nearest_config_file_name.as_str())
            .unwrap_or("")
    }

    /// Returns the ancestor config file name for the given path,
    /// starting from `higher_than_config`.
    // Go: internal/project/configfileregistry.go:ConfigFileRegistry.GetAncestorConfigFileName
    pub fn get_ancestor_config_file_name(&self, path: &Path, higher_than_config: &str) -> &str {
        self.config_file_names
            .get(path)
            .and_then(|e| e.ancestors.get(higher_than_config))
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// Creates a shallow clone of this registry.
    // Go: internal/project/configfileregistry.go:ConfigFileRegistry.clone
    pub fn shallow_clone(&self) -> Self {
        Self {
            configs: self.configs.clone(),
            config_file_names: self.config_file_names.clone(),
            custom_config_file_name: self.custom_config_file_name.clone(),
        }
    }

    /// Returns the entry for a given config path.
    pub fn get_entry(&self, path: &Path) -> Option<&ConfigFileEntry> {
        self.configs.get(path)
    }

    /// Iterates over all config entries (for testing).
    // Go: internal/project/configfileregistry.go:ConfigFileRegistry.ForEachTestConfigEntry
    pub fn for_each_config_entry<F: FnMut(&Path, &ConfigFileEntry)>(&self, mut cb: F) {
        for (path, entry) in &self.configs {
            cb(path, entry);
        }
    }

    /// Iterates over all config file name entries (for testing).
    // Go: internal/project/configfileregistry.go:ConfigFileRegistry.ForEachTestConfigFileNamesEntry
    pub fn for_each_config_file_names_entry<F: FnMut(&Path, &ConfigFileNames)>(&self, mut cb: F) {
        for (path, entry) in &self.config_file_names {
            cb(path, entry);
        }
    }

    /// Gets a test view of a config entry by path.
    // Go: internal/project/configfileregistry.go:ConfigFileRegistry.GetTestConfigEntry
    pub fn get_test_config_entry(&self, path: &Path) -> Option<TestConfigEntry> {
        self.configs.get(path).map(|entry| TestConfigEntry {
            file_name: entry.file_name.clone(),
            retaining_projects: entry.retaining_projects.keys().cloned().collect(),
            retaining_open_files: entry.retaining_open_files.keys().cloned().collect(),
            retaining_configs: entry.retaining_configs.keys().cloned().collect(),
        })
    }

    /// Gets a test view of a config-file-names entry by path.
    // Go: internal/project/configfileregistry.go:ConfigFileRegistry.GetTestConfigFileNamesEntry
    pub fn get_test_config_file_names_entry(
        &self,
        path: &Path,
    ) -> Option<TestConfigFileNamesEntry> {
        self.config_file_names
            .get(path)
            .map(|entry| TestConfigFileNamesEntry {
                nearest_config_file_name: entry.nearest_config_file_name.clone(),
                ancestors: entry.ancestors.clone(),
            })
    }

    /// Registers a config file for a project. If the entry already exists,
    /// adds the project to the retaining set (incrementing refcount).
    /// If it doesn't exist, creates a new entry with PendingReload::Full.
    // Go: configFileRegistryBuilder.acquireConfigForProject (simplified)
    pub fn register_config(&mut self, config_path: &Path, file_name: &str, project_path: &Path) {
        let entry = self
            .configs
            .entry(config_path.clone())
            .or_insert_with(|| ConfigFileEntry {
                file_name: file_name.to_string(),
                pending_reload: PendingReload::Full,
                retaining_projects: HashMap::new(),
                retaining_open_files: HashMap::new(),
                retaining_configs: HashMap::new(),
            });
        entry.retaining_projects.insert(project_path.clone(), ());
    }

    /// Unregisters a project from a config file. Removes the project from
    /// the retaining set; if no retainers remain, removes the entry entirely.
    // Go: configFileRegistryBuilder.releaseConfigForProject (simplified)
    pub fn unregister_config(&mut self, config_path: &Path, project_path: &Path) {
        let should_remove = if let Some(entry) = self.configs.get_mut(config_path) {
            entry.retaining_projects.remove(project_path);
            entry.retaining_projects.is_empty()
                && entry.retaining_open_files.is_empty()
                && entry.retaining_configs.is_empty()
        } else {
            false
        };
        if should_remove {
            self.configs.remove(config_path);
        }
    }

    /// Marks a config entry for full reload (simulating file-system change).
    // Go: configFileRegistryBuilder.invalidateCache / DidChangeFiles
    pub fn update_config(&mut self, config_path: &Path) -> bool {
        if let Some(entry) = self.configs.get_mut(config_path) {
            entry.pending_reload = PendingReload::Full;
            true
        } else {
            false
        }
    }

    /// Inserts or replaces a config entry (used by builder).
    #[allow(dead_code)]
    pub(crate) fn set_config(&mut self, path: Path, entry: ConfigFileEntry) {
        self.configs.insert(path, entry);
    }

    /// Inserts or replaces a config file names entry (used by builder).
    #[allow(dead_code)]
    pub(crate) fn set_config_file_names(&mut self, path: Path, names: ConfigFileNames) {
        self.config_file_names.insert(path, names);
    }

    /// Removes a config entry (used by cleanup).
    #[allow(dead_code)]
    pub(crate) fn remove_config(&mut self, path: &Path) {
        self.configs.remove(path);
    }

    /// Removes a config file names entry.
    #[allow(dead_code)]
    pub(crate) fn remove_config_file_names(&mut self, path: &Path) {
        self.config_file_names.remove(path);
    }
}

impl Default for ConfigFileRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Test-only view of a config entry.
// Go: internal/project/configfileregistry.go:TestConfigEntry
#[derive(Debug, Clone)]
pub struct TestConfigEntry {
    /// The config file name.
    pub file_name: String,
    /// Paths of retaining projects.
    pub retaining_projects: Vec<Path>,
    /// Paths of retaining open files.
    pub retaining_open_files: Vec<Path>,
    /// Paths of retaining (extending) configs.
    pub retaining_configs: Vec<Path>,
}

/// Test-only view of a config file names entry.
// Go: internal/project/configfileregistry.go:TestConfigFileNamesEntry
#[derive(Debug, Clone)]
pub struct TestConfigFileNamesEntry {
    /// Nearest ancestor config file name.
    pub nearest_config_file_name: String,
    /// Map from config path to next ancestor config.
    pub ancestors: HashMap<String, String>,
}

#[cfg(test)]
#[path = "configfileregistry_test.rs"]
mod tests;
