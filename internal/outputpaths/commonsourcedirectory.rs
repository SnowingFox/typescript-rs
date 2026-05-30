//! Common source directory derivation.
//!
//! 1:1 port of Go `internal/outputpaths/commonsourcedirectory.go`.

use tsgo_core::compileroptions::CompilerOptions;
use tsgo_tspath::{
    ensure_trailing_directory_separator, get_canonical_file_name, get_directory_path,
    get_normalized_path_components, get_path_from_path_components,
};

/// Computes the longest common directory of `file_names` (each contributes its
/// directory components; the base name is dropped).
///
/// Returns `""` if the very first path component differs across files, or
/// `current_directory` when `file_names` is empty (which happens when every
/// input file is a `.d.ts` and is therefore filtered out before this call).
///
/// Side effects: none (pure).
// Go: internal/outputpaths/commonsourcedirectory.go:computeCommonSourceDirectoryOfFilenames
fn compute_common_source_directory_of_filenames(
    file_names: &[String],
    current_directory: &str,
    use_case_sensitive_file_names: bool,
) -> String {
    let mut common_path_components: Option<Vec<String>> = None;
    for source_file in file_names {
        // Each file contributes into common source file path.
        let mut source_path_components =
            get_normalized_path_components(source_file, current_directory);
        // The base file name is not part of the common directory path.
        source_path_components.pop();

        let common = match &mut common_path_components {
            None => {
                // first file
                common_path_components = Some(source_path_components);
                continue;
            }
            Some(common) => common,
        };

        let n = common.len().min(source_path_components.len());
        for i in 0..n {
            if get_canonical_file_name(&common[i], use_case_sensitive_file_names)
                != get_canonical_file_name(
                    &source_path_components[i],
                    use_case_sensitive_file_names,
                )
            {
                if i == 0 {
                    // Failed to find any common path component.
                    return String::new();
                }
                // New common path found that is 0 -> i-1.
                common.truncate(i);
                break;
            }
        }

        // If sourcePathComponents was shorter than commonPathComponents, truncate
        // to the sourcePathComponents.
        if source_path_components.len() < common.len() {
            common.truncate(source_path_components.len());
        }
    }

    match common_path_components {
        // Can happen when all input files are .d.ts files (empty input list) or
        // the common path narrowed to nothing.
        Some(common) if !common.is_empty() => get_path_from_path_components(&common),
        _ => current_directory.to_string(),
    }
}

/// Returns the computed common source directory for `emitted_files`, with a
/// trailing directory separator appended when non-empty.
///
/// # Examples
/// ```
/// use tsgo_outputpaths::get_computed_common_source_directory;
/// let files = vec!["/src/a.ts".to_string()];
/// assert_eq!(get_computed_common_source_directory(&files, "/", true), "/src/");
/// ```
///
/// Side effects: none (pure).
// Go: internal/outputpaths/commonsourcedirectory.go:GetComputedCommonSourceDirectory
pub fn get_computed_common_source_directory(
    emitted_files: &[String],
    current_directory: &str,
    use_case_sensitive_file_names: bool,
) -> String {
    let common_source_directory = compute_common_source_directory_of_filenames(
        emitted_files,
        current_directory,
        use_case_sensitive_file_names,
    );
    if !common_source_directory.is_empty() {
        ensure_trailing_directory_separator(&common_source_directory)
    } else {
        common_source_directory
    }
}

// Diagnostic callback for `get_common_source_directory`: receives the file list
// and the candidate root path. Mirrors Go's `func([]string, string) bool`; the
// return value is ignored (kept only for parity with the Go signature).
type CheckSourceFilesBelongToPath<'a> = dyn Fn(&[String], &str) -> bool + 'a;

/// Determines the program's common source directory.
///
/// Uses `rootDir` if set, otherwise the directory of the config file, otherwise
/// the computed longest common directory of `files()`. The result has a
/// trailing directory separator when non-empty. `files` is evaluated lazily so
/// the file-name list is only built when actually needed.
///
/// `check_source_files_belong_to_path`, when provided, is invoked (as in Go)
/// for its diagnostic side effects in the `rootDir`/config branches; its return
/// value is ignored.
///
/// Side effects: invokes `check_source_files_belong_to_path` when supplied (its
/// own side effects only); otherwise none (pure).
// Go: internal/outputpaths/commonsourcedirectory.go:GetCommonSourceDirectory
pub fn get_common_source_directory(
    options: &CompilerOptions,
    files: impl Fn() -> Vec<String>,
    current_directory: &str,
    use_case_sensitive_file_names: bool,
    check_source_files_belong_to_path: Option<&CheckSourceFilesBelongToPath<'_>>,
) -> String {
    let mut common_source_directory = if !options.root_dir.is_empty() {
        // If a rootDir is specified use it as the commonSourceDirectory.
        if let Some(check) = check_source_files_belong_to_path {
            check(&files(), &options.root_dir);
        }
        options.root_dir.clone()
    } else if !options.config_file_path.is_empty() {
        // If the rootDir is not specified, then the common source directory is
        // the directory of the config file.
        let dir = get_directory_path(&options.config_file_path);
        if let Some(check) = check_source_files_belong_to_path {
            check(&files(), &dir);
        }
        dir
    } else {
        compute_common_source_directory_of_filenames(
            &files(),
            current_directory,
            use_case_sensitive_file_names,
        )
    };

    if !common_source_directory.is_empty() {
        // Make sure the directory path ends with a directory separator so this
        // string can be used directly to replace with "" to get the relative
        // path of the source file (without a leading "/" making it rooted).
        common_source_directory = ensure_trailing_directory_separator(&common_source_directory);
    }

    common_source_directory
}

#[cfg(test)]
#[path = "commonsourcedirectory_test.rs"]
mod tests;
