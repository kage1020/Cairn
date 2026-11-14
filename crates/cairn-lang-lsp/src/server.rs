//! The stdio JSON-RPC loop: diagnostics pushed on each change or answered when the client pulls
//! them, against what the client's settings pin; completion, hover and quick-fixes from the
//! stored text.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::thread;

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeConfiguration, DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Exit,
    Notification as _, PublishDiagnostics, ShowMessage,
};
use lsp_types::request::{
    CodeActionRequest, Completion, DocumentDiagnosticRequest, HoverRequest, Request as _, Shutdown,
    WorkspaceDiagnosticRefresh,
};

use crate::code_action::{SET_TARGET, code_actions};
use crate::completion::completions;
use crate::diagnostics::compute_diagnostics;
use crate::hover::hover;
use crate::pin::{Pin, PinRefused};
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
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        code_action_provider: Some(lsp_types::CodeActionProviderCapability::Options(
            lsp_types::CodeActionOptions {
                code_action_kinds: Some(vec![lsp_types::CodeActionKind::QUICKFIX]),
                ..lsp_types::CodeActionOptions::default()
            },
        )),
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

/// What the client's `initialize` request says it supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Client {
    mode: DiagnosticsMode,
    /// Whether it re-pulls on `workspace/diagnostic/refresh`, which the server sends when the pin
    /// changes.
    refreshes: bool,
    /// Whether it runs [`SET_TARGET`], which a quick-fix for `E_VERSION_CAP` asks it to.
    runs_set_target: bool,
}

impl Client {
    /// The client whose `initialize` params are `params`.
    fn negotiate(params: &serde_json::Value) -> Self {
        let pulls = params
            .pointer("/capabilities/textDocument/diagnostic")
            .is_some_and(|declared| !declared.is_null());
        let refreshes = params
            .pointer("/capabilities/workspace/diagnostics/refreshSupport")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        // Where a client lists the client-side commands it runs, as no standard capability does.
        let runs_set_target = params
            .pointer("/capabilities/experimental/commands/commands")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|commands| commands.iter().any(|command| command == SET_TARGET));
        Self {
            mode: if pulls {
                DiagnosticsMode::Pull
            } else {
                DiagnosticsMode::Push
            },
            refreshes,
            runs_set_target,
        }
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
    let params = connection.initialize(capabilities)?;
    let teardown = main_loop(&connection, Client::negotiate(&params))?;
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
    /// The findings on one revision of its text against one pin; `version` is `None` when the
    /// pin changed and the text did not.
    Findings {
        version: Option<i32>,
        text: String,
        pin: Pin,
    },
    /// No findings, for a closed document.
    Cleared,
}

/// A document and what to publish for it.
type Queued = (lsp_types::Uri, Publication);

/// A pulled report the client does not hold yet: the findings on one stored revision.
struct Report {
    id: RequestId,
    uri: lsp_types::Uri,
    text: String,
    pin: Pin,
    result_id: String,
}

/// What the diagnostics thread does next.
enum Job {
    /// Publish for one document, in place of what is still queued for it.
    Publish(Queued),
    /// Answer one `textDocument/diagnostic` request.
    Report(Report),
}

/// Runs [`dispatch`] until it ends, while a second thread computes diagnostics, so a request is
/// answered while a long document is still being diagnosed.
fn main_loop(connection: &Connection, client: Client) -> Result<Teardown, DynError> {
    let (queue, queued) = mpsc::channel();
    thread::scope(|scope| {
        let diagnostics_thread = thread::Builder::new()
            .name("cairn-lsp-diagnostics".to_owned())
            .spawn_scoped(scope, move || serve_queued(connection, &queued))?;
        let dispatched = dispatch(connection, &queue, client);
        drop(queue);
        let published = diagnostics_thread
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        // The diagnostics thread's failure is why a dispatch could not queue.
        published.and(dispatched)
    })
}

/// Does what `queued` holds until the queue closes: every report first, then the publications,
/// skipping every revision of a document that a newer one queued after it replaces.
fn serve_queued(connection: &Connection, queued: &mpsc::Receiver<Job>) -> Result<(), DynError> {
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
            Job::Report(report) => respond_with_report(connection, report)?,
            Job::Publish((uri, Publication::Findings { version, text, pin })) => {
                let diagnostics = compute_diagnostics(&uri, &text, &pin);
                publish(connection, uri, version, diagnostics)?;
            }
            // An empty set clears the closed document's squiggles.
            Job::Publish((uri, Publication::Cleared)) => {
                publish(connection, uri, None, Vec::new())?;
            }
        }
    }
}

/// Dispatches messages until `exit` or closed input. Not `Connection::handle_shutdown`: it fails
/// on any message between `shutdown` and `exit`, and editors send `didClose` there.
fn dispatch(
    connection: &Connection,
    queue: &mpsc::Sender<Job>,
    client: Client,
) -> Result<Teardown, DynError> {
    let mut store = DocumentStore::new();
    let mut pin = Pin::default();
    let mut refreshes_sent: u64 = 0;
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
            Message::Request(request) if request.method == DocumentDiagnosticRequest::METHOD => {
                handle_pull(connection, queue, &store, &pin, request)?;
            }
            Message::Request(request) => {
                handle_request(connection, &store, request, client.runs_set_target)?;
            }
            Message::Notification(notification) if notification.method == Exit::METHOD => {
                return if shutdown_requested {
                    Ok(Teardown::AfterShutdown)
                } else {
                    Err("exit notification received before shutdown request".into())
                };
            }
            Message::Notification(_) if shutdown_requested => {}
            Message::Notification(notification)
                if notification.method == DidChangeConfiguration::METHOD =>
            {
                let Some(params) = parse_params::<lsp_types::DidChangeConfigurationParams>(
                    DidChangeConfiguration::METHOD,
                    notification.params,
                ) else {
                    continue;
                };
                if repin(connection, &mut pin, &params.settings)? {
                    store.renumber();
                    refresh(connection, queue, &store, &pin, client, &mut refreshes_sent)?;
                }
            }
            Message::Notification(notification) => {
                handle_notification(queue, &mut store, notification, client.mode, &pin)?;
            }
            Message::Response(_) => {
                // Only `workspace/diagnostic/refresh` is asked, and its answer carries nothing.
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
    runs_set_target: bool,
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
    } else if request.method == HoverRequest::METHOD {
        answer_hover(store, request)
    } else if request.method == CodeActionRequest::METHOD {
        answer_code_action(store, request, runs_set_target)
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

/// The answer to a `textDocument/codeAction` request: the quick-fixes for the diagnostics it
/// carries back, for a client that asks for quick-fixes.
fn answer_code_action(store: &DocumentStore, request: Request, runs_set_target: bool) -> Response {
    let params = match serde_json::from_value::<lsp_types::CodeActionParams>(request.params) {
        Ok(params) => params,
        Err(err) => {
            return Response::new_err(
                request.id,
                ErrorCode::InvalidParams as i32,
                format!("malformed `{}` params: {err}", CodeActionRequest::METHOD),
            );
        }
    };
    let uri = params.text_document.uri;
    let Some(source) = store.get(&uri) else {
        return Response::new_err(
            request.id,
            ErrorCode::InvalidParams as i32,
            format!("document not open: {}", uri.as_str()),
        );
    };
    let asks_for_quick_fixes = params.context.only.as_ref().is_none_or(|kinds| {
        kinds
            .iter()
            .any(|kind| kind.as_str().is_empty() || *kind == lsp_types::CodeActionKind::QUICKFIX)
    });
    let actions = if asks_for_quick_fixes {
        code_actions(&uri, source, &params.context.diagnostics, runs_set_target)
    } else {
        Vec::new()
    };
    Response::new_ok(request.id, actions)
}

/// Takes the pin `settings` ask for, showing the client a setting it refused; whether the pin
/// changed.
fn repin(
    connection: &Connection,
    pin: &mut Pin,
    settings: &serde_json::Value,
) -> Result<bool, DynError> {
    let asked = match Pin::from_settings(settings) {
        Ok(asked) => asked,
        Err(PinRefused { reason, pinned }) => {
            let params = lsp_types::ShowMessageParams {
                typ: lsp_types::MessageType::ERROR,
                message: format!("cairn-lsp: {reason}"),
            };
            let notification = Notification::new(
                ShowMessage::METHOD.to_owned(),
                serde_json::to_value(params)?,
            );
            connection
                .sender
                .send(Message::Notification(notification))?;
            pinned
        }
    };
    if asked == *pin {
        return Ok(false);
    }
    *pin = asked;
    Ok(true)
}

/// Has the client shown every open document's findings against a new pin: published again, or,
/// for a client that pulls, re-pulled once it is told to.
fn refresh(
    connection: &Connection,
    queue: &mpsc::Sender<Job>,
    store: &DocumentStore,
    pin: &Pin,
    client: Client,
    refreshes_sent: &mut u64,
) -> Result<(), DynError> {
    match client.mode {
        DiagnosticsMode::Push => {
            for (uri, text) in store.documents() {
                let findings = Publication::Findings {
                    version: None,
                    text: text.to_owned(),
                    pin: pin.clone(),
                };
                queue
                    .send(Job::Publish((uri.clone(), findings)))
                    .map_err(|_| "the diagnostics thread stopped")?;
            }
        }
        DiagnosticsMode::Pull if client.refreshes => {
            *refreshes_sent += 1;
            let request = Request::new(
                RequestId::from(format!("cairn-refresh-{refreshes_sent}")),
                WorkspaceDiagnosticRefresh::METHOD.to_owned(),
                serde_json::Value::Null,
            );
            connection.sender.send(Message::Request(request))?;
        }
        // Such a client pulls when it next would, and the new revisions make it a full report.
        DiagnosticsMode::Pull => {}
    }
    Ok(())
}

/// Answers a `textDocument/diagnostic` request: at once when the client holds the stored
/// revision's report already, and otherwise from the diagnostics thread.
fn handle_pull(
    connection: &Connection,
    queue: &mpsc::Sender<Job>,
    store: &DocumentStore,
    pin: &Pin,
    request: Request,
) -> Result<(), DynError> {
    let refusal = |id, message| {
        let response = Response::new_err(id, ErrorCode::InvalidParams as i32, message);
        connection.sender.send(Message::Response(response))
    };
    let params = match serde_json::from_value::<lsp_types::DocumentDiagnosticParams>(request.params)
    {
        Ok(params) => params,
        Err(err) => {
            let method = DocumentDiagnosticRequest::METHOD;
            refusal(request.id, format!("malformed `{method}` params: {err}"))?;
            return Ok(());
        }
    };
    let uri = params.text_document.uri;
    let (Some(text), Some(revision)) = (store.get(&uri), store.revision(&uri)) else {
        refusal(request.id, format!("document not open: {}", uri.as_str()))?;
        return Ok(());
    };
    // A new pin renumbers every document, so a revision names its report.
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
        pin: pin.clone(),
        result_id,
    };
    queue
        .send(Job::Report(report))
        .map_err(|_| "the diagnostics thread stopped")?;
    Ok(())
}

/// Sends `report`'s findings as the full report answering its request.
fn respond_with_report(connection: &Connection, report: Report) -> Result<(), DynError> {
    let Report {
        id,
        uri,
        text,
        pin,
        result_id,
    } = report;
    let full =
        lsp_types::DocumentDiagnosticReport::Full(lsp_types::RelatedFullDocumentDiagnosticReport {
            related_documents: None,
            full_document_diagnostic_report: lsp_types::FullDocumentDiagnosticReport {
                result_id: Some(result_id),
                items: compute_diagnostics(&uri, &text, &pin),
            },
        });
    let result = lsp_types::DocumentDiagnosticReportResult::Report(full);
    connection
        .sender
        .send(Message::Response(Response::new_ok(id, result)))?;
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
/// queues that document's publication.
fn handle_notification(
    queue: &mpsc::Sender<Job>,
    store: &mut DocumentStore,
    notification: Notification,
    mode: DiagnosticsMode,
    pin: &Pin,
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
                version: Some(document.version),
                text: document.text,
                pin: pin.clone(),
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
                version: Some(params.text_document.version),
                text: source.to_owned(),
                pin: pin.clone(),
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
    if mode == DiagnosticsMode::Push {
        queue
            .send(Job::Publish(queued))
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
