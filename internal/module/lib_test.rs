use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use tsgo_collections::OrderedMap;
use tsgo_core::compileroptions::{CompilerOptions, ModuleKind, ModuleResolutionKind, ScriptTarget};
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::{Entries, FileInfo, Fs, FsResult, WalkDirFunc};

use super::*;
use crate::test_support::{resolver, StubHost};

// Regression test for https://github.com/microsoft/typescript-go/issues/3526.
//
// Resolving a node_modules import with a trailing slash (`pkg/`) must produce
// the same result as without one.
// Go: internal/module/resolver_test.go:TestResolveModuleNameTrailingSlash
#[test]
fn resolve_module_name_trailing_slash() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r#"{"name":"pkg","main":"main.js","types":"main.d.ts"}"#,
        ),
        (
            "/repo/node_modules/pkg/main.d.ts",
            "export const x: number;",
        ),
        ("/repo/node_modules/pkg/main.js", "exports.x = 1;"),
        ("/repo/src/file.ts", ""),
    ];
    let opts = CompilerOptions {
        module_resolution: ModuleResolutionKind::Bundler,
        module: ModuleKind::EsNext,
        target: ScriptTarget::EsNext,
        ..Default::default()
    };
    let r = resolver(&files, "/repo", opts);
    for name in ["pkg", "pkg/"] {
        let (resolved, _) =
            r.resolve_module_name(name, "/repo/src/file.ts", ModuleKind::EsNext, None);
        assert!(resolved.is_resolved(), "{name:?} failed to resolve");
    }
}

/// A `Fs` wrapper that blocks `file_exists(target_path)` on a gate until
/// released, counting how many threads are waiting. Mirrors Go's `blockingFS`.
struct BlockingFs {
    inner: MapFs,
    target_path: String,
    released: Mutex<bool>,
    cvar: Condvar,
    waiting: AtomicI32,
}

impl BlockingFs {
    fn release(&self) {
        let mut released = self.released.lock().unwrap();
        *released = true;
        self.cvar.notify_all();
    }
}

impl Fs for BlockingFs {
    fn file_exists(&self, path: &str) -> bool {
        if path == self.target_path {
            self.waiting.fetch_add(1, Ordering::SeqCst);
            let mut released = self.released.lock().unwrap();
            while !*released {
                released = self.cvar.wait(released).unwrap();
            }
        }
        self.inner.file_exists(path)
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }
    fn read_file(&self, path: &str) -> Option<String> {
        self.inner.read_file(path)
    }
    fn write_file(&self, path: &str, data: &str) -> FsResult<()> {
        self.inner.write_file(path, data)
    }
    fn append_file(&self, path: &str, data: &str) -> FsResult<()> {
        self.inner.append_file(path, data)
    }
    fn remove(&self, path: &str) -> FsResult<()> {
        self.inner.remove(path)
    }
    fn chtimes(
        &self,
        path: &str,
        atime: std::time::SystemTime,
        mtime: std::time::SystemTime,
    ) -> FsResult<()> {
        self.inner.chtimes(path, atime, mtime)
    }
    fn directory_exists(&self, path: &str) -> bool {
        self.inner.directory_exists(path)
    }
    fn get_accessible_entries(&self, path: &str) -> Entries {
        self.inner.get_accessible_entries(path)
    }
    fn stat(&self, path: &str) -> Option<FileInfo> {
        self.inner.stat(path)
    }
    fn walk_dir(&self, root: &str, walk_fn: &mut WalkDirFunc) -> FsResult<()> {
        self.inner.walk_dir(root, walk_fn)
    }
    fn realpath(&self, path: &str) -> String {
        self.inner.realpath(path)
    }
}

struct BlockingHost {
    fs: Arc<BlockingFs>,
    cwd: String,
}

impl ResolutionHost for BlockingHost {
    fn fs(&self) -> &dyn Fs {
        self.fs.as_ref()
    }
    fn get_current_directory(&self) -> &str {
        &self.cwd
    }
}

// Regression test for https://github.com/microsoft/typescript-go/issues/3526.
//
// Two threads resolve the same package via `pkg` and `pkg/`. Both are held at
// the `package.json` `file_exists` gate after observing an info-cache miss; on
// release they race through `LoadOrStore`. Without the candidate normalization
// (and the matching `ComparePaths` guard), the loser would skip loading
// `types`/`main` and resolution would fall through to unresolved.
// Go: internal/module/resolver_test.go:TestResolveModuleNameTrailingSlashRace
#[test]
fn resolve_module_name_trailing_slash_race() {
    const PKG_JSON_PATH: &str = "/repo/node_modules/pkg/package.json";
    let files = [
        (
            PKG_JSON_PATH,
            r#"{"name":"pkg","types":"./typings/index.d.ts"}"#,
        ),
        (
            "/repo/node_modules/pkg/typings/index.d.ts",
            "export const x: number;",
        ),
        ("/repo/src/a/file.ts", ""),
        ("/repo/src/b/file.ts", ""),
    ];
    let fs = Arc::new(BlockingFs {
        inner: MapFs::from_map(files.iter().copied(), true),
        target_path: PKG_JSON_PATH.to_string(),
        released: Mutex::new(false),
        cvar: Condvar::new(),
        waiting: AtomicI32::new(0),
    });
    let host = Arc::new(BlockingHost {
        fs: fs.clone(),
        cwd: "/repo".to_string(),
    });
    let opts = CompilerOptions {
        module_resolution: ModuleResolutionKind::Bundler,
        module: ModuleKind::EsNext,
        target: ScriptTarget::EsNext,
        ..Default::default()
    };
    let r = Resolver::new(host, Arc::new(opts), "", "");

    let results = std::sync::Mutex::new(Vec::<(String, bool)>::new());
    std::thread::scope(|scope| {
        for name in ["pkg", "pkg/"] {
            let containing_file = if name.ends_with('/') {
                "/repo/src/b/file.ts"
            } else {
                "/repo/src/a/file.ts"
            };
            let r = &r;
            let results = &results;
            scope.spawn(move || {
                let (resolved, _) =
                    r.resolve_module_name(name, containing_file, ModuleKind::EsNext, None);
                results
                    .lock()
                    .unwrap()
                    .push((name.to_string(), resolved.is_resolved()));
            });
        }

        // Wait for both threads to reach the gate, then release.
        let deadline = Instant::now() + Duration::from_secs(5);
        while fs.waiting.load(Ordering::SeqCst) < 2 {
            if Instant::now() > deadline {
                fs.release();
                panic!(
                    "timed out waiting for both threads at the gate; got {}",
                    fs.waiting.load(Ordering::SeqCst)
                );
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        fs.release();
    });

    for (name, resolved) in results.lock().unwrap().iter() {
        assert!(resolved, "{name:?} failed to resolve");
    }
}

// Go: internal/module/resolver.go:GetCompilerOptionsWithRedirect
#[test]
fn get_compiler_options_with_redirect_none_returns_same() {
    let opts = Arc::new(CompilerOptions::default());
    let result = get_compiler_options_with_redirect(opts.clone(), None);
    assert!(Arc::ptr_eq(&opts, &result));
}

// Go: internal/module/resolver.go:TryParsePatterns + MatchPatternOrExact
#[test]
fn try_parse_patterns_and_match() {
    let mut paths: OrderedMap<String, Vec<String>> = OrderedMap::default();
    paths.set("@app/*".to_string(), vec!["./src/*".to_string()]);
    paths.set("exact".to_string(), vec!["./exact.ts".to_string()]);
    let parsed = try_parse_patterns(Some(&paths));

    let exact = match_pattern_or_exact(&parsed, "exact");
    assert!(exact.is_valid());
    assert_eq!(exact.star_index, -1);

    let wildcard = match_pattern_or_exact(&parsed, "@app/thing");
    assert!(wildcard.is_valid());
    assert_eq!(wildcard.text, "@app/*");
    assert_eq!(wildcard.matched_text("@app/thing"), "thing");

    let no_match = match_pattern_or_exact(&parsed, "unrelated");
    assert!(!no_match.is_valid());

    // None mapping yields an empty (invalid) result for any candidate.
    let empty = try_parse_patterns(None);
    assert!(!match_pattern_or_exact(&empty, "x").is_valid());
}

// Go: internal/module/resolver.go:matchesPatternWithTrailer
#[test]
fn matches_pattern_with_trailer_behaviors() {
    assert!(matches_pattern_with_trailer("./*.js", "./foo.js"));
    assert!(!matches_pattern_with_trailer("./*.js", "./foo.ts"));
    // A target ending in `*` is not a trailer pattern.
    assert!(!matches_pattern_with_trailer("./*", "./foo"));
    // A target with no `*` is not a pattern.
    assert!(!matches_pattern_with_trailer("./foo", "./foo"));
}

// Go: internal/module/resolver.go:extensionIsOk
#[test]
fn extension_is_ok_behaviors() {
    assert!(extension_is_ok(Extensions::TYPE_SCRIPT, ".ts"));
    assert!(extension_is_ok(Extensions::DECLARATION, ".d.ts"));
    assert!(extension_is_ok(Extensions::JAVA_SCRIPT, ".js"));
    assert!(extension_is_ok(Extensions::JSON, ".json"));
    assert!(!extension_is_ok(Extensions::TYPE_SCRIPT, ".js"));
    assert!(!extension_is_ok(Extensions::JAVA_SCRIPT, ".ts"));
}

// Go: internal/module/resolver.go:normalizePathForCJSResolution
#[test]
fn normalize_path_for_cjs_resolution_behaviors() {
    // A trailing `.` keeps the trailing separator (look inside the dir).
    assert_eq!(normalize_path_for_cjs_resolution("/foo", "."), "/foo/");
    assert_eq!(normalize_path_for_cjs_resolution("/foo", ".."), "/");
    // A normal relative path is just normalized.
    assert_eq!(
        normalize_path_for_cjs_resolution("/foo", "./bar"),
        "/foo/bar"
    );
}

// Go: internal/module/resolver.go:GetAutomaticTypeDirectiveNames
#[test]
fn get_automatic_type_directive_names_no_wildcard_returns_types() {
    let opts = CompilerOptions {
        types: vec!["node".to_string(), "jest".to_string()],
        ..Default::default()
    };
    let fs = MapFs::from_map([("/repo/tsconfig.json", "{}")], true);
    let host = StubHost {
        fs,
        cwd: "/repo".to_string(),
    };
    assert_eq!(
        get_automatic_type_directive_names(&opts, &host),
        vec!["node".to_string(), "jest".to_string()]
    );
}

// Go: internal/module/resolver.go:GetAutomaticTypeDirectiveNames (wildcard expansion)
#[test]
fn get_automatic_type_directive_names_wildcard_expands_at_types() {
    let opts = CompilerOptions {
        types: vec!["*".to_string()],
        ..Default::default()
    };
    let files = [
        ("/repo/node_modules/@types/node/index.d.ts", ""),
        ("/repo/node_modules/@types/jest/index.d.ts", ""),
    ];
    let fs = MapFs::from_map(files.iter().copied(), true);
    let host = StubHost {
        fs,
        cwd: "/repo".to_string(),
    };
    let mut names = get_automatic_type_directive_names(&opts, &host);
    names.sort();
    assert_eq!(names, vec!["jest".to_string(), "node".to_string()]);
}

// Go: internal/module/resolver.go:Resolver.ResolveModuleName (relative import probing)
#[test]
fn resolve_module_name_relative_adds_extension() {
    let files = [("/repo/src/a.ts", ""), ("/repo/src/b.ts", "")];
    let opts = CompilerOptions {
        module_resolution: ModuleResolutionKind::Bundler,
        ..Default::default()
    };
    let r = resolver(&files, "/repo", opts);
    let (resolved, _) = r.resolve_module_name("./b", "/repo/src/a.ts", ModuleKind::EsNext, None);
    assert!(resolved.is_resolved());
    assert_eq!(resolved.resolved_file_name, "/repo/src/b.ts");
    assert_eq!(resolved.extension, ".ts");
}

// Go: internal/module/resolver.go:Resolver.ResolveModuleName (unresolved)
#[test]
fn resolve_module_name_unresolved() {
    let files = [("/repo/src/a.ts", "")];
    let opts = CompilerOptions {
        module_resolution: ModuleResolutionKind::Bundler,
        ..Default::default()
    };
    let r = resolver(&files, "/repo", opts);
    let (resolved, _) =
        r.resolve_module_name("./missing", "/repo/src/a.ts", ModuleKind::EsNext, None);
    assert!(!resolved.is_resolved());
}
