//! `tsgo_symlinks` — 1:1 Rust port of Go `internal/symlinks`.
//!
//! A bidirectional cache mapping symlink paths to their on-disk realpaths (and
//! back), at both file and directory granularity. It is fed incrementally from
//! module-resolution results and can guess directory-level symlinks by
//! comparing the common suffix of a symlink path and its realpath.
//!
//! Consumed by `modulespecifiers` (to prefer the in-project symlink path over a
//! deep `node_modules/.pnpm` realpath when generating auto-imports) and by the
//! program.

use std::sync::Arc;

use tsgo_ast::SourceFileData;
use tsgo_collections::{SyncMap, SyncSet};
use tsgo_core::compileroptions::ResolutionMode;
use tsgo_module::{ResolvedModule, ResolvedTypeReferenceDirective};
use tsgo_tspath::{
    contains_ignored_path, ensure_trailing_directory_separator, get_canonical_file_name,
    get_normalized_absolute_path, get_path_components, get_path_from_path_components, to_path,
    Path,
};

/// A resolved directory symlink target.
///
/// Both fields always carry a trailing directory separator; this is an
/// invariant maintained by [`KnownSymlinks::process_resolution`].
///
/// # Examples
/// ```
/// use tsgo_symlinks::KnownDirectoryLink;
/// use tsgo_tspath::{to_path, Path};
/// let link = KnownDirectoryLink {
///     real: "/real/path/".to_string(),
///     real_path: to_path("/real/path", "/cwd", true).ensure_trailing_directory_separator(),
/// };
/// assert_eq!(link.real, "/real/path/");
/// ```
///
/// Side effects: none (plain data).
// Go: internal/symlinks/knownsymlinks.go:KnownDirectoryLink
#[derive(Clone, Debug)]
pub struct KnownDirectoryLink {
    /// Matches the casing returned by `realpath`; used to compute the realpath
    /// of children. Always has a trailing directory separator.
    pub real: String,
    /// `to_path(real)`, stored to avoid repeated recomputation. Always has a
    /// trailing directory separator.
    pub real_path: Path,
}

/// A concurrent, bidirectional symlink ↔ realpath cache at file and directory
/// granularity.
///
/// # Examples
/// ```
/// use tsgo_symlinks::new_known_symlink;
/// let cache = new_known_symlink("/test/dir", true);
/// assert!(!cache.has_directory(tsgo_tspath::to_path("/x", "/test/dir", true)));
/// ```
///
/// Side effects: methods mutate the shared caches; see each method.
// Go: internal/symlinks/knownsymlinks.go:KnownSymlinks
pub struct KnownSymlinks {
    directories: SyncMap<Path, Option<KnownDirectoryLink>>,
    directories_by_realpath: SyncMap<Path, Arc<SyncSet<String>>>,
    files: SyncMap<Path, String>,
    files_by_realpath: SyncMap<Path, Arc<SyncSet<String>>>,
    cwd: String,
    use_case_sensitive_file_names: bool,
}

/// Creates an empty symlink cache rooted at `current_directory`.
///
/// # Examples
/// ```
/// use tsgo_symlinks::new_known_symlink;
/// let cache = new_known_symlink("/test/dir", true);
/// assert!(cache.files().is_empty());
/// ```
///
/// Side effects: allocates the four concurrent caches.
// Go: internal/symlinks/knownsymlinks.go:NewKnownSymlink
pub fn new_known_symlink(
    current_directory: &str,
    use_case_sensitive_file_names: bool,
) -> KnownSymlinks {
    KnownSymlinks {
        directories: SyncMap::default(),
        directories_by_realpath: SyncMap::default(),
        files: SyncMap::default(),
        files_by_realpath: SyncMap::default(),
        cwd: current_directory.to_string(),
        use_case_sensitive_file_names,
    }
}

impl KnownSymlinks {
    /// Reports whether a symlink directory `symlink_path` is known.
    ///
    /// The key is normalized with a trailing directory separator before lookup,
    /// so callers may pass a path with or without one.
    ///
    /// # Examples
    /// ```
    /// use tsgo_symlinks::new_known_symlink;
    /// use tsgo_tspath::to_path;
    /// let cache = new_known_symlink("/test/dir", true);
    /// assert!(!cache.has_directory(to_path("/test/symlink", "/test/dir", true)));
    /// ```
    ///
    /// Side effects: none (pure read).
    // Go: internal/symlinks/knownsymlinks.go:HasDirectory
    pub fn has_directory(&self, symlink_path: Path) -> bool {
        self.directories
            .load(&symlink_path.ensure_trailing_directory_separator())
            .1
    }

    /// Returns the symlink-directory → realpath map. Keys carry a trailing
    /// directory separator.
    ///
    /// # Examples
    /// ```
    /// use tsgo_symlinks::new_known_symlink;
    /// let cache = new_known_symlink("/test/dir", true);
    /// assert!(cache.directories().is_empty());
    /// ```
    ///
    /// Side effects: none (returns a shared reference).
    // Go: internal/symlinks/knownsymlinks.go:Directories
    pub fn directories(&self) -> &SyncMap<Path, Option<KnownDirectoryLink>> {
        &self.directories
    }

    /// Returns the realpath → {symlink directories} reverse map.
    ///
    /// # Examples
    /// ```
    /// use tsgo_symlinks::new_known_symlink;
    /// let cache = new_known_symlink("/test/dir", true);
    /// assert!(cache.directories_by_realpath().is_empty());
    /// ```
    ///
    /// Side effects: none (returns a shared reference).
    // Go: internal/symlinks/knownsymlinks.go:DirectoriesByRealpath
    pub fn directories_by_realpath(&self) -> &SyncMap<Path, Arc<SyncSet<String>>> {
        &self.directories_by_realpath
    }

    /// Stores the symlink directory `symlink_path` → `real_directory` mapping.
    ///
    /// When `real_directory` is `Some` and `symlink_path` was not already
    /// present, `symlink` is also recorded in the realpath reverse map. A
    /// `None` value records that `symlink_path` is known *not* to be a symlink
    /// directory.
    ///
    /// # Examples
    /// ```
    /// use tsgo_symlinks::{new_known_symlink, KnownDirectoryLink};
    /// use tsgo_tspath::to_path;
    /// let cache = new_known_symlink("/test/dir", true);
    /// let p = to_path("/test/symlink", "/test/dir", true).ensure_trailing_directory_separator();
    /// let link = KnownDirectoryLink {
    ///     real: "/real/path/".to_string(),
    ///     real_path: to_path("/real/path", "/test/dir", true).ensure_trailing_directory_separator(),
    /// };
    /// cache.set_directory("/test/symlink", p.clone(), Some(link));
    /// assert!(cache.directories().load(&p).1);
    /// ```
    ///
    /// Side effects: mutates `directories` and (conditionally)
    /// `directories_by_realpath`.
    // Go: internal/symlinks/knownsymlinks.go:SetDirectory
    pub fn set_directory(
        &self,
        symlink: &str,
        symlink_path: Path,
        real_directory: Option<KnownDirectoryLink>,
    ) {
        if let Some(real_directory) = &real_directory {
            if !self.directories.load(&symlink_path).1 {
                let (set, _) = self.directories_by_realpath.load_or_store(
                    real_directory.real_path.clone(),
                    Arc::new(SyncSet::default()),
                );
                set.add(symlink.to_string());
            }
        }
        self.directories.store(symlink_path, real_directory);
    }

    /// Returns the symlink-file → realpath map.
    ///
    /// # Examples
    /// ```
    /// use tsgo_symlinks::new_known_symlink;
    /// let cache = new_known_symlink("/test/dir", true);
    /// assert!(cache.files().is_empty());
    /// ```
    ///
    /// Side effects: none (returns a shared reference).
    // Go: internal/symlinks/knownsymlinks.go:Files
    pub fn files(&self) -> &SyncMap<Path, String> {
        &self.files
    }

    /// Returns the realpath → {symlink files} reverse map.
    ///
    /// # Examples
    /// ```
    /// use tsgo_symlinks::new_known_symlink;
    /// let cache = new_known_symlink("/test/dir", true);
    /// assert!(cache.files_by_realpath().is_empty());
    /// ```
    ///
    /// Side effects: none (returns a shared reference).
    // Go: internal/symlinks/knownsymlinks.go:FilesByRealpath
    pub fn files_by_realpath(&self) -> &SyncMap<Path, Arc<SyncSet<String>>> {
        &self.files_by_realpath
    }

    /// Stores the symlink file `symlink_path` → `realpath` mapping, recording
    /// `symlink` in the realpath reverse map the first time `symlink_path` is
    /// seen.
    ///
    /// # Examples
    /// ```
    /// use tsgo_symlinks::new_known_symlink;
    /// use tsgo_tspath::to_path;
    /// let cache = new_known_symlink("/test/dir", true);
    /// let p = to_path("/test/symlink/file.ts", "/test/dir", true);
    /// cache.set_file("/test/symlink/file.ts", p.clone(), "/real/path/file.ts");
    /// assert_eq!(cache.files().load(&p).0, "/real/path/file.ts");
    /// ```
    ///
    /// Side effects: mutates `files` and (on first insert) `files_by_realpath`.
    // Go: internal/symlinks/knownsymlinks.go:SetFile
    pub fn set_file(&self, symlink: &str, symlink_path: Path, realpath: &str) {
        if !self.files.load(&symlink_path).1 {
            let realpath_path = to_path(realpath, &self.cwd, self.use_case_sensitive_file_names);
            let (set, _) = self
                .files_by_realpath
                .load_or_store(realpath_path, Arc::new(SyncSet::default()));
            set.add(symlink.to_string());
        }
        self.files.store(symlink_path, realpath.to_string());
    }

    /// Feeds every resolved module and type-reference directive through
    /// [`process_resolution`](Self::process_resolution).
    ///
    /// The two arguments are the program's `forEach*` iterators; each receives a
    /// callback to invoke per resolution plus an (unused here) source file. The
    /// callback ignores the module name, mode, and file path — only the
    /// resolution's original/resolved paths matter for the symlink cache.
    ///
    /// # Examples
    /// ```
    /// use tsgo_symlinks::new_known_symlink;
    /// let cache = new_known_symlink("/test/dir", true);
    /// // No resolutions: both iterators are no-ops.
    /// cache.set_symlinks_from_resolutions(|_cb, _file| {}, |_cb, _file| {});
    /// assert!(cache.files().is_empty());
    /// ```
    ///
    /// Side effects: mutates the caches via `process_resolution`.
    // Go: internal/symlinks/knownsymlinks.go:SetSymlinksFromResolutions
    pub fn set_symlinks_from_resolutions<FM, FT>(
        &self,
        for_each_resolved_module: FM,
        for_each_resolved_type_reference_directive: FT,
    ) where
        FM: FnOnce(
            &mut dyn FnMut(&ResolvedModule, &str, ResolutionMode, Path),
            Option<&SourceFileData>,
        ),
        FT: FnOnce(
            &mut dyn FnMut(&ResolvedTypeReferenceDirective, &str, ResolutionMode, Path),
            Option<&SourceFileData>,
        ),
    {
        for_each_resolved_module(
            &mut |resolution, _module_name, _mode, _file_path| {
                self.process_resolution(&resolution.original_path, &resolution.resolved_file_name);
            },
            None,
        );
        for_each_resolved_type_reference_directive(
            &mut |resolution, _module_name, _mode, _file_path| {
                self.process_resolution(&resolution.original_path, &resolution.resolved_file_name);
            },
            None,
        );
    }

    /// Processes one module-resolution result: a non-empty `(original_path,
    /// resolved_file_name)` pair records the file mapping and, when the two
    /// paths share a directory-level symlink (and the symlink path is not an
    /// ignored path), records the directory mapping too.
    ///
    /// Empty `original_path` or `resolved_file_name` is a no-op.
    ///
    /// # Examples
    /// ```
    /// use tsgo_symlinks::new_known_symlink;
    /// use tsgo_tspath::to_path;
    /// let cache = new_known_symlink("/test/dir", true);
    /// cache.process_resolution("/test/original/file.ts", "/test/resolved/file.ts");
    /// let p = to_path("/test/original/file.ts", "/test/dir", true);
    /// assert_eq!(cache.files().load(&p).0, "/test/resolved/file.ts");
    /// ```
    ///
    /// Side effects: mutates the file (and possibly directory) caches.
    // Go: internal/symlinks/knownsymlinks.go:ProcessResolution
    pub fn process_resolution(&self, original_path: &str, resolved_file_name: &str) {
        if original_path.is_empty() || resolved_file_name.is_empty() {
            return;
        }
        self.set_file(
            original_path,
            to_path(original_path, &self.cwd, self.use_case_sensitive_file_names),
            resolved_file_name,
        );
        let (common_resolved, common_original) =
            self.guess_directory_symlink(resolved_file_name, original_path, &self.cwd);
        if !common_resolved.is_empty() && !common_original.is_empty() {
            let symlink_path = to_path(
                &common_original,
                &self.cwd,
                self.use_case_sensitive_file_names,
            );
            if !contains_ignored_path(symlink_path.as_str()) {
                self.set_directory(
                    &common_original,
                    symlink_path.ensure_trailing_directory_separator(),
                    Some(KnownDirectoryLink {
                        real: ensure_trailing_directory_separator(&common_resolved),
                        real_path: to_path(
                            &common_resolved,
                            &self.cwd,
                            self.use_case_sensitive_file_names,
                        )
                        .ensure_trailing_directory_separator(),
                    }),
                );
            }
        }
    }

    // Go: internal/symlinks/knownsymlinks.go:guessDirectorySymlink
    fn guess_directory_symlink(&self, a: &str, b: &str, cwd: &str) -> (String, String) {
        let mut a_parts = get_path_components(&get_normalized_absolute_path(a, cwd), "");
        let mut b_parts = get_path_components(&get_normalized_absolute_path(b, cwd), "");
        let mut is_directory = false;
        while a_parts.len() >= 2
            && b_parts.len() >= 2
            && !self.is_node_modules_or_scoped_package_directory(&a_parts[a_parts.len() - 2])
            && !self.is_node_modules_or_scoped_package_directory(&b_parts[b_parts.len() - 2])
            && get_canonical_file_name(
                &a_parts[a_parts.len() - 1],
                self.use_case_sensitive_file_names,
            ) == get_canonical_file_name(
                &b_parts[b_parts.len() - 1],
                self.use_case_sensitive_file_names,
            )
        {
            a_parts.pop();
            b_parts.pop();
            is_directory = true;
        }
        if is_directory {
            (
                get_path_from_path_components(&a_parts),
                get_path_from_path_components(&b_parts),
            )
        } else {
            (String::new(), String::new())
        }
    }

    // Go: internal/symlinks/knownsymlinks.go:isNodeModulesOrScopedPackageDirectory
    fn is_node_modules_or_scoped_package_directory(&self, s: &str) -> bool {
        !s.is_empty()
            && (get_canonical_file_name(s, self.use_case_sensitive_file_names) == "node_modules"
                || s.starts_with('@'))
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
