use super::*;

// Go: internal/core/binarysearch.go:BinarySearchUniqueFunc
#[test]
fn binary_search_unique() {
    let xs = [1, 3, 5, 7, 9];
    // Hit: 5 is at index 2.
    assert_eq!(binary_search_unique_func(&xs, |_, &e| e - 5), (2, true));
    // Miss: 4 would insert at index 2.
    assert_eq!(binary_search_unique_func(&xs, |_, &e| e - 4), (2, false));
    // Miss below all (target 0 < first element): insert at 0.
    assert_eq!(binary_search_unique_func(&xs, |_, &e| e), (0, false));
    // Miss above all: insert at len.
    assert_eq!(binary_search_unique_func(&xs, |_, &e| e - 10), (5, false));
    // Empty slice.
    let empty: [i32; 0] = [];
    assert_eq!(binary_search_unique_func(&empty, |_, &e| e - 1), (0, false));
}
