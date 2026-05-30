use super::*;

// Go: internal/packagejson/cache.go (no direct Go unit test; behavior level,
// expectations derived from Go semantics).

fn package_json(content: &[u8]) -> PackageJson {
    PackageJson::new(crate::parse(content).expect("valid json"), true)
}

// Go: internal/packagejson/cache.go:GetVersionPaths (missing typesVersions)
#[test]
fn version_paths_absent_field() {
    let pj = package_json(b"{}");
    assert!(!pj.get_version_paths(None).exists());

    let mut calls: Vec<(i32, Vec<String>)> = Vec::new();
    {
        let mut trace = |m: &'static Message, args: &[&str]| {
            calls.push((m.code(), args.iter().map(|s| s.to_string()).collect()));
        };
        pj.get_version_paths(Some(&mut trace));
    }
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, 6100);
    assert_eq!(calls[0].1, vec!["typesVersions".to_string()]);
}

// Go: internal/packagejson/cache.go:GetVersionPaths (typesVersions wrong type)
#[test]
fn version_paths_wrong_type() {
    let pj = package_json(br#"{"typesVersions":1}"#);
    assert!(!pj.get_version_paths(None).exists());

    let mut calls: Vec<(i32, Vec<String>)> = Vec::new();
    {
        let mut trace = |m: &'static Message, args: &[&str]| {
            calls.push((m.code(), args.iter().map(|s| s.to_string()).collect()));
        };
        pj.get_version_paths(Some(&mut trace));
    }
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, 6105);
    assert_eq!(
        calls[0].1,
        vec![
            "typesVersions".to_string(),
            "object".to_string(),
            "number".to_string()
        ]
    );
}

// Go: internal/packagejson/cache.go:GetVersionPaths + GetPaths (matching entry)
#[test]
fn version_paths_match() {
    let pj = package_json(br#"{"typesVersions":{">=4.0":{"*":["ts4/*"]}}}"#);
    let vp = pj.get_version_paths(None);
    assert!(vp.exists());
    assert_eq!(vp.version(), ">=4.0");

    let paths = vp.get_paths().expect("paths present");
    assert_eq!(
        paths.get(&"*".to_string()),
        Some(&vec!["ts4/*".to_string()])
    );
}

// Go: internal/packagejson/cache.go:InfoCache (Set then Get round-trips by path)
#[test]
fn info_cache_set_get_roundtrip() {
    let cache = InfoCache::new("/".to_string(), true);
    let entry = Arc::new(InfoCacheEntry {
        package_directory: "/p".to_string(),
        directory_exists: true,
        contents: None,
    });
    cache.set("/p/package.json", entry.clone());

    let got = cache.get("/p/package.json").expect("entry present");
    assert_eq!(got.get_directory(), "/p");
    assert!(Arc::ptr_eq(&got, &entry));
    assert!(cache.get("/other/package.json").is_none());
}

// Go: internal/packagejson/cache.go:Set (LoadOrStore keeps the first writer)
#[test]
fn info_cache_load_or_store() {
    let cache = InfoCache::new("/".to_string(), true);
    let first = Arc::new(InfoCacheEntry {
        package_directory: "/a".to_string(),
        directory_exists: true,
        contents: None,
    });
    let second = Arc::new(InfoCacheEntry {
        package_directory: "/b".to_string(),
        directory_exists: true,
        contents: None,
    });

    let r1 = cache.set("/x/package.json", first.clone());
    assert!(Arc::ptr_eq(&r1, &first));

    let r2 = cache.set("/x/package.json", second.clone());
    assert!(Arc::ptr_eq(&r2, &first));
    assert!(!Arc::ptr_eq(&r2, &second));
}
