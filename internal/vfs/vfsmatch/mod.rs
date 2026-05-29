//! tsconfig `include`/`exclude` glob matching without regular expressions.
//!
//! 1:1 port of Go `internal/vfs/vfsmatch/vfsmatch.go` (and its generated
//! `Usage` stringer). Implements the algorithm described in the upstream
//! `MATCHING_ALGORITHM.md`.
//!
//! DIVERGENCE(port): `nextPathPart` returns an owned `String` rather than a
//! borrowed slice to avoid lifetime entanglement (`// PERF(port)`); the
//! behavior is identical.

use std::collections::HashSet;
use std::fmt;

use tsgo_tspath::{
    combine_paths, contains_path, file_extension_is_one_of, get_canonical_file_name,
    get_directory_path, get_normalized_path_components, has_extension, is_rooted_disk_path,
    normalize_path, remove_trailing_directory_separator, ComparePathsOptions,
};

use crate::Fs;

/// How a set of glob patterns is being used: to match files, to match
/// directories (for pruning), or to exclude paths.
///
/// # Examples
/// ```
/// use tsgo_vfs::vfsmatch::Usage;
/// assert_eq!(Usage::Files.to_string(), "Files");
/// assert_eq!(Usage::Exclude.to_string(), "Exclude");
/// ```
///
/// Side effects: none (pure value type).
// Go: internal/vfs/vfsmatch/vfsmatch.go:Usage
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Usage {
    /// Patterns used to select files.
    Files,
    /// Patterns used to decide which directories to descend into.
    Directories,
    /// Patterns used to exclude paths.
    Exclude,
}

impl fmt::Display for Usage {
    // Go: internal/vfs/vfsmatch/stringer_generated.go
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Usage::Files => "Files",
            Usage::Directories => "Directories",
            Usage::Exclude => "Exclude",
        };
        f.write_str(s)
    }
}

/// Sentinel depth meaning "no depth limit".
///
/// # Examples
/// ```
/// assert!(tsgo_vfs::vfsmatch::UNLIMITED_DEPTH > 1_000_000);
/// ```
// Go: internal/vfs/vfsmatch/vfsmatch.go:UnlimitedDepth
pub const UNLIMITED_DEPTH: i32 = i32::MAX;

/// Lists the files under `path` that match the `includes` globs and are not
/// excluded, restricted to `extensions` (when non-empty) and `depth`.
///
/// # Examples
/// ```
/// use tsgo_vfs::vfsmatch::{read_directory, UNLIMITED_DEPTH};
/// use tsgo_vfs::vfstest::MapFs;
/// let host = MapFs::from_map([("/dev/a.ts", ""), ("/dev/a.js", "")], true);
/// let files = read_directory(&host, "/dev", "/dev", &[".ts".into()], &[], &["*.ts".into()], UNLIMITED_DEPTH);
/// assert_eq!(files, ["/dev/a.ts"]);
/// ```
///
/// Side effects: reads directories from `host`.
// Go: internal/vfs/vfsmatch/vfsmatch.go:ReadDirectory
pub fn read_directory(
    host: &dyn Fs,
    current_dir: &str,
    path: &str,
    extensions: &[String],
    excludes: &[String],
    includes: &[String],
    depth: i32,
) -> Vec<String> {
    match_files(
        path,
        extensions,
        excludes,
        includes,
        host.use_case_sensitive_file_names(),
        current_dir,
        depth,
        host,
    )
}

/// Reports whether `last_path_component` is implicitly a glob.
///
/// An include path `foo` is implicitly `foo/**/*` when its last component has no
/// extension and contains no glob characters.
///
/// # Examples
/// ```
/// use tsgo_vfs::vfsmatch::is_implicit_glob;
/// assert!(is_implicit_glob("src"));
/// assert!(!is_implicit_glob("foo.ts"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/vfs/vfsmatch/vfsmatch.go:IsImplicitGlob
pub fn is_implicit_glob(last_path_component: &str) -> bool {
    !last_path_component.contains(['.', '*', '?'])
}

fn get_include_base_path(absolute: &str) -> String {
    match absolute.find(['*', '?']) {
        None => {
            if !has_extension(absolute) {
                absolute.to_string()
            } else {
                remove_trailing_directory_separator(&get_directory_path(absolute)).to_string()
            }
        }
        Some(offset) => {
            let cut = absolute[..offset].rfind('/').unwrap_or(0);
            absolute[..cut].to_string()
        }
    }
}

// Go: internal/vfs/vfsmatch/vfsmatch.go:getBasePaths
fn get_base_paths(
    path: &str,
    includes: &[String],
    use_case_sensitive_file_names: bool,
) -> Vec<String> {
    let mut base_paths = vec![path.to_string()];
    if includes.is_empty() {
        return base_paths;
    }

    let options = ComparePathsOptions {
        current_directory: path.to_string(),
        use_case_sensitive_file_names,
    };
    let comparer = options.get_comparer();

    let mut include_base_paths: Vec<String> = includes
        .iter()
        .map(|include| {
            let absolute = if is_rooted_disk_path(include) {
                include.clone()
            } else {
                normalize_path(&combine_paths(path, &[include]))
            };
            get_include_base_path(&absolute)
        })
        .collect();

    include_base_paths.sort_by(|a, b| comparer(a, b));

    for include_base_path in include_base_paths {
        if base_paths
            .iter()
            .all(|basepath| !contains_path(basepath, &include_base_path, &options))
        {
            base_paths.push(include_base_path);
        }
    }

    base_paths
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ComponentKind {
    Literal,
    Wildcard,
    DoubleAsterisk,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SegmentKind {
    Literal,
    Star,
    Question,
}

#[derive(Clone)]
struct Segment {
    kind: SegmentKind,
    literal: String,
}

#[derive(Clone)]
struct Component {
    kind: ComponentKind,
    literal: String,
    segments: Vec<Segment>,
    skip_package_folders: bool,
}

/// A compiled glob pattern (a list of path-segment matchers).
///
/// Side effects: none (pure value type).
// Go: internal/vfs/vfsmatch/vfsmatch.go:globPattern
#[derive(Clone)]
struct GlobPattern {
    components: Vec<Component>,
    is_exclude: bool,
    case_sensitive: bool,
    exclude_min_js: bool,
}

// Go: internal/vfs/vfsmatch/vfsmatch.go:compileGlobPattern
fn compile_glob_pattern(
    spec: &str,
    base_path: &str,
    usage: Usage,
    case_sensitive: bool,
) -> Option<GlobPattern> {
    let mut parts = get_normalized_path_components(spec, base_path);

    if usage != Usage::Exclude && parts.last().map(String::as_str) == Some("**") {
        return None;
    }

    parts[0] = remove_trailing_directory_separator(&parts[0]).to_string();

    if is_implicit_glob(parts.last().map(String::as_str).unwrap_or("")) {
        parts.push("**".to_string());
        parts.push("*".to_string());
    }

    let is_include = usage != Usage::Exclude;
    let components = parts
        .iter()
        .map(|part| parse_component(part, is_include))
        .collect();

    Some(GlobPattern {
        components,
        is_exclude: usage == Usage::Exclude,
        case_sensitive,
        exclude_min_js: usage == Usage::Files,
    })
}

// Go: internal/vfs/vfsmatch/vfsmatch.go:parseComponent
fn parse_component(s: &str, is_include: bool) -> Component {
    if s == "**" {
        return Component {
            kind: ComponentKind::DoubleAsterisk,
            literal: String::new(),
            segments: Vec::new(),
            skip_package_folders: false,
        };
    }
    if !s.contains(['*', '?']) {
        return Component {
            kind: ComponentKind::Literal,
            literal: s.to_string(),
            segments: Vec::new(),
            skip_package_folders: false,
        };
    }
    Component {
        kind: ComponentKind::Wildcard,
        literal: String::new(),
        segments: parse_segments(s),
        skip_package_folders: is_include,
    }
}

// Go: internal/vfs/vfsmatch/vfsmatch.go:parseSegments
fn parse_segments(s: &str) -> Vec<Segment> {
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut start = 0usize;
    for i in 0..bytes.len() {
        if bytes[i] == b'*' || bytes[i] == b'?' {
            if i > start {
                result.push(Segment {
                    kind: SegmentKind::Literal,
                    literal: s[start..i].to_string(),
                });
            }
            result.push(Segment {
                kind: if bytes[i] == b'*' {
                    SegmentKind::Star
                } else {
                    SegmentKind::Question
                },
                literal: String::new(),
            });
            start = i + 1;
        }
    }
    if start < bytes.len() {
        result.push(Segment {
            kind: SegmentKind::Literal,
            literal: s[start..].to_string(),
        });
    }
    result
}

impl GlobPattern {
    fn matches(&self, path: &str) -> bool {
        self.match_path_parts(path, "", 0, 0, false)
    }

    fn matches_parts(&self, prefix: &str, suffix: &str) -> bool {
        self.match_path_parts(prefix, suffix, 0, 0, false)
    }

    fn matches_prefix_parts(&self, prefix: &str, suffix: &str) -> bool {
        self.match_path_parts(prefix, suffix, 0, 0, true)
    }

    // Go: internal/vfs/vfsmatch/vfsmatch.go:matchPathParts
    fn match_path_parts(
        &self,
        prefix: &str,
        suffix: &str,
        mut path_offset: usize,
        mut comp_idx: usize,
        prefix_only: bool,
    ) -> bool {
        loop {
            let (path_part, next_offset, ok) = next_path_part_parts(prefix, suffix, path_offset);
            if !ok {
                if prefix_only {
                    return true;
                }
                return self.pattern_satisfied(comp_idx);
            }

            if comp_idx >= self.components.len() {
                return self.is_exclude && !prefix_only;
            }

            let comp = &self.components[comp_idx];
            match comp.kind {
                ComponentKind::DoubleAsterisk => {
                    if self.match_path_parts(prefix, suffix, path_offset, comp_idx + 1, prefix_only)
                    {
                        return true;
                    }
                    if !self.is_exclude
                        && (is_hidden_path(&path_part) || is_package_folder(&path_part))
                    {
                        return false;
                    }
                    path_offset = next_offset;
                    continue;
                }
                ComponentKind::Literal => {
                    if !self.strings_equal(&comp.literal, &path_part) {
                        return false;
                    }
                }
                ComponentKind::Wildcard => {
                    if comp.skip_package_folders && is_package_folder(&path_part) {
                        return false;
                    }
                    if !self.match_wildcard(&comp.segments, &path_part) {
                        return false;
                    }
                }
            }

            path_offset = next_offset;
            comp_idx += 1;
        }
    }

    // Go: internal/vfs/vfsmatch/vfsmatch.go:patternSatisfied
    fn pattern_satisfied(&self, comp_idx: usize) -> bool {
        self.components[comp_idx..]
            .iter()
            .all(|c| c.kind == ComponentKind::DoubleAsterisk)
    }

    // Go: internal/vfs/vfsmatch/vfsmatch.go:matchWildcard
    fn match_wildcard(&self, segs: &[Segment], s: &str) -> bool {
        if !self.is_exclude
            && !segs.is_empty()
            && is_hidden_path(s)
            && (segs[0].kind == SegmentKind::Star || segs[0].kind == SegmentKind::Question)
        {
            return false;
        }

        // Fast path: single * followed by a literal suffix (e.g., "*.ts").
        if segs.len() == 2
            && segs[0].kind == SegmentKind::Star
            && segs[1].kind == SegmentKind::Literal
        {
            let suffix = &segs[1].literal;
            if s.len() < suffix.len() || !self.strings_equal(suffix, &s[s.len() - suffix.len()..]) {
                return false;
            }
            return self.should_include_min_js(s, segs);
        }

        self.match_segments(segs, s) && self.should_include_min_js(s, segs)
    }

    // Go: internal/vfs/vfsmatch/vfsmatch.go:matchSegments
    fn match_segments(&self, segs: &[Segment], s: &str) -> bool {
        let bytes = s.as_bytes();
        let mut seg_idx = 0usize;
        let mut s_idx = 0usize;
        let mut star_seg_idx: isize = -1;
        let mut star_s_idx = 0usize;

        while s_idx < bytes.len() {
            if seg_idx < segs.len() {
                let seg = &segs[seg_idx];
                match seg.kind {
                    SegmentKind::Literal => {
                        let end = s_idx + seg.literal.len();
                        // Compare on raw bytes: Go matches byte-wise, so `end`
                        // may land inside a multi-byte UTF-8 sequence during star
                        // backtracking (slicing `&s[..]` there would panic).
                        if end <= bytes.len()
                            && self.bytes_equal(seg.literal.as_bytes(), &bytes[s_idx..end])
                        {
                            s_idx = end;
                            seg_idx += 1;
                            continue;
                        }
                    }
                    SegmentKind::Question => {
                        if bytes[s_idx] != b'/' {
                            let size = utf8_size(&s[s_idx..]);
                            s_idx += size;
                            seg_idx += 1;
                            continue;
                        }
                    }
                    SegmentKind::Star => {
                        star_seg_idx = seg_idx as isize;
                        star_s_idx = s_idx;
                        seg_idx += 1;
                        continue;
                    }
                }
            }

            if star_seg_idx >= 0 && star_s_idx < bytes.len() && bytes[star_s_idx] != b'/' {
                let size = utf8_size(&s[star_s_idx..]);
                star_s_idx += size;
                s_idx = star_s_idx;
                seg_idx = (star_seg_idx + 1) as usize;
                continue;
            }

            return false;
        }

        while seg_idx < segs.len() && segs[seg_idx].kind == SegmentKind::Star {
            seg_idx += 1;
        }
        seg_idx >= segs.len()
    }

    // Go: internal/vfs/vfsmatch/vfsmatch.go:shouldIncludeMinJs
    fn should_include_min_js(&self, filename: &str, segs: &[Segment]) -> bool {
        if !self.exclude_min_js {
            return true;
        }
        if !self.has_min_js_suffix(filename) {
            return true;
        }
        if self.pattern_mentions_min_suffix(segs) {
            return true;
        }
        false
    }

    // Go: internal/vfs/vfsmatch/vfsmatch.go:hasMinJsSuffix
    fn has_min_js_suffix(&self, filename: &str) -> bool {
        const MIN_JS: &str = ".min.js";
        if self.case_sensitive {
            return filename.ends_with(MIN_JS);
        }
        if filename.len() < MIN_JS.len() {
            return false;
        }
        filename[filename.len() - MIN_JS.len()..].eq_ignore_ascii_case(MIN_JS)
    }

    // Go: internal/vfs/vfsmatch/vfsmatch.go:patternMentionsMinSuffix
    fn pattern_mentions_min_suffix(&self, segs: &[Segment]) -> bool {
        for seg in segs {
            if seg.kind != SegmentKind::Literal {
                continue;
            }
            let lit = if self.case_sensitive {
                seg.literal.clone()
            } else {
                seg.literal.to_lowercase()
            };
            if lit.contains(".min.js") || lit.contains(".min.") {
                return true;
            }
        }
        false
    }

    // Go: internal/vfs/vfsmatch/vfsmatch.go:stringsEqual
    fn strings_equal(&self, a: &str, b: &str) -> bool {
        if self.case_sensitive {
            a == b
        } else {
            a.eq_ignore_ascii_case(b)
        }
    }

    // Byte-wise variant of `strings_equal` for segment matching, where a slice
    // boundary can fall inside a multi-byte UTF-8 sequence. ASCII case folding
    // when case-insensitive, matching Go's `stringsEqual`.
    fn bytes_equal(&self, a: &[u8], b: &[u8]) -> bool {
        if self.case_sensitive {
            a == b
        } else {
            a.eq_ignore_ascii_case(b)
        }
    }
}

// Returns the byte length of the first UTF-8 character in `s` (>= 1).
fn utf8_size(s: &str) -> usize {
    s.chars().next().map(char::len_utf8).unwrap_or(1)
}

// Go: internal/vfs/vfsmatch/vfsmatch.go:isHiddenPath
fn is_hidden_path(name: &str) -> bool {
    name.starts_with('.')
}

// Go: internal/vfs/vfsmatch/vfsmatch.go:isPackageFolder
fn is_package_folder(name: &str) -> bool {
    name.eq_ignore_ascii_case("node_modules")
        || name.eq_ignore_ascii_case("jspm_packages")
        || name.eq_ignore_ascii_case("bower_components")
}

// Go: internal/vfs/vfsmatch/vfsmatch.go:ensureTrailingSlash
fn ensure_trailing_slash(s: &str) -> String {
    if !s.is_empty() && !s.ends_with('/') {
        format!("{s}/")
    } else {
        s.to_string()
    }
}

// Go: internal/vfs/vfsmatch/vfsmatch.go:nextPathPartSingle
fn next_path_part_single(s: &str, mut offset: usize) -> (String, usize, bool) {
    let bytes = s.as_bytes();
    if offset >= bytes.len() {
        return (String::new(), offset, false);
    }
    if offset == 0 && !bytes.is_empty() && bytes[0] == b'/' {
        return (String::new(), 1, true);
    }
    while offset < bytes.len() && bytes[offset] == b'/' {
        offset += 1;
    }
    if offset >= bytes.len() {
        return (String::new(), offset, false);
    }
    let rest = &s[offset..];
    if let Some(idx) = rest.find('/') {
        return (rest[..idx].to_string(), offset + idx, true);
    }
    (rest.to_string(), bytes.len(), true)
}

// Go: internal/vfs/vfsmatch/vfsmatch.go:nextPathPartParts
fn next_path_part_parts(prefix: &str, suffix: &str, mut offset: usize) -> (String, usize, bool) {
    if suffix.is_empty() {
        return next_path_part_single(prefix, offset);
    }
    if prefix.is_empty() {
        return next_path_part_single(suffix, offset);
    }

    let prefix_bytes = prefix.as_bytes();
    let total_len = prefix.len() + suffix.len();
    if offset >= total_len {
        return (String::new(), offset, false);
    }

    if offset == 0 && prefix_bytes[0] == b'/' {
        return (String::new(), 1, true);
    }

    if offset < prefix.len() {
        while offset < prefix.len() && prefix_bytes[offset] == b'/' {
            offset += 1;
        }
        if offset < prefix.len() {
            let rest = &prefix[offset..];
            // For our call sites, prefix ends in '/', so idx is always found.
            let idx = rest.find('/').unwrap_or(rest.len());
            return (rest[..idx].to_string(), offset + idx, true);
        }
    }

    let s_off = offset - prefix.len();
    if s_off >= suffix.len() {
        return (String::new(), offset, false);
    }
    (suffix[s_off..].to_string(), total_len, true)
}

/// Combines include and exclude patterns to test files and directories.
// Go: internal/vfs/vfsmatch/vfsmatch.go:globMatcher
struct GlobMatcher {
    includes: Vec<GlobPattern>,
    excludes: Vec<GlobPattern>,
    had_includes: bool,
}

// Go: internal/vfs/vfsmatch/vfsmatch.go:newGlobMatcher
fn new_glob_matcher(
    include_specs: &[String],
    exclude_specs: &[String],
    base_path: &str,
    case_sensitive: bool,
    usage: Usage,
) -> GlobMatcher {
    let mut m = GlobMatcher {
        had_includes: !include_specs.is_empty(),
        includes: Vec::new(),
        excludes: Vec::new(),
    };
    for spec in include_specs {
        if let Some(p) = compile_glob_pattern(spec, base_path, usage, case_sensitive) {
            m.includes.push(p);
        }
    }
    for spec in exclude_specs {
        if let Some(p) = compile_glob_pattern(spec, base_path, Usage::Exclude, case_sensitive) {
            m.excludes.push(p);
        }
    }
    m
}

impl GlobMatcher {
    // Go: internal/vfs/vfsmatch/vfsmatch.go:matchesFileParts
    fn matches_file_parts(&self, prefix: &str, suffix: &str) -> Option<usize> {
        for e in &self.excludes {
            if e.matches_parts(prefix, suffix) {
                return None;
            }
        }
        if self.includes.is_empty() {
            return if self.had_includes { None } else { Some(0) };
        }
        for (i, inc) in self.includes.iter().enumerate() {
            if inc.matches_parts(prefix, suffix) {
                return Some(i);
            }
        }
        None
    }

    // Go: internal/vfs/vfsmatch/vfsmatch.go:matchesDirectoryParts
    fn matches_directory_parts(&self, prefix: &str, suffix: &str) -> bool {
        for e in &self.excludes {
            if e.matches_parts(prefix, suffix) {
                return false;
            }
        }
        if self.includes.is_empty() {
            return !self.had_includes;
        }
        self.includes
            .iter()
            .any(|inc| inc.matches_prefix_parts(prefix, suffix))
    }
}

// Go: internal/vfs/vfsmatch/vfsmatch.go:globVisitor
struct GlobVisitor<'a> {
    host: &'a dyn Fs,
    file_matcher: GlobMatcher,
    directory_matcher: GlobMatcher,
    extensions: Vec<String>,
    use_case_sensitive_file_names: bool,
    visited: HashSet<String>,
    results: Vec<Vec<String>>,
}

impl GlobVisitor<'_> {
    // Go: internal/vfs/vfsmatch/vfsmatch.go:globVisitor.visit
    fn visit(&mut self, path: &str, absolute_path: &str, mut depth: i32, resolved_real_path: &str) {
        let real_path = if !resolved_real_path.is_empty() {
            resolved_real_path.to_string()
        } else {
            self.host.realpath(absolute_path)
        };
        let canonical_path =
            get_canonical_file_name(&real_path, self.use_case_sensitive_file_names);
        if self.visited.contains(&canonical_path) {
            return;
        }
        self.visited.insert(canonical_path);

        let entries = self.host.get_accessible_entries(absolute_path);

        let path_prefix = ensure_trailing_slash(path);
        let abs_prefix = ensure_trailing_slash(absolute_path);

        let ext_refs: Vec<&str> = self.extensions.iter().map(String::as_str).collect();
        for file in &entries.files {
            if !ext_refs.is_empty() && !file_extension_is_one_of(file, &ext_refs) {
                continue;
            }
            if let Some(idx) = self.file_matcher.matches_file_parts(&abs_prefix, file) {
                self.results[idx].push(format!("{path_prefix}{file}"));
            }
        }

        if depth != UNLIMITED_DEPTH {
            depth -= 1;
            if depth == 0 {
                return;
            }
        }

        for dir in &entries.directories {
            if !self
                .directory_matcher
                .matches_directory_parts(&abs_prefix, dir)
            {
                continue;
            }
            let abs_dir = format!("{abs_prefix}{dir}");
            let child_real_path = match &entries.symlinks {
                Some(symlinks) => {
                    if !symlinks.contains(dir) {
                        combine_paths(&real_path, &[dir])
                    } else {
                        String::new()
                    }
                }
                None => String::new(),
            };
            let child_path = format!("{path_prefix}{dir}");
            self.visit(&child_path, &abs_dir, depth, &child_real_path);
        }
    }
}

// Go: internal/vfs/vfsmatch/vfsmatch.go:matchFiles
#[allow(clippy::too_many_arguments)]
fn match_files(
    path: &str,
    extensions: &[String],
    excludes: &[String],
    includes: &[String],
    use_case_sensitive_file_names: bool,
    current_directory: &str,
    depth: i32,
    host: &dyn Fs,
) -> Vec<String> {
    let path = normalize_path(path);
    let current_directory = normalize_path(current_directory);
    let absolute_path = combine_paths(&current_directory, &[&path]);

    let file_matcher = new_glob_matcher(
        includes,
        excludes,
        &absolute_path,
        use_case_sensitive_file_names,
        Usage::Files,
    );
    let directory_matcher = new_glob_matcher(
        includes,
        excludes,
        &absolute_path,
        use_case_sensitive_file_names,
        Usage::Directories,
    );

    let bucket_count = file_matcher.includes.len().max(1);
    let mut v = GlobVisitor {
        host,
        file_matcher,
        directory_matcher,
        extensions: extensions.to_vec(),
        use_case_sensitive_file_names,
        visited: HashSet::new(),
        results: vec![Vec::new(); bucket_count],
    };

    for base_path in get_base_paths(&path, includes, use_case_sensitive_file_names) {
        let abs = combine_paths(&current_directory, &[&base_path]);
        v.visit(&base_path, &abs, depth, "");
    }

    if v.results.len() == 1 {
        return v.results.into_iter().next().unwrap();
    }
    v.results.into_iter().flatten().collect()
}

/// Matches paths against one or more compiled glob specs.
///
/// # Examples
/// ```
/// use tsgo_vfs::vfsmatch::{new_spec_matcher, Usage};
/// let m = new_spec_matcher(&["*.ts".into()], "/project", Usage::Files, true).unwrap();
/// assert!(m.match_string("/project/a.ts"));
/// assert!(!m.match_string("/project/a.js"));
/// ```
///
/// Side effects: none (pure).
// Go: internal/vfs/vfsmatch/vfsmatch.go:SpecMatcher
pub struct SpecMatcher {
    patterns: Vec<GlobPattern>,
}

impl SpecMatcher {
    /// Reports whether any pattern matches `path`.
    ///
    /// Side effects: none (pure).
    // Go: internal/vfs/vfsmatch/vfsmatch.go:MatchString
    pub fn match_string(&self, path: &str) -> bool {
        self.patterns.iter().any(|p| p.matches(path))
    }

    /// Returns the index of the first matching pattern, or `-1`.
    ///
    /// Side effects: none (pure).
    // Go: internal/vfs/vfsmatch/vfsmatch.go:MatchIndex
    pub fn match_index(&self, path: &str) -> i32 {
        for (i, p) in self.patterns.iter().enumerate() {
            if p.matches(path) {
                return i as i32;
            }
        }
        -1
    }
}

/// Creates a [`SpecMatcher`] for one or more glob specs, or `None` if no spec
/// compiles to a usable pattern.
///
/// # Examples
/// ```
/// use tsgo_vfs::vfsmatch::{new_spec_matcher, Usage};
/// assert!(new_spec_matcher(&[], "/p", Usage::Files, true).is_none());
/// ```
///
/// Side effects: none (pure).
// Go: internal/vfs/vfsmatch/vfsmatch.go:NewSpecMatcher
pub fn new_spec_matcher(
    specs: &[String],
    base_path: &str,
    usage: Usage,
    use_case_sensitive_file_names: bool,
) -> Option<SpecMatcher> {
    if specs.is_empty() {
        return None;
    }
    let mut patterns = Vec::new();
    for spec in specs {
        if let Some(p) = compile_glob_pattern(spec, base_path, usage, use_case_sensitive_file_names)
        {
            patterns.push(p);
        }
    }
    if patterns.is_empty() {
        None
    } else {
        Some(SpecMatcher { patterns })
    }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
