use crate::{Session, SessionOptions, StdioServer, StdioServerOptions};

// Go: internal/api/server.go:NewStdioServer — requires Cwd
#[test]
#[should_panic(expected = "StdioServerOptions.Cwd is required")]
fn stdio_server_new_panics_without_cwd() {
    let _ = StdioServer::new(StdioServerOptions::default());
}

// Go: internal/api/server.go:NewStdioServer
#[test]
fn stdio_server_new_with_cwd() {
    let server = StdioServer::new(StdioServerOptions {
        cwd: "/tmp".into(),
        ..Default::default()
    });
    let _ = server;
}

// Go: internal/api/session.go:NewSession
#[test]
fn session_binary_responses_flag() {
    let s = Session::new(SessionOptions {
        use_binary_responses: true,
    });
    assert!(s.use_binary_responses());
}
