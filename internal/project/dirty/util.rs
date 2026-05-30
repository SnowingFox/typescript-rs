//! 1:1 port of Go `internal/project/dirty/util.go`.

use std::collections::HashMap;
use std::hash::Hash;

/// Returns the dirty map if it is present, otherwise a clone of the original
/// map, otherwise an empty map.
///
/// This mirrors Go's `CloneMapIfNil`: it lets builder code obtain a writable map
/// without disturbing the original. When the dirty overlay already has a map it
/// is returned as-is; when it does not, the original is cloned (or an empty map
/// is produced when there is no original).
///
/// The Go version threads `*T` pointers and a `getMap` accessor to read a map
/// field through Go generics; the Rust port lets the caller extract that field
/// directly and pass the two `Option`s, which is equivalent and avoids the
/// accessor indirection.
///
/// # Examples
/// ```
/// use std::collections::HashMap;
/// use tsgo_project_dirty::clone_map_if_nil;
///
/// let original: HashMap<&str, i32> = HashMap::from([("a", 1)]);
/// // No dirty map yet: clone the original.
/// let cloned = clone_map_if_nil::<&str, i32>(None, Some(&original));
/// assert_eq!(cloned.get("a"), Some(&1));
///
/// // No dirty and no original: empty map.
/// let empty = clone_map_if_nil::<&str, i32>(None, None);
/// assert!(empty.is_empty());
/// ```
///
/// Side effects: none (pure).
// Go: internal/project/dirty/util.go:CloneMapIfNil
pub fn clone_map_if_nil<K: Clone + Eq + Hash, V: Clone>(
    dirty: Option<HashMap<K, V>>,
    original: Option<&HashMap<K, V>>,
) -> HashMap<K, V> {
    match dirty {
        Some(dirty_map) => dirty_map,
        None => original.cloned().unwrap_or_default(),
    }
}

#[cfg(test)]
#[path = "util_test.rs"]
mod tests;
