//! The stdio JSON-RPC loop: diagnostics pushed on each change, completion from the stored text.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::thread;

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Exit, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{Completion, Request as _, Shutdown};

use crate::completion::completions;
use crate::diagnostics::compute_diagnostics;
use crate::store::DocumentStore;

/// A transport or protocol failure, which ends the process.
type DynError = Box<dyn std::error::Error + Send + Sync>;

/// The capabilities the `initialize` response advertises.
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

/// Runs the language server over stdio until the client sends `exit`.
///
/// # Errors
///
/// A transport or protocol failure, or an `exit` that no `shutdown` preceded.
pub fn run() -> Result<(), DynError> {
    let (connection, io_threads) = Connection::stdio();
    let capabilities = serde_json::to_value(server_capabilities())?;
    connection.initialize(capabilities)?;
    let teardown = main_loop(&connection)?;
    // The writer thread ends only once `connection` drops.
    drop(connection);
    match (io_threads.join(), teardown) {
        (Ok(()), Teardown::AfterShutdown) => Ok(()),
        (Ok(()), Teardown::WithoutShutdown) => {
            // A killed editor: nobody is left to tell, so not an error.
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

/// How [`main_loop`] ended without an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Teardown {
    AfterShutdown,
    /// The input closed, or the reader failed on a frame, before any `shutdown`.
    WithoutShutdown,
}

/// What the diagnostics thread publishes next for one document.
enum Publication {
    /// The findings on one revision of its text.
    Findings { version: i32, text: String },
    /// No findings, for a closed document.
    Cleared,
}

/// A document and what to publish for it.
type Queued = (lsp_types::Uri, Publication);

/// Runs [`dispatch`] until it ends, while a second thread computes and publishes diagnostics, so
/// a request is answered while a long document is still being diagnosed.
fn main_loop(connection: &Connection) -> Result<Teardown, DynError> {
    let (queue, queued) = mpsc::channel();
    thread::scope(|scope| {
        let diagnostics_thread = thread::Builder::new()
            .name("cairn-lsp-diagnostics".to_owned())
            .spawn_scoped(scope, move || publish_queued(connection, &queued))?;
        let dispatched = dispatch(connection, &queue);
        drop(queue);
        let published = diagnostics_thread
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        // The diagnostics thread's failure is why a dispatch could not queue.
        published.and(dispatched)
    })
}

/// Publishes what `queued` holds until the queue closes, skipping every revision of a document
/// that a newer one queued after it replaces.
fn publish_queued(
    connection: &Connection,
    queued: &mpsc::Receiver<Queued>,
) -> Result<(), DynError> {
    let mut pending: VecDeque<Queued> = VecDeque::new();
    loop {
        for (uri, publication) in queued.try_iter() {
            match pending.iter_mut().find(|(seen, _)| *seen == uri) {
                Some((_, replaced)) => *replaced = publication,
                None => pending.push_back((uri, publication)),
            }
        }
        let Some((uri, publication)) = pending.pop_front().or_else(|| queued.recv().ok()) else {
            return Ok(());
        };
        match publication {
            Publication::Findings { version, text } => {
                let diagnostics = compute_diagnostics(&uri, &text);
                publish(connection, uri, Some(version), diagnostics)?;
            }
            // An empty set clears the closed document's squiggles.
            Publication::Cleared => publish(connection, uri, None, Vec::new())?,
        }
    }
}

/// Dispatches messages until `exit` or closed input. Not `Connection::handle_shutdown`: it fails
/// on any message between `shutdown` and `exit`, and editors send `didClose` there.
fn dispatch(connection: &Connection, queue: &mpsc::Sender<Queued>) -> Result<Teardown, DynError> {
    let mut store = DocumentStore::new();
    let mut shutdown_requested = false;
    for message in &connection.receiver {
        match message {
            // Ahead of the `shutdown` arm, so a second `shutdown` is refused.
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
                return if shutdown_requested {
                    Ok(Teardown::AfterShutdown)
                } else {
                    Err("exit notification received before shutdown request".into())
                };
            }
            Message::Notification(_) if shutdown_requested => {}
            Message::Notification(notification) => {
                handle_notification(queue, &mut store, notification)?;
            }
            Message::Response(_) => {
                // The server sends no requests.
            }
        }
    }
    Ok(if shutdown_requested {
        Teardown::AfterShutdown
    } else {
        Teardown::WithoutShutdown
    })
}

/// Answers one request; a request left unanswered blocks the client on its id forever.
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
                    None => Response::new_err(
                        request.id,
                        ErrorCode::InvalidParams as i32,
                        format!("document not open: {}", uri.as_str()),
                    ),
                    Some(source) => match completions(source, position) {
                        Some(items) => Response::new_ok(request.id, items),
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

/// A notification's params, or `None`, logged, when they do not match the method's schema.
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

/// Mirrors one notification's text into the store and queues that document's diagnostics.
fn handle_notification(
    queue: &mpsc::Sender<Queued>,
    store: &mut DocumentStore,
    notification: Notification,
) -> Result<(), DynError> {
    let queued = match notification.method.as_str() {
        DidOpenTextDocument::METHOD => {
            let Some(params) = parse_params::<lsp_types::DidOpenTextDocumentParams>(
                DidOpenTextDocument::METHOD,
                notification.params,
            ) else {
                return Ok(());
            };
            let document = params.text_document;
            store.open(document.uri.clone(), document.text.clone());
            let findings = Publication::Findings {
                version: document.version,
                text: document.text,
            };
            (document.uri, findings)
        }
        DidChangeTextDocument::METHOD => {
            let Some(params) = parse_params::<lsp_types::DidChangeTextDocumentParams>(
                DidChangeTextDocument::METHOD,
                notification.params,
            ) else {
                return Ok(());
            };
            if params.content_changes.is_empty() {
                eprintln!(
                    "cairn-lsp: ignoring `{}` notification with empty contentChanges",
                    DidChangeTextDocument::METHOD,
                );
                return Ok(());
            }
            let uri = params.text_document.uri;
            // A refused revision publishes nothing: the editor may have no buffer to clear it from.
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
            let findings = Publication::Findings {
                version: params.text_document.version,
                text: source.to_owned(),
            };
            (uri, findings)
        }
        DidCloseTextDocument::METHOD => {
            let Some(params) = parse_params::<lsp_types::DidCloseTextDocumentParams>(
                DidCloseTextDocument::METHOD,
                notification.params,
            ) else {
                return Ok(());
            };
            store.close(&params.text_document.uri);
            (params.text_document.uri, Publication::Cleared)
        }
        _ => return Ok(()),
    };
    queue
        .send(queued)
        .map_err(|_| "the diagnostics thread stopped")?;
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

#[cfg(test)]
mod tests;
