use super::*;

use tsgo_tspath::ComparePathsOptions;

fn cpo(current_directory: &str, use_case_sensitive_file_names: bool) -> ComparePathsOptions {
    ComparePathsOptions {
        current_directory: current_directory.to_string(),
        use_case_sensitive_file_names,
    }
}

// Go: internal/tsoptions/wildcarddirectories_test.go:TestGetWildcardDirectories_NonASCIICharacters
#[test]
fn wildcard_norwegian() {
    let result = get_wildcard_directories(
        &[
            "src/**/*.test.ts".into(),
            "src/**/*.stories.ts".into(),
            "src/**/*.mdx".into(),
        ],
        &["node_modules".into()],
        &cpo(
            "C:/Users/TobiasLægreid/dev/app/frontend/packages/react",
            false,
        ),
    );
    assert!(result.is_some());
}

#[test]
fn wildcard_japanese() {
    let result = get_wildcard_directories(
        &["src/**/*.ts".into()],
        &["テスト".into()],
        &cpo("/Users/ユーザー/プロジェクト", true),
    );
    assert!(result.is_some());
}

#[test]
fn wildcard_chinese() {
    let result = get_wildcard_directories(
        &["源代码/**/*.js".into()],
        &["节点模块".into()],
        &cpo("/home/用户/项目", true),
    );
    assert!(result.is_some());
}

#[test]
fn wildcard_various_unicode() {
    let result = get_wildcard_directories(
        &["src/**/*.ts".into()],
        &["node_modules".into()],
        &cpo("/Users/Müller/café/naïve/résumé", false),
    );
    assert!(result.is_some());
}

// Behavior-level (PORTING.md §8.6): empty include -> None.
#[test]
fn empty_include_is_none() {
    let result = get_wildcard_directories(&[], &[], &cpo("/home/project", true));
    assert!(result.is_none());
}

// Behavior-level: a wildcard in a directory segment is recursive; a wildcard
// only in the file segment is non-recursive (expected from the Go doc comment).
#[test]
fn recursive_vs_non_recursive() {
    let recursive =
        get_wildcard_directories(&["a/b/**/d.ts".into()], &[], &cpo("/root", true)).unwrap();
    assert_eq!(recursive.get(&"/root/a/b".to_string()), Some(&true));

    let non_recursive =
        get_wildcard_directories(&["a/b/*.ts".into()], &[], &cpo("/root", true)).unwrap();
    assert_eq!(non_recursive.get(&"/root/a/b".to_string()), Some(&false));
}
