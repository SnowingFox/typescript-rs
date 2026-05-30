//! Port of Go `internal/project/logging/logtree.go`.
//!
//! A `LogTree` accumulates indented, timestamped log entries and can fork
//! child sub-trees or embed other trees, rendering the whole structure on
//! demand via [`LogTree::string`].

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::{format_time, Logger};

// Go's `logEntry.seq` (a process-global `atomic.Uint64`) is assigned on every
// entry but never read: it affects neither ordering nor rendered output. It is
// therefore omitted from this port (see divergences in the package report).

/// Counters shared by every node of a tree, owned conceptually by the root.
///
/// They serve only as capacity hints for [`LogTree::string`] and mirror Go's
/// root-only `count`/`stringLength` atomics.
struct RootCounters {
    count: AtomicI32,
    string_length: AtomicI32,
}

struct LogEntry {
    time: SystemTime,
    message: String,
    child: Option<LogTree>,
}

struct LogTreeInner {
    name: String,
    level: i32,
    verbose: AtomicBool,
    logs: Mutex<Vec<LogEntry>>,
    counters: Arc<RootCounters>,
    is_root: bool,
}

/// Tree-structured log collector.
///
/// A `LogTree` is a cheap handle (`Arc` internally): cloning or forking yields
/// another reference to the same shared node, matching Go's `*LogTree`
/// pointer semantics. Only the root may be rendered with [`LogTree::string`].
///
/// # Examples
/// ```
/// use tsgo_project_logging::{LogTree, Logger};
///
/// let root = LogTree::new("session");
/// root.log("started");
/// let child = root.fork("loading project");
/// child.log("read tsconfig.json");
/// let rendered = root.string();
/// assert!(rendered.starts_with("======== session ========\n"));
/// assert!(rendered.contains("started"));
/// assert!(rendered.contains("\tloading project") || rendered.contains("loading project"));
/// ```
pub struct LogTree {
    inner: Arc<LogTreeInner>,
}

impl LogTree {
    /// Creates a new root tree labeled `name`.
    ///
    /// Side effects: none.
    // Go: internal/project/logging/logtree.go:NewLogTree
    pub fn new(name: impl Into<String>) -> LogTree {
        LogTree {
            inner: Arc::new(LogTreeInner {
                name: name.into(),
                level: 0,
                verbose: AtomicBool::new(false),
                logs: Mutex::new(Vec::new()),
                counters: Arc::new(RootCounters {
                    count: AtomicI32::new(0),
                    string_length: AtomicI32::new(0),
                }),
                is_root: true,
            }),
        }
    }

    /// Returns another handle to the same underlying node.
    fn handle(&self) -> LogTree {
        LogTree {
            inner: Arc::clone(&self.inner),
        }
    }

    // Go: internal/project/logging/logtree.go:LogTree.add
    fn add(&self, entry: LogEntry) {
        // indent + header + message + newline (capacity hint only).
        let delta = self.inner.level + 15 + entry.message.len() as i32 + 1;
        self.inner
            .counters
            .string_length
            .fetch_add(delta, Ordering::Relaxed);
        self.inner.counters.count.fetch_add(1, Ordering::Relaxed);
        self.inner.logs.lock().unwrap().push(entry);
    }

    fn write_logs_recursive(&self, out: &mut String, indent: &str) {
        let logs = self.inner.logs.lock().unwrap();
        for entry in logs.iter() {
            out.push_str(indent);
            out.push_str(&format_time(entry.time));
            out.push(' ');
            out.push_str(&entry.message);
            out.push('\n');
            if let Some(child) = &entry.child {
                child.write_logs_recursive(out, &format!("{indent}\t"));
            }
        }
    }

    /// Appends a child sub-tree labeled `message` and returns it.
    ///
    /// The child inherits the current verbose flag (a snapshot, like Go).
    ///
    /// Side effects: records an entry in this node.
    // Go: internal/project/logging/logtree.go:LogTree.Fork
    pub fn fork(&self, message: impl Into<String>) -> LogTree {
        let child = LogTree {
            inner: Arc::new(LogTreeInner {
                name: String::new(),
                level: self.inner.level + 1,
                verbose: AtomicBool::new(self.inner.verbose.load(Ordering::Relaxed)),
                logs: Mutex::new(Vec::new()),
                counters: Arc::clone(&self.inner.counters),
                is_root: false,
            }),
        };
        self.add(LogEntry {
            time: SystemTime::now(),
            message: message.into(),
            child: Some(child.handle()),
        });
        child
    }

    /// Embeds another tree as a child entry labeled with that tree's name.
    ///
    /// Side effects: records an entry in this node referencing `logs`.
    // Go: internal/project/logging/logtree.go:LogTree.Embed
    pub fn embed(&self, logs: &LogTree) {
        let count = logs.inner.counters.count.load(Ordering::Relaxed);
        let extra =
            logs.inner.counters.string_length.load(Ordering::Relaxed) + count * self.inner.level;
        self.inner
            .counters
            .string_length
            .fetch_add(extra, Ordering::Relaxed);
        self.inner
            .counters
            .count
            .fetch_add(count, Ordering::Relaxed);
        self.add(LogEntry {
            time: SystemTime::now(),
            message: logs.inner.name.clone(),
            child: Some(logs.handle()),
        });
    }

    /// Renders the whole tree, starting with a `======== <name> ========`
    /// header followed by each entry as `<indent>[HH:MM:SS.mmm] <message>`.
    ///
    /// # Panics
    /// Panics if called on a non-root node (matching Go's `String`).
    ///
    /// Side effects: none.
    // Go: internal/project/logging/logtree.go:LogTree.String
    pub fn string(&self) -> String {
        assert!(self.inner.is_root, "can only call String on root LogTree");
        let header = format!("======== {} ========\n", self.inner.name);
        let hint = self.inner.counters.string_length.load(Ordering::Relaxed);
        let mut out = String::with_capacity(hint.max(0) as usize + header.len());
        out.push_str(&header);
        self.write_logs_recursive(&mut out, "");
        out
    }
}

impl Logger for LogTree {
    fn log(&self, message: &str) {
        self.add(LogEntry {
            time: SystemTime::now(),
            message: message.to_string(),
            child: None,
        });
    }

    fn logf(&self, args: fmt::Arguments<'_>) {
        self.add(LogEntry {
            time: SystemTime::now(),
            message: args.to_string(),
            child: None,
        });
    }

    fn verbose(&self) -> Option<&dyn Logger> {
        if self.inner.verbose.load(Ordering::Relaxed) {
            Some(self)
        } else {
            None
        }
    }

    fn is_verbose(&self) -> bool {
        self.inner.verbose.load(Ordering::Relaxed)
    }

    fn set_verbose(&self, verbose: bool) {
        self.inner.verbose.store(verbose, Ordering::Relaxed);
    }
}

impl fmt::Display for LogTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.string())
    }
}

#[cfg(test)]
#[path = "logtree_test.rs"]
mod tests;
