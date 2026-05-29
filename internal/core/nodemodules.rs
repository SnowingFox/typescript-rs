//! Node.js built-in module tables.
//!
//! 1:1 port of Go `internal/core/nodemodules.go`. Go exposes these as
//! `map[string]bool`; here the source lists are `&[&str]` slices and the merged
//! lookup is a [`rustc_hash::FxHashSet`].

use std::sync::LazyLock;

use rustc_hash::FxHashSet;

/// Node core modules that may be imported without the `node:` prefix.
// Go: internal/core/nodemodules.go:UnprefixedNodeCoreModules
pub const UNPREFIXED_NODE_CORE_MODULES: &[&str] = &[
    "assert",
    "assert/strict",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "dns/promises",
    "domain",
    "events",
    "fs",
    "fs/promises",
    "http",
    "http2",
    "https",
    "inspector",
    "inspector/promises",
    "module",
    "net",
    "os",
    "path",
    "path/posix",
    "path/win32",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "readline/promises",
    "repl",
    "stream",
    "stream/consumers",
    "stream/promises",
    "stream/web",
    "string_decoder",
    "sys",
    "timers",
    "timers/promises",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "util/types",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
];

/// Node core modules that must be imported with the `node:` prefix.
// Go: internal/core/nodemodules.go:ExclusivelyPrefixedNodeCoreModules
pub const EXCLUSIVELY_PREFIXED_NODE_CORE_MODULES: &[&str] = &[
    "node:quic",
    "node:sea",
    "node:sqlite",
    "node:test",
    "node:test/reporters",
];

/// Returns the merged set of all Node core module specifiers (both prefixed and
/// unprefixed).
///
/// Side effects: none (pure; initialized once).
// Go: internal/core/nodemodules.go:NodeCoreModules
pub fn node_core_modules() -> &'static FxHashSet<String> {
    static MODULES: LazyLock<FxHashSet<String>> = LazyLock::new(|| {
        let mut modules = FxHashSet::default();
        for &unprefixed in UNPREFIXED_NODE_CORE_MODULES {
            modules.insert(unprefixed.to_string());
            modules.insert(format!("node:{unprefixed}"));
        }
        for &prefixed in EXCLUSIVELY_PREFIXED_NODE_CORE_MODULES {
            modules.insert(prefixed.to_string());
        }
        modules
    });
    &MODULES
}

/// Returns `"node"` for any Node core module, otherwise `module_name`.
///
/// Side effects: none (pure).
// Go: internal/core/nodemodules.go:NonRelativeModuleNameForTypingCache
pub fn non_relative_module_name_for_typing_cache(module_name: &str) -> String {
    if node_core_modules().contains(module_name) {
        "node".to_string()
    } else {
        module_name.to_string()
    }
}

#[cfg(test)]
#[path = "nodemodules_test.rs"]
mod tests;
