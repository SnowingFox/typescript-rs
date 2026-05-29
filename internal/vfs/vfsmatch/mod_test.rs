use super::*;
use crate::vfstest::{MapFile, MapFs};

fn v(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

// Runs read_directory against `/dev` from `/` with the given specs, no depth limit.
fn rd(host: &MapFs, ext: &[&str], excl: &[&str], incl: &[&str]) -> Vec<String> {
    read_directory(
        host,
        "/",
        "/dev",
        &v(ext),
        &v(excl),
        &v(incl),
        UNLIMITED_DEPTH,
    )
}

fn case_insensitive_host() -> MapFs {
    MapFs::from_map(
        [
            ("/dev/a.ts", ""),
            ("/dev/a.d.ts", ""),
            ("/dev/a.js", ""),
            ("/dev/b.ts", ""),
            ("/dev/b.js", ""),
            ("/dev/c.d.ts", ""),
            ("/dev/z/a.ts", ""),
            ("/dev/z/abz.ts", ""),
            ("/dev/z/aba.ts", ""),
            ("/dev/z/b.ts", ""),
            ("/dev/z/bbz.ts", ""),
            ("/dev/z/bba.ts", ""),
            ("/dev/x/a.ts", ""),
            ("/dev/x/aa.ts", ""),
            ("/dev/x/b.ts", ""),
            ("/dev/x/y/a.ts", ""),
            ("/dev/x/y/b.ts", ""),
            ("/dev/js/a.js", ""),
            ("/dev/js/b.js", ""),
            ("/dev/js/d.min.js", ""),
            ("/dev/js/ab.min.js", ""),
            ("/ext/ext.ts", ""),
            ("/ext/b/a..b.ts", ""),
        ],
        false,
    )
}

fn case_sensitive_host() -> MapFs {
    MapFs::from_map(
        [
            ("/dev/a.ts", ""),
            ("/dev/a.d.ts", ""),
            ("/dev/a.js", ""),
            ("/dev/b.ts", ""),
            ("/dev/b.js", ""),
            ("/dev/A.ts", ""),
            ("/dev/B.ts", ""),
            ("/dev/c.d.ts", ""),
            ("/dev/z/a.ts", ""),
            ("/dev/z/abz.ts", ""),
            ("/dev/z/aba.ts", ""),
            ("/dev/z/b.ts", ""),
            ("/dev/z/bbz.ts", ""),
            ("/dev/z/bba.ts", ""),
            ("/dev/x/a.ts", ""),
            ("/dev/x/b.ts", ""),
            ("/dev/x/y/a.ts", ""),
            ("/dev/x/y/b.ts", ""),
            ("/dev/q/a/c/b/d.ts", ""),
            ("/dev/js/a.js", ""),
            ("/dev/js/b.js", ""),
            ("/dev/js/d.MIN.js", ""),
        ],
        true,
    )
}

fn common_folders_host() -> MapFs {
    MapFs::from_map(
        [
            ("/dev/a.ts", ""),
            ("/dev/a.d.ts", ""),
            ("/dev/a.js", ""),
            ("/dev/b.ts", ""),
            ("/dev/x/a.ts", ""),
            ("/dev/node_modules/a.ts", ""),
            ("/dev/bower_components/a.ts", ""),
            ("/dev/jspm_packages/a.ts", ""),
        ],
        false,
    )
}

fn dotted_folders_host() -> MapFs {
    MapFs::from_map(
        [
            ("/dev/x/d.ts", ""),
            ("/dev/x/y/d.ts", ""),
            ("/dev/x/y/.e.ts", ""),
            ("/dev/x/.y/a.ts", ""),
            ("/dev/.z/.b.ts", ""),
            ("/dev/.z/c.ts", ""),
            ("/dev/w/.u/e.ts", ""),
            ("/dev/g.min.js/.g/g.ts", ""),
        ],
        false,
    )
}

fn mixed_extension_host() -> MapFs {
    MapFs::from_map(
        [
            ("/dev/a.ts", ""),
            ("/dev/a.d.ts", ""),
            ("/dev/a.js", ""),
            ("/dev/b.tsx", ""),
            ("/dev/b.d.ts", ""),
            ("/dev/b.jsx", ""),
            ("/dev/c.tsx", ""),
            ("/dev/c.js", ""),
            ("/dev/d.js", ""),
            ("/dev/e.jsx", ""),
            ("/dev/f.other", ""),
        ],
        false,
    )
}

fn same_named_declarations_host() -> MapFs {
    MapFs::from_map(
        [
            ("/dev/a.tsx", ""),
            ("/dev/a.d.ts", ""),
            ("/dev/b.tsx", ""),
            ("/dev/b.ts", ""),
            ("/dev/c.tsx", ""),
            ("/dev/m.ts", ""),
            ("/dev/m.d.ts", ""),
            ("/dev/n.tsx", ""),
            ("/dev/n.ts", ""),
            ("/dev/n.d.ts", ""),
            ("/dev/o.ts", ""),
            ("/dev/x.d.ts", ""),
        ],
        false,
    )
}

const TS_EXTS: &[&str] = &[".ts", ".tsx", ".d.ts"];

fn has(got: &[String], path: &str) -> bool {
    got.iter().any(|f| f == path)
}

// ---- TestReadDirectory -----------------------------------------------------

// Go: vfsmatch_test.go:TestReadDirectory/defaults include common package folders
#[test]
fn defaults_include_common_package_folders() {
    let got = rd(&common_folders_host(), TS_EXTS, &[], &[]);
    for p in [
        "/dev/a.ts",
        "/dev/b.ts",
        "/dev/x/a.ts",
        "/dev/node_modules/a.ts",
        "/dev/bower_components/a.ts",
        "/dev/jspm_packages/a.ts",
    ] {
        assert!(has(&got, p), "missing {p}");
    }
}

// Go: vfsmatch_test.go:TestReadDirectory/literal includes without exclusions
#[test]
fn literal_includes_without_exclusions() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["a.ts", "b.ts"]);
    assert_eq!(got, ["/dev/a.ts", "/dev/b.ts"]);
}

// Go: vfsmatch_test.go:TestReadDirectory/literal includes with non ts extensions excluded
#[test]
fn literal_includes_non_ts_excluded() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["a.js", "b.js"]);
    assert_eq!(got.len(), 0);
}

// Go: vfsmatch_test.go:TestReadDirectory/literal includes missing files excluded
#[test]
fn literal_includes_missing_excluded() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["z.ts", "x.ts"]);
    assert_eq!(got.len(), 0);
}

// Go: vfsmatch_test.go:TestReadDirectory/literal includes with literal excludes
#[test]
fn literal_includes_with_literal_excludes() {
    let got = rd(
        &case_insensitive_host(),
        TS_EXTS,
        &["b.ts"],
        &["a.ts", "b.ts"],
    );
    assert_eq!(got, ["/dev/a.ts"]);
}

// Go: vfsmatch_test.go:TestReadDirectory/literal includes with wildcard excludes
#[test]
fn literal_includes_with_wildcard_excludes() {
    let got = rd(
        &case_insensitive_host(),
        TS_EXTS,
        &["*.ts", "z/??z.ts", "*/b.ts"],
        &["a.ts", "b.ts", "z/a.ts", "z/abz.ts", "z/aba.ts", "x/b.ts"],
    );
    assert_eq!(got, ["/dev/z/a.ts", "/dev/z/aba.ts"]);
}

// Go: vfsmatch_test.go:TestReadDirectory/literal includes with recursive excludes
#[test]
fn literal_includes_with_recursive_excludes() {
    let got = rd(
        &case_insensitive_host(),
        TS_EXTS,
        &["**/b.ts"],
        &["a.ts", "b.ts", "x/a.ts", "x/b.ts", "x/y/a.ts", "x/y/b.ts"],
    );
    assert_eq!(got, ["/dev/a.ts", "/dev/x/a.ts", "/dev/x/y/a.ts"]);
}

// Go: vfsmatch_test.go:TestReadDirectory/case sensitive exclude is respected
#[test]
fn case_sensitive_exclude_respected() {
    let got = rd(&case_sensitive_host(), TS_EXTS, &["**/b.ts"], &["B.ts"]);
    assert_eq!(got, ["/dev/B.ts"]);
}

// Go: vfsmatch_test.go:TestReadDirectory/explicit includes keep common package folders
#[test]
fn explicit_includes_keep_common_package_folders() {
    let got = rd(
        &common_folders_host(),
        TS_EXTS,
        &[],
        &[
            "a.ts",
            "b.ts",
            "node_modules/a.ts",
            "bower_components/a.ts",
            "jspm_packages/a.ts",
        ],
    );
    for p in [
        "/dev/a.ts",
        "/dev/b.ts",
        "/dev/node_modules/a.ts",
        "/dev/bower_components/a.ts",
        "/dev/jspm_packages/a.ts",
    ] {
        assert!(has(&got, p), "missing {p}");
    }
}

// Go: vfsmatch_test.go:TestReadDirectory/wildcard include sorted order
#[test]
fn wildcard_include_sorted_order() {
    let got = rd(
        &case_insensitive_host(),
        TS_EXTS,
        &[],
        &["z/*.ts", "x/*.ts"],
    );
    assert_eq!(
        got,
        [
            "/dev/z/a.ts",
            "/dev/z/aba.ts",
            "/dev/z/abz.ts",
            "/dev/z/b.ts",
            "/dev/z/bba.ts",
            "/dev/z/bbz.ts",
            "/dev/x/a.ts",
            "/dev/x/aa.ts",
            "/dev/x/b.ts",
        ]
    );
}

// Go: vfsmatch_test.go:TestReadDirectory/wildcard include same named declarations excluded
#[test]
fn wildcard_include_same_named_declarations_excluded() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["*.ts"]);
    for p in ["/dev/a.ts", "/dev/b.ts", "/dev/a.d.ts", "/dev/c.d.ts"] {
        assert!(has(&got, p), "missing {p}");
    }
}

// Go: vfsmatch_test.go:TestReadDirectory/wildcard star matches only ts files
#[test]
fn wildcard_star_matches_only_ts() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["*"]);
    for f in &got {
        assert!(
            f.contains(".ts") || f.contains(".tsx") || f.contains(".d.ts"),
            "unexpected file: {f}"
        );
    }
    assert!(!has(&got, "/dev/a.js"));
    assert!(!has(&got, "/dev/b.js"));
}

// Go: vfsmatch_test.go:TestReadDirectory/wildcard question mark single character
#[test]
fn wildcard_question_single_char() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["x/?.ts"]);
    assert_eq!(got, ["/dev/x/a.ts", "/dev/x/b.ts"]);
}

// Go: vfsmatch_test.go:TestReadDirectory/wildcard recursive directory
#[test]
fn wildcard_recursive_directory() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["**/a.ts"]);
    for p in ["/dev/a.ts", "/dev/z/a.ts", "/dev/x/a.ts", "/dev/x/y/a.ts"] {
        assert!(has(&got, p), "missing {p}");
    }
}

// Go: vfsmatch_test.go:TestReadDirectory/double asterisk matches zero-or-more directories
#[test]
fn double_asterisk_zero_or_more_dirs() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["x/**/a.ts"]);
    assert_eq!(got.len(), 2);
    assert!(has(&got, "/dev/x/a.ts"));
    assert!(has(&got, "/dev/x/y/a.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/wildcard multiple recursive directories
#[test]
fn wildcard_multiple_recursive_dirs() {
    let got = rd(
        &case_insensitive_host(),
        TS_EXTS,
        &[],
        &["x/y/**/a.ts", "x/**/a.ts", "z/**/a.ts"],
    );
    assert!(!got.is_empty());
}

// Go: vfsmatch_test.go:TestReadDirectory/wildcard case sensitive matching
#[test]
fn wildcard_case_sensitive_matching() {
    let got = rd(&case_sensitive_host(), TS_EXTS, &[], &["**/A.ts"]);
    assert_eq!(got, ["/dev/A.ts"]);
}

// Go: vfsmatch_test.go:TestReadDirectory/wildcard missing files excluded
#[test]
fn wildcard_missing_files_excluded() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["*/z.ts"]);
    assert_eq!(got.len(), 0);
}

// Go: vfsmatch_test.go:TestReadDirectory/exclude folders with wildcards
#[test]
fn exclude_folders_with_wildcards() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &["z", "x"], &["**/*"]);
    for f in &got {
        assert!(
            !f.contains("/z/") && !f.contains("/x/"),
            "should not contain z or x: {f}"
        );
    }
    assert!(has(&got, "/dev/a.ts"));
    assert!(has(&got, "/dev/b.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/include paths outside project absolute
#[test]
fn include_paths_outside_project_absolute() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["*", "/ext/*"]);
    assert!(has(&got, "/dev/a.ts"));
    assert!(has(&got, "/ext/ext.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/include paths outside project relative
#[test]
fn include_paths_outside_project_relative() {
    let got = rd(
        &case_insensitive_host(),
        TS_EXTS,
        &["**"],
        &["*", "../ext/*"],
    );
    assert!(has(&got, "/ext/ext.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/include files containing double dots
#[test]
fn include_files_double_dots() {
    let got = rd(
        &case_insensitive_host(),
        TS_EXTS,
        &["**"],
        &["/ext/b/a..b.ts"],
    );
    assert!(has(&got, "/ext/b/a..b.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/exclude files containing double dots
#[test]
fn exclude_files_double_dots() {
    let got = rd(
        &case_insensitive_host(),
        TS_EXTS,
        &["/ext/b/a..b.ts"],
        &["/ext/**/*"],
    );
    assert!(has(&got, "/ext/ext.ts"));
    assert!(!has(&got, "/ext/b/a..b.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/common package folders implicitly excluded
#[test]
fn common_package_folders_implicitly_excluded() {
    let got = rd(&common_folders_host(), TS_EXTS, &[], &["**/a.ts"]);
    assert!(has(&got, "/dev/a.ts"));
    assert!(has(&got, "/dev/x/a.ts"));
    assert!(!has(&got, "/dev/node_modules/a.ts"));
    assert!(!has(&got, "/dev/bower_components/a.ts"));
    assert!(!has(&got, "/dev/jspm_packages/a.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/common package folders explicit recursive include
#[test]
fn common_package_folders_explicit_recursive() {
    let got = rd(
        &common_folders_host(),
        TS_EXTS,
        &[],
        &["**/a.ts", "**/node_modules/a.ts"],
    );
    assert!(has(&got, "/dev/a.ts"));
    assert!(has(&got, "/dev/node_modules/a.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/common package folders wildcard include
#[test]
fn common_package_folders_wildcard() {
    let got = rd(&common_folders_host(), TS_EXTS, &[], &["*/a.ts"]);
    assert!(has(&got, "/dev/x/a.ts"));
    assert!(!has(&got, "/dev/node_modules/a.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/common package folders explicit wildcard include
#[test]
fn common_package_folders_explicit_wildcard() {
    let got = rd(
        &common_folders_host(),
        TS_EXTS,
        &[],
        &["*/a.ts", "node_modules/a.ts"],
    );
    assert!(has(&got, "/dev/x/a.ts"));
    assert!(has(&got, "/dev/node_modules/a.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/dotted folders not implicitly included
#[test]
fn dotted_folders_not_implicitly_included() {
    let got = rd(&dotted_folders_host(), TS_EXTS, &[], &["x/**/*", "w/*/*"]);
    assert!(has(&got, "/dev/x/d.ts"));
    assert!(has(&got, "/dev/x/y/d.ts"));
    assert!(!has(&got, "/dev/x/.y/a.ts"));
    assert!(!has(&got, "/dev/x/y/.e.ts"));
    assert!(!has(&got, "/dev/w/.u/e.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/dotted folders explicitly included
#[test]
fn dotted_folders_explicitly_included() {
    let got = rd(
        &dotted_folders_host(),
        TS_EXTS,
        &[],
        &["x/.y/a.ts", "/dev/.z/.b.ts"],
    );
    assert!(has(&got, "/dev/x/.y/a.ts"));
    assert!(has(&got, "/dev/.z/.b.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/dotted folders recursive wildcard matches directories
#[test]
fn dotted_folders_recursive_wildcard() {
    let got = rd(&dotted_folders_host(), TS_EXTS, &[], &["**/.*/*"]);
    assert!(has(&got, "/dev/x/.y/a.ts"));
    assert!(has(&got, "/dev/.z/c.ts"));
    assert!(has(&got, "/dev/w/.u/e.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/trailing recursive include returns empty
#[test]
fn trailing_recursive_include_empty() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["**"]);
    assert_eq!(got.len(), 0);
}

// Go: vfsmatch_test.go:TestReadDirectory/trailing recursive exclude removes everything
#[test]
fn trailing_recursive_exclude_removes_all() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &["**"], &["**/*"]);
    assert_eq!(got.len(), 0);
}

// Go: vfsmatch_test.go:TestReadDirectory/multiple recursive directory patterns in includes
#[test]
fn multiple_recursive_dir_in_includes() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["**/x/**/*"]);
    assert!(has(&got, "/dev/x/a.ts"));
    assert!(has(&got, "/dev/x/y/a.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/multiple recursive directory patterns in excludes
#[test]
fn multiple_recursive_dir_in_excludes() {
    let got = rd(
        &case_insensitive_host(),
        TS_EXTS,
        &["**/x/**"],
        &["**/a.ts"],
    );
    assert!(has(&got, "/dev/a.ts"));
    assert!(has(&got, "/dev/z/a.ts"));
    assert!(!has(&got, "/dev/x/a.ts"));
    assert!(!has(&got, "/dev/x/y/a.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/implicit globbification expands directory
#[test]
fn implicit_globbification_expands_directory() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["z"]);
    assert!(has(&got, "/dev/z/a.ts"));
    assert!(has(&got, "/dev/z/aba.ts"));
    assert!(has(&got, "/dev/z/b.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/exclude patterns starting with starstar
#[test]
fn exclude_patterns_starting_starstar() {
    let got = rd(&case_sensitive_host(), TS_EXTS, &["**/x"], &[]);
    for f in &got {
        assert!(!f.contains("/x/"), "should not contain /x/: {f}");
    }
}

// Go: vfsmatch_test.go:TestReadDirectory/include patterns starting with starstar
#[test]
fn include_patterns_starting_starstar() {
    let got = rd(&case_sensitive_host(), TS_EXTS, &[], &["**/x", "**/a/**/b"]);
    assert!(has(&got, "/dev/x/a.ts"));
    assert!(has(&got, "/dev/q/a/c/b/d.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/depth limit one
#[test]
fn depth_limit_one() {
    let got = read_directory(
        &case_insensitive_host(),
        "/",
        "/dev",
        &v(TS_EXTS),
        &[],
        &[],
        1,
    );
    for f in &got {
        let suffix = &f["/dev/".len()..];
        assert!(
            !suffix.contains('/'),
            "depth 1 should not include nested files: {f}"
        );
    }
}

// Go: vfsmatch_test.go:TestReadDirectory/depth limit two
#[test]
fn depth_limit_two() {
    let got = read_directory(
        &case_insensitive_host(),
        "/",
        "/dev",
        &v(TS_EXTS),
        &[],
        &[],
        2,
    );
    assert!(has(&got, "/dev/a.ts"));
    assert!(has(&got, "/dev/z/a.ts"));
    assert!(!has(&got, "/dev/x/y/a.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/mixed extensions only ts
#[test]
fn mixed_extensions_only_ts() {
    let got = rd(&mixed_extension_host(), &[".ts"], &[], &[]);
    for f in &got {
        assert!(f.ends_with(".ts"), "should only have .ts files: {f}");
    }
}

// Go: vfsmatch_test.go:TestReadDirectory/mixed extensions ts and tsx
#[test]
fn mixed_extensions_ts_tsx() {
    let got = rd(&mixed_extension_host(), &[".ts", ".tsx"], &[], &[]);
    for f in &got {
        assert!(f.ends_with(".ts") || f.ends_with(".tsx"), "unexpected: {f}");
    }
}

// Go: vfsmatch_test.go:TestReadDirectory/mixed extensions js and jsx
#[test]
fn mixed_extensions_js_jsx() {
    let got = rd(&mixed_extension_host(), &[".js", ".jsx"], &[], &[]);
    for f in &got {
        assert!(f.ends_with(".js") || f.ends_with(".jsx"), "unexpected: {f}");
    }
}

// Go: vfsmatch_test.go:TestReadDirectory/min js files excluded by wildcard
#[test]
fn min_js_excluded_by_wildcard() {
    let got = rd(&case_insensitive_host(), &[".js"], &[], &["js/*"]);
    assert!(has(&got, "/dev/js/a.js"));
    assert!(has(&got, "/dev/js/b.js"));
    assert!(!has(&got, "/dev/js/d.min.js"));
    assert!(!has(&got, "/dev/js/ab.min.js"));
}

// Go: vfsmatch_test.go:TestReadDirectory/min js exclusion is case-sensitive on case-sensitive FS
#[test]
fn min_js_exclusion_case_sensitive() {
    let got = rd(&case_sensitive_host(), &[".js"], &[], &["js/*"]);
    assert!(has(&got, "/dev/js/a.js"));
    assert!(has(&got, "/dev/js/b.js"));
    assert!(has(&got, "/dev/js/d.MIN.js"));
}

// Go: vfsmatch_test.go:TestReadDirectory/min js files explicitly included
#[test]
fn min_js_explicitly_included() {
    let got = rd(&case_insensitive_host(), &[".js"], &[], &["js/*.min.js"]);
    assert!(has(&got, "/dev/js/d.min.js"));
    assert!(has(&got, "/dev/js/ab.min.js"));
}

// Go: vfsmatch_test.go:TestReadDirectory/min js files included when pattern mentions .min.
#[test]
fn min_js_included_when_pattern_mentions_min() {
    let got = rd(&case_insensitive_host(), &[".js"], &[], &["js/*.min.*"]);
    assert_eq!(got.len(), 2);
    assert!(has(&got, "/dev/js/d.min.js"));
    assert!(has(&got, "/dev/js/ab.min.js"));
}

// Go: vfsmatch_test.go:TestReadDirectory/exclude literal node_modules folder
#[test]
fn exclude_literal_node_modules() {
    let got = rd(
        &common_folders_host(),
        TS_EXTS,
        &["node_modules"],
        &["**/*"],
    );
    assert!(has(&got, "/dev/a.ts"));
    assert!(!has(&got, "/dev/node_modules/a.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/same named declarations include ts
#[test]
fn same_named_declarations_include_ts() {
    let got = rd(&same_named_declarations_host(), TS_EXTS, &[], &["*.ts"]);
    assert!(!got.is_empty());
}

// Go: vfsmatch_test.go:TestReadDirectory/same named declarations include tsx
#[test]
fn same_named_declarations_include_tsx() {
    let got = rd(&same_named_declarations_host(), TS_EXTS, &[], &["*.tsx"]);
    for f in &got {
        assert!(f.ends_with(".tsx"), "should only have .tsx files: {f}");
    }
}

// Go: vfsmatch_test.go:TestReadDirectory/empty includes returns all matching files
#[test]
fn empty_includes_returns_all_matching() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &[]);
    assert!(!got.is_empty());
    assert!(has(&got, "/dev/a.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectory/nil extensions returns all files
#[test]
fn nil_extensions_returns_all() {
    let got = rd(&case_insensitive_host(), &[], &[], &[]);
    assert!(has(&got, "/dev/a.ts"));
    assert!(has(&got, "/dev/a.js"));
}

// Go: vfsmatch_test.go:TestReadDirectory/empty extensions slice returns all files
#[test]
fn empty_extensions_slice_returns_all() {
    let got = rd(&case_insensitive_host(), &[], &[], &[]);
    assert!(!got.is_empty());
}

// ---- TestIsImplicitGlob ----------------------------------------------------

// Go: vfsmatch_test.go:TestIsImplicitGlob
#[test]
fn is_implicit_glob_cases() {
    assert!(is_implicit_glob("foo")); // simple
    assert!(is_implicit_glob("src")); // folder
    assert!(!is_implicit_glob("foo.ts")); // with extension
    assert!(!is_implicit_glob("foo.")); // trailing dot
    assert!(!is_implicit_glob("*")); // star
    assert!(!is_implicit_glob("?")); // question
    assert!(!is_implicit_glob("foo*")); // star suffix
    assert!(!is_implicit_glob("foo?")); // question suffix
    assert!(!is_implicit_glob("foo.bar")); // dot name
    assert!(is_implicit_glob("")); // empty
}

// ---- TestReadDirectoryEdgeCases --------------------------------------------

// Go: vfsmatch_test.go:TestReadDirectoryEdgeCases/rooted include path
#[test]
fn edge_rooted_include_path() {
    let got = rd(&case_insensitive_host(), &[".ts"], &[], &["/dev/a.ts"]);
    assert!(has(&got, "/dev/a.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectoryEdgeCases/include with extension in path
#[test]
fn edge_include_with_extension() {
    let got = rd(&case_insensitive_host(), &[".ts"], &[], &["a.ts"]);
    assert!(has(&got, "/dev/a.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectoryEdgeCases/special regex characters in path
#[test]
fn edge_special_regex_chars() {
    let host = MapFs::from_map(
        [
            ("/dev/file+test.ts", ""),
            ("/dev/file[0].ts", ""),
            ("/dev/file(1).ts", ""),
            ("/dev/file$money.ts", ""),
            ("/dev/file^start.ts", ""),
            ("/dev/file|pipe.ts", ""),
            ("/dev/file#hash.ts", ""),
        ],
        false,
    );
    let got = rd(&host, &[".ts"], &[], &["file+test.ts"]);
    assert!(has(&got, "/dev/file+test.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectoryEdgeCases/include pattern starting with question mark
#[test]
fn edge_include_question_prefix() {
    let got = rd(&case_insensitive_host(), &[".ts"], &[], &["?.ts"]);
    assert!(has(&got, "/dev/a.ts"));
    assert!(has(&got, "/dev/b.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectoryEdgeCases/include pattern starting with star
#[test]
fn edge_include_star_prefix() {
    let got = rd(&case_insensitive_host(), &[".ts"], &[], &["*b.ts"]);
    assert!(has(&got, "/dev/b.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectoryEdgeCases/case insensitive file matching
#[test]
fn edge_case_insensitive_matching() {
    let host = MapFs::from_map([("/dev/File.ts", ""), ("/dev/FILE.ts", "")], true);
    let got = rd(&host, &[".ts"], &[], &["*.ts"]);
    assert_eq!(got.len(), 2);
}

// Go: vfsmatch_test.go:TestReadDirectoryEdgeCases/nested subdirectory base path
#[test]
fn edge_nested_subdir_base_path() {
    let got = rd(&case_sensitive_host(), &[".ts"], &[], &["q/a/c/b/d.ts"]);
    assert!(has(&got, "/dev/q/a/c/b/d.ts"));
}

// Go: vfsmatch_test.go:TestReadDirectoryEdgeCases/current directory differs from path
#[test]
fn edge_current_dir_differs() {
    let got = rd(&case_insensitive_host(), &[".ts"], &[], &["z/*.ts"]);
    assert!(!got.is_empty());
}

// ---- TestReadDirectoryEmptyIncludes / SymlinkCycle -------------------------

// Go: vfsmatch_test.go:TestReadDirectoryEmptyIncludes/empty includes slice behavior
#[test]
fn empty_includes_slice_behavior() {
    let host = MapFs::from_map([("/root/a.ts", "")], true);
    let got = read_directory(&host, "/", "/root", &v(&[".ts"]), &[], &[], UNLIMITED_DEPTH);
    if !got.is_empty() {
        assert!(has(&got, "/root/a.ts"));
    }
}

// Go: vfsmatch_test.go:TestReadDirectorySymlinkCycle/detects and skips symlink cycles
#[test]
fn symlink_cycle_detected() {
    let host = MapFs::from_map(
        [
            ("/root/file.ts", MapFile::text("")),
            ("/root/a/file.ts", MapFile::text("")),
            ("/root/a/b", MapFile::symlink("/root/a")),
        ],
        true,
    );
    let got = read_directory(
        &host,
        "/",
        "/root",
        &v(&[".ts"]),
        &[],
        &v(&["**/*"]),
        UNLIMITED_DEPTH,
    );
    assert_eq!(got, ["/root/file.ts", "/root/a/file.ts"]);
}

// ---- TestReadDirectoryMatchesTypeScriptBaselines ---------------------------

// Go: vfsmatch_test.go:TestReadDirectoryMatchesTypeScriptBaselines/sorted in include order then alphabetical
#[test]
fn baseline_sorted_include_order_then_alpha() {
    let host = MapFs::from_map(
        [
            ("/dev/z/a.ts", ""),
            ("/dev/z/aba.ts", ""),
            ("/dev/z/abz.ts", ""),
            ("/dev/z/b.ts", ""),
            ("/dev/z/bba.ts", ""),
            ("/dev/z/bbz.ts", ""),
            ("/dev/x/a.ts", ""),
            ("/dev/x/aa.ts", ""),
            ("/dev/x/b.ts", ""),
        ],
        false,
    );
    let got = rd(&host, TS_EXTS, &[], &["z/*.ts", "x/*.ts"]);
    assert_eq!(
        got,
        [
            "/dev/z/a.ts",
            "/dev/z/aba.ts",
            "/dev/z/abz.ts",
            "/dev/z/b.ts",
            "/dev/z/bba.ts",
            "/dev/z/bbz.ts",
            "/dev/x/a.ts",
            "/dev/x/aa.ts",
            "/dev/x/b.ts",
        ]
    );
}

// Go: vfsmatch_test.go:TestReadDirectoryMatchesTypeScriptBaselines/recursive wildcards match dotted directories
#[test]
fn baseline_recursive_wildcards_match_dotted_dirs() {
    let got = rd(&dotted_folders_host(), TS_EXTS, &[], &["**/.*/*"]);
    let expected = [
        "/dev/.z/c.ts",
        "/dev/g.min.js/.g/g.ts",
        "/dev/w/.u/e.ts",
        "/dev/x/.y/a.ts",
    ];
    assert_eq!(got.len(), expected.len());
    for want in expected {
        assert!(has(&got, want), "missing {want}");
    }
}

// Go: vfsmatch_test.go:TestReadDirectoryMatchesTypeScriptBaselines/common package folders implicitly excluded with wildcard
#[test]
fn baseline_common_package_folders_implicitly_excluded_wildcard() {
    let host = MapFs::from_map(
        [
            ("/dev/a.ts", ""),
            ("/dev/a.d.ts", ""),
            ("/dev/a.js", ""),
            ("/dev/b.ts", ""),
            ("/dev/x/a.ts", ""),
            ("/dev/node_modules/a.ts", ""),
            ("/dev/bower_components/a.ts", ""),
            ("/dev/jspm_packages/a.ts", ""),
        ],
        false,
    );
    let got = rd(&host, TS_EXTS, &[], &["**/a.ts"]);
    assert_eq!(got, ["/dev/a.ts", "/dev/x/a.ts"]);
}

// Go: vfsmatch_test.go:TestReadDirectoryMatchesTypeScriptBaselines/js wildcard excludes min js files
#[test]
fn baseline_js_wildcard_excludes_min_js() {
    let host = MapFs::from_map(
        [
            ("/dev/js/a.js", ""),
            ("/dev/js/b.js", ""),
            ("/dev/js/d.min.js", ""),
            ("/dev/js/ab.min.js", ""),
        ],
        false,
    );
    let got = rd(&host, &[".js"], &[], &["js/*"]);
    assert_eq!(got, ["/dev/js/a.js", "/dev/js/b.js"]);
}

// Go: vfsmatch_test.go:TestReadDirectoryMatchesTypeScriptBaselines/explicit min js pattern includes min files
#[test]
fn baseline_explicit_min_js_pattern() {
    let host = MapFs::from_map(
        [
            ("/dev/js/a.js", ""),
            ("/dev/js/b.js", ""),
            ("/dev/js/d.min.js", ""),
            ("/dev/js/ab.min.js", ""),
        ],
        false,
    );
    let got = rd(&host, &[".js"], &[], &["js/*.min.js"]);
    assert_eq!(got.len(), 2);
    assert!(has(&got, "/dev/js/ab.min.js"));
    assert!(has(&got, "/dev/js/d.min.js"));
}

// Go: vfsmatch_test.go:TestReadDirectoryMatchesTypeScriptBaselines/literal excludes baseline
#[test]
fn baseline_literal_excludes() {
    let got = rd(
        &case_insensitive_host(),
        TS_EXTS,
        &["b.ts"],
        &["a.ts", "b.ts"],
    );
    assert_eq!(got, ["/dev/a.ts"]);
}

// Go: vfsmatch_test.go:TestReadDirectoryMatchesTypeScriptBaselines/recursive directory pattern baseline
#[test]
fn baseline_recursive_directory_pattern() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["**/a.ts"]);
    assert_eq!(
        got,
        ["/dev/a.ts", "/dev/x/a.ts", "/dev/x/y/a.ts", "/dev/z/a.ts"]
    );
}

// Go: vfsmatch_test.go:TestReadDirectoryMatchesTypeScriptBaselines/case sensitive baseline
#[test]
fn baseline_case_sensitive() {
    let got = rd(&case_sensitive_host(), TS_EXTS, &[], &["**/A.ts"]);
    assert_eq!(got, ["/dev/A.ts"]);
}

// Go: vfsmatch_test.go:TestReadDirectoryMatchesTypeScriptBaselines/implicit glob expansion baseline
#[test]
fn baseline_implicit_glob_expansion() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["z"]);
    assert_eq!(
        got,
        [
            "/dev/z/a.ts",
            "/dev/z/aba.ts",
            "/dev/z/abz.ts",
            "/dev/z/b.ts",
            "/dev/z/bba.ts",
            "/dev/z/bbz.ts",
        ]
    );
}

// Go: vfsmatch_test.go:TestReadDirectoryMatchesTypeScriptBaselines/multiple recursive directory patterns baseline
#[test]
fn baseline_multiple_recursive_dir_patterns() {
    let got = rd(&case_insensitive_host(), TS_EXTS, &[], &["**/x/**/*"]);
    for p in [
        "/dev/x/a.ts",
        "/dev/x/aa.ts",
        "/dev/x/b.ts",
        "/dev/x/y/a.ts",
        "/dev/x/y/b.ts",
    ] {
        assert!(has(&got, p), "missing {p}");
    }
}

// ---- TestSpecMatcher series ------------------------------------------------

fn specs_to_vec(specs: &[&str]) -> Vec<String> {
    v(specs)
}

// Go: vfsmatch_test.go:TestSpecMatcher
#[test]
fn spec_matcher_match_string_groups() {
    // simple wildcard
    let m = new_spec_matcher(&specs_to_vec(&["*.ts"]), "/project", Usage::Files, true).unwrap();
    for p in ["/project/a.ts", "/project/b.ts", "/project/foo.ts"] {
        assert!(m.match_string(p));
    }
    for p in ["/project/a.js", "/project/sub/a.ts"] {
        assert!(!m.match_string(p));
    }

    // recursive wildcard
    let m = new_spec_matcher(&specs_to_vec(&["**/*.ts"]), "/project", Usage::Files, true).unwrap();
    for p in [
        "/project/a.ts",
        "/project/sub/a.ts",
        "/project/sub/deep/a.ts",
    ] {
        assert!(m.match_string(p));
    }
    assert!(!m.match_string("/project/a.js"));

    // exclude pattern
    let m = new_spec_matcher(
        &specs_to_vec(&["node_modules"]),
        "/project",
        Usage::Exclude,
        true,
    )
    .unwrap();
    assert!(m.match_string("/project/node_modules/foo"));
    assert!(!m.match_string("/project/node_modules"));
    assert!(!m.match_string("/project/src"));

    // case insensitive
    let m = new_spec_matcher(&specs_to_vec(&["*.ts"]), "/project", Usage::Files, false).unwrap();
    assert!(m.match_string("/project/A.TS"));
    assert!(m.match_string("/project/B.Ts"));
    assert!(!m.match_string("/project/a.js"));

    // multiple specs
    let m = new_spec_matcher(
        &specs_to_vec(&["*.ts", "*.tsx"]),
        "/project",
        Usage::Files,
        true,
    )
    .unwrap();
    assert!(m.match_string("/project/a.ts"));
    assert!(m.match_string("/project/b.tsx"));
    assert!(!m.match_string("/project/a.js"));
}

// Go: vfsmatch_test.go:TestSpecMatcher_MatchString
#[test]
fn spec_matcher_match_string_expected() {
    let m = new_spec_matcher(&specs_to_vec(&["*.ts"]), "/project", Usage::Files, true).unwrap();
    assert_eq!(
        ["/project/a.ts", "/project/sub/a.ts", "/project/a.js"].map(|p| m.match_string(p)),
        [true, false, false]
    );

    let m = new_spec_matcher(&specs_to_vec(&["**/*.ts"]), "/project", Usage::Files, true).unwrap();
    assert_eq!(
        ["/project/a.ts", "/project/sub/a.ts", "/project/a.js"].map(|p| m.match_string(p)),
        [true, true, false]
    );

    let m = new_spec_matcher(
        &specs_to_vec(&["node_modules"]),
        "/project",
        Usage::Exclude,
        true,
    )
    .unwrap();
    assert_eq!(
        [
            "/project/node_modules",
            "/project/node_modules/foo",
            "/project/src"
        ]
        .map(|p| m.match_string(p)),
        [false, true, false]
    );
}

// Go: vfsmatch_test.go:TestSingleSpecMatcher_MatchString
#[test]
fn single_spec_match_string() {
    let m = new_spec_matcher(&specs_to_vec(&["*.ts"]), "/project", Usage::Files, true).unwrap();
    assert_eq!(
        ["/project/a.ts", "/project/sub/a.ts", "/project/a.js"].map(|p| m.match_string(p)),
        [true, false, false]
    );

    let m = new_spec_matcher(&specs_to_vec(&["**"]), "/project", Usage::Exclude, true).unwrap();
    assert_eq!(
        ["/project/a.ts", "/project/sub/a.ts"].map(|p| m.match_string(p)),
        [true, true]
    );
}

// Go: vfsmatch_test.go:TestSpecMatchers_MatchIndex
#[test]
fn spec_matchers_match_index() {
    let m = new_spec_matcher(
        &specs_to_vec(&["*.ts", "*.tsx"]),
        "/project",
        Usage::Files,
        true,
    )
    .unwrap();
    assert_eq!(
        ["/project/a.ts", "/project/a.tsx", "/project/a.js"].map(|p| m.match_index(p)),
        [0, 1, -1]
    );

    let m = new_spec_matcher(
        &specs_to_vec(&["node_modules", "bower_components"]),
        "/project",
        Usage::Exclude,
        true,
    )
    .unwrap();
    assert_eq!(
        [
            "/project/node_modules",
            "/project/node_modules/foo",
            "/project/bower_components",
            "/project/bower_components/bar",
            "/project/src",
        ]
        .map(|p| m.match_index(p)),
        [-1, 0, -1, 1, -1]
    );
}

// Go: vfsmatch_test.go:TestSingleSpecMatcher
#[test]
fn single_spec_matcher_variants() {
    let m = new_spec_matcher(&specs_to_vec(&["*.ts"]), "/project", Usage::Files, true).unwrap();
    assert!(m.match_string("/project/a.ts"));
    assert!(!m.match_string("/project/a.js"));

    // trailing ** non-exclude returns nil
    assert!(new_spec_matcher(&specs_to_vec(&["**"]), "/project", Usage::Files, true).is_none());

    // trailing ** exclude works
    let m = new_spec_matcher(&specs_to_vec(&["**"]), "/project", Usage::Exclude, true).unwrap();
    assert!(m.match_string("/project/anything"));
    assert!(m.match_string("/project/deep/path"));
}

// Go: vfsmatch_test.go:TestSpecMatchers
#[test]
fn spec_matchers_multiple_index_and_empty() {
    let m = new_spec_matcher(
        &specs_to_vec(&["*.ts", "*.tsx", "*.js"]),
        "/project",
        Usage::Files,
        true,
    )
    .unwrap();
    assert_eq!(m.match_index("/project/a.ts"), 0);
    assert_eq!(m.match_index("/project/b.tsx"), 1);
    assert_eq!(m.match_index("/project/c.js"), 2);
    assert_eq!(m.match_index("/project/d.css"), -1);

    assert!(new_spec_matcher(&[], "/project", Usage::Files, true).is_none());
}

// ---- TestGlobPatternInternals ----------------------------------------------

// Go: vfsmatch_test.go:TestGlobPatternInternals/nextPathPart handles consecutive slashes
#[test]
fn glob_next_path_part_consecutive_slashes() {
    let path = "/dev//foo///bar";
    let (part, offset, ok) = next_path_part_parts(path, "", 0);
    assert!(ok);
    assert_eq!(part, "");
    assert_eq!(offset, 1);

    let (part, offset, ok) = next_path_part_parts(path, "", 1);
    assert!(ok);
    assert_eq!(part, "dev");

    let (part, offset, ok) = next_path_part_parts(path, "", offset);
    assert!(ok);
    assert_eq!(part, "foo");

    let (part, _, ok) = next_path_part_parts(path, "", offset);
    assert!(ok);
    assert_eq!(part, "bar");
}

// Go: vfsmatch_test.go:TestGlobPatternInternals/nextPathPart handles path ending with slashes
#[test]
fn glob_next_path_part_trailing_slashes() {
    let path = "/dev/";
    let (_, offset, ok) = next_path_part_parts(path, "", 0);
    assert!(ok);
    let (_, offset, ok) = next_path_part_parts(path, "", offset);
    assert!(ok);
    let (_, _, ok) = next_path_part_parts(path, "", offset);
    assert!(!ok);
}

// Go: vfsmatch_test.go:TestGlobPatternInternals/nextPathPartParts handles empty prefix
#[test]
fn glob_next_path_part_empty_prefix() {
    let path = "/dev//foo";
    let (part, offset, ok) = next_path_part_parts("", path, 0);
    assert!(ok);
    assert_eq!(part, "");
    assert_eq!(offset, 1);
    let (part, offset, ok) = next_path_part_parts("", path, offset);
    assert!(ok);
    assert_eq!(part, "dev");
    let (part, _, ok) = next_path_part_parts("", path, offset);
    assert!(ok);
    assert_eq!(part, "foo");
}

// Go: vfsmatch_test.go:TestGlobPatternInternals/nextPathPartParts returns not ok when only slashes remain
#[test]
fn glob_next_path_part_only_slashes_remain() {
    let prefix = "/dev/";
    let suffix = "foo";
    let (_, offset, ok) = next_path_part_parts(prefix, suffix, 0);
    assert!(ok);
    let (part, offset, ok) = next_path_part_parts(prefix, suffix, offset);
    assert!(ok);
    assert_eq!(part, "dev");
    let (part, offset, ok) = next_path_part_parts(prefix, suffix, offset);
    assert!(ok);
    assert_eq!(part, "foo");
    assert_eq!(offset, prefix.len() + suffix.len());
    let (_, _, ok) = next_path_part_parts(prefix, suffix, offset);
    assert!(!ok);
}

// Go: vfsmatch_test.go:TestGlobPatternInternals/nextPathPartParts parses from suffix region
#[test]
fn glob_next_path_part_from_suffix() {
    let prefix = "/";
    let suffix = "a";
    let (part, offset, ok) = next_path_part_parts(prefix, suffix, 0);
    assert!(ok);
    assert_eq!(part, "");
    assert_eq!(offset, 1);
    let (part, _, ok) = next_path_part_parts(prefix, suffix, offset);
    assert!(ok);
    assert_eq!(part, "a");
}

// Go: vfsmatch_test.go:TestGlobPatternInternals/question mark segment at end of string
#[test]
fn glob_question_mark_at_end() {
    let p = compile_glob_pattern("a?", "/", Usage::Files, true).unwrap();
    assert!(p.matches("/ab"));
    assert!(!p.matches("/a"));
}

// Go: vfsmatch_test.go:TestGlobPatternInternals/star segment with complex pattern
#[test]
fn glob_star_complex_pattern() {
    let p = compile_glob_pattern("a*b*c", "/", Usage::Files, true).unwrap();
    assert!(p.matches("/abc"));
    assert!(p.matches("/aXbYc"));
    assert!(p.matches("/aXXXbYYYc"));
    assert!(!p.matches("/aXbY"));
}

// Go: vfsmatch_test.go:TestGlobPatternInternals/ensureTrailingSlash with existing slash
#[test]
fn glob_ensure_trailing_slash_existing() {
    assert_eq!(ensure_trailing_slash("/dev/"), "/dev/");
    assert_eq!(ensure_trailing_slash("/"), "/");
}

// Go: vfsmatch_test.go:TestGlobPatternInternals/ensureTrailingSlash with empty string
#[test]
fn glob_ensure_trailing_slash_empty() {
    assert_eq!(ensure_trailing_slash(""), "");
}

// Go: vfsmatch_test.go:TestGlobPatternInternals/literal component with package folder in include
#[test]
fn glob_literal_with_package_folder_include() {
    let host = MapFs::from_map([("/dev/node_modules/pkg/index.ts", "")], false);
    let got = read_directory(
        &host,
        "/",
        "/dev",
        &v(&[".ts"]),
        &[],
        &v(&["node_modules/pkg/index.ts"]),
        UNLIMITED_DEPTH,
    );
    assert!(has(&got, "/dev/node_modules/pkg/index.ts"));
}

// ---- TestMatchSegmentsEdgeCases --------------------------------------------

// Go: vfsmatch_test.go:TestMatchSegmentsEdgeCases/question mark before slash in string
#[test]
fn match_seg_question_before_slash() {
    let p = compile_glob_pattern("a?b", "/", Usage::Files, true).unwrap();
    assert!(p.matches("/aXb"));
    assert!(!p.matches("/ab"));
    assert!(!p.matches("/aXYb"));
}

// Go: vfsmatch_test.go:TestMatchSegmentsEdgeCases/star with no trailing content
#[test]
fn match_seg_star_no_trailing() {
    let p = compile_glob_pattern("a*", "/", Usage::Files, true).unwrap();
    assert!(p.matches("/a"));
    assert!(p.matches("/abc"));
    assert!(p.matches("/aXYZ"));
}

// Go: vfsmatch_test.go:TestMatchSegmentsEdgeCases/multiple stars in pattern
#[test]
fn match_seg_multiple_stars() {
    let p = compile_glob_pattern("*a*", "/", Usage::Files, true).unwrap();
    assert!(p.matches("/a"));
    assert!(p.matches("/Xa"));
    assert!(p.matches("/aX"));
    assert!(p.matches("/XaY"));
    assert!(!p.matches("/XYZ"));
}

// Go: vfsmatch_test.go:TestMatchSegmentsEdgeCases/multiple stars requiring backtracking
#[test]
fn match_seg_backtracking() {
    let p1 = compile_glob_pattern("*a*a", "/", Usage::Files, true).unwrap();
    assert!(p1.matches("/aa"));
    assert!(p1.matches("/Xaa"));
    assert!(p1.matches("/aXa"));
    assert!(p1.matches("/XaYa"));
    assert!(p1.matches("/aaaa"));
    assert!(!p1.matches("/a"));
    assert!(!p1.matches("/Xa"));
    assert!(!p1.matches("/aX"));
    assert!(!p1.matches("/XaYaZ"));

    let p2 = compile_glob_pattern("*a*b*c", "/", Usage::Files, true).unwrap();
    assert!(p2.matches("/abc"));
    assert!(p2.matches("/XaYbZc"));
    assert!(p2.matches("/aXbYc"));
    assert!(p2.matches("/aaabbbccc"));
    assert!(!p2.matches("/ab"));
    assert!(!p2.matches("/ac"));
    assert!(!p2.matches("/cba"));
    assert!(!p2.matches("/abcX"));

    let p3 = compile_glob_pattern("*a*a*a", "/", Usage::Files, true).unwrap();
    assert!(p3.matches("/aaa"));
    assert!(p3.matches("/aXaYa"));
    assert!(p3.matches("/XaYaZa"));
    assert!(!p3.matches("/aa"));
    assert!(!p3.matches("/aaX"));

    let p4 = compile_glob_pattern("a*b*a", "/", Usage::Files, true).unwrap();
    assert!(p4.matches("/aba"));
    assert!(p4.matches("/aXbYa"));
    assert!(p4.matches("/abba"));
    assert!(!p4.matches("/ab"));
    assert!(!p4.matches("/aba "));
    assert!(!p4.matches("/Xaba"));
}

// Go: vfsmatch_test.go:TestMatchSegmentsEdgeCases/pathological pattern performance
#[test]
fn match_seg_pathological_perf() {
    let p = compile_glob_pattern("*a*a*a*a*b", "/", Usage::Files, true).unwrap();
    assert!(!p.matches("/aaaaaaaaaaaaaaaa"));
    assert!(!p.matches("/aaaaaaaaaaaaaaaaX"));
    assert!(p.matches("/aaaab"));
    assert!(p.matches("/XaYaZaWab"));
}

// Go: vfsmatch_test.go:TestMatchSegmentsEdgeCases/literal segment not matching
#[test]
fn match_seg_literal_not_matching() {
    let p = compile_glob_pattern("abcdefgh.ts", "/", Usage::Files, true).unwrap();
    assert!(!p.matches("/abc.ts"));
    assert!(p.matches("/abcdefgh.ts"));
}

// Go: vfsmatch_test.go:TestMatchSegmentsEdgeCases/question mark matches multi-byte unicode rune
#[test]
fn match_seg_question_multibyte() {
    let p1 = compile_glob_pattern("?.ts", "/", Usage::Files, true).unwrap();
    assert!(p1.matches("/a.ts"));
    assert!(p1.matches("/é.ts"));
    assert!(p1.matches("/中.ts"));
    assert!(p1.matches("/🎉.ts"));
    assert!(!p1.matches("/.ts"));
    assert!(!p1.matches("/ab.ts"));

    let p2 = compile_glob_pattern("??.ts", "/", Usage::Files, true).unwrap();
    assert!(p2.matches("/ab.ts"));
    assert!(p2.matches("/é中.ts"));
    assert!(p2.matches("/🎉é.ts"));
    assert!(!p2.matches("/a.ts"));
    assert!(!p2.matches("/abc.ts"));
}

// Go: vfsmatch_test.go:TestMatchSegmentsEdgeCases/star matches multi-byte unicode runes correctly
#[test]
fn match_seg_star_multibyte() {
    let p = compile_glob_pattern("*é.ts", "/", Usage::Files, true).unwrap();
    assert!(p.matches("/é.ts"));
    assert!(p.matches("/café.ts"));
    assert!(!p.matches("/cafe.ts"));

    let p2 = compile_glob_pattern("*🎉*", "/", Usage::Files, true).unwrap();
    assert!(p2.matches("/🎉"));
    assert!(p2.matches("/a🎉b"));
    assert!(!p2.matches("/abc"));
}

// ---- misc ------------------------------------------------------------------

// Go: vfsmatch_test.go:TestReadDirectoryConsecutiveSlashes
#[test]
fn readdir_consecutive_slashes() {
    let host = MapFs::from_map([("/dev/a.ts", ""), ("/dev/x/b.ts", "")], false);
    let got = read_directory(
        &host,
        "/",
        "/dev",
        &v(&[".ts"]),
        &[],
        &v(&["**/*.ts"]),
        UNLIMITED_DEPTH,
    );
    assert!(got.len() >= 2);
    assert!(has(&got, "/dev/a.ts"));
    assert!(has(&got, "/dev/x/b.ts"));
}

// Go: vfsmatch_test.go:TestGlobPatternLiteralWithPackageFolders/wildcard skips package folders
#[test]
fn glob_literal_pkg_wildcard_skips() {
    let host = MapFs::from_map([("/dev/a.ts", ""), ("/dev/node_modules/b.ts", "")], false);
    let got = read_directory(
        &host,
        "/",
        "/dev",
        &v(&[".ts"]),
        &[],
        &v(&["*/*.ts"]),
        UNLIMITED_DEPTH,
    );
    assert!(!has(&got, "/dev/node_modules/b.ts"));
}

// Go: vfsmatch_test.go:TestGlobPatternLiteralWithPackageFolders/explicit literal includes package folder
#[test]
fn glob_literal_pkg_explicit_includes() {
    let host = MapFs::from_map([("/dev/node_modules/b.ts", "")], false);
    let got = read_directory(
        &host,
        "/",
        "/dev",
        &v(&[".ts"]),
        &[],
        &v(&["node_modules/b.ts"]),
        UNLIMITED_DEPTH,
    );
    assert!(has(&got, "/dev/node_modules/b.ts"));
}

// Go: vfsmatch_test.go:TestGetBasePathsCaseSensitivity/case-sensitive does not dedup differently-cased paths
#[test]
fn get_base_paths_case_sensitive_no_dedup() {
    let base_paths = get_base_paths("/root", &v(&["../Other/**/*.ts", "../other/**/*.ts"]), true);
    assert!(
        base_paths.iter().any(|p| p == "/Other"),
        "expected /Other: {base_paths:?}"
    );
    assert!(
        base_paths.iter().any(|p| p == "/other"),
        "expected /other: {base_paths:?}"
    );
}

// Go: vfsmatch_test.go:TestGetBasePathsCaseSensitivity/case-insensitive dedups differently-cased paths
#[test]
fn get_base_paths_case_insensitive_dedup() {
    let base_paths = get_base_paths(
        "/root",
        &v(&["../Other/**/*.ts", "../other/**/*.ts"]),
        false,
    );
    let count = base_paths
        .iter()
        .filter(|p| *p == "/Other" || *p == "/other")
        .count();
    assert!(count <= 1, "expected at most one: {base_paths:?}");
}
