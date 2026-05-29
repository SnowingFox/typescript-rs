//! Binary search that assumes a unique match (`binary_search_unique_func`).
//!
//! 1:1 port of Go `internal/core/binarysearch.go`.

/// Binary searches `x` for a unique element, like Go's `slices.BinarySearchFunc`
/// but avoiding extra comparator calls by assuming at most one element matches.
///
/// The comparator is passed the *index* of the element being compared (not the
/// target) along with a reference to that element, and must return a negative,
/// zero, or positive value when the element sorts before, equal to, or after
/// the target. Returns `(index, true)` on a hit, or `(insertion_point, false)`
/// otherwise.
///
/// # Examples
/// ```
/// use tsgo_core::binarysearch::binary_search_unique_func;
/// let xs = [1, 3, 5, 7, 9];
/// assert_eq!(binary_search_unique_func(&xs, |_, &e| e - 5), (2, true));
/// assert_eq!(binary_search_unique_func(&xs, |_, &e| e - 4), (2, false));
/// ```
///
/// Side effects: none (pure).
// Go: internal/core/binarysearch.go:BinarySearchUniqueFunc
pub fn binary_search_unique_func<E, F>(x: &[E], cmp: F) -> (usize, bool)
where
    F: Fn(usize, &E) -> i32,
{
    let n = x.len();
    if n == 0 {
        return (0, false);
    }
    let mut low = 0usize;
    let mut high = n - 1;
    while low <= high {
        let middle = low + ((high - low) >> 1);
        let value = cmp(middle, &x[middle]);
        if value < 0 {
            low = middle + 1;
        } else if value > 0 {
            // `middle` is unsigned; guard against underflow at index 0.
            if middle == 0 {
                break;
            }
            high = middle - 1;
        } else {
            return (middle, true);
        }
    }
    (low, false)
}

#[cfg(test)]
#[path = "binarysearch_test.rs"]
mod tests;
