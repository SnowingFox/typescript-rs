//! Shared test helpers: an in-memory [`ResolutionHost`] over `MapFs`.

use std::sync::Arc;

use tsgo_core::compileroptions::CompilerOptions;
use tsgo_vfs::vfstest::MapFs;
use tsgo_vfs::Fs;

use crate::{ResolutionHost, Resolver};

pub(crate) struct StubHost {
    pub(crate) fs: MapFs,
    pub(crate) cwd: String,
}

impl ResolutionHost for StubHost {
    fn fs(&self) -> &dyn Fs {
        &self.fs
    }
    fn get_current_directory(&self) -> &str {
        &self.cwd
    }
}

/// Builds a [`Resolver`] over an in-memory FS populated from `files`.
pub(crate) fn resolver(files: &[(&str, &str)], cwd: &str, options: CompilerOptions) -> Resolver {
    let fs = MapFs::from_map(files.iter().copied(), true);
    let host = Arc::new(StubHost {
        fs,
        cwd: cwd.to_string(),
    });
    Resolver::new(host, Arc::new(options), "", "")
}
