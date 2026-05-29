//! Concurrency-limiting semaphores (`Semaphore` trait with unlimited and
//! bounded implementations).
//!
//! 1:1 port of Go `internal/core/semaphore.go`.
//!
//! DIVERGENCE(port): Go returns a `release func()` closure from `Acquire`; here
//! each implementation returns an RAII guard that releases the slot on drop.
//! `TryAcquire`'s `context.Context` becomes a [`Cancel`] token (PORTING §3),
//! and the `(release, acquired)` pair becomes an `Option<Guard>` (`None` =
//! not acquired).

use crossbeam_channel::{unbounded, Receiver, Sender};
use std::sync::Mutex;

/// A cancellation token, replacing Go's `context.Context` Done() signal.
///
/// A fresh token is uncancelled; [`Cancel::cancel`] disconnects an internal
/// channel so that waiters selecting on the internal done signal wake
/// immediately.
///
/// # Examples
/// ```
/// use tsgo_core::semaphore::Cancel;
/// let c = Cancel::new();
/// assert!(!c.is_cancelled());
/// c.cancel();
/// assert!(c.is_cancelled());
/// ```
///
/// Side effects: none (holds an internal channel).
pub struct Cancel {
    // Holding the sender keeps the `done` channel connected; dropping it (via
    // `cancel`) disconnects the channel so `done` recv operations become ready.
    sender: Mutex<Option<Sender<()>>>,
    done: Receiver<()>,
}

impl Cancel {
    /// Creates a fresh, uncancelled token.
    ///
    /// Side effects: allocates an internal channel.
    pub fn new() -> Self {
        let (tx, rx) = unbounded::<()>();
        Cancel {
            sender: Mutex::new(Some(tx)),
            done: rx,
        }
    }

    /// Creates a token that is already cancelled.
    ///
    /// Side effects: allocates an internal channel, then cancels it.
    pub fn cancelled() -> Self {
        let c = Self::new();
        c.cancel();
        c
    }

    /// Marks the token cancelled, waking any waiters blocked on the done signal.
    ///
    /// Side effects: disconnects the internal channel.
    pub fn cancel(&self) {
        *self.sender.lock().expect("cancel mutex poisoned") = None;
    }

    /// Reports whether the token has been cancelled.
    ///
    /// Side effects: none (pure).
    pub fn is_cancelled(&self) -> bool {
        self.sender.lock().expect("cancel mutex poisoned").is_none()
    }

    // Returns the receiver that becomes ready (disconnected) once cancelled.
    // Mirrors Go's `ctx.Done()`.
    fn done(&self) -> &Receiver<()> {
        &self.done
    }
}

impl Default for Cancel {
    fn default() -> Self {
        Self::new()
    }
}

/// A concurrency limiter that hands out RAII slot guards.
///
/// Side effects: implementations may block in [`Self::acquire`].
// Go: internal/core/semaphore.go:Semaphore
pub trait Semaphore {
    /// The RAII guard returned on a successful acquisition; the slot is released
    /// when it is dropped.
    type Guard;

    /// Acquires a slot, blocking until one is available.
    ///
    /// Side effects: may block; reserves a slot until the guard is dropped.
    // Go: internal/core/semaphore.go:Semaphore.Acquire
    fn acquire(&self) -> Self::Guard;

    /// Tries to acquire a slot, blocking until one is available or `cancel` is
    /// cancelled. Returns `None` if it was cancelled first.
    ///
    /// Side effects: may block; reserves a slot until the guard is dropped.
    // Go: internal/core/semaphore.go:Semaphore.TryAcquire
    fn try_acquire(&self, cancel: &Cancel) -> Option<Self::Guard>;
}

/// A semaphore that imposes no limit; every acquisition succeeds immediately.
///
/// Side effects: none.
// Go: internal/core/semaphore.go:UnlimitedSemaphore
#[derive(Copy, Clone, Debug, Default)]
pub struct UnlimitedSemaphore;

impl Semaphore for UnlimitedSemaphore {
    type Guard = ();

    // Go: internal/core/semaphore.go:UnlimitedSemaphore.Acquire
    fn acquire(&self) -> Self::Guard {}

    // Go: internal/core/semaphore.go:UnlimitedSemaphore.TryAcquire
    fn try_acquire(&self, _cancel: &Cancel) -> Option<Self::Guard> {
        Some(())
    }
}

/// A semaphore that allows at most `max_concurrency` concurrent holders.
///
/// Backed by a bounded channel: acquiring sends a token (blocking when full)
/// and releasing receives one, mirroring Go's `chan struct{}`.
///
/// Side effects: holds an internal channel.
// Go: internal/core/semaphore.go:LimitedSemaphore
pub struct LimitedSemaphore {
    tx: Sender<()>,
    rx: Receiver<()>,
}

impl LimitedSemaphore {
    /// Creates a semaphore allowing `max_concurrency` concurrent holders.
    ///
    /// # Panics
    /// Panics if `max_concurrency` is not positive.
    ///
    /// Side effects: allocates an internal bounded channel.
    // Go: internal/core/semaphore.go:NewLimitedSemaphore
    pub fn new(max_concurrency: usize) -> Self {
        if max_concurrency == 0 {
            panic!("maxConcurrency must be positive");
        }
        let (tx, rx) = crossbeam_channel::bounded::<()>(max_concurrency);
        LimitedSemaphore { tx, rx }
    }
}

impl Semaphore for LimitedSemaphore {
    type Guard = SemaphoreGuard;

    // Go: internal/core/semaphore.go:LimitedSemaphore.Acquire
    fn acquire(&self) -> Self::Guard {
        self.tx.send(()).expect("semaphore channel disconnected");
        SemaphoreGuard {
            rx: self.rx.clone(),
        }
    }

    // Go: internal/core/semaphore.go:LimitedSemaphore.TryAcquire
    fn try_acquire(&self, cancel: &Cancel) -> Option<Self::Guard> {
        crossbeam_channel::select! {
            send(self.tx, ()) -> res => {
                res.ok()?;
                Some(SemaphoreGuard { rx: self.rx.clone() })
            }
            recv(cancel.done()) -> _ => None,
        }
    }
}

/// RAII guard for a [`LimitedSemaphore`] slot; releases the slot on drop.
///
/// Side effects: releases one slot when dropped.
pub struct SemaphoreGuard {
    rx: Receiver<()>,
}

impl Drop for SemaphoreGuard {
    fn drop(&mut self) {
        // Receive one token to free a slot (Go: `<-s.ch`). Ignore errors that
        // can only arise if the semaphore itself was already dropped.
        let _ = self.rx.recv();
    }
}

#[cfg(test)]
#[path = "semaphore_test.rs"]
mod tests;
