//! Stdio JSON-RPC server loop.
//!
//! Owns the transport half of the language server: capability negotiation,
//! pushing [`crate::diagnostics::compute_diagnostics`] results to the client
//! via `textDocument/publishDiagnostics`, and answering
//! `textDocument/completion` from [`crate::completion::completions`] against
//! the [`DocumentStore`]-held text. The loop is synchronous (one message at
//! a time) — the compiler pipeline is fast enough that full recomputation
//! per keystroke stays well under interactive latency for the file sizes
//! `.crn` sources reach.

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Exit, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{Completion, Request as _, Shutdown};

use crate::completion::completions;
use crate::diagnostics::compute_diagnostics;
use crate::store::DocumentStore;

/// Errors the transport layer can surface. Boxed because every failure mode
/// here (broken pipe, malformed JSON, protocol violation) is terminal for
/// the process — the binary prints it and exits non-zero.
type DynError = Box<dyn std::error::Error + Send + Sync>;

/// Capabilities advertised in the `initialize` response: full-content
/// document sync with open/close notifications, and completion triggered by
/// the characters that open a closed-vocabulary position (`@` a material
/// token, `=` a `mat_slot` value, `.` a segment inside an abstract token).
/// Everything else (hover, code actions) is intentionally absent until it
/// is implemented.
///
/// FULL is what is advertised, but a `didChange` event that carries a
/// `range` anyway is still applied as an edit to that range
/// ([`DocumentStore::apply`]), not stored as the whole text: a client that
/// sends ranges would otherwise leave the server diagnosing and completing
/// text nobody has.
fn server_capabilities() -> lsp_types::ServerCapabilities {
    lsp_types::ServerCapabilities {
        text_document_sync: Some(lsp_types::TextDocumentSyncCapability::Options(
            lsp_types::TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(lsp_types::TextDocumentSyncKind::FULL),
                ..lsp_types::TextDocumentSyncOptions::default()
            },
        )),
        completion_provider: Some(lsp_types::CompletionOptions {
            trigger_characters: Some(vec!["@".to_owned(), "=".to_owned(), ".".to_owned()]),
            ..lsp_types::CompletionOptions::default()
        }),
        ..lsp_types::ServerCapabilities::default()
    }
}

/// Run the language server over stdio until the client sends `exit`.
///
/// Performs the `initialize` handshake, then processes messages until the
/// `shutdown`/`exit` sequence completes. Returns an error only on transport
/// or protocol failures — a clean client-driven exit returns `Ok(())`, and
/// so does input that closes before `shutdown`, after one line on stderr
/// saying so.
///
/// # Errors
///
/// Propagates I/O failures on stdin/stdout, JSON (de)serialisation failures
/// on the server's own outgoing messages, protocol violations detected by
/// `lsp-server` (e.g. messages before `initialize`), and an `exit`
/// notification arriving without a preceding `shutdown` request — the LSP
/// spec requires that sequence to end the process with a non-zero code.
///
/// When the dispatch loop ends without an error of its own, the I/O
/// threads are joined before anything is reported, and `lsp-server` joins
/// the reader first. An unreadable frame ends the reader with an error and
/// drops its sender, which the loop sees exactly as it sees the input
/// closing; only the join tells the two apart. So a frame that could not be
/// read comes back as that error, and is not also reported as the input
/// closing. The join does not say whether the reader or the writer failed,
/// so when the loop ended before `shutdown`, its error comes back with
/// "the session ended without `shutdown`" appended, which is true of
/// either.
pub fn run() -> Result<(), DynError> {
    let (connection, io_threads) = Connection::stdio();
    let capabilities = serde_json::to_value(server_capabilities())?;
    connection.initialize(capabilities)?;
    let teardown = main_loop(&connection)?;
    // The writer thread only terminates once the outgoing channel closes,
    // which happens when the `Connection` (and with it `sender`) drops —
    // joining before that would deadlock the shutdown.
    drop(connection);
    match (io_threads.join(), teardown) {
        (Ok(()), Teardown::AfterShutdown) => Ok(()),
        (Ok(()), Teardown::WithoutShutdown) => {
            // The reader returned `Ok`, so the input closed: the client
            // shut stdin without saying anything — an editor that was
            // killed rather than one that quit. There is nobody left to
            // answer, so this is not an error, but it is not the orderly
            // teardown either and the stream should say which one
            // happened.
            eprintln!(
                "cairn-lsp: client closed stdin without `{}`; the session ended abnormally",
                Shutdown::METHOD,
            );
            Ok(())
        }
        (Err(err), Teardown::AfterShutdown) => Err(err.into()),
        (Err(err), Teardown::WithoutShutdown) => {
            Err(format!("{err}; the session ended without `{}`", Shutdown::METHOD).into())
        }
    }
}

/// How [`main_loop`] ended, when it ended without an error: the one bit the
/// loop has is whether `shutdown` came first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Teardown {
    /// `shutdown` was requested, and the loop then ended on `exit` or on
    /// its receiver closing.
    AfterShutdown,
    /// The receiver closed before any `shutdown`: the input closed, or the
    /// reader failed on a frame. Only joining the reader tells which, so
    /// [`run`] reports it once it has.
    WithoutShutdown,
}

/// Dispatch loop: requests are answered (`shutdown` and `completion` are
/// known), notifications keep the document store in sync and drive
/// diagnostics publishing.
///
/// `shutdown` does not end the loop — it moves the server into a state the
/// spec describes precisely: further requests are refused with
/// `InvalidRequest`, further notifications are ignored, and `exit` ends
/// the session. `lsp_server::Connection::handle_shutdown` models
/// that window as "the very next message is `exit`", which no real client
/// guarantees: `$/cancelRequest` may arrive at any time, and an editor
/// closing its last buffer sends `didClose` on the way out. Every one of
/// those became a `ProtocolError` that ended `run()` with an error before
/// the `exit` behind it was ever read, so the process died with code 1 and
/// the editor reported the language server as crashed.
///
/// The loop ends without an error on `exit` after `shutdown` or when its
/// receiver closes; whether `shutdown` came first comes back as a
/// [`Teardown`].
fn main_loop(connection: &Connection) -> Result<Teardown, DynError> {
    let mut store = DocumentStore::new();
    let mut shutdown_requested = false;
    for message in &connection.receiver {
        match message {
            // Checked before the method, so a second `shutdown` is refused
            // like any other post-shutdown request rather than answered
            // twice.
            Message::Request(request) if shutdown_requested => {
                let response = Response::new_err(
                    request.id,
                    ErrorCode::InvalidRequest as i32,
                    format!(
                        "`{}` received after `{}`; the server is shutting down",
                        request.method,
                        Shutdown::METHOD,
                    ),
                );
                connection.sender.send(Message::Response(response))?;
            }
            Message::Request(request) if request.method == Shutdown::METHOD => {
                shutdown_requested = true;
                let response = Response::new_ok(request.id, serde_json::Value::Null);
                connection.sender.send(Message::Response(response))?;
            }
            Message::Request(request) => handle_request(connection, &store, request)?,
            Message::Notification(notification) if notification.method == Exit::METHOD => {
                // The one message that ends the loop, and only after
                // `shutdown`: the spec requires an `exit` without one to
                // terminate the process with a non-zero code.
                return if shutdown_requested {
                    Ok(Teardown::AfterShutdown)
                } else {
                    Err("exit notification received before shutdown request".into())
                };
            }
            // Dropped here rather than inside `handle_notification`: the
            // store write and the `publishDiagnostics` push both live down
            // that path, and a squiggle arriving after `shutdown` is an
            // observable violation of "notifications are ignored".
            Message::Notification(_) if shutdown_requested => {}
            Message::Notification(notification) => {
                handle_notification(connection, &mut store, notification)?;
            }
            Message::Response(_) => {
                // The server sends no requests, so no responses are
                // expected; ignore rather than fail on a confused client.
            }
        }
    }
    // The receiver closed. `run` reports a `Teardown::WithoutShutdown`.
    Ok(if shutdown_requested {
        Teardown::AfterShutdown
    } else {
        Teardown::WithoutShutdown
    })
}

/// Answer one client request. Every request gets a response — a silent
/// server leaves the client blocked on the response id forever — so unknown
/// methods are refused with `MethodNotFound` and a completion request whose
/// params or document are unusable is refused with `InvalidParams`.
fn handle_request(
    connection: &Connection,
    store: &DocumentStore,
    request: Request,
) -> Result<(), DynError> {
    let response = if request.method == Completion::METHOD {
        match serde_json::from_value::<lsp_types::CompletionParams>(request.params) {
            Err(err) => Response::new_err(
                request.id,
                ErrorCode::InvalidParams as i32,
                format!("malformed `{}` params: {err}", Completion::METHOD),
            ),
            Ok(params) => {
                let position = params.text_document_position.position;
                let uri = params.text_document_position.text_document.uri;
                match store.get(&uri) {
                    // Asking about a document the client never synced is a
                    // client-side protocol violation; refuse it loud instead
                    // of answering from nothing.
                    None => Response::new_err(
                        request.id,
                        ErrorCode::InvalidParams as i32,
                        format!("document not open: {}", uri.as_str()),
                    ),
                    Some(source) => match completions(source, position) {
                        Some(items) => Response::new_ok(request.id, items),
                        // A position far past the document is a client bug,
                        // not a revision race — refused, not clamped.
                        None => Response::new_err(
                            request.id,
                            ErrorCode::InvalidParams as i32,
                            format!(
                                "position {}:{} is past the end of {}",
                                position.line,
                                position.character,
                                uri.as_str(),
                            ),
                        ),
                    },
                }
            }
        }
    } else {
        Response::new_err(
            request.id,
            ErrorCode::MethodNotFound as i32,
            format!("method not supported: {}", request.method),
        )
    };
    connection.sender.send(Message::Response(response))?;
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

/// React to one client notification: mirror the document text into the
/// store (completion requests identify their document by URI alone, so the
/// server has to remember the last synced revision), then republish
/// diagnostics for the affected document. Unknown notifications are ignored
/// per the LSP spec.
fn handle_notification(
    connection: &Connection,
    store: &mut DocumentStore,
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
            store.open(uri.clone(), params.text_document.text);
            publish(
                connection,
                uri,
                Some(params.text_document.version),
                diagnostics,
            )?;
        }
        DidChangeTextDocument::METHOD => {
            let Some(params) = parse_params::<lsp_types::DidChangeTextDocumentParams>(
                DidChangeTextDocument::METHOD,
                notification.params,
            ) else {
                return Ok(());
            };
            // An empty `contentChanges` changes nothing: `apply` would hand
            // back the stored text as it is, and the publish below would
            // repeat the last diagnostics under a new version. The notification is dropped
            // instead, with a line on stderr so a client that sends one
            // shows up in the log.
            if params.content_changes.is_empty() {
                eprintln!(
                    "cairn-lsp: ignoring `{}` notification with empty contentChanges",
                    DidChangeTextDocument::METHOD,
                );
                return Ok(());
            }
            let uri = params.text_document.uri;
            // Ahead of the diagnostics run, not after it: a refused
            // revision is dropped with a line on stderr, the way a
            // malformed payload is, and the parse that would have been
            // thrown away with it never runs. Nothing is published for it
            // either. For a document that is not open, a publish would
            // leave a squiggle on a file the editor has no buffer for and
            // therefore no way to clear; for a range the text does not
            // have, the stored text stays the last one that could be
            // built, rather than becoming a guess.
            let source = match store.apply(&uri, params.content_changes) {
                Ok(source) => source,
                Err(refused) => {
                    eprintln!(
                        "cairn-lsp: ignoring `{}` for {}: {refused}",
                        DidChangeTextDocument::METHOD,
                        uri.as_str(),
                    );
                    return Ok(());
                }
            };
            let diagnostics = compute_diagnostics(&uri, source);
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
            store.close(&params.text_document.uri);
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
