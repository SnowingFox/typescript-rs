use super::*;
use tsgo_jsonrpc::{Id, Message};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Builds a JSON-RPC request message for a given method with raw JSON params.
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

/// Builds a JSON-RPC notification (no id) for a given method with raw JSON params.
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

/// Minimal `InitializeParams` as raw JSON, enough to pass validation.
fn initialize_params() -> serde_json::Value {
    serde_json::json!({
        "capabilities": {}
    })
}

/// A stub session that records calls but does nothing.
#[derive(Default)]
struct StubSession {
    opened_files: std::cell::RefCell<Vec<(String, String)>>,
    changed_files: std::cell::RefCell<Vec<String>>,
    closed_files: std::cell::RefCell<Vec<String>>,
}

impl Session for StubSession {
    fn did_open_file(&self, uri: &str, _version: i32, text: &str) {
        self.opened_files
            .borrow_mut()
            .push((uri.to_string(), text.to_string()));
    }

    fn did_change_file(&self, uri: &str, _version: i32, _changes: &serde_json::Value) {
        self.changed_files.borrow_mut().push(uri.to_string());
    }

    fn did_close_file(&self, uri: &str) {
        self.closed_files.borrow_mut().push(uri.to_string());
    }

    fn hover(&self, uri: &str, _position: &serde_json::Value) -> Option<serde_json::Value> {
        if uri == "file:///test.ts" {
            Some(serde_json::json!({
                "contents": { "kind": "plaintext", "value": "number" },
                "range": {
                    "start": { "line": 0, "character": 6 },
                    "end": { "line": 0, "character": 7 }
                }
            }))
        } else {
            None
        }
    }

    fn completion(&self, uri: &str, _position: &serde_json::Value) -> Option<serde_json::Value> {
        if uri == "file:///test.ts" {
            Some(serde_json::json!({
                "isIncomplete": false,
                "items": [
                    { "label": "foo", "kind": 6 },
                    { "label": "bar", "kind": 6 }
                ]
            }))
        } else {
            None
        }
    }
}

/// Creates a Server with an in-memory message queue and a stub session.
fn test_server(incoming: Vec<Message>) -> Server<StubSession> {
    Server::new(incoming, StubSession::default())
}

// ---------------------------------------------------------------------------
// T1: initialize request → response with ServerCapabilities (headline test)
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:Server.handleInitialize
#[test]
fn initialize_returns_server_capabilities() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_request(2, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    let responses = server.run().unwrap();

    // The first response should be the initialize result.
    let init_resp = &responses[0];
    assert_eq!(init_resp.id, Some(Id::Int(1)));
    assert!(init_resp.error.is_none(), "initialize should not error");
    let result: serde_json::Value =
        serde_json::from_str(init_resp.result.as_ref().unwrap().get()).unwrap();
    assert!(
        result.get("capabilities").is_some(),
        "must have capabilities"
    );
    let caps = &result["capabilities"];
    assert!(
        caps.get("hoverProvider").is_some(),
        "hoverProvider expected"
    );
    assert!(
        caps.get("definitionProvider").is_some(),
        "definitionProvider expected"
    );
    // Check server info
    let info = &result["serverInfo"];
    assert_eq!(info["name"], "typescript-go");
}

// ---------------------------------------------------------------------------
// T2: unknown method → error response (not a crash)
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:Server.handleRequestOrNotification (unknown method branch)
#[test]
fn unknown_method_returns_error() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_request(2, "nonexistent/method", serde_json::json!({})),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    let responses = server.run().unwrap();

    // Response for id=2 should be an error.
    let unknown_resp = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(
        unknown_resp.error.is_some(),
        "unknown method should return error"
    );
    let err = unknown_resp.error.as_ref().unwrap();
    assert_eq!(err.code, tsgo_jsonrpc::CODE_METHOD_NOT_FOUND);
}

// ---------------------------------------------------------------------------
// T3: request before initialize → ServerNotInitialized error
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:Server.readLoop (pre-initialize guard)
#[test]
fn request_before_initialize_returns_not_initialized() {
    let msgs = vec![
        make_request(1, "textDocument/hover", serde_json::json!({})),
        make_request(2, "initialize", initialize_params()),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    let responses = server.run().unwrap();

    let pre_init = responses.iter().find(|r| r.id == Some(Id::Int(1))).unwrap();
    assert!(pre_init.error.is_some());
    let err = pre_init.error.as_ref().unwrap();
    assert_eq!(err.code, ERROR_CODE_SERVER_NOT_INITIALIZED);
}

// ---------------------------------------------------------------------------
// T4: shutdown + exit lifecycle
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:Server.handleShutdown / handleExit
#[test]
fn shutdown_then_exit_stops_server() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_request(2, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    let responses = server.run().unwrap();

    let shutdown_resp = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(shutdown_resp.error.is_none(), "shutdown should succeed");
    assert!(shutdown_resp.result.is_some());
}

// ---------------------------------------------------------------------------
// T5: didOpen notification is forwarded to session
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:Server.handleDidOpen
#[test]
fn did_open_forwarded_to_session() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_notification("initialized", serde_json::json!({})),
        make_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": "file:///test.ts",
                    "languageId": "typescript",
                    "version": 1,
                    "text": "const x = 1;"
                }
            }),
        ),
        make_request(2, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    server.run().unwrap();

    let opened = server.session().opened_files.borrow();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].0, "file:///test.ts");
    assert_eq!(opened[0].1, "const x = 1;");
}

// ---------------------------------------------------------------------------
// T6: didClose notification is forwarded to session
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:Server.handleDidClose
#[test]
fn did_close_forwarded_to_session() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_notification("initialized", serde_json::json!({})),
        make_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": "file:///test.ts",
                    "languageId": "typescript",
                    "version": 1,
                    "text": "const x = 1;"
                }
            }),
        ),
        make_notification(
            "textDocument/didClose",
            serde_json::json!({
                "textDocument": { "uri": "file:///test.ts" }
            }),
        ),
        make_request(2, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    server.run().unwrap();

    let closed = server.session().closed_files.borrow();
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0], "file:///test.ts");
}

// ---------------------------------------------------------------------------
// T7: didChange notification is forwarded to session
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:Server.handleDidChange
#[test]
fn did_change_forwarded_to_session() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_notification("initialized", serde_json::json!({})),
        make_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": "file:///test.ts",
                    "languageId": "typescript",
                    "version": 1,
                    "text": "const x = 1;"
                }
            }),
        ),
        make_notification(
            "textDocument/didChange",
            serde_json::json!({
                "textDocument": { "uri": "file:///test.ts", "version": 2 },
                "contentChanges": [{ "text": "const x = 2;" }]
            }),
        ),
        make_request(2, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    server.run().unwrap();

    let changed = server.session().changed_files.borrow();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0], "file:///test.ts");
}

// ---------------------------------------------------------------------------
// T8: notification for unknown method does not crash (no error response for
//     notifications per JSON-RPC spec)
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:Server.handleRequestOrNotification (unknown notification)
#[test]
fn unknown_notification_ignored_no_crash() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_notification("initialized", serde_json::json!({})),
        make_notification("unknownNotification/foo", serde_json::json!({})),
        make_request(2, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    let responses = server.run().unwrap();
    // Only init + shutdown responses; no crash, no response for the notification.
    assert!(responses
        .iter()
        .all(|r| r.error.is_none() || r.id != Some(Id::Int(1))));
}

// ---------------------------------------------------------------------------
// T9: double initialize returns error
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:Server.handleInitialize (initializeParams != nil guard)
#[test]
fn double_initialize_returns_error() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_request(2, "initialize", initialize_params()),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    let responses = server.run().unwrap();

    let second_init = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(second_init.error.is_some(), "second initialize should fail");
}

// ---------------------------------------------------------------------------
// T10: hover request dispatches through session and returns result
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:handlers() → "textDocument/hover"
#[test]
fn hover_request_returns_result() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_request(
            2,
            "textDocument/hover",
            serde_json::json!({
                "textDocument": { "uri": "file:///test.ts" },
                "position": { "line": 0, "character": 6 }
            }),
        ),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    let responses = server.run().unwrap();

    let hover_resp = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(hover_resp.error.is_none(), "hover should not error");
    let result: serde_json::Value =
        serde_json::from_str(hover_resp.result.as_ref().unwrap().get()).unwrap();
    assert_eq!(result["contents"]["value"], "number");
    assert_eq!(result["range"]["start"]["character"], 6);
}

// ---------------------------------------------------------------------------
// T11: hover on unknown file returns null (no error)
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:handlers() → "textDocument/hover" (nil hover)
#[test]
fn hover_on_unknown_file_returns_null() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_request(
            2,
            "textDocument/hover",
            serde_json::json!({
                "textDocument": { "uri": "file:///unknown.ts" },
                "position": { "line": 0, "character": 0 }
            }),
        ),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    let responses = server.run().unwrap();

    let hover_resp = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(hover_resp.error.is_none(), "no-hover should not error");
    let result: serde_json::Value =
        serde_json::from_str(hover_resp.result.as_ref().unwrap().get()).unwrap();
    assert!(result.is_null(), "expected null result for no hover info");
}

// ---------------------------------------------------------------------------
// T12: completion request dispatches through session and returns items
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:handlers() → "textDocument/completion"
#[test]
fn completion_request_returns_items() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_request(
            2,
            "textDocument/completion",
            serde_json::json!({
                "textDocument": { "uri": "file:///test.ts" },
                "position": { "line": 0, "character": 8 }
            }),
        ),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    let responses = server.run().unwrap();

    let comp_resp = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(comp_resp.error.is_none(), "completion should not error");
    let result: serde_json::Value =
        serde_json::from_str(comp_resp.result.as_ref().unwrap().get()).unwrap();
    let items = result["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["label"], "foo");
    assert_eq!(items[1]["label"], "bar");
}

// ---------------------------------------------------------------------------
// T13: definition request (default impl returns null)
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:handlers() → "textDocument/definition"
#[test]
fn definition_request_returns_null_from_default_impl() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_request(
            2,
            "textDocument/definition",
            serde_json::json!({
                "textDocument": { "uri": "file:///test.ts" },
                "position": { "line": 0, "character": 0 }
            }),
        ),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    let responses = server.run().unwrap();

    let def_resp = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(def_resp.error.is_none(), "definition should not error");
    let result: serde_json::Value =
        serde_json::from_str(def_resp.result.as_ref().unwrap().get()).unwrap();
    assert!(result.is_null());
}

// ---------------------------------------------------------------------------
// T14: semanticTokens/full request (default impl returns null)
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:handlers() → "textDocument/semanticTokens/full"
#[test]
fn semantic_tokens_full_returns_null_from_default_impl() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_request(
            2,
            "textDocument/semanticTokens/full",
            serde_json::json!({
                "textDocument": { "uri": "file:///test.ts" }
            }),
        ),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    let responses = server.run().unwrap();

    let tok_resp = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(tok_resp.error.is_none(), "semanticTokens should not error");
    let result: serde_json::Value =
        serde_json::from_str(tok_resp.result.as_ref().unwrap().get()).unwrap();
    assert!(result.is_null());
}

// ---------------------------------------------------------------------------
// T15: references request (default impl returns null)
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:handlers() → "textDocument/references"
#[test]
fn references_request_returns_null_from_default_impl() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_request(
            2,
            "textDocument/references",
            serde_json::json!({
                "textDocument": { "uri": "file:///test.ts" },
                "position": { "line": 0, "character": 0 },
                "context": { "includeDeclaration": true }
            }),
        ),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    let responses = server.run().unwrap();

    let ref_resp = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(ref_resp.error.is_none(), "references should not error");
    let result: serde_json::Value =
        serde_json::from_str(ref_resp.result.as_ref().unwrap().get()).unwrap();
    assert!(result.is_null());
}

// ---------------------------------------------------------------------------
// T16: rename request (default impl returns null)
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:handlers() → "textDocument/rename"
#[test]
fn rename_request_returns_null_from_default_impl() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_request(
            2,
            "textDocument/rename",
            serde_json::json!({
                "textDocument": { "uri": "file:///test.ts" },
                "position": { "line": 0, "character": 6 },
                "newName": "y"
            }),
        ),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    let responses = server.run().unwrap();

    let ren_resp = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(ren_resp.error.is_none(), "rename should not error");
    let result: serde_json::Value =
        serde_json::from_str(ren_resp.result.as_ref().unwrap().get()).unwrap();
    assert!(result.is_null());
}

// ---------------------------------------------------------------------------
// T17: formatting request (default impl returns null)
// ---------------------------------------------------------------------------
// Go: internal/lsp/server.go:handlers() → "textDocument/formatting"
#[test]
fn formatting_request_returns_null_from_default_impl() {
    let msgs = vec![
        make_request(1, "initialize", initialize_params()),
        make_request(
            2,
            "textDocument/formatting",
            serde_json::json!({
                "textDocument": { "uri": "file:///test.ts" },
                "options": { "tabSize": 4, "insertSpaces": true }
            }),
        ),
        make_request(3, "shutdown", serde_json::json!(null)),
        make_notification("exit", serde_json::json!(null)),
    ];
    let mut server = test_server(msgs);
    let responses = server.run().unwrap();

    let fmt_resp = responses.iter().find(|r| r.id == Some(Id::Int(2))).unwrap();
    assert!(fmt_resp.error.is_none(), "formatting should not error");
    let result: serde_json::Value =
        serde_json::from_str(fmt_resp.result.as_ref().unwrap().get()).unwrap();
    assert!(result.is_null());
}
