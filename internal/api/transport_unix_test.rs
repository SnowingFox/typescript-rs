use super::generate_pipe_path;

// Go: internal/api/transport_unix.go:GeneratePipePath
#[test]
fn generate_pipe_path_under_temp_dir() {
    let p = generate_pipe_path("tsgo-api-test.sock");
    let file_name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
    assert_eq!(file_name, "tsgo-api-test.sock");
    assert!(p.parent().is_some());
}
