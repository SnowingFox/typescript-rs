//! Port of Go `internal/testutil/harnessutil/recorderfs.go`: a file-system
//! wrapper that records every emitted file so the harness can baseline the
//! compiler's outputs.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::SystemTime;

use tsgo_vfs::{Entries, FileInfo, Fs, FsResult, WalkDirFunc};

use crate::TestFile;

/// A [`Fs`] wrapper that forwards every operation to an inner file system but
/// also records each [`write_file`](Fs::write_file) under its real path, so the
/// emitted documents can be read back in write order.
///
/// A later write to the same path overwrites the recorded entry in place
/// (keeping the original ordering slot), mirroring Go's `outputsMap`.
///
/// # Examples
/// ```
/// use std::sync::Arc;
/// use tsgo_testutil_harnessutil::OutputRecorderFs;
/// use tsgo_vfs::vfstest::MapFs;
/// use tsgo_vfs::Fs;
///
/// let recorder = OutputRecorderFs::new(MapFs::from_map([("/a.ts", "x")], true));
/// recorder.write_file("/out.js", "emitted").unwrap();
/// let outputs = recorder.outputs();
/// assert_eq!(outputs.len(), 1);
/// assert_eq!(outputs[0].unit_name, "/out.js");
/// assert_eq!(outputs[0].content, "emitted");
/// ```
///
/// Side effects: writes pass through to the inner file system.
// Go: internal/testutil/harnessutil/recorderfs.go:OutputRecorderFS
pub struct OutputRecorderFs<F: Fs> {
    inner: F,
    state: Mutex<RecorderState>,
}

#[derive(Default)]
struct RecorderState {
    index_by_path: HashMap<String, usize>,
    outputs: Vec<TestFile>,
}

impl<F: Fs> OutputRecorderFs<F> {
    /// Wraps `inner`, recording subsequent writes.
    ///
    /// Side effects: none at construction.
    // Go: internal/testutil/harnessutil/recorderfs.go:NewOutputRecorderFS
    pub fn new(inner: F) -> OutputRecorderFs<F> {
        OutputRecorderFs {
            inner,
            state: Mutex::new(RecorderState::default()),
        }
    }

    /// Returns the recorded outputs, in first-write order.
    ///
    /// Side effects: none (clones the recorded list).
    // Go: internal/testutil/harnessutil/recorderfs.go:OutputRecorderFS.Outputs
    pub fn outputs(&self) -> Vec<TestFile> {
        self.state
            .lock()
            .expect("recorder state poisoned")
            .outputs
            .clone()
    }

    fn record(&self, path: String, data: &str) {
        let mut state = self.state.lock().expect("recorder state poisoned");
        if let Some(&index) = state.index_by_path.get(&path) {
            state.outputs[index] = TestFile {
                unit_name: path,
                content: data.to_string(),
            };
        } else {
            let index = state.outputs.len();
            state.index_by_path.insert(path.clone(), index);
            state.outputs.push(TestFile {
                unit_name: path,
                content: data.to_string(),
            });
        }
    }
}

impl<F: Fs> Fs for OutputRecorderFs<F> {
    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }

    fn file_exists(&self, path: &str) -> bool {
        self.inner.file_exists(path)
    }

    fn read_file(&self, path: &str) -> Option<String> {
        self.inner.read_file(path)
    }

    // Go: internal/testutil/harnessutil/recorderfs.go:OutputRecorderFS.WriteFile
    fn write_file(&self, path: &str, data: &str) -> FsResult<()> {
        self.inner.write_file(path, data)?;
        let real_path = self.inner.realpath(path);
        self.record(real_path, data);
        Ok(())
    }

    fn append_file(&self, path: &str, data: &str) -> FsResult<()> {
        self.inner.append_file(path, data)
    }

    fn remove(&self, path: &str) -> FsResult<()> {
        self.inner.remove(path)
    }

    fn chtimes(&self, path: &str, atime: SystemTime, mtime: SystemTime) -> FsResult<()> {
        self.inner.chtimes(path, atime, mtime)
    }

    fn directory_exists(&self, path: &str) -> bool {
        self.inner.directory_exists(path)
    }

    fn get_accessible_entries(&self, path: &str) -> Entries {
        self.inner.get_accessible_entries(path)
    }

    fn stat(&self, path: &str) -> Option<FileInfo> {
        self.inner.stat(path)
    }

    fn walk_dir(&self, root: &str, walk_fn: &mut WalkDirFunc) -> FsResult<()> {
        self.inner.walk_dir(root, walk_fn)
    }

    fn realpath(&self, path: &str) -> String {
        self.inner.realpath(path)
    }
}

#[cfg(test)]
#[path = "recorderfs_test.rs"]
mod tests;
