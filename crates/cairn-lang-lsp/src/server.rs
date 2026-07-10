//! Stdio JSON-RPC server loop.
//!
//! Owns the transport half of the language server: capability negotiation
//! and pushing [`crate::diagnostics::compute_diagnostics`] results to the
//! client via `textDocument/publishDiagnostics`. The loop is synchronous
//! (one message at a time) — the compiler pipeline is fast enough that full
//! recomputation per keystroke stays well under interactive latency for the
//! file sizes `.crn` sources reach.

use lsp_server::{Connection, ErrorCode, Message, Notification, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Exit, Notification as _,
    PublishDiagnostics,
};

use crate::diagnostics::compute_diagnostics;

/// Errors the transport layer can surface. Boxed because every failure mode
/// here (broken pipe, malformed JSON, protocol violation) is terminal for
/// the process — the binary prints it and exits non-zero.
type DynError = Box<dyn std::error::Error + Send + Sync>;

/// Capabilities advertised in the `initialize` response: full-content
/// document sync with open/close notifications. Everything else (completion,
/// hover, code actions) is intentionally absent until it is implemented.
fn server_capabilities() -> lsp_types::ServerCapabilities {
    lsp_types::ServerCapabilities {
        text_document_sync: Some(lsp_types::TextDocumentSyncCapability::Options(
            lsp_types::TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(lsp_types::TextDocumentSyncKind::FULL),
                ..lsp_types::TextDocumentSyncOptions::default()
            },
        )),
        ..lsp_types::ServerCapabilities::default()
    }
}

/// Run the language server over stdio until the client sends `exit`.
///
/// Performs the `initialize` handshake, then processes messages until the
/// `shutdown`/`exit` sequence completes. Returns an error only on transport
/// or protocol failures — a clean client-driven exit returns `Ok(())`.
///
/// # Errors
///
/// Propagates I/O failures on stdin/stdout, JSON (de)serialisation failures
/// on the server's own outgoing messages, protocol violations detected by
/// `lsp-server` (e.g. messages before `initialize`), and an `exit`
/// notification arriving without a preceding `shutdown` request — the LSP
/// spec requires that sequence to end the process with a non-zero code.
pub fn run() -> Result<(), DynError> {
    let (connection, io_threads) = Connection::stdio();
    let capabilities = serde_json::to_value(server_capabilities())?;
    connection.initialize(capabilities)?;
    main_loop(&connection)?;
    // The writer thread only terminates once the outgoing channel closes,
    // which happens when the `Connection` (and with it `sender`) drops —
    // joining before that would deadlock the shutdown.
    drop(connection);
    io_threads.join()?;
    Ok(())
}

/// Dispatch loop: requests are answered (only `shutdown` is known),
/// notifications drive diagnostics publishing.
fn main_loop(connection: &Connection) -> Result<(), DynError> {
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    return Ok(());
                }
                // No other requests are supported yet; answer instead of
                // staying silent so a client waiting on the response id
                // does not hang.
                let response = Response::new_err(
                    request.id,
                    ErrorCode::MethodNotFound as i32,
                    format!("method not supported: {}", request.method),
                );
                connection.sender.send(Message::Response(response))?;
            }
            Message::Notification(notification) => {
                // `handle_shutdown` consumes the `exit` that follows a
                // `shutdown` request, so an `exit` reaching this loop
                // skipped the handshake — the spec requires that to end
                // the process with a non-zero code.
                if notification.method == Exit::METHOD {
                    return Err("exit notification received before shutdown request".into());
                }
                handle_notification(connection, notification)?;
            }
            Message::Response(_) => {
                // The server sends no requests, so no responses are
                // expected; ignore rather than fail on a confused client.
            }
        }
    }
    Ok(())
}

/// Deserialise a notification's params, logging and discarding the
/// notification when the payload does not match the method's schema.
/// Notifications are fire-and-forget — one malformed message from a buggy
/// client must not take down the server (and with it every open document's
/// diagnostics).
fn parse_params<T: serde::de::DeserializeOwned>(
    method: &str,
    params: serde_json::Value,
) -> Option<T> {
    match serde_json::from_value(params) {
        Ok(parsed) => Some(parsed),
        Err(err) => {
            eprintln!("cairn-lsp: ignoring malformed `{method}` notification: {err}");
            None
        }
    }
}

/// React to one client notification by republishing diagnostics for the
/// affected document. Unknown notifications are ignored per the LSP spec.
///
/// No document store is kept: both `didOpen` and `didChange` (full sync)
/// carry the complete text, and `didClose` only needs the URI — so the
/// notification itself is always the freshest source of truth. A store
/// arrives with the completion feature, which has to read documents outside
/// a change notification.
fn handle_notification(
    connection: &Connection,
    notification: Notification,
) -> Result<(), DynError> {
    match notification.method.as_str() {
        DidOpenTextDocument::METHOD => {
            let Some(params) = parse_params::<lsp_types::DidOpenTextDocumentParams>(
                DidOpenTextDocument::METHOD,
                notification.params,
            ) else {
                return Ok(());
            };
            let uri = params.text_document.uri;
            let diagnostics = compute_diagnostics(&uri, &params.text_document.text);
            publish(
                connection,
                uri,
                Some(params.text_document.version),
                diagnostics,
            )?;
        }
        DidChangeTextDocument::METHOD => {
            let Some(mut params) = parse_params::<lsp_types::DidChangeTextDocumentParams>(
                DidChangeTextDocument::METHOD,
                notification.params,
            ) else {
                return Ok(());
            };
            // Full sync: the last change event carries the complete new
            // text. A client honouring the advertised FULL kind sends
            // exactly one event; taking the last is correct either way.
            let Some(change) = params.content_changes.pop() else {
                eprintln!(
                    "cairn-lsp: ignoring `{}` notification with empty contentChanges",
                    DidChangeTextDocument::METHOD,
                );
                return Ok(());
            };
            let uri = params.text_document.uri;
            let diagnostics = compute_diagnostics(&uri, &change.text);
            publish(
                connection,
                uri,
                Some(params.text_document.version),
                diagnostics,
            )?;
        }
        DidCloseTextDocument::METHOD => {
            let Some(params) = parse_params::<lsp_types::DidCloseTextDocumentParams>(
                DidCloseTextDocument::METHOD,
                notification.params,
            ) else {
                return Ok(());
            };
            // Publish an empty set so the editor clears stale squiggles
            // for the closed document.
            publish(connection, params.text_document.uri, None, Vec::new())?;
        }
        _ => {}
    }
    Ok(())
}

/// Send a `textDocument/publishDiagnostics` notification for `uri`.
fn publish(
    connection: &Connection,
    uri: lsp_types::Uri,
    version: Option<i32>,
    diagnostics: Vec<lsp_types::Diagnostic>,
) -> Result<(), DynError> {
    let params = lsp_types::PublishDiagnosticsParams {
        uri,
        diagnostics,
        version,
    };
    let notification = Notification::new(
        PublishDiagnostics::METHOD.to_owned(),
        serde_json::to_value(params)?,
    );
    connection
        .sender
        .send(Message::Notification(notification))?;
    Ok(())
}
