//! Port of Go `internal/lsp/server.go`: the LSP server skeleton.
//!
//! The [`Server`] reads JSON-RPC messages from an in-memory queue (suitable for
//! testing) or, later, from stdio, dispatches them to the appropriate handler,
//! and collects response messages.
//!
//! # Divergence from Go
//! - Go uses goroutine-based concurrent read/dispatch/write loops. This port
//!   uses a synchronous single-threaded loop for correctness-first testing; the
//!   async split is deferred to the CLI `--lsp` wiring (T3-8).
//! - Go's `Server` holds a `*project.Session` directly. This port takes a
//!   generic `S: Session` trait so tests can plug in stubs.

use std::io::{Read, Write};

use serde_json::value::RawValue;
use tsgo_jsonrpc::{Id, Message, MessageKind, Reader, ResponseError, Writer};

/// LSP error code: the server has not been initialized.
///
/// Side effects: none (pure constant).
// Go: internal/lsp/lsproto/lsp_generated.go:ErrorCodeServerNotInitialized
pub const ERROR_CODE_SERVER_NOT_INITIALIZED: i32 = -32002;

/// LSP error code: invalid request.
///
/// Side effects: none (pure constant).
// Go: internal/lsp/lsproto/lsp_generated.go:ErrorCodeInvalidRequest
pub const ERROR_CODE_INVALID_REQUEST: i32 = -32600;

/// Errors produced by the server's run loop.
///
/// Side effects: none (pure error enum).
// Go: internal/lsp/server.go (various error paths)
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// The server shut down cleanly via `shutdown` + `exit`.
    #[error("server shutdown")]
    Shutdown,
    /// JSON serialization/deserialization failure.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// The session trait: the bridge between the LSP server and the project system.
///
/// Tests inject a stub; production code will wire in the real project session.
///
/// Side effects: implementations may read/write project state.
// Go: internal/lsp/server.go:Server.session (*project.Session usage)
pub trait Session {
    /// A file was opened in the editor.
    // Go: internal/lsp/server.go:Server.handleDidOpen → session.DidOpenFile
    fn did_open_file(&self, uri: &str, version: i32, text: &str);

    /// A file's content changed.
    // Go: internal/lsp/server.go:Server.handleDidChange → session.DidChangeFile
    fn did_change_file(&self, uri: &str, version: i32, changes: &serde_json::Value);

    /// A file was closed in the editor.
    // Go: internal/lsp/server.go:Server.handleDidClose → session.DidCloseFile
    fn did_close_file(&self, uri: &str);

    /// Handles `textDocument/hover`: returns a serialized `Hover` result or
    /// `None` if no info is available.
    // Go: internal/lsp/server.go:handlers() → "textDocument/hover" → session.ProvideHover
    fn hover(&self, uri: &str, position: &serde_json::Value) -> Option<serde_json::Value> {
        let _ = (uri, position);
        None
    }

    /// Handles `textDocument/definition`: returns a serialized `Location[]`
    /// result. Default returns an empty locations array.
    // Go: internal/lsp/server.go:handlers() → "textDocument/definition"
    fn definition(&self, uri: &str, position: &serde_json::Value) -> Option<serde_json::Value> {
        let _ = (uri, position);
        Some(serde_json::json!([]))
    }

    /// Handles `textDocument/completion`: returns a serialized `CompletionList`
    /// result or `None`.
    // Go: internal/lsp/server.go:handlers() → "textDocument/completion"
    fn completion(&self, uri: &str, position: &serde_json::Value) -> Option<serde_json::Value> {
        let _ = (uri, position);
        None
    }

    /// Handles `textDocument/references`: returns a serialized `Location[]`
    /// result. Default returns an empty locations array.
    // Go: internal/lsp/server.go:handlers() → "textDocument/references"
    fn references(&self, uri: &str, position: &serde_json::Value) -> Option<serde_json::Value> {
        let _ = (uri, position);
        Some(serde_json::json!([]))
    }

    /// Handles `textDocument/rename`: returns a serialized rename locations
    /// result or `None`.
    // Go: internal/lsp/server.go:handlers() → "textDocument/rename"
    fn rename(
        &self,
        uri: &str,
        position: &serde_json::Value,
        new_name: &str,
    ) -> Option<serde_json::Value> {
        let _ = (uri, position, new_name);
        None
    }

    /// Handles `textDocument/formatting`: returns a serialized `TextEdit[]`
    /// result. Default returns an empty edits array.
    // Go: internal/lsp/server.go:handlers() → "textDocument/formatting"
    fn formatting(&self, uri: &str, options: &serde_json::Value) -> Option<serde_json::Value> {
        let _ = (uri, options);
        Some(serde_json::json!([]))
    }

    /// Handles `textDocument/semanticTokens/full`: returns a serialized
    /// `SemanticTokens` result or `None`.
    // Go: internal/lsp/server.go:handlers() → "textDocument/semanticTokens/full"
    fn semantic_tokens_full(&self, uri: &str) -> Option<serde_json::Value> {
        let _ = uri;
        None
    }

    /// Handles `textDocument/signatureHelp`: returns a serialized
    /// `SignatureHelp` result or `None` (null = not available).
    // Go: internal/lsp/server.go:handlers() → "textDocument/signatureHelp"
    fn signature_help(&self, uri: &str, position: &serde_json::Value) -> Option<serde_json::Value> {
        let _ = (uri, position);
        None
    }

    /// Handles `textDocument/codeAction`: returns a serialized
    /// `(Command | CodeAction)[]` result. Default returns an empty array.
    // Go: internal/lsp/server.go:handlers() → "textDocument/codeAction"
    fn code_action(
        &self,
        uri: &str,
        range: &serde_json::Value,
        context: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        let _ = (uri, range, context);
        Some(serde_json::json!([]))
    }

    /// Handles `textDocument/documentSymbol`: returns a serialized
    /// `DocumentSymbol[] | SymbolInformation[]` result. Default returns an empty
    /// array.
    // Go: internal/lsp/server.go:handlers() → "textDocument/documentSymbol"
    fn document_symbol(&self, uri: &str) -> Option<serde_json::Value> {
        let _ = uri;
        Some(serde_json::json!([]))
    }
}

/// The LSP server: reads JSON-RPC messages, dispatches to handlers, collects
/// responses.
///
/// Generic over `S: Session` so tests can inject stubs without the full project
/// system.
///
/// Side effects: construction is pure; [`Server::run`] processes messages.
// Go: internal/lsp/server.go:Server
pub struct Server<S: Session> {
    incoming: Vec<Message>,
    session: S,
    initialized: bool,
    shutdown: bool,
}

impl<S: Session> Server<S> {
    /// Creates a server with an in-memory message queue and the given session.
    ///
    /// # Examples
    /// ```ignore
    /// let server = Server::new(vec![], StubSession::default());
    /// ```
    ///
    /// Side effects: none (pure construction).
    // Go: internal/lsp/server.go:NewServer
    pub fn new(incoming: Vec<Message>, session: S) -> Self {
        Server {
            incoming,
            session,
            initialized: false,
            shutdown: false,
        }
    }

    /// Returns a reference to the session, useful for test assertions.
    ///
    /// Side effects: none (pure accessor).
    // Go: internal/lsp/server.go:Server.Session
    pub fn session(&self) -> &S {
        &self.session
    }

    /// The main dispatch loop: processes all incoming messages and returns
    /// the collected response messages.
    ///
    /// The loop exits when an `exit` notification is received, or when all
    /// messages are consumed.
    ///
    /// Side effects: dispatches handlers which may mutate session state.
    // Go: internal/lsp/server.go:Server.Run (+ readLoop + dispatchLoop)
    pub fn run(&mut self) -> Result<Vec<Message>, ServerError> {
        let messages: Vec<Message> = std::mem::take(&mut self.incoming);
        let mut responses: Vec<Message> = Vec::new();

        for msg in messages {
            match msg.kind() {
                MessageKind::Request => {
                    let resp = self.handle_request(&msg);
                    responses.push(resp);
                }
                MessageKind::Notification => {
                    if self.handle_notification(&msg) {
                        break; // exit notification
                    }
                }
                MessageKind::Response => {
                    // Client responses are not expected in this skeleton.
                }
            }
        }

        Ok(responses)
    }

    /// Runs the server over a streaming JSON-RPC transport (stdin/stdout in
    /// production). Reads messages one at a time from `reader`, dispatches
    /// them, and writes response messages to `writer`.
    ///
    /// The loop exits when `exit` is received or the reader yields EOF.
    ///
    /// Side effects: reads from `reader`, writes to `writer`, dispatches
    /// handlers which may mutate session state.
    // Go: internal/lsp/server.go:Server.Run (the goroutine-based read/write loops)
    pub fn run_stdio<R: Read, W: Write>(
        &mut self,
        reader: &mut Reader<R>,
        writer: &mut Writer<W>,
    ) -> Result<(), ServerError> {
        loop {
            let data = match reader.read() {
                Ok(Some(data)) => data,
                Ok(None) => return Ok(()),
                Err(_) => return Ok(()),
            };
            let msg: Message = serde_json::from_slice(&data)?;

            match msg.kind() {
                MessageKind::Request => {
                    let resp = self.handle_request(&msg);
                    let payload = serde_json::to_vec(&resp)?;
                    let _ = writer.write(&payload);
                }
                MessageKind::Notification => {
                    if self.handle_notification(&msg) {
                        return Ok(());
                    }
                }
                MessageKind::Response => {}
            }
        }
    }

    /// Dispatches a request message (has id + method) and returns a response.
    ///
    /// Side effects: may mutate initialization state and session.
    // Go: internal/lsp/server.go:Server.readLoop (pre-init guard) + handleRequestOrNotification
    fn handle_request(&mut self, msg: &Message) -> Message {
        let id = msg.id.clone();
        let method = msg.method.as_str();

        if !self.initialized && method != "initialize" {
            return self.error_response(
                id,
                ERROR_CODE_SERVER_NOT_INITIALIZED,
                "server not initialized",
            );
        }

        match method {
            "initialize" => self.handle_initialize(msg),
            "shutdown" => self.handle_shutdown(msg),
            _ => {
                if self.shutdown {
                    return self.error_response(
                        id,
                        ERROR_CODE_INVALID_REQUEST,
                        "server is shutting down",
                    );
                }
                self.dispatch_feature_request(msg)
            }
        }
    }

    /// Dispatches a notification message (no id).
    ///
    /// Returns `true` if the server should exit (i.e. `exit` notification).
    ///
    /// Side effects: may mutate session state.
    // Go: internal/lsp/server.go:Server.handleRequestOrNotification (notification path)
    fn handle_notification(&mut self, msg: &Message) -> bool {
        let method = msg.method.as_str();

        if !self.initialized && method != "exit" {
            // Pre-initialize: only `initialized` after `initialize` response is sent.
            // But `initialized` notification is how the client confirms; if we haven't
            // gotten `initialize` yet, ignore silently.
            return false;
        }

        match method {
            "initialized" => {
                self.handle_initialized(msg);
                false
            }
            "exit" => {
                // Go: internal/lsp/server.go:Server.handleExit → io.EOF
                true
            }
            "textDocument/didOpen" => {
                self.handle_did_open(msg);
                false
            }
            "textDocument/didChange" => {
                self.handle_did_change(msg);
                false
            }
            "textDocument/didClose" => {
                self.handle_did_close(msg);
                false
            }
            _ => {
                // Unknown notification: log and ignore (per JSON-RPC spec,
                // no response for notifications).
                false
            }
        }
    }

    /// Handles the `initialize` request: validates single-init, returns
    /// [`ServerCapabilities`].
    ///
    /// Side effects: sets `self.initialized = true`.
    // Go: internal/lsp/server.go:Server.handleInitialize
    fn handle_initialize(&mut self, msg: &Message) -> Message {
        let id = msg.id.clone();

        if self.initialized {
            return self.error_response(
                id,
                ERROR_CODE_INVALID_REQUEST,
                "server already initialized",
            );
        }

        self.initialized = true;

        let result = serde_json::json!({
            "serverInfo": {
                "name": "typescript-go",
                "version": tsgo_core::version::version()
            },
            "capabilities": {
                "positionEncoding": "utf-16",
                "textDocumentSync": {
                    "openClose": true,
                    "change": 2,
                    "save": true
                },
                "hoverProvider": true,
                "definitionProvider": true,
                "typeDefinitionProvider": true,
                "referencesProvider": true,
                "implementationProvider": true,
                "completionProvider": {
                    "triggerCharacters": [".", "\"", "'", "`", "/", "@", "<", "#", " "],
                    "resolveProvider": true
                },
                "signatureHelpProvider": {
                    "triggerCharacters": ["(", ",", "<"],
                    "retriggerCharacters": [")"]
                },
                "documentFormattingProvider": true,
                "documentRangeFormattingProvider": true,
                "documentOnTypeFormattingProvider": {
                    "firstTriggerCharacter": "{",
                    "moreTriggerCharacter": ["}", ";", "\n"]
                },
                "workspaceSymbolProvider": true,
                "documentSymbolProvider": true,
                "foldingRangeProvider": true,
                "renameProvider": { "prepareProvider": true },
                "documentHighlightProvider": true,
                "selectionRangeProvider": true,
                "linkedEditingRangeProvider": true,
                "inlayHintProvider": true,
                "codeLensProvider": { "resolveProvider": true },
                "codeActionProvider": {
                    "codeActionKinds": [
                        "quickfix",
                        "source.organizeImports",
                        "source.removeUnusedImports",
                        "source.sortImports",
                        "source.fixAll"
                    ]
                },
                "callHierarchyProvider": true,
                "diagnosticProvider": {
                    "interFileDependencies": true
                },
                "semanticTokensProvider": {
                    "full": true,
                    "range": true
                }
            }
        });

        self.success_response(id, result)
    }

    /// Handles the `initialized` notification (no-op in skeleton).
    ///
    /// Side effects: none in skeleton; production would start the session.
    // Go: internal/lsp/server.go:Server.handleInitialized
    fn handle_initialized(&mut self, _msg: &Message) {
        // In the full implementation, this initializes the project.Session.
        // Skeleton: no-op.
    }

    /// Handles the `shutdown` request.
    ///
    /// Side effects: sets `self.shutdown = true`.
    // Go: internal/lsp/server.go:Server.handleShutdown
    fn handle_shutdown(&mut self, msg: &Message) -> Message {
        self.shutdown = true;
        self.success_response(msg.id.clone(), serde_json::Value::Null)
    }

    /// Dispatches a feature request to the appropriate handler.
    ///
    /// Routes recognized `textDocument/*` methods to [`Session`] trait methods,
    /// returning the serialized result. Unrecognized methods yield a
    /// method-not-found error.
    ///
    /// Side effects: delegates to session methods which may read project state.
    // Go: internal/lsp/server.go:handlers() dispatch table
    fn dispatch_feature_request(&self, msg: &Message) -> Message {
        let id = msg.id.clone();
        let method = msg.method.as_str();
        let params = msg
            .params
            .as_ref()
            .and_then(|p| serde_json::from_str::<serde_json::Value>(p.get()).ok())
            .unwrap_or(serde_json::Value::Null);

        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let position = &params["position"];

        let result: Option<serde_json::Value> = match method {
            "textDocument/hover" => self.session.hover(uri, position),
            "textDocument/definition" => self.session.definition(uri, position),
            "textDocument/completion" => self.session.completion(uri, position),
            "textDocument/references" => self.session.references(uri, position),
            "textDocument/rename" => {
                let new_name = params["newName"].as_str().unwrap_or("");
                self.session.rename(uri, position, new_name)
            }
            "textDocument/formatting" => {
                let options = &params["options"];
                self.session.formatting(uri, options)
            }
            "textDocument/signatureHelp" => self.session.signature_help(uri, position),
            "textDocument/codeAction" => {
                let range = &params["range"];
                let context = &params["context"];
                self.session.code_action(uri, range, context)
            }
            "textDocument/documentSymbol" => self.session.document_symbol(uri),
            "textDocument/semanticTokens/full" => self.session.semantic_tokens_full(uri),
            _ => {
                return self.error_response(
                    id,
                    tsgo_jsonrpc::CODE_METHOD_NOT_FOUND,
                    &format!("method not found: {method}"),
                );
            }
        };

        match result {
            Some(value) => self.success_response(id, value),
            None => self.success_response(id, serde_json::Value::Null),
        }
    }

    /// Handles `textDocument/didOpen`: extracts URI and text, forwards to session.
    ///
    /// Side effects: calls `session.did_open_file`.
    // Go: internal/lsp/server.go:Server.handleDidOpen
    fn handle_did_open(&self, msg: &Message) {
        if let Some(params) = &msg.params {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(params.get()) {
                let td = &v["textDocument"];
                let uri = td["uri"].as_str().unwrap_or("");
                let version = td["version"].as_i64().unwrap_or(0) as i32;
                let text = td["text"].as_str().unwrap_or("");
                self.session.did_open_file(uri, version, text);
            }
        }
    }

    /// Handles `textDocument/didChange`: extracts URI and changes, forwards to
    /// session.
    ///
    /// Side effects: calls `session.did_change_file`.
    // Go: internal/lsp/server.go:Server.handleDidChange
    fn handle_did_change(&self, msg: &Message) {
        if let Some(params) = &msg.params {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(params.get()) {
                let td = &v["textDocument"];
                let uri = td["uri"].as_str().unwrap_or("");
                let version = td["version"].as_i64().unwrap_or(0) as i32;
                let changes = &v["contentChanges"];
                self.session.did_change_file(uri, version, changes);
            }
        }
    }

    /// Handles `textDocument/didClose`: extracts URI, forwards to session.
    ///
    /// Side effects: calls `session.did_close_file`.
    // Go: internal/lsp/server.go:Server.handleDidClose
    fn handle_did_close(&self, msg: &Message) {
        if let Some(params) = &msg.params {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(params.get()) {
                let uri = v["textDocument"]["uri"].as_str().unwrap_or("");
                self.session.did_close_file(uri);
            }
        }
    }

    /// Builds a success response with the given result.
    ///
    /// Side effects: none (pure).
    // Go: internal/lsp/server.go:Server.sendResult
    fn success_response(&self, id: Option<Id>, result: serde_json::Value) -> Message {
        Message {
            id,
            result: Some(RawValue::from_string(serde_json::to_string(&result).unwrap()).unwrap()),
            ..Default::default()
        }
    }

    /// Builds an error response with the given code and message.
    ///
    /// Side effects: none (pure).
    // Go: internal/lsp/server.go:Server.sendError
    fn error_response(&self, id: Option<Id>, code: i32, message: &str) -> Message {
        Message {
            id,
            error: Some(ResponseError {
                code,
                message: message.to_string(),
                data: None,
            }),
            ..Default::default()
        }
    }
}

#[cfg(test)]
#[path = "server_test.rs"]
mod tests;
