//! The stdio JSON-RPC loop: diagnostics pushed on each change or answered when the client pulls
//! them, and completion and hover from the stored text.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::mpsc;
use std::thread;

use lsp_server::{
    Connection, ErrorCode, Message, Notification, Request, RequestId, Response, ResponseError,
};
use lsp_types::notification::{
    Cancel, DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Exit,
    Notification as _, PublishDiagnostics,
};
use lsp_types::request::{
    Completion, DocumentDiagnosticRequest, HoverRequest, Request as _, Shutdown,
};

use crate::completion::completions;
use crate::diagnostics::compute_diagnostics;
use crate::hover::hover;
use crate::store::{DocumentStore, Revision};

/// A transport or protocol failure, which ends the process.
type DynError = Box<dyn std::error::Error + Send + Sync>;

/// What computes the findings on one document's text, as [`compute_diagnostics`] does.
type Diagnose = fn(&lsp_types::Uri, &str) -> Vec<lsp_types::Diagnostic>;

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
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        diagnostic_provider: Some(lsp_types::DiagnosticServerCapabilities::Options(
            lsp_types::DiagnosticOptions {
                identifier: Some("cairn".to_owned()),
                // A document's findings depend on its own text alone.
                inter_file_dependencies: false,
                workspace_diagnostics: false,
                ..lsp_types::DiagnosticOptions::default()
            },
        )),
        ..lsp_types::ServerCapabilities::default()
    }
}

/// How the client takes diagnostics, settled by what its `initialize` request says it supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiagnosticsMode {
    /// Published on every change, for a client that does not pull.
    Push,
    /// Answered when the client asks with `textDocument/diagnostic`, and never published, which
    /// would show the client each finding twice.
    Pull,
}

impl DiagnosticsMode {
    /// `Pull` when `initialize` declares the `textDocument.diagnostic` capability object, and
    /// `Push` otherwise.
    fn negotiate(params: &serde_json::Value) -> Self {
        let pulls = params
            .pointer("/capabilities/textDocument/diagnostic")
            .is_some_and(serde_json::Value::is_object);
        if pulls { Self::Pull } else { Self::Push }
    }
}

/// Runs the language server over stdio until the client sends `exit`.
///
/// # Errors
///
/// A transport or protocol failure, an `exit` that no `shutdown` preceded, or a panic on the
/// diagnostics thread.
pub fn run() -> Result<(), DynError> {
    let (connection, io_threads) = Connection::stdio();
    let capabilities = serde_json::to_value(server_capabilities())?;
    let params = connection.initialize(capabilities)?;
    let mode = DiagnosticsMode::negotiate(&params);
    let teardown = main_loop(&connection, mode, compute_diagnostics)?;
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
#[derive(Debug, PartialEq, Eq)]
enum Publication {
    /// The findings on one text, published with the `version` the client gave that text.
    Findings { version: i32, text: String },
    /// No findings, for a closed document.
    Cleared,
}

/// A document and what to publish for it.
type Queued = (lsp_types::Uri, Publication);

/// A `textDocument/diagnostic` request still to answer, with the text of the revision it names.
#[derive(Debug, PartialEq, Eq)]
struct Report {
    id: RequestId,
    uri: lsp_types::Uri,
    text: String,
    revision: Revision,
}

/// What the diagnostics thread does next.
#[derive(Debug, PartialEq, Eq)]
enum Job {
    /// Publish for one document, in place of what is still queued for it.
    Publish(Queued),
    /// Answer one `textDocument/diagnostic` request.
    Report(Report),
    /// Answer the report this `$/cancelRequest` names as cancelled, if it is still queued.
    Cancel(RequestId),
}

/// Runs [`dispatch`] while a second thread computes diagnostics, so only a pull waits on them,
/// and ends with an error as soon as that thread stops before `dispatch` does.
fn main_loop(
    connection: &Connection,
    mode: DiagnosticsMode,
    diagnose: Diagnose,
) -> Result<Teardown, DynError> {
    let (queue, queued) = mpsc::channel();
    let (alive, diagnostics_ended) = crossbeam_channel::bounded::<Infallible>(0);
    thread::scope(|scope| {
        let diagnostics_thread = thread::Builder::new()
            .name("cairn-lsp-diagnostics".to_owned())
            .spawn_scoped(scope, move || {
                let served = serve_queued(connection, &queued, diagnose);
                // A panic drops `alive` too, as it unwinds, so `dispatch` wakes however this ends.
                drop(alive);
                served
            })?;
        let dispatched = dispatch(connection, &queue, &diagnostics_ended, mode);
        drop(queue);
        let published = diagnostics_thread.join().unwrap_or_else(|panic| {
            let message = panic_message(&*panic);
            Err(format!("the diagnostics thread panicked: {message}").into())
        });
        // The diagnostics thread's failure is why a dispatch could not queue.
        published.and(dispatched)
    })
}

/// What a panic said, when it said it with a string.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message
    } else {
        "a payload that is not a string"
    }
}

/// Does what `queued` holds until the queue closes: every report first, in the order they came,
/// then the publications, skipping every revision of a document that a newer one queued after it
/// replaces. A report cancelled while still queued is answered as cancelled and never computed.
fn serve_queued(
    connection: &Connection,
    queued: &mpsc::Receiver<Job>,
    diagnose: Diagnose,
) -> Result<(), DynError> {
    let mut pending: VecDeque<Queued> = VecDeque::new();
    let mut reports: VecDeque<Report> = VecDeque::new();
    loop {
        for job in queued.try_iter() {
            match job {
                Job::Report(report) => reports.push_back(report),
                Job::Publish((uri, publication)) => {
                    match pending.iter_mut().find(|(seen, _)| *seen == uri) {
                        Some((_, replaced)) => *replaced = publication,
                        None => pending.push_back((uri, publication)),
                    }
                }
                Job::Cancel(id) => {
                    let cancelled = reports
                        .iter()
                        .position(|report| report.id == id)
                        .and_then(|at| reports.remove(at));
                    if let Some(cancelled) = cancelled {
                        respond_cancelled(connection, cancelled.id)?;
                    }
                }
            }
        }
        // A client waits on a report's id, and on no publication.
        let next = reports
            .pop_front()
            .map(Job::Report)
            .or_else(|| pending.pop_front().map(Job::Publish))
            .or_else(|| queued.recv().ok());
        let Some(job) = next else {
            return Ok(());
        };
        match job {
            Job::Report(report) => respond_with_report(connection, report, diagnose)?,
            Job::Publish((uri, Publication::Findings { version, text })) => {
                let diagnostics = diagnose(&uri, &text);
                publish(connection, uri, Some(version), diagnostics)?;
            }
            // An empty set clears the closed document's squiggles.
            Job::Publish((uri, Publication::Cleared)) => {
                publish(connection, uri, None, Vec::new())?;
            }
            // Received while nothing was queued, so no report is left to cancel.
            Job::Cancel(_) => {}
        }
    }
}

/// Dispatches messages until `exit`, closed input, or the end of the diagnostics thread, which
/// drops the sender of `diagnostics_ended`. Not `Connection::handle_shutdown`: it fails on any
/// message between `shutdown` and `exit`, and editors send `didClose` there.
fn dispatch(
    connection: &Connection,
    queue: &mpsc::Sender<Job>,
    diagnostics_ended: &crossbeam_channel::Receiver<Infallible>,
    mode: DiagnosticsMode,
) -> Result<Teardown, DynError> {
    let mut store = DocumentStore::new();
    let mut shutdown_requested = false;
    loop {
        let message = crossbeam_channel::select! {
            recv(connection.receiver) -> message => message,
            recv(diagnostics_ended) -> _ => return Err("the diagnostics thread stopped".into()),
        };
        let Ok(message) = message else {
            break;
        };
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
            Message::Request(request) if request.method == DocumentDiagnosticRequest::METHOD => {
                handle_pull(connection, queue, &store, request)?;
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
                handle_notification(queue, &mut store, notification, mode)?;
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
                        Some(list) => Response::new_ok(request.id, list),
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
    } else if request.method == HoverRequest::METHOD {
        answer_hover(store, request)
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

/// The answer to a `textDocument/hover` request: `null` where nothing has a description.
fn answer_hover(store: &DocumentStore, request: Request) -> Response {
    let params = match serde_json::from_value::<lsp_types::HoverParams>(request.params) {
        Ok(params) => params.text_document_position_params,
        Err(err) => {
            return Response::new_err(
                request.id,
                ErrorCode::InvalidParams as i32,
                format!("malformed `{}` params: {err}", HoverRequest::METHOD),
            );
        }
    };
    let uri = params.text_document.uri;
    match store.get(&uri) {
        Some(source) => Response::new_ok(request.id, hover(source, params.position)),
        None => Response::new_err(
            request.id,
            ErrorCode::InvalidParams as i32,
            format!("document not open: {}", uri.as_str()),
        ),
    }
}

/// Answers a `textDocument/diagnostic` request: at once when it is refused or the client holds
/// the stored revision's report already, and otherwise from the diagnostics thread.
fn handle_pull(
    connection: &Connection,
    queue: &mpsc::Sender<Job>,
    store: &DocumentStore,
    request: Request,
) -> Result<(), DynError> {
    let params = match serde_json::from_value::<lsp_types::DocumentDiagnosticParams>(request.params)
    {
        Ok(params) => params,
        Err(err) => {
            let method = DocumentDiagnosticRequest::METHOD;
            let message = format!("malformed `{method}` params: {err}");
            let response = Response::new_err(request.id, ErrorCode::InvalidParams as i32, message);
            connection.sender.send(Message::Response(response))?;
            return Ok(());
        }
    };
    let uri = params.text_document.uri;
    let Some((text, revision)) = store.get_with_revision(&uri) else {
        // A retry before the `didOpen` that brings the text is refused too, and the client pulls
        // after that `didOpen` anyway.
        let data = lsp_types::DiagnosticServerCancellationData {
            retrigger_request: false,
        };
        let error = ResponseError {
            code: ErrorCode::ServerCancelled as i32,
            message: format!("document not open: {}", uri.as_str()),
            data: Some(serde_json::to_value(data)?),
        };
        let response = Response {
            id: request.id,
            response_result: Err(error),
        };
        connection.sender.send(Message::Response(response))?;
        return Ok(());
    };
    // Findings are a function of the text alone, so a revision names its report.
    let result_id = revision.to_string();
    if params.previous_result_id.as_deref() == Some(result_id.as_str()) {
        let unchanged = lsp_types::DocumentDiagnosticReport::Unchanged(
            lsp_types::RelatedUnchangedDocumentDiagnosticReport {
                related_documents: None,
                unchanged_document_diagnostic_report:
                    lsp_types::UnchangedDocumentDiagnosticReport { result_id },
            },
        );
        let result = lsp_types::DocumentDiagnosticReportResult::Report(unchanged);
        let response = Response::new_ok(request.id, result);
        connection.sender.send(Message::Response(response))?;
        return Ok(());
    }
    let report = Report {
        id: request.id,
        uri,
        text: text.to_owned(),
        revision,
    };
    queue
        .send(Job::Report(report))
        .map_err(|_| "the diagnostics thread stopped")?;
    Ok(())
}

/// Sends `report`'s findings as the full report answering its request.
fn respond_with_report(
    connection: &Connection,
    report: Report,
    diagnose: Diagnose,
) -> Result<(), DynError> {
    let Report {
        id,
        uri,
        text,
        revision,
    } = report;
    let full =
        lsp_types::DocumentDiagnosticReport::Full(lsp_types::RelatedFullDocumentDiagnosticReport {
            related_documents: None,
            full_document_diagnostic_report: lsp_types::FullDocumentDiagnosticReport {
                result_id: Some(revision.to_string()),
                items: diagnose(&uri, &text),
            },
        });
    let result = lsp_types::DocumentDiagnosticReportResult::Report(full);
    connection
        .sender
        .send(Message::Response(Response::new_ok(id, result)))?;
    Ok(())
}

/// Answers the pull `id` names as cancelled before its report was computed.
fn respond_cancelled(connection: &Connection, id: RequestId) -> Result<(), DynError> {
    let response = Response::new_err(
        id,
        ErrorCode::RequestCanceled as i32,
        format!(
            "`{}` cancelled by the client before its report was computed",
            DocumentDiagnosticRequest::METHOD,
        ),
    );
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

/// Mirrors one notification's text into the store and, for a client diagnostics are pushed to,
/// queues that document's publication; hands a `$/cancelRequest` to the diagnostics thread.
fn handle_notification(
    queue: &mpsc::Sender<Job>,
    store: &mut DocumentStore,
    notification: Notification,
    mode: DiagnosticsMode,
) -> Result<(), DynError> {
    let pushed = mode == DiagnosticsMode::Push;
    let job = match notification.method.as_str() {
        DidOpenTextDocument::METHOD => {
            let Some(params) = parse_params::<lsp_types::DidOpenTextDocumentParams>(
                DidOpenTextDocument::METHOD,
                notification.params,
            ) else {
                return Ok(());
            };
            let lsp_types::TextDocumentItem {
                uri, version, text, ..
            } = params.text_document;
            let job = pushed.then(|| {
                let findings = Publication::Findings {
                    version,
                    text: text.clone(),
                };
                Job::Publish((uri.clone(), findings))
            });
            store.open(uri, text);
            job
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
            pushed.then(|| {
                let findings = Publication::Findings {
                    version: params.text_document.version,
                    text: source.to_owned(),
                };
                Job::Publish((uri, findings))
            })
        }
        DidCloseTextDocument::METHOD => {
            let Some(params) = parse_params::<lsp_types::DidCloseTextDocumentParams>(
                DidCloseTextDocument::METHOD,
                notification.params,
            ) else {
                return Ok(());
            };
            let uri = params.text_document.uri;
            store.close(&uri);
            pushed.then(|| Job::Publish((uri, Publication::Cleared)))
        }
        Cancel::METHOD => {
            let Some(params) =
                parse_params::<lsp_types::CancelParams>(Cancel::METHOD, notification.params)
            else {
                return Ok(());
            };
            let id = match params.id {
                lsp_types::NumberOrString::Number(id) => RequestId::from(id),
                lsp_types::NumberOrString::String(id) => RequestId::from(id),
            };
            Some(Job::Cancel(id))
        }
        _ => return Ok(()),
    };
    if let Some(job) = job {
        queue
            .send(job)
            .map_err(|_| "the diagnostics thread stopped")?;
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

#[cfg(test)]
mod tests;
