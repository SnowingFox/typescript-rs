use super::*;

// Go side has no `*_test.go` (heavy side effects via runtime/pprof); behavior is
// covered by P10 parity and manual verification. The cases below pin the
// side-effect-free state machine, file naming, and directory handling through
// the public API, isolating disk writes in a temp dir. Expected values are the
// Go error-message literals and file-name suffixes.

// Go: internal/pprof/pprof.go:(*CPUProfiler).StartCPUProfile — double start errors
#[test]
fn cpu_profiler_double_start_errors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let profiler = CpuProfiler::new();
    profiler.start_cpu_profile(tmp.path()).unwrap();
    let err = profiler.start_cpu_profile(tmp.path()).unwrap_err();
    assert_eq!(err.to_string(), "CPU profiling already in progress");
    let _ = profiler.stop_cpu_profile();
}

// Go: internal/pprof/pprof.go:(*CPUProfiler).StopCPUProfile — stop without start errors
#[test]
fn cpu_profiler_stop_without_start_errors() {
    let profiler = CpuProfiler::new();
    let err = profiler.stop_cpu_profile().unwrap_err();
    assert_eq!(err.to_string(), "CPU profiling not in progress");
}

// Go: internal/pprof/pprof.go:StartCPUProfile/StopCPUProfile — start/stop returns path
#[test]
fn cpu_profiler_start_stop_returns_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let profiler = CpuProfiler::new();
    profiler.start_cpu_profile(tmp.path()).unwrap();
    let path = profiler.stop_cpu_profile().unwrap();
    assert!(path.ends_with("-cpuprofile.pb.gz"), "got {path}");
}

// Go: internal/pprof/pprof.go:StartCPUProfile — filename embeds the pid
#[test]
fn cpu_profile_filename_contains_pid() {
    let tmp = tempfile::TempDir::new().unwrap();
    let profiler = CpuProfiler::new();
    profiler.start_cpu_profile(tmp.path()).unwrap();
    let path = profiler.stop_cpu_profile().unwrap();
    let pid = std::process::id().to_string();
    assert!(path.contains(&pid), "path {path} should contain pid {pid}");
}

// Go: internal/pprof/pprof.go:BeginProfiling — creates the dir, sets cpu/mem paths
#[test]
fn begin_profiling_creates_dir() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("nested").join("profiles");
    let session = begin_profiling(&dir, Box::new(io::sink()));
    assert!(dir.exists());
    assert!(session
        .cpu_file_path
        .to_string_lossy()
        .ends_with("-cpuprofile.pb.gz"));
    assert!(session
        .mem_file_path
        .to_string_lossy()
        .ends_with("-memprofile.pb.gz"));
    session.stop();
}

// Go: internal/pprof/pprof.go:SaveHeapProfile — returns heap profile path
#[test]
fn save_heap_profile_returns_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = save_heap_profile(tmp.path()).unwrap();
    assert!(path.ends_with("-heapprofile.pb.gz"), "got {path}");
}

// Go: internal/pprof/pprof.go:SaveAllocProfile — returns alloc profile path
#[test]
fn save_alloc_profile_returns_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = save_alloc_profile(tmp.path()).unwrap();
    assert!(path.ends_with("-allocprofile.pb.gz"), "got {path}");
}

// Go: internal/pprof/pprof.go:RunGC — no-op, must not panic
#[test]
fn run_gc_no_panic() {
    run_gc();
}
