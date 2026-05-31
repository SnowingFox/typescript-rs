use tsgo_parser::SourceFileParseOptions;
use tsgo_tspath::Path;

use super::*;
use crate::host::parse_file;

fn loaded(name: &str, subs: &[&str]) -> ParseTask {
    let file = parse_file(
        SourceFileParseOptions {
            file_name: name.to_string(),
        },
        String::new(),
    );
    ParseTask {
        normalized_file_path: name.to_string(),
        sub_tasks: subs.iter().map(|s| Path(s.to_string())).collect(),
        file: Some(file),
        loaded: true,
        is_lib: false,
    }
}

fn parser_with(roots: &[&str], tasks: &[(&str, &[&str])]) -> FilesParser {
    let mut p = FilesParser::new();
    for (name, subs) in tasks {
        p.tasks_by_path
            .insert(Path(name.to_string()), loaded(name, subs));
    }
    p.root_paths = roots.iter().map(|r| Path(r.to_string())).collect();
    p
}

fn collected_names(p: FilesParser) -> Vec<String> {
    // These cases register only non-lib tasks, so the lib directory is unused.
    p.collect_files("")
        .files()
        .iter()
        .map(|f| f.file_name().to_string())
        .collect()
}

/// `collect_files` appends a file after its imports, so a referenced file
/// precedes its referrer.
// Go: internal/compiler/filesparser.go:collectFiles (post-order)
#[test]
fn collect_orders_imports_before_referrer() {
    let p = parser_with(&["/index.ts"], &[("/index.ts", &["/a.ts"]), ("/a.ts", &[])]);
    assert_eq!(collected_names(p), vec!["/a.ts", "/index.ts"]);
}

/// A diamond import graph collects the shared dependency exactly once, in
/// depth-first order.
// Go: internal/compiler/filesparser.go:collectFiles (seen dedup)
#[test]
fn collect_dedups_diamond() {
    let p = parser_with(
        &["/root.ts"],
        &[
            ("/root.ts", &["/a.ts", "/b.ts"]),
            ("/a.ts", &["/c.ts"]),
            ("/b.ts", &["/c.ts"]),
            ("/c.ts", &[]),
        ],
    );
    assert_eq!(
        collected_names(p),
        vec!["/c.ts", "/a.ts", "/b.ts", "/root.ts"]
    );
}

/// An import cycle terminates and visits each file once.
// Go: internal/compiler/filesparser.go:collectFiles (cycle / seen guard)
#[test]
fn collect_handles_cycle() {
    let p = parser_with(&["/a.ts"], &[("/a.ts", &["/b.ts"]), ("/b.ts", &["/a.ts"])]);
    assert_eq!(collected_names(p), vec!["/b.ts", "/a.ts"]);
}
