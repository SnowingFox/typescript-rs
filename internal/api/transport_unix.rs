//! Unix domain socket transport helpers (`transport_unix.go`).

use std::io;
use std::path::{Path, PathBuf};

/// Creates a Unix domain socket listener, removing any existing socket file first.
///
/// # Examples
/// ```no_run
/// # #[cfg(unix)]
/// # fn example() -> std::io::Result<()> {
/// let dir = std::env::temp_dir();
/// let path = dir.join("tsgo_api_test.sock");
/// let _listener = tsgo_api::new_pipe_listener(&path)?;
/// let _ = std::fs::remove_file(&path);
/// # Ok(())
/// # }
/// ```
///
/// Side effects: may delete an existing socket path; binds a listening socket.
// Go: internal/api/transport_unix.go:newPipeListener
pub fn new_pipe_listener(path: impl AsRef<Path>) -> io::Result<std::os::unix::net::UnixListener> {
    let path = path.as_ref();
    let _ = std::fs::remove_file(path);
    std::os::unix::net::UnixListener::bind(path)
}

/// Returns a platform-appropriate pipe path under the system temp directory.
///
/// # Examples
/// ```
/// # #[cfg(unix)]
/// # {
/// let p = tsgo_api::generate_pipe_path("tsgo-test.sock");
/// assert!(p.ends_with("tsgo-test.sock"));
/// assert!(p.parent().is_some());
/// # }
/// ```
///
/// Side effects: none (pure path construction).
// Go: internal/api/transport_unix.go:GeneratePipePath
pub fn generate_pipe_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

#[cfg(test)]
#[path = "transport_unix_test.rs"]
mod tests;
