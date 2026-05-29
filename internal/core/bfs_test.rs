use super::*;
use std::collections::HashMap;
use std::sync::Mutex;
use tsgo_collections::SyncSet;

// The simple DAG used by several subcases: A -> {B,C}, B -> {D}, C -> {D}, D -> {}.
fn abcd_graph() -> HashMap<String, Vec<String>> {
    let mut g = HashMap::new();
    g.insert("A".to_string(), vec!["B".to_string(), "C".to_string()]);
    g.insert("B".to_string(), vec!["D".to_string()]);
    g.insert("C".to_string(), vec!["D".to_string()]);
    g.insert("D".to_string(), vec![]);
    g
}

// Go: internal/core/bfs_test.go:TestBreadthFirstSearchParallel/basic functionality/find specific node
#[test]
fn bfs_find_specific_node() {
    let graph = abcd_graph();
    let children = |node: &String| graph.get(node).cloned().unwrap_or_default();
    let result = breadth_first_search_parallel("A".to_string(), children, |node: &String| {
        (node.as_str() == "D", true)
    });
    assert!(result.stopped, "Expected search to stop at D");
    assert_eq!(
        result.path,
        vec!["D".to_string(), "B".to_string(), "A".to_string()]
    );
}

// Go: internal/core/bfs_test.go:TestBreadthFirstSearchParallel/basic functionality/visit all nodes
#[test]
fn bfs_visit_all_nodes() {
    let graph = abcd_graph();
    let children = |node: &String| graph.get(node).cloned().unwrap_or_default();
    let visited_nodes = Mutex::new(Vec::<String>::new());
    let result = breadth_first_search_parallel("A".to_string(), children, |node: &String| {
        visited_nodes.lock().unwrap().push(node.clone());
        (false, false) // Never stop early.
    });

    // No node ever returns true, so there is no path.
    assert!(!result.stopped, "Expected search to not stop early");
    assert!(
        result.path.is_empty(),
        "Expected empty path when visit never returns true"
    );

    // Each node should be visited exactly once.
    let mut visited = visited_nodes.into_inner().unwrap();
    visited.sort();
    assert_eq!(
        visited,
        vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
            "D".to_string()
        ]
    );
}

// Go: internal/core/bfs_test.go:TestBreadthFirstSearchParallel/early termination
#[test]
fn bfs_early_termination() {
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    graph.insert(
        "Root".to_string(),
        vec!["L1A".to_string(), "L1B".to_string()],
    );
    graph.insert(
        "L1A".to_string(),
        vec!["L2A".to_string(), "L2B".to_string()],
    );
    graph.insert("L1B".to_string(), vec!["L2C".to_string()]);
    graph.insert("L2A".to_string(), vec!["L3A".to_string()]);
    graph.insert("L2B".to_string(), vec![]);
    graph.insert("L2C".to_string(), vec![]);
    graph.insert("L3A".to_string(), vec![]);

    let children = |node: &String| graph.get(node).cloned().unwrap_or_default();
    let visited: SyncSet<String> = SyncSet::default();
    breadth_first_search_parallel_ex(
        "Root".to_string(),
        children,
        |node: &String| (node.as_str() == "L2B", true), // Stop at level 2.
        BreadthFirstSearchOptions {
            visited: Some(&visited),
            ..Default::default()
        },
        |n: &String| n.clone(),
    );

    assert!(visited.has(&"Root".to_string()), "Expected to visit Root");
    assert!(visited.has(&"L1A".to_string()), "Expected to visit L1A");
    assert!(visited.has(&"L1B".to_string()), "Expected to visit L1B");
    assert!(visited.has(&"L2A".to_string()), "Expected to visit L2A");
    assert!(visited.has(&"L2B".to_string()), "Expected to visit L2B");
    // L2C is non-deterministic, so it is not asserted.
    assert!(
        !visited.has(&"L3A".to_string()),
        "Expected not to visit L3A"
    );
}

// Go: internal/core/bfs_test.go:TestBreadthFirstSearchParallel/returns fallback when no other result found
#[test]
fn bfs_returns_fallback() {
    let graph = abcd_graph();
    let children = |node: &String| graph.get(node).cloned().unwrap_or_default();
    let visited: SyncSet<String> = SyncSet::default();
    let result = breadth_first_search_parallel_ex(
        "A".to_string(),
        children,
        |node: &String| (node.as_str() == "A", false), // Record A as a fallback, but do not stop.
        BreadthFirstSearchOptions {
            visited: Some(&visited),
            ..Default::default()
        },
        |n: &String| n.clone(),
    );

    assert!(!result.stopped, "Expected search to not stop early");
    assert_eq!(result.path, vec!["A".to_string()]);
    assert!(visited.has(&"B".to_string()), "Expected to visit B");
    assert!(visited.has(&"C".to_string()), "Expected to visit C");
    assert!(visited.has(&"D".to_string()), "Expected to visit D");
}

// Go: internal/core/bfs_test.go:TestBreadthFirstSearchParallel/returns a stop result over a fallback
#[test]
fn bfs_stop_over_fallback() {
    let graph = abcd_graph();
    let children = |node: &String| graph.get(node).cloned().unwrap_or_default();
    let result = breadth_first_search_parallel("A".to_string(), children, |node: &String| {
        match node.as_str() {
            "A" => (true, false), // Record fallback.
            "D" => (true, true),  // Stop at D.
            _ => (false, false),
        }
    });

    assert!(result.stopped, "Expected search to stop at D");
    assert_eq!(
        result.path,
        vec!["D".to_string(), "B".to_string(), "A".to_string()]
    );
}
