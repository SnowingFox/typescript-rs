//! Behavior tests for Node core module tables (Go has no `_test.go`).

use super::*;

// Go: internal/core/nodemodules.go:NodeCoreModules (behavior-level; no Go _test.go)
#[test]
fn node_core_modules_contains_prefixed_and_unprefixed() {
    let modules = node_core_modules();
    // Unprefixed and its node: form are both present.
    assert!(modules.contains("fs"));
    assert!(modules.contains("node:fs"));
    // Exclusively-prefixed module: only the node: form.
    assert!(modules.contains("node:test"));
    assert!(!modules.contains("test"));
    // Non-core module is absent.
    assert!(!modules.contains("lodash"));
}

// Go: internal/core/nodemodules.go:NonRelativeModuleNameForTypingCache (behavior-level)
#[test]
fn typing_cache_name_collapses_core_modules_to_node() {
    assert_eq!(non_relative_module_name_for_typing_cache("fs"), "node");
    assert_eq!(non_relative_module_name_for_typing_cache("node:fs"), "node");
    assert_eq!(
        non_relative_module_name_for_typing_cache("lodash"),
        "lodash"
    );
}
