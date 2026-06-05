//! Windows named pipe path helper (`transport_windows.go`).

use std::path::PathBuf;

/// Returns a Windows named pipe path for the given name.
///
/// # Examples
/// ```
/// # #[cfg(windows)]
/// # {
/// let p = tsgo_api::generate_pipe_path("tsgo-test");
/// assert!(p.to_string_lossy().contains(r"\\.\pipe\"));
/// # }
/// ```
///
/// Side effects: none (pure path construction).
// Go: internal/api/transport_windows.go:GeneratePipePath
pub fn generate_pipe_path(name: &str) -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\{name}"))
}

// Go: internal/api/transport_windows.go:newPipeListener
// DEFER(phase-8): blocked-by: interprocess or std::os::windows::named_pipe wiring
