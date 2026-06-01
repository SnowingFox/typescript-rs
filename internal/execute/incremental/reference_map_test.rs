use super::*;
use tsgo_collections::Set;
use tsgo_tspath::Path;

fn p(s: &str) -> Path {
    Path(s.to_string())
}

// Go: internal/execute/incremental/referencemap.go:referenceMap.getReferencedBy
#[test]
fn referenced_by_inverts_the_references() {
    // A references B (i.e. A imports B).
    let mut map = ReferenceMap::new();
    map.store_references(p("/a.ts"), Set::from_items([p("/b.ts")]));

    // B is referenced by A.
    assert_eq!(map.get_referenced_by(&p("/b.ts")), vec![p("/a.ts")]);
    // A is referenced by nobody.
    assert!(map.get_referenced_by(&p("/a.ts")).is_empty());
    // An unrelated file is referenced by nobody.
    assert!(map.get_referenced_by(&p("/c.ts")).is_empty());
}

// Go: internal/execute/incremental/referencemap.go:referenceMap.getReferencedBy
// (multiple referrers are returned sorted)
#[test]
fn referenced_by_returns_all_referrers_sorted() {
    let mut map = ReferenceMap::new();
    map.store_references(p("/z.ts"), Set::from_items([p("/shared.ts")]));
    map.store_references(p("/a.ts"), Set::from_items([p("/shared.ts")]));

    assert_eq!(
        map.get_referenced_by(&p("/shared.ts")),
        vec![p("/a.ts"), p("/z.ts")]
    );
}
