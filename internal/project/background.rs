//! Background task queue.
//!
//! 1:1 port of Go `internal/project/background/queue.go`.
//! Manages background task execution with graceful close semantics.
//!
//! # Differences from Go
//! - Go uses `sync.WaitGroup` + `context.Context` cancellation.
//!   Rust uses `std::thread::spawn` via a collected `JoinHandle` pool and
//!   `RwLock<bool>` for the closed flag. Context cancellation is not modeled
//!   because Rust does not have a built-in cancellation token; callers that
//!   need cancellation can use `Arc<AtomicBool>`.

use std::sync::{Mutex, RwLock};
use std::thread;

/// A queue that manages background task execution.
///
/// Tasks are spawned as OS threads. [`Queue::wait`] blocks until all
/// enqueued tasks finish. [`Queue::close`] prevents new tasks from being
/// accepted.
///
/// # Examples
/// ```
/// use tsgo_project::background::Queue;
/// use std::sync::{Arc, Mutex};
///
/// let q = Queue::new();
/// let flag = Arc::new(Mutex::new(false));
/// let flag2 = Arc::clone(&flag);
/// q.enqueue(move || { *flag2.lock().unwrap() = true; });
/// q.wait();
/// assert!(*flag.lock().unwrap());
/// ```
// Go: internal/project/background/queue.go:Queue
pub struct Queue {
    closed: RwLock<bool>,
    handles: Mutex<Vec<thread::JoinHandle<()>>>,
}

impl Queue {
    /// Creates a new background queue.
    // Go: internal/project/background/queue.go:NewQueue
    pub fn new() -> Self {
        Queue {
            closed: RwLock::new(false),
            handles: Mutex::new(Vec::new()),
        }
    }

    /// Enqueues `f` for background execution.
    ///
    /// If the queue is closed, the task is silently dropped.
    ///
    /// # Side effects
    /// Spawns an OS thread.
    // Go: internal/project/background/queue.go:Enqueue
    pub fn enqueue<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let closed = self.closed.read().unwrap();
        if *closed {
            return;
        }
        drop(closed);

        let handle = thread::spawn(f);
        self.handles.lock().unwrap().push(handle);
    }

    /// Waits for all active tasks to complete.
    ///
    /// Does not prevent new tasks from being enqueued while waiting.
    // Go: internal/project/background/queue.go:Wait
    pub fn wait(&self) {
        let handles: Vec<_> = {
            let mut guard = self.handles.lock().unwrap();
            guard.drain(..).collect()
        };
        for h in handles {
            let _ = h.join();
        }
        let remaining: Vec<_> = {
            let mut guard = self.handles.lock().unwrap();
            guard.drain(..).collect()
        };
        for h in remaining {
            let _ = h.join();
        }
    }

    /// Marks the queue as closed, preventing future enqueues.
    // Go: internal/project/background/queue.go:Close
    pub fn close(&self) {
        let mut closed = self.closed.write().unwrap();
        *closed = true;
    }
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
}

// Allow Queue to be shared across threads (for nested enqueue pattern).
// SAFETY: All interior state is behind std::sync primitives.
unsafe impl Sync for Queue {}

#[cfg(test)]
#[path = "background_test.rs"]
mod tests;
