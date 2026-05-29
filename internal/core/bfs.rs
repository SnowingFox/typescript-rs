//! Parallel breadth-first search over a graph.
//!
//! 1:1 port of Go `internal/core/bfs.go`.
//!
//! DIVERGENCE(port):
//! - Go spawns one goroutine per node and synchronizes with `sync.WaitGroup`;
//!   here each level is processed with a `rayon` indexed parallel iterator.
//! - Determinism is preserved exactly as Go does it: next-level jobs are written
//!   to fixed per-parent slots (so insertion order into the next level is
//!   independent of thread scheduling), and the chosen result/fallback is the
//!   lowest matching index via an atomic compare-and-swap min (`update_min`).
//! - The parent-pointer job chain (`*breadthFirstSearchJob`) becomes
//!   `Arc<Job<N>>` so child jobs can share their parent across threads.

use std::hash::Hash;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use rayon::prelude::*;

use tsgo_collections::{MapEntry, OrderedMap, SyncSet};

/// The outcome of a breadth-first search: whether it stopped on a result, and
/// the path from the matching node back to the start (empty when none).
///
/// Side effects: none (plain data).
// Go: internal/core/bfs.go:BreadthFirstSearchResult
#[derive(Clone, Debug)]
pub struct BreadthFirstSearchResult<N> {
    /// Whether the search stopped early on a `stop` result.
    pub stopped: bool,
    /// The path from the matching node back to the start. Empty when there was
    /// no result (mirrors Go's `nil` path).
    pub path: Vec<N>,
}

// A search job: a node plus a link to the parent job, used to rebuild the path.
// Go: internal/core/bfs.go:breadthFirstSearchJob
struct Job<N> {
    node: N,
    parent: Option<Arc<Job<N>>>,
}

/// A single level of the search, exposed to [`BreadthFirstSearchOptions`]'s
/// preprocessing hook so callers can drop nodes before they are processed.
///
/// Side effects: mutating methods modify the underlying level.
// Go: internal/core/bfs.go:BreadthFirstSearchLevel
pub struct BreadthFirstSearchLevel<'a, K, N> {
    jobs: &'a mut OrderedMap<K, Arc<Job<N>>>,
}

impl<K: Eq + Hash, N> BreadthFirstSearchLevel<'_, K, N> {
    /// Reports whether `key` is present in this level.
    ///
    /// Side effects: none (pure).
    // Go: internal/core/bfs.go:BreadthFirstSearchLevel.Has
    pub fn has(&self, key: &K) -> bool {
        self.jobs.has(key)
    }

    /// Removes `key` from this level.
    ///
    /// Side effects: mutates the level.
    // Go: internal/core/bfs.go:BreadthFirstSearchLevel.Delete
    pub fn delete(&mut self, key: &K) {
        self.jobs.delete(key);
    }

    /// Calls `f` for each node in insertion order until it returns false.
    ///
    /// Side effects: invokes `f`.
    // Go: internal/core/bfs.go:BreadthFirstSearchLevel.Range
    pub fn range(&self, mut f: impl FnMut(&N) -> bool) {
        for job in self.jobs.values() {
            if !f(&job.node) {
                return;
            }
        }
    }
}

/// Optional inputs to [`breadth_first_search_parallel_ex`].
///
/// Side effects: none (plain data, plus an optional borrowed visited set).
// Go: internal/core/bfs.go:BreadthFirstSearchOptions
pub struct BreadthFirstSearchOptions<'a, K, N> {
    /// A pre-seeded set of already-visited keys. When `None`, a fresh set is
    /// used internally.
    pub visited: Option<&'a SyncSet<K>>,
    /// A hook called with each level before it is processed in parallel, letting
    /// the caller remove nodes.
    #[allow(clippy::type_complexity)]
    pub preprocess_level: Option<Box<dyn Fn(&mut BreadthFirstSearchLevel<'_, K, N>) + 'a>>,
}

impl<K, N> Default for BreadthFirstSearchOptions<'_, K, N> {
    fn default() -> Self {
        BreadthFirstSearchOptions {
            visited: None,
            preprocess_level: None,
        }
    }
}

/// Performs a breadth-first search starting from `start`, processing each level
/// in parallel and returning the path from the first node that satisfies
/// `visit` back to the start.
///
/// `visit` returns `(is_result, stop)`: `is_result` records the node as a
/// candidate; `stop` ends the search at this level. The lowest-index result on a
/// level wins, keeping the output deterministic.
///
/// Side effects: runs `neighbors`/`visit` in parallel across rayon threads.
// Go: internal/core/bfs.go:BreadthFirstSearchParallel
pub fn breadth_first_search_parallel<N>(
    start: N,
    neighbors: impl Fn(&N) -> Vec<N> + Sync,
    visit: impl Fn(&N) -> (bool, bool) + Sync,
) -> BreadthFirstSearchResult<N>
where
    N: Clone + Eq + Hash + Send + Sync,
{
    // getKey is the identity: each node is its own key (Go: core.Identity).
    breadth_first_search_parallel_ex(
        start,
        neighbors,
        visit,
        BreadthFirstSearchOptions::default(),
        |n: &N| n.clone(),
    )
}

/// Extension of [`breadth_first_search_parallel`] that accepts a pre-seeded
/// visited set and a per-level preprocessing hook, and projects each node to a
/// key with `get_key`.
///
/// Side effects: runs `neighbors`/`visit`/`get_key` in parallel across rayon
/// threads; updates the (possibly caller-owned) visited set.
// Go: internal/core/bfs.go:BreadthFirstSearchParallelEx
pub fn breadth_first_search_parallel_ex<K, N>(
    start: N,
    neighbors: impl Fn(&N) -> Vec<N> + Sync,
    visit: impl Fn(&N) -> (bool, bool) + Sync,
    options: BreadthFirstSearchOptions<'_, K, N>,
    get_key: impl Fn(&N) -> K + Sync,
) -> BreadthFirstSearchResult<N>
where
    K: Clone + Eq + Hash + Send + Sync,
    N: Clone + Send + Sync,
{
    let owned_visited;
    let visited: &SyncSet<K> = match options.visited {
        Some(v) => v,
        None => {
            owned_visited = SyncSet::default();
            &owned_visited
        }
    };
    let preprocess = options.preprocess_level;

    let mut fallback: Option<Arc<Job<N>>> = None;
    let mut level: OrderedMap<K, Arc<Job<N>>> = OrderedMap::from_list(vec![MapEntry {
        key: get_key(&start),
        value: Arc::new(Job {
            node: start,
            parent: None,
        }),
    }]);

    while level.size() > 0 {
        // Give the caller a chance to drop nodes from this level first.
        if let Some(pp) = &preprocess {
            let mut lvl = BreadthFirstSearchLevel { jobs: &mut level };
            pp(&mut lvl);
        }

        let fallback_is_none = fallback.is_none();
        let lowest_goal = AtomicI64::new(i64::MAX);
        let lowest_fallback = AtomicI64::new(i64::MAX);
        let next_job_count = AtomicI64::new(0);

        // Snapshot the level's jobs in order; the index is used both for result
        // selection (EntryAt) and to place next-level jobs deterministically.
        let jobs_vec: Vec<Arc<Job<N>>> = level.values().cloned().collect();
        let next: Vec<Vec<Arc<Job<N>>>> = jobs_vec
            .par_iter()
            .enumerate()
            .map(|(i, j)| {
                let i = i as i64;
                if i >= lowest_goal.load(Ordering::SeqCst) {
                    // A lower-index result already stops the search.
                    return Vec::new();
                }
                if !visited.add_if_absent(get_key(&j.node)) {
                    // Visited at a previous level (so `visit` was false there);
                    // jobs are deduplicated before queuing, so skip it.
                    return Vec::new();
                }

                let (is_result, stop) = visit(&j.node);
                if is_result {
                    if stop {
                        update_min(&lowest_goal, i);
                        return Vec::new();
                    }
                    if fallback_is_none {
                        update_min(&lowest_fallback, i);
                    }
                }

                if i >= lowest_goal.load(Ordering::SeqCst) {
                    // Another job found a lower-index result while we worked, so
                    // there is no need to collect this node's neighbors.
                    return Vec::new();
                }
                let neighbor_nodes = neighbors(&j.node);
                if neighbor_nodes.is_empty() {
                    return Vec::new();
                }
                next_job_count.fetch_add(neighbor_nodes.len() as i64, Ordering::SeqCst);
                crate::map(&neighbor_nodes, |child| {
                    Arc::new(Job {
                        node: child.clone(),
                        parent: Some(j.clone()),
                    })
                })
            })
            .collect();

        let goal = lowest_goal.load(Ordering::SeqCst);
        if goal != i64::MAX {
            let (_, job) = level
                .entry_at(goal as usize)
                .expect("goal index within level");
            return BreadthFirstSearchResult {
                stopped: true,
                path: create_path(Some(job.clone())),
            };
        }

        if fallback_is_none {
            let fb = lowest_fallback.load(Ordering::SeqCst);
            if fb != i64::MAX {
                let (_, job) = level
                    .entry_at(fb as usize)
                    .expect("fallback index within level");
                fallback = Some(job.clone());
            }
        }

        let mut next_jobs: OrderedMap<K, Arc<Job<N>>> =
            OrderedMap::with_size_hint(next_job_count.load(Ordering::SeqCst).max(0) as usize);
        for jobs_list in next {
            for j in jobs_list {
                let key = get_key(&j.node);
                if !next_jobs.has(&key) {
                    // Deduplicate synchronously to keep ordering deterministic
                    // and avoid extra synchronization.
                    next_jobs.set(key, j);
                }
            }
        }
        level = next_jobs;
    }

    BreadthFirstSearchResult {
        stopped: false,
        path: create_path(fallback),
    }
}

// Rebuilds the path by walking the parent chain: [node, parent, ..., start].
// An absent job yields an empty path (Go's `nil`).
// Go: internal/core/bfs.go:createPath
fn create_path<N: Clone>(job: Option<Arc<Job<N>>>) -> Vec<N> {
    let mut path = Vec::new();
    let mut current = job;
    while let Some(j) = current {
        path.push(j.node.clone());
        current = j.parent.clone();
    }
    path
}

/// Atomically lowers `a` to `candidate` if `candidate` is smaller, returning
/// whether the store happened.
///
/// Side effects: may store into `a`.
// Go: internal/core/bfs.go:updateMin
fn update_min(a: &AtomicI64, candidate: i64) -> bool {
    loop {
        let current = a.load(Ordering::SeqCst);
        if current < candidate {
            return false;
        }
        if a.compare_exchange(current, candidate, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return true;
        }
    }
}

#[cfg(test)]
#[path = "bfs_test.rs"]
mod tests;
