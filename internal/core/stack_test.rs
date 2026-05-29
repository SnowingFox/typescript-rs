use super::*;

// Go: internal/core/stack.go:Push/Pop/Peek/Len
#[test]
fn stack_push_pop_peek() {
    let mut s: Stack<i32> = Stack::new();
    assert_eq!(s.len(), 0);
    s.push(1);
    s.push(2);
    assert_eq!(*s.peek(), 2);
    assert_eq!(s.pop(), 2);
    assert_eq!(s.pop(), 1);
    assert_eq!(s.len(), 0);
}

// Go: internal/core/stack.go:Pop (empty panics)
#[test]
#[should_panic(expected = "stack is empty")]
fn stack_pop_empty_panics() {
    let mut s: Stack<i32> = Stack::new();
    s.pop();
}
