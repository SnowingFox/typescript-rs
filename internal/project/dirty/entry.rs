//! 1:1 port of Go `internal/project/dirty/entry.go` (the unexported
//! `mapEntry` data shared by `Map` and `SyncMap` entries).

use crate::interfaces::Cloneable;

/// The common per-key state of a dirty entry: its key, the original (loaded)
/// value, the current value, and the dirty/delete flags.
///
/// In Go this is the embedded `mapEntry` struct guarded by each entry's lock.
/// Here it is held inside a `RefCell` (for `Map`) or a `Mutex` (for `SyncMap`)
/// by the owning entry type. Its only behavioral method is [`value`], which
/// mirrors Go's zero-value-on-delete read; the remaining `mapEntry` getters are
/// inlined as direct field reads at the call sites.
pub(crate) struct MapEntryData<K, V> {
    pub(crate) key: K,
    pub(crate) original: V,
    pub(crate) value: V,
    pub(crate) dirty: bool,
    pub(crate) delete: bool,
}

impl<K, V: Cloneable + Default> MapEntryData<K, V> {
    /// Returns the current value, or `V::default()` when marked for deletion.
    pub(crate) fn value(&self) -> V {
        if self.delete {
            V::default()
        } else {
            self.value.clone_cow()
        }
    }
}
