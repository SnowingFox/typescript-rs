// Go: internal/project/background/queue_test.go
use super::*;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

#[test]
fn basic_enqueue() {
    // Go: internal/project/background/queue_test.go:TestQueue/BasicEnqueue
    let q = Queue::new();
    let executed = Arc::new(Mutex::new(false));
    let executed_clone = Arc::clone(&executed);
    q.enqueue(move || {
        *executed_clone.lock().unwrap() = true;
    });
    q.wait();
    assert!(*executed.lock().unwrap());
}

#[test]
fn multiple_tasks_execution() {
    // Go: internal/project/background/queue_test.go:TestQueue/MultipleTasksExecution
    let q = Queue::new();
    let counter = Arc::new(AtomicI64::new(0));
    let num_tasks = 10;
    for _ in 0..num_tasks {
        let counter = Arc::clone(&counter);
        q.enqueue(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
    }
    q.wait();
    assert_eq!(counter.load(Ordering::SeqCst), num_tasks);
}

#[test]
fn nested_enqueue() {
    // Go: internal/project/background/queue_test.go:TestQueue/NestedEnqueue
    let q = Arc::new(Queue::new());
    let executed = Arc::new(Mutex::new(Vec::<String>::new()));

    let executed_clone = Arc::clone(&executed);
    let q_clone = Arc::clone(&q);
    q.enqueue(move || {
        executed_clone.lock().unwrap().push("parent".to_string());
        let executed_inner = Arc::clone(&executed_clone);
        q_clone.enqueue(move || {
            executed_inner.lock().unwrap().push("child".to_string());
        });
    });
    q.wait();

    let result = executed.lock().unwrap();
    assert_eq!(result.len(), 2);
}

#[test]
fn closed_queue_rejects_new_tasks() {
    // Go: internal/project/background/queue_test.go:TestQueue/ClosedQueueRejectsNewTasks
    let q = Queue::new();
    q.close();

    let executed = Arc::new(Mutex::new(false));
    let executed_clone = Arc::clone(&executed);
    q.enqueue(move || {
        *executed_clone.lock().unwrap() = true;
    });
    q.wait();

    assert!(
        !*executed.lock().unwrap(),
        "Task should not execute after queue is closed"
    );
}

#[test]
fn wait_on_empty_queue_returns_immediately() {
    let q = Queue::new();
    q.wait();
}

#[test]
fn close_is_idempotent() {
    let q = Queue::new();
    q.close();
    q.close();
}
