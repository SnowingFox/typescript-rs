// Integration tests: ProjectSession adapter bridges tsgo_project::session::Session
// to the LSP server::Session trait.
// Go: internal/lsp/server.go (session integration)

use super::*;
use crate::server::Server;
use std::collections::HashMap;
use std::sync::Arc;
use tsgo_jsonrpc::{Id, Message};
use tsgo_project::client::NoopClient;
use tsgo_project::overlayfs::OverlayFS;
use tsgo_project::session::{Session as ProjectSessionInner, SessionOptions};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_project_session() -> ProjectSession {
    let empty: Vec<(&str, &str)> = Vec::new();
    let fs: Box<dyn tsgo_vfs::Fs + Send + Sync> =
        Box::new(tsgo_vfs::vfstest::MapFs::from_map(empty, false));
    let overlay_fs = OverlayFS::new(fs, HashMap::new(), |s: &str| {
        tsgo_tspath::Path(s.to_lowercase())
    });
    let opts = SessionOptions {
        current_directory: "/".to_string(),
    };
    let inner = ProjectSessionInner::new(opts, overlay_fs, Arc::new(NoopClient));
    ProjectSession::new(inner)
}

fn make_request(id: i32, method: &str, params: serde_json::Value) -> Message {
    Message {
        id: Some(Id::Int(id)),
        method: method.to_string(),
        params: Some(
            serde_json::value::RawValue::from_string(serde_json::to_string(&params).unwrap())
                .unwrap(),
        ),
        ..Default::default()
    }
}

fn make_notification(method: &str, params: serde_json::Value) -> Message {
    Message {
        method: method.to_string(),
        params: Some(
            serde_json::value::RawValue::from_string(serde_json::to_string(&params).unwrap())
                .unwrap(),
        ),
        ..Default::default()
    }
}

fn initialize_params() -> serde_json::Value {
    serde_json::json!({ "capabilities": {} })
}

// ---------------------------------------------------------------------------
// T1: didOpen → session has the file registered
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:Server.handleDidOpen → session.DidOpenFile
#[test]
fn did_open_via_project_session_registers_file() {
    let session = make_project_session();
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_notification("initialized", serde_json::json!({})),
        make_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": "file:///app/index.ts",
                    "languageId": "typescript",
                    "version": 1,
                    "text": "const x = 1;"
                }
            }),
        ),
        make_request(2, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = Server::new(msgs, session);
    server.run().unwrap();

    assert!(
        server.session().overlay_has_file("file:///app/index.ts"),
        "didOpen should register the file in the project session overlay"
    );
}

// ---------------------------------------------------------------------------
// T2: didClose → session no longer has the file
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:Server.handleDidClose → session.DidCloseFile
#[test]
fn did_close_via_project_session_unregisters_file() {
    let session = make_project_session();
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_notification("initialized", serde_json::json!({})),
        make_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": "file:///app/index.ts",
                    "languageId": "typescript",
                    "version": 1,
                    "text": "const x = 1;"
                }
            }),
        ),
        make_notification(
            "textDocument/didClose",
            serde_json::json!({
                "textDocument": { "uri": "file:///app/index.ts" }
            }),
        ),
        make_request(2, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = Server::new(msgs, session);
    server.run().unwrap();

    assert!(
        !server.session().overlay_has_file("file:///app/index.ts"),
        "didClose should remove the file from the project session overlay"
    );
}

// ---------------------------------------------------------------------------
// T3: didChange → session overlay updated (snapshot ID bumps)
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:Server.handleDidChange → session.DidChangeFile
#[test]
fn did_change_via_project_session_updates_overlay() {
    let session = make_project_session();
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_notification("initialized", serde_json::json!({})),
        make_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": "file:///app/index.ts",
                    "languageId": "typescript",
                    "version": 1,
                    "text": "const x = 1;"
                }
            }),
        ),
        make_notification(
            "textDocument/didChange",
            serde_json::json!({
                "textDocument": { "uri": "file:///app/index.ts", "version": 2 },
                "contentChanges": [{ "text": "const x = 42;" }]
            }),
        ),
        make_request(2, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = Server::new(msgs, session);
    server.run().unwrap();

    assert!(
        server.session().overlay_has_file("file:///app/index.ts"),
        "file should still be in overlay after change"
    );
    assert!(
        server.session().snapshot_id() >= 2,
        "snapshot should have been rebuilt at least twice (open + change)"
    );
}

// ---------------------------------------------------------------------------
// T4: hover for opened file → returns a response (placeholder)
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:handlers() → "textDocument/hover"
#[test]
fn hover_for_opened_file_returns_placeholder() {
    let session = make_project_session();
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_notification("initialized", serde_json::json!({})),
        make_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": "file:///app/index.ts",
                    "languageId": "typescript",
                    "version": 1,
                    "text": "const x = 1;"
                }
            }),
        ),
        make_request(
            2,
            "textDocument/hover",
            serde_json::json!({
                "textDocument": { "uri": "file:///app/index.ts" },
                "position": { "line": 0, "character": 6 }
            }),
        ),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = Server::new(msgs, session);
    let responses = server.run().unwrap();

    let hover_resp = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(hover_resp.error.is_none(), "hover should not error");
    let result: serde_json::Value =
        serde_json::from_str(hover_resp.result.as_ref().unwrap().get()).unwrap();
    assert!(
        !result.is_null(),
        "hover for an opened file should return a non-null placeholder"
    );
    assert!(
        result["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("project"),
        "placeholder should mention the project"
    );
}

// ---------------------------------------------------------------------------
// T5: hover for unknown file → returns null result
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:handlers() → "textDocument/hover" (no project found)
#[test]
fn hover_for_unknown_file_returns_null() {
    let session = make_project_session();
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_notification("initialized", serde_json::json!({})),
        make_request(
            2,
            "textDocument/hover",
            serde_json::json!({
                "textDocument": { "uri": "file:///unknown/file.ts" },
                "position": { "line": 0, "character": 0 }
            }),
        ),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = Server::new(msgs, session);
    let responses = server.run().unwrap();

    let hover_resp = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(
        hover_resp.error.is_none(),
        "hover should not error even for unknown file"
    );
    let result: serde_json::Value =
        serde_json::from_str(hover_resp.result.as_ref().unwrap().get()).unwrap();
    assert!(
        result.is_null(),
        "hover for an unknown file should return null"
    );
}

// ---------------------------------------------------------------------------
// T6: completion for opened file → returns empty completion list
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:handlers() → "textDocument/completion"
#[test]
fn completion_for_opened_file_returns_empty_list() {
    let session = make_project_session();
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_notification("initialized", serde_json::json!({})),
        make_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": "file:///app/index.ts",
                    "languageId": "typescript",
                    "version": 1,
                    "text": "const x = 1;"
                }
            }),
        ),
        make_request(
            2,
            "textDocument/completion",
            serde_json::json!({
                "textDocument": { "uri": "file:///app/index.ts" },
                "position": { "line": 0, "character": 12 }
            }),
        ),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = Server::new(msgs, session);
    let responses = server.run().unwrap();

    let comp_resp = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(comp_resp.error.is_none(), "completion should not error");
    let result: serde_json::Value =
        serde_json::from_str(comp_resp.result.as_ref().unwrap().get()).unwrap();
    assert!(
        !result.is_null(),
        "completion for opened file should return a non-null result"
    );
    let items = result["items"].as_array().unwrap();
    assert!(
        items.is_empty(),
        "completion stub should return an empty items list"
    );
    assert_eq!(
        result["isIncomplete"], false,
        "completion stub should report isIncomplete=false"
    );
}

// ---------------------------------------------------------------------------
// T7: completion for unknown file → returns null
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:handlers() → "textDocument/completion" (no project found)
#[test]
fn completion_for_unknown_file_returns_null() {
    let session = make_project_session();
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_notification("initialized", serde_json::json!({})),
        make_request(
            2,
            "textDocument/completion",
            serde_json::json!({
                "textDocument": { "uri": "file:///unknown/file.ts" },
                "position": { "line": 0, "character": 0 }
            }),
        ),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = Server::new(msgs, session);
    let responses = server.run().unwrap();

    let comp_resp = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(
        comp_resp.error.is_none(),
        "completion should not error for unknown file"
    );
    let result: serde_json::Value =
        serde_json::from_str(comp_resp.result.as_ref().unwrap().get()).unwrap();
    assert!(
        result.is_null(),
        "completion for unknown file should return null"
    );
}
