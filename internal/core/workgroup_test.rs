use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// Go: internal/core/workgroup.go:parallelWorkGroup
#[test]
fn parallel_work_group_runs_all() {
    let wg = new_work_group(false);
    let counter = Arc::new(AtomicUsize::new(0));
    for _ in 0..16 {
        let c = counter.clone();
        wg.queue(Box::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        }));
    }
    wg.run_and_wait();
    assert_eq!(counter.load(Ordering::SeqCst), 16);
}

// Go: internal/core/workgroup.go:singleThreadedWorkGroup
#[test]
fn single_threaded_work_group_runs_all_lifo() {
    let wg = new_work_group(true);
    let order = Arc::new(Mutex::new(Vec::new()));
    for i in 0..3 {
        let o = order.clone();
        wg.queue(Box::new(move || {
            o.lock().unwrap().push(i);
        }));
    }
    wg.run_and_wait();
    // Go pops from the end of the queue, so tasks run in reverse (LIFO) order.
    assert_eq!(*order.lock().unwrap(), vec![2, 1, 0]);
}

// Go: internal/core/workgroup.go:singleThreadedWorkGroup.Queue
#[test]
#[should_panic(expected = "Queue called after RunAndWait returned")]
fn single_threaded_queue_after_run_panics() {
    let wg = new_work_group(true);
    wg.run_and_wait();
    wg.queue(Box::new(|| {}));
}

// Go: internal/core/workgroup.go:parallelWorkGroup.Queue
#[test]
#[should_panic(expected = "Queue called after RunAndWait returned")]
fn parallel_queue_after_run_panics() {
    let wg = new_work_group(false);
    wg.run_and_wait();
    wg.queue(Box::new(|| {}));
}

// Go: internal/core/workgroup.go:ThrottleGroup
#[test]
fn throttle_group_runs_all_and_reports_ok() {
    let sem = Arc::new(LimitedSemaphore::new(2));
    let group: ThrottleGroup<String> = ThrottleGroup::new(sem);
    let counter = Arc::new(AtomicUsize::new(0));
    for _ in 0..8 {
        let c = counter.clone();
        group.go(move || {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
    }
    assert_eq!(group.wait(), Ok(()));
    assert_eq!(counter.load(Ordering::SeqCst), 8);
}

// Go: internal/core/workgroup.go:ThrottleGroup.Wait
#[test]
fn throttle_group_reports_first_error() {
    let sem = Arc::new(LimitedSemaphore::new(2));
    let group: ThrottleGroup<String> = ThrottleGroup::new(sem);
    group.go(|| Ok(()));
    group.go(|| Err("boom".to_string()));
    group.go(|| Ok(()));
    assert_eq!(group.wait(), Err("boom".to_string()));
}
