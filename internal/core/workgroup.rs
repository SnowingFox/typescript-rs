//! Task work groups (`WorkGroup` trait with single-threaded and parallel
//! implementations) and a concurrency-throttled `ThrottleGroup`.
//!
//! 1:1 port of Go `internal/core/workgroup.go`.
//!
//! DIVERGENCE(port):
//! - Go queues `func()`; here a task is a `Box<dyn FnOnce() + Send + 'static>`
//!   because the parallel group spawns OS threads.
//! - The parallel group spawns one `std::thread` per `Queue` (mirroring Go's
//!   `wg.Go`) and joins them all in `RunAndWait`.
//! - `ThrottleGroup` drops Go's `context.Context` (PORTING §3); Go discards the
//!   derived context from `errgroup.WithContext` anyway, so cancellation was
//!   unobservable. It is generic over the task error type and `Wait` returns the
//!   first observed error.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::semaphore::{LimitedSemaphore, Semaphore};

/// A unit of deferred work.
pub type Task = Box<dyn FnOnce() + Send + 'static>;

/// A group of tasks run either immediately in parallel or deferred and run
/// sequentially.
///
/// Side effects: implementations may spawn threads and run queued tasks.
// Go: internal/core/workgroup.go:WorkGroup
pub trait WorkGroup {
    /// Queues a task. It may run immediately or be deferred until
    /// [`Self::run_and_wait`].
    ///
    /// # Panics
    /// Panics if called after [`Self::run_and_wait`] has returned.
    ///
    /// Side effects: may spawn a thread or store the task.
    // Go: internal/core/workgroup.go:WorkGroup.Queue
    fn queue(&self, f: Task);

    /// Runs all queued tasks, blocking until they have all completed.
    ///
    /// Side effects: runs tasks; may join spawned threads.
    // Go: internal/core/workgroup.go:WorkGroup.RunAndWait
    fn run_and_wait(&self);
}

/// Creates a [`WorkGroup`]: single-threaded (deferred, sequential) when
/// `single_threaded`, otherwise parallel (spawns a thread per task).
///
/// Side effects: none (allocates the group).
// Go: internal/core/workgroup.go:NewWorkGroup
pub fn new_work_group(single_threaded: bool) -> Box<dyn WorkGroup> {
    if single_threaded {
        Box::new(SingleThreadedWorkGroup::default())
    } else {
        Box::new(ParallelWorkGroup::default())
    }
}

/// A [`WorkGroup`] that runs each queued task immediately on its own thread.
///
/// Side effects: spawns threads.
// Go: internal/core/workgroup.go:parallelWorkGroup
#[derive(Default)]
pub struct ParallelWorkGroup {
    done: AtomicBool,
    handles: Mutex<Vec<JoinHandle<()>>>,
}

impl WorkGroup for ParallelWorkGroup {
    // Go: internal/core/workgroup.go:parallelWorkGroup.Queue
    fn queue(&self, f: Task) {
        if self.done.load(Ordering::Acquire) {
            panic!("Queue called after RunAndWait returned");
        }
        let handle = std::thread::spawn(f);
        self.handles
            .lock()
            .expect("work group mutex poisoned")
            .push(handle);
    }

    // Go: internal/core/workgroup.go:parallelWorkGroup.RunAndWait
    fn run_and_wait(&self) {
        // Drain handles, joining each. A running task may queue more work, so we
        // keep popping until none remain (Go relies on `sync.WaitGroup` here).
        loop {
            let handle = self
                .handles
                .lock()
                .expect("work group mutex poisoned")
                .pop();
            match handle {
                Some(h) => {
                    let _ = h.join();
                }
                None => break,
            }
        }
        self.done.store(true, Ordering::Release);
    }
}

/// A [`WorkGroup`] that defers tasks and runs them sequentially in
/// last-in-first-out order.
///
/// Side effects: runs tasks on the calling thread.
// Go: internal/core/workgroup.go:singleThreadedWorkGroup
#[derive(Default)]
pub struct SingleThreadedWorkGroup {
    done: AtomicBool,
    fns: Mutex<Vec<Task>>,
}

impl WorkGroup for SingleThreadedWorkGroup {
    // Go: internal/core/workgroup.go:singleThreadedWorkGroup.Queue
    fn queue(&self, f: Task) {
        if self.done.load(Ordering::Acquire) {
            panic!("Queue called after RunAndWait returned");
        }
        self.fns.lock().expect("work group mutex poisoned").push(f);
    }

    // Go: internal/core/workgroup.go:singleThreadedWorkGroup.RunAndWait
    fn run_and_wait(&self) {
        loop {
            let f = self.fns.lock().expect("work group mutex poisoned").pop();
            match f {
                Some(f) => f(),
                None => break,
            }
        }
        self.done.store(true, Ordering::Release);
    }
}

/// Like an `errgroup.Group` but with global concurrency limiting via a shared
/// [`LimitedSemaphore`].
///
/// Side effects: spawns threads.
// Go: internal/core/workgroup.go:ThrottleGroup
pub struct ThrottleGroup<E> {
    semaphore: Arc<LimitedSemaphore>,
    handles: Mutex<Vec<JoinHandle<Result<(), E>>>>,
}

impl<E: Send + 'static> ThrottleGroup<E> {
    /// Creates a group that throttles concurrency with the shared `semaphore`.
    ///
    /// Side effects: none (allocates the group).
    // Go: internal/core/workgroup.go:NewThrottleGroup
    pub fn new(semaphore: Arc<LimitedSemaphore>) -> Self {
        ThrottleGroup {
            semaphore,
            handles: Mutex::new(Vec::new()),
        }
    }

    /// Runs `f` on a new thread after acquiring a semaphore slot, releasing the
    /// slot when it completes.
    ///
    /// Side effects: spawns a thread; reserves a semaphore slot while running.
    // Go: internal/core/workgroup.go:ThrottleGroup.Go
    pub fn go(&self, f: impl FnOnce() -> Result<(), E> + Send + 'static) {
        let semaphore = self.semaphore.clone();
        let handle = std::thread::spawn(move || {
            let _guard = semaphore.acquire();
            f()
        });
        self.handles
            .lock()
            .expect("throttle group mutex poisoned")
            .push(handle);
    }

    /// Waits for all tasks to complete and returns the first error encountered,
    /// if any.
    ///
    /// Side effects: joins all spawned threads.
    // Go: internal/core/workgroup.go:ThrottleGroup.Wait
    pub fn wait(&self) -> Result<(), E> {
        let handles =
            std::mem::take(&mut *self.handles.lock().expect("throttle group mutex poisoned"));
        let mut first_err: Option<E> = None;
        for handle in handles {
            if let Ok(Err(e)) = handle.join() {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
#[path = "workgroup_test.rs"]
mod tests;
