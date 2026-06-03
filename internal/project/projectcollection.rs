//! Project collection — manages the set of configured/inferred projects.
//!
//! 1:1 port of Go `internal/project/projectcollection.go`.
//!
//! [`ProjectCollection`] holds configured projects (keyed by their config
//! file path), the inferred project (if any), and file-to-project mappings.

use std::collections::HashMap;

use tsgo_tspath::Path;

use crate::configfileregistry::ConfigFileRegistry;
use crate::project::{Project, INFERRED_PROJECT_NAME};

/// The collection of configured and inferred projects for a snapshot.
///
/// # Examples
/// ```
/// use tsgo_project::projectcollection::ProjectCollection;
/// use tsgo_project::configfileregistry::ConfigFileRegistry;
/// let pc = ProjectCollection::new(
///     Box::new(|s: &str| tsgo_tspath::Path(s.to_string())),
///     ConfigFileRegistry::new(),
/// );
/// assert!(pc.inferred_project().is_none());
/// assert!(pc.configured_projects().is_empty());
/// ```
// Go: internal/project/projectcollection.go:ProjectCollection
pub struct ProjectCollection {
    #[allow(dead_code)]
    to_path: Box<dyn Fn(&str) -> Path + Send + Sync>,
    config_file_registry: ConfigFileRegistry,
    file_default_projects: HashMap<Path, Path>,
    configured_projects: HashMap<Path, Project>,
    inferred_project: Option<Project>,
}

impl ProjectCollection {
    /// Creates a new empty project collection.
    pub fn new(
        to_path: Box<dyn Fn(&str) -> Path + Send + Sync>,
        config_file_registry: ConfigFileRegistry,
    ) -> Self {
        Self {
            to_path,
            config_file_registry,
            file_default_projects: HashMap::new(),
            configured_projects: HashMap::new(),
            inferred_project: None,
        }
    }

    /// Returns a reference to the config file registry.
    // Go: internal/project/projectcollection.go:ProjectCollection.ConfigFileRegistry
    pub fn config_file_registry(&self) -> &ConfigFileRegistry {
        &self.config_file_registry
    }

    /// Returns a configured project by its config file path.
    // Go: internal/project/projectcollection.go:ProjectCollection.ConfiguredProject
    pub fn configured_project(&self, path: &Path) -> Option<&Project> {
        self.configured_projects.get(path)
    }

    /// Returns a project by its path (checks configured first, then inferred).
    // Go: internal/project/projectcollection.go:ProjectCollection.GetProjectByPath
    pub fn get_project_by_path(&self, project_path: &Path) -> Option<&Project> {
        if let Some(p) = self.configured_projects.get(project_path) {
            return Some(p);
        }
        if project_path.0 == INFERRED_PROJECT_NAME {
            return self.inferred_project.as_ref();
        }
        None
    }

    /// Returns all configured projects in a stable (sorted by name) order.
    // Go: internal/project/projectcollection.go:ProjectCollection.ConfiguredProjects
    pub fn configured_projects(&self) -> Vec<&Project> {
        let mut projects: Vec<&Project> = self.configured_projects.values().collect();
        projects.sort_by(|a, b| a.name().cmp(b.name()));
        projects
    }

    /// Returns all projects including the inferred project in a stable order.
    // Go: internal/project/projectcollection.go:ProjectCollection.Projects
    pub fn projects(&self) -> Vec<&Project> {
        let mut all = self.configured_projects();
        if let Some(ref inferred) = self.inferred_project {
            all.push(inferred);
        }
        all
    }

    /// Returns the inferred project, if any.
    // Go: internal/project/projectcollection.go:ProjectCollection.InferredProject
    pub fn inferred_project(&self) -> Option<&Project> {
        self.inferred_project.as_ref()
    }

    /// Inserts or replaces a configured project.
    pub fn set_configured_project(&mut self, path: Path, project: Project) {
        self.configured_projects.insert(path, project);
    }

    /// Sets the inferred project.
    pub fn set_inferred_project(&mut self, project: Project) {
        self.inferred_project = Some(project);
    }

    /// Removes the inferred project.
    pub fn clear_inferred_project(&mut self) {
        self.inferred_project = None;
    }

    /// Removes a configured project by path.
    pub fn remove_configured_project(&mut self, path: &Path) -> Option<Project> {
        self.configured_projects.remove(path)
    }

    /// Sets the file → default-project mapping.
    pub fn set_file_default_project(&mut self, file_path: Path, project_path: Path) {
        self.file_default_projects.insert(file_path, project_path);
    }

    /// Gets the default project for a file based on file → project mapping,
    /// falling back to scanning configured projects.
    ///
    /// DEFER(phase-8): full `containsFile` logic requires loaded `Program`.
    // Go: internal/project/projectcollection.go:ProjectCollection.GetDefaultProject
    pub fn get_default_project(&self, path: &Path) -> Option<&Project> {
        if let Some(project_path) = self.file_default_projects.get(path) {
            if project_path.0 == INFERRED_PROJECT_NAME {
                return self.inferred_project.as_ref();
            }
            return self.configured_projects.get(project_path);
        }
        None
    }

    /// Creates a shallow clone of the project collection.
    // Go: internal/project/projectcollection.go:ProjectCollection.clone
    pub fn shallow_clone(&self) -> Self
    where
        Self: Sized,
    {
        // We can't clone `to_path` (it's a Box<dyn Fn>), so we create a
        // trivial identity closure. The real clone needs the same `to_path`
        // passed in from the session.
        Self {
            to_path: Box::new(|s: &str| Path(s.to_string())),
            config_file_registry: self.config_file_registry.shallow_clone(),
            file_default_projects: self.file_default_projects.clone(),
            configured_projects: self.configured_projects.clone(),
            inferred_project: self.inferred_project.clone(),
        }
    }
}

/// Finds the default configured project from program inclusion.
///
/// The projects should be sorted, as ties are broken by slice order.
/// Returns the project path and whether there were multiple direct inclusions.
///
/// DEFER(phase-8): full implementation requires `Program.containsFile` and
/// `IsSourceFromProjectReference`.
// Go: internal/project/projectcollection.go:findDefaultConfiguredProjectFromProgramInclusion
pub fn find_default_configured_project_from_program_inclusion(
    _file_name: &str,
    _path: &Path,
    project_paths: &[Path],
    _get_project: &dyn Fn(&Path) -> Option<&Project>,
) -> (Option<Path>, bool) {
    if project_paths.is_empty() {
        return (None, false);
    }
    // Stub: return first project path. Full logic needs containsFile.
    (Some(project_paths[0].clone()), false)
}

#[cfg(test)]
#[path = "projectcollection_test.rs"]
mod tests;
