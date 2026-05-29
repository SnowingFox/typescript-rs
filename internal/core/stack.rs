//! A simple LIFO stack (`Stack`).
//!
//! 1:1 port of Go `internal/core/stack.go`.

/// A growable last-in-first-out stack.
#[derive(Clone, Debug)]
pub struct Stack<T> {
    data: Vec<T>,
}

impl<T> Default for Stack<T> {
    fn default() -> Self {
        Stack { data: Vec::new() }
    }
}

impl<T> Stack<T> {
    /// Creates an empty stack.
    ///
    /// Side effects: none (pure).
    pub fn new() -> Stack<T> {
        Stack::default()
    }

    /// Pushes `item` onto the top of the stack.
    ///
    /// Side effects: appends to the internal buffer.
    // Go: internal/core/stack.go:Push
    pub fn push(&mut self, item: T) {
        self.data.push(item);
    }

    /// Removes and returns the top element.
    ///
    /// Side effects: shrinks the internal buffer. Panics if the stack is empty.
    // Go: internal/core/stack.go:Pop
    pub fn pop(&mut self) -> T {
        match self.data.pop() {
            Some(v) => v,
            None => panic!("stack is empty"),
        }
    }

    /// Returns a reference to the top element without removing it.
    ///
    /// Side effects: none. Panics if the stack is empty.
    // Go: internal/core/stack.go:Peek
    pub fn peek(&self) -> &T {
        match self.data.last() {
            Some(v) => v,
            None => panic!("stack is empty"),
        }
    }

    /// Returns the number of elements on the stack.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/stack.go:Len
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Reports whether the stack is empty.
    ///
    /// Side effects: none (pure).
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[cfg(test)]
#[path = "stack_test.rs"]
mod tests;
