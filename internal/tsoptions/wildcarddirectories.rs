//! Derives the set of directories to watch from `include`/`exclude` specs.
//!
//! 1:1 port of Go `internal/tsoptions/wildcarddirectories.go`. Go returns a
//! `map[string]bool`; this port returns an insertion-ordered
//! [`OrderedMap<String, bool>`] (`None` when there is nothing to watch) so the
//! output is deterministic, matching PORTING.md's map-ordering guidance.

use tsgo_collections::OrderedMap;
use tsgo_tspath::{
    combine_paths, contains_path, normalize_slashes, remove_trailing_directory_separator,
    ComparePathsOptions,
};
use tsgo_vfs::vfsmatch::{is_implicit_glob, new_spec_matcher, Usage};

/// Returns the directories that should be watched for the given `include`/
/// `exclude` specs (mapping path -> recursive), or `None` when `include` is
/// empty.
///
/// A wildcard in a directory segment implies a recursive watch; a wildcard only
/// in the file segment implies a non-recursive watch. Subpaths beneath an
/// already recursively-watched directory are removed.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/wildcarddirectories.go:getWildcardDirectories
pub fn get_wildcard_directories(
    include: &[String],
    exclude: &[String],
    compare_paths_options: &ComparePathsOptions,
) -> Option<OrderedMap<String, bool>> {
    if include.is_empty() {
        return None;
    }

    let ucs = compare_paths_options.use_case_sensitive_file_names;
    let exclude_matcher = new_spec_matcher(
        exclude,
        &compare_paths_options.current_directory,
        Usage::Exclude,
        ucs,
    );

    let mut wildcard_directories: OrderedMap<String, bool> = OrderedMap::default();
    // wildcard key -> the path first recorded for that key.
    let mut wildcard_key_to_path: OrderedMap<String, String> = OrderedMap::default();
    let mut recursive_keys: Vec<String> = Vec::new();

    for file in include {
        let spec = normalize_slashes(&combine_paths(
            &compare_paths_options.current_directory,
            &[file],
        ));
        if exclude_matcher
            .as_ref()
            .is_some_and(|m| m.match_string(&spec))
        {
            continue;
        }

        if let Some(m) = get_wildcard_directory_from_spec(&spec, ucs) {
            let existing_path = wildcard_key_to_path.get(&m.key).cloned();
            let existing_recursive = existing_path
                .as_ref()
                .and_then(|p| wildcard_directories.get(p).copied())
                .unwrap_or(false);

            if existing_path.is_none() || (!existing_recursive && m.recursive) {
                let path_to_use = existing_path.clone().unwrap_or_else(|| m.path.clone());
                wildcard_directories.set(path_to_use, m.recursive);

                if existing_path.is_none() {
                    wildcard_key_to_path.set(m.key.clone(), m.path.clone());
                }

                if m.recursive {
                    recursive_keys.push(m.key.clone());
                }
            }
        }

        // Remove any subpaths under an existing recursively-watched directory.
        let paths: Vec<String> = wildcard_directories.keys().cloned().collect();
        for path in paths {
            for recursive_key in &recursive_keys {
                let key = to_canonical_key(&path, ucs);
                if key != *recursive_key
                    && contains_path(recursive_key, &key, compare_paths_options)
                {
                    wildcard_directories.delete(&path);
                }
            }
        }
    }

    Some(wildcard_directories)
}

/// Canonicalizes `path` for use as a map key (lowercased when file names are
/// case-insensitive).
///
/// Side effects: none (pure).
// Go: internal/tsoptions/wildcarddirectories.go:toCanonicalKey
fn to_canonical_key(path: &str, use_case_sensitive_file_names: bool) -> String {
    if use_case_sensitive_file_names {
        path.to_string()
    } else {
        path.to_lowercase()
    }
}

/// The result of matching a single include spec to a wildcard directory.
///
/// Side effects: none (pure value type).
// Go: internal/tsoptions/wildcarddirectories.go:wildcardDirectoryMatch
#[derive(Clone, Debug)]
struct WildcardDirectoryMatch {
    key: String,
    path: String,
    recursive: bool,
}

/// Derives the wildcard directory (and recursion) implied by a single spec.
///
/// Side effects: none (pure).
// Go: internal/tsoptions/wildcarddirectories.go:getWildcardDirectoryFromSpec
fn get_wildcard_directory_from_spec(
    spec: &str,
    use_case_sensitive_file_names: bool,
) -> Option<WildcardDirectoryMatch> {
    // `*` and `?` are ASCII, so byte indices land on char boundaries.
    if let Some(first_wildcard) = spec.find(['*', '?']) {
        if let Some(last_sep_before_wildcard) = spec[..first_wildcard].rfind('/') {
            let path = &spec[..last_sep_before_wildcard];
            let last_directory_separator_index = spec.rfind('/').unwrap_or(0);
            // Recursive if the wildcard appears in a directory segment (not just
            // the final file segment).
            let recursive = first_wildcard < last_directory_separator_index;
            return Some(WildcardDirectoryMatch {
                key: to_canonical_key(path, use_case_sensitive_file_names),
                path: path.to_string(),
                recursive,
            });
        }
    }

    if let Some(last_sep_index) = spec.rfind('/') {
        let last_segment = &spec[last_sep_index + 1..];
        if is_implicit_glob(last_segment) {
            let path = remove_trailing_directory_separator(spec);
            return Some(WildcardDirectoryMatch {
                key: to_canonical_key(path, use_case_sensitive_file_names),
                path: path.to_string(),
                recursive: true,
            });
        }
    }

    None
}

#[cfg(test)]
#[path = "wildcarddirectories_test.rs"]
mod tests;
