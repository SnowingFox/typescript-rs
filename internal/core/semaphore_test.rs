use super::*;

// Go: internal/core/semaphore.go:UnlimitedSemaphore.Acquire
// Go: internal/core/semaphore.go:UnlimitedSemaphore.TryAcquire
#[test]
fn unlimited_semaphore_always_acquires() {
    let s = UnlimitedSemaphore;
    // Acquire returns a (no-op) guard immediately.
    s.acquire();
    // TryAcquire always succeeds, even with an already-cancelled token.
    assert!(s.try_acquire(&Cancel::new()).is_some());
    assert!(s.try_acquire(&Cancel::cancelled()).is_some());
}

// Go: internal/core/semaphore.go:NewLimitedSemaphore
#[test]
#[should_panic(expected = "maxConcurrency must be positive")]
fn limited_semaphore_panics_on_non_positive() {
    let _ = LimitedSemaphore::new(0);
}

// Go: internal/core/semaphore.go:LimitedSemaphore.Acquire
#[test]
fn limited_semaphore_acquire_releases_on_drop() {
    let s = LimitedSemaphore::new(1);
    {
        let _guard = s.acquire();
        // The single slot is taken; a non-blocking try with a cancelled token
        // must fail rather than block.
        assert!(s.try_acquire(&Cancel::cancelled()).is_none());
    }
    // Dropping the guard released the slot, so we can acquire again without
    // deadlocking.
    let _guard = s.acquire();
}

// Go: internal/core/semaphore.go:LimitedSemaphore.TryAcquire
#[test]
fn limited_semaphore_try_acquire() {
    let s = LimitedSemaphore::new(2);
    // A free slot is acquired immediately even if the token is not cancelled.
    let g1 = s.try_acquire(&Cancel::new());
    assert!(g1.is_some());
    let g2 = s.try_acquire(&Cancel::new());
    assert!(g2.is_some());
    // Now both slots are taken; a cancelled token makes the next try fail.
    assert!(s.try_acquire(&Cancel::cancelled()).is_none());
    drop(g1);
    // After releasing one slot, a try succeeds again.
    assert!(s.try_acquire(&Cancel::new()).is_some());
}
