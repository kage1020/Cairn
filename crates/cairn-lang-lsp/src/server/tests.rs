//! The dispatch loop driven in-process over an in-memory connection.

use std::str::FromStr;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::*;
use crate::test_fixtures::{CLEAN, DUPLICATE};

const TEST_URI: &str = "file:///test.crn";

/// How long a test waits for a message, so one that never arrives fails rather than hangs.
const READ_TIMEOUT: Duration = Duration::from_secs(20);

/// [`main_loop`] running on its own thread, with the client end of its connection.
struct Session {
    client: Connection,
    server: JoinHandle<Result<Teardown, DynError>>,
}

impl Session {
    fn start() -> Self {
        let (server, client) = Connection::memory();
        let server = thread::spawn(move || main_loop(&server));
        Self { client, server }
    }

    /// Send one message, written as the JSON a client puts on the wire.
    fn send(&self, message: &serde_json::Value) {
        let message: Message = serde_json::from_value(message.clone()).expect("a JSON-RPC message");
        self.client
            .sender
            .send(message)
            .expect("the loop is running");
    }

    /// Take the next message the loop sent, as JSON, or fail the test.
    fn read_message(&self) -> serde_json::Value {
        match self.client.receiver.recv_timeout(READ_TIMEOUT) {
            Ok(message) => serde_json::to_value(message).expect("serialise message"),
            Err(err) => panic!("the loop sent nothing within {READ_TIMEOUT:?}: {err}"),
        }
    }

    /// Read messages until one with the given method arrives.
    fn read_until_method(&self, method: &str) -> serde_json::Value {
        loop {
            let message = self.read_message();
            if message.get("method").and_then(serde_json::Value::as_str) == Some(method) {
                return message;
            }
        }
    }

    /// Read messages until the response carrying `id` arrives.
    fn read_response(&self, id: i64) -> serde_json::Value {
        loop {
            let message = self.read_message();
            if message.get("id") == Some(&serde_json::json!(id)) {
                return message;
            }
        }
    }

    fn did_open(&self, text: &str, version: i32) {
        self.did_open_uri(TEST_URI, text, version);
    }

    fn did_open_uri(&self, uri: &str, text: &str, version: i32) {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "cairn",
                    "version": version,
                    "text": text,
                },
            },
        }));
    }

    fn did_change(&self, uri: &str, text: &str, version: i32) {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": text }],
            },
        }));
    }

    fn did_close(&self, uri: &str) {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": { "textDocument": { "uri": uri } },
        }));
    }

    /// Send a `textDocument/completion` request for a test that reads the next message itself.
    fn send_completion(&self, id: i64, uri: &str, line: u32, character: u32) {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            },
        }));
    }

    /// Send a `textDocument/completion` request and return its response.
    fn request_completion(
        &self,
        id: i64,
        uri: &str,
        line: u32,
        character: u32,
    ) -> serde_json::Value {
        self.send_completion(id, uri, line, character);
        self.read_response(id)
    }

    /// Send `shutdown` and assert its `null` result, leaving the loop running until `exit`.
    fn request_shutdown(&self) {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "shutdown",
        }));
        let response = self.read_response(99);
        assert_eq!(
            response.get("result"),
            Some(&serde_json::Value::Null),
            "shutdown should return null, got: {response}",
        );
    }

    /// Send `exit` and return what the loop ended with.
    fn exit(self) -> Result<Teardown, String> {
        self.send(&serde_json::json!({ "jsonrpc": "2.0", "method": "exit" }));
        self.join()
    }

    /// Close the client's end without a word, and return what the loop ended with.
    fn hang_up(self) -> Result<Teardown, String> {
        let Self { client, server } = self;
        drop(client);
        Self::join_handle(server)
    }

    fn join(self) -> Result<Teardown, String> {
        Self::join_handle(self.server)
    }

    fn join_handle(server: JoinHandle<Result<Teardown, DynError>>) -> Result<Teardown, String> {
        server
            .join()
            .expect("the loop thread panicked")
            .map_err(|err| err.to_string())
    }

    /// Run the `shutdown`/`exit` handshake and assert it ends the loop cleanly.
    fn shutdown(self) {
        self.request_shutdown();
        assert_eq!(self.exit(), Ok(Teardown::AfterShutdown));
    }
}

/// The diagnostics of a `publishDiagnostics` notification, asserting it targets `uri`.
fn diagnostics_for<'m>(message: &'m serde_json::Value, uri: &str) -> &'m Vec<serde_json::Value> {
    assert_eq!(
        message["params"]["uri"].as_str(),
        Some(uri),
        "diagnostics should target the expected document",
    );
    message["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array")
}

fn diagnostics_of(message: &serde_json::Value) -> &Vec<serde_json::Value> {
    diagnostics_for(message, TEST_URI)
}

/// Labels of a completion response's item array.
fn completion_labels(response: &serde_json::Value) -> Vec<&str> {
    response["result"]
        .as_array()
        .expect("completion result should be an item array")
        .iter()
        .map(|item| item["label"].as_str().expect("string label"))
        .collect()
}

#[test]
fn capabilities_advertise_full_sync_and_the_completion_triggers() {
    let capabilities = serde_json::to_value(server_capabilities()).expect("serialise");
    let sync = &capabilities["textDocumentSync"];
    assert_eq!(sync["openClose"], serde_json::json!(true));
    assert_eq!(sync["change"], serde_json::json!(1), "change kind FULL");
    assert_eq!(
        capabilities["completionProvider"]["triggerCharacters"],
        serde_json::json!(["@", "=", "."]),
    );
}

// ---------------------------------------------------------- notifications

#[test]
fn did_open_publishes_the_documents_diagnostics_with_its_version() {
    let session = Session::start();
    session.did_open(DUPLICATE, 1);
    let message = session.read_until_method("textDocument/publishDiagnostics");
    let uri = lsp_types::Uri::from_str(TEST_URI).expect("valid uri");
    let expected = serde_json::to_value(compute_diagnostics(&uri, DUPLICATE)).expect("serialise");
    assert_eq!(message["params"]["diagnostics"], expected);
    assert!(
        !diagnostics_of(&message).is_empty(),
        "the fixture is broken"
    );
    assert_eq!(message["params"]["version"], serde_json::json!(1));

    session.did_open(CLEAN, 2);
    let message = session.read_until_method("textDocument/publishDiagnostics");
    assert!(diagnostics_of(&message).is_empty());
    session.shutdown();
}

#[test]
fn did_change_republishes_and_echoes_the_version() {
    let session = Session::start();
    session.did_open(DUPLICATE, 1);
    let first = session.read_until_method("textDocument/publishDiagnostics");
    assert!(!diagnostics_of(&first).is_empty(), "broken file diagnosed");
    session.did_change(TEST_URI, CLEAN, 2);
    let second = session.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_of(&second).len(), 0);
    assert_eq!(second["params"]["version"], serde_json::json!(2));
    session.shutdown();
}

#[test]
fn did_close_clears_diagnostics() {
    let session = Session::start();
    session.did_open(DUPLICATE, 1);
    let first = session.read_until_method("textDocument/publishDiagnostics");
    assert!(!diagnostics_of(&first).is_empty(), "broken file diagnosed");
    session.did_close(TEST_URI);
    let second = session.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_of(&second).len(), 0);
    session.shutdown();
}

#[test]
fn documents_publish_under_their_own_uris() {
    let broken_uri = "file:///broken.crn";
    let clean_uri = "file:///clean.crn";
    let session = Session::start();
    session.did_open_uri(broken_uri, DUPLICATE, 1);
    let first = session.read_until_method("textDocument/publishDiagnostics");
    assert!(!diagnostics_for(&first, broken_uri).is_empty());
    session.did_open_uri(clean_uri, CLEAN, 1);
    let second = session.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_for(&second, clean_uri).len(), 0);
    session.did_change(broken_uri, CLEAN, 2);
    let third = session.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_for(&third, broken_uri).len(), 0);
    session.shutdown();
}

#[test]
fn malformed_notifications_do_not_end_the_loop() {
    let session = Session::start();
    session.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": { "textDocument": { "uri": TEST_URI } },
    }));
    session.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": TEST_URI, "version": 2 },
            "contentChanges": [],
        },
    }));
    session.did_open(CLEAN, 3);
    let message = session.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_of(&message).len(), 0);
    session.shutdown();
}

#[test]
fn an_unknown_notification_before_shutdown_is_ignored() {
    let session = Session::start();
    session.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "$/cancelRequest",
        "params": { "id": 1 },
    }));
    session.did_open(CLEAN, 1);
    let published = session.read_until_method("textDocument/publishDiagnostics");
    assert!(diagnostics_of(&published).is_empty());
    session.shutdown();
}

#[test]
fn a_deeply_nested_document_is_diagnosed_and_the_loop_keeps_serving() {
    // A stack overflow would abort the server, so the next document must still publish.
    let mut indented = String::from("struct a size=1x1\n");
    for level in 1..=500 {
        indented.push_str(&"  ".repeat(level));
        indented.push_str("level y=0\n");
    }
    let brackets = format!(
        "struct a size=1x1\n  window mat={}x{}\n",
        "[".repeat(400),
        "]".repeat(400),
    );

    for (shape, deep) in [("brackets", brackets), ("indent", indented)] {
        let session = Session::start();
        session.did_open(&deep, 1);
        let published = session.read_until_method("textDocument/publishDiagnostics");
        let diagnostics = diagnostics_of(&published);
        assert_eq!(
            diagnostics.len(),
            1,
            "{shape}: a parse failure publishes one diagnostic; got {diagnostics:?}",
        );
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .expect("message")
                .contains("nesting"),
            "{shape}: the message should name the nesting limit; got {:?}",
            diagnostics[0]["message"],
        );
        session.did_open(CLEAN, 2);
        let after = session.read_until_method("textDocument/publishDiagnostics");
        assert_eq!(
            diagnostics_of(&after).len(),
            0,
            "{shape}: the server must still serve the next document",
        );
        session.shutdown();
    }
}

// ---------------------------------------------------------------- requests

#[test]
fn an_unknown_request_gets_method_not_found() {
    // -32601 is MethodNotFound.
    let session = Session::start();
    session.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "textDocument/hover",
        "params": {},
    }));
    let response = session.read_response(42);
    assert_eq!(response["error"]["code"], serde_json::json!(-32601));
    session.shutdown();
}

#[test]
fn completion_at_mat_slot_returns_declared_slot_names() {
    let session = Session::start();
    let source = "theme a:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=";
    session.did_open(source, 1);
    let response = session.request_completion(7, TEST_URI, 3, 17);
    assert_eq!(completion_labels(&response), vec!["floor"]);
    session.shutdown();
}

#[test]
fn completion_reflects_did_change_revisions() {
    let session = Session::start();
    let source = "theme a:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=";
    session.did_open(source, 1);
    let revised = "theme a:\n  slot floor -> @oak_planks\n  slot wall -> @cobblestone\n\
                   struct s size=2x2\n  floor mat_slot=";
    session.did_change(TEST_URI, revised, 2);
    let response = session.request_completion(8, TEST_URI, 4, 17);
    assert_eq!(completion_labels(&response), vec!["floor", "wall"]);
    session.shutdown();
}

#[test]
fn completion_on_an_unopened_document_is_invalid_params() {
    // -32602 is InvalidParams.
    let session = Session::start();
    let response = session.request_completion(9, "file:///never-opened.crn", 0, 0);
    assert_eq!(response["error"]["code"], serde_json::json!(-32602));
    session.shutdown();
}

#[test]
fn a_position_one_line_past_the_document_is_refused() {
    let session = Session::start();
    session.did_open("st", 1);
    session.read_until_method("textDocument/publishDiagnostics");
    let response = session.request_completion(36, TEST_URI, 1, 0);
    assert_eq!(response["error"]["code"], serde_json::json!(-32602));
    session.shutdown();
}

#[test]
fn completion_answers_at_the_end_of_a_newline_terminated_document() {
    let session = Session::start();
    session.did_open("struct s size=2x2\n", 1);
    session.read_until_method("textDocument/publishDiagnostics");
    let response = session.request_completion(38, TEST_URI, 1, 0);
    let items = response["result"]
        .as_array()
        .unwrap_or_else(|| panic!("expected items at the empty final line, got: {response}"));
    assert!(!items.is_empty(), "the line exists, so it has candidates");
    session.shutdown();
}

#[test]
fn completion_answers_in_an_empty_document() {
    let session = Session::start();
    session.did_open("", 1);
    session.read_until_method("textDocument/publishDiagnostics");
    let response = session.request_completion(39, TEST_URI, 0, 0);
    let items = response["result"]
        .as_array()
        .unwrap_or_else(|| panic!("expected items in an empty document, got: {response}"));
    assert!(!items.is_empty());
    session.shutdown();
}

// ------------------------------------- the store mirrors the client's open set

#[test]
fn did_change_after_did_close_leaves_the_document_closed() {
    let session = Session::start();
    session.did_open(DUPLICATE, 1);
    let opened = session.read_until_method("textDocument/publishDiagnostics");
    assert!(!diagnostics_of(&opened).is_empty(), "the fixture is broken");
    session.did_close(TEST_URI);
    let closed = session.read_until_method("textDocument/publishDiagnostics");
    assert!(diagnostics_of(&closed).is_empty(), "close clears the marks");
    session.did_change(TEST_URI, DUPLICATE, 2);
    session.send_completion(34, TEST_URI, 0, 0);
    let next = session.read_message();
    assert_eq!(
        next.get("id"),
        Some(&serde_json::json!(34)),
        "nothing may be published for a closed document, got: {next}",
    );
    assert_eq!(
        next["error"]["code"],
        serde_json::json!(-32602),
        "the document is closed, so completion has nothing to answer from",
    );
    session.shutdown();
}

#[test]
fn did_change_for_a_never_opened_document_is_ignored() {
    const OTHER_URI: &str = "file:///never-opened.crn";
    let session = Session::start();
    session.did_change(OTHER_URI, CLEAN, 1);
    session.send_completion(35, OTHER_URI, 0, 0);
    let next = session.read_message();
    assert_eq!(
        next.get("id"),
        Some(&serde_json::json!(35)),
        "nothing may be published for a document that was never opened, got: {next}",
    );
    assert_eq!(next["error"]["code"], serde_json::json!(-32602));
    session.shutdown();
}

#[test]
fn a_change_whose_did_open_was_dropped_is_also_dropped() {
    let session = Session::start();
    session.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            // No `languageId`, so this payload does not deserialise.
            "textDocument": { "uri": TEST_URI, "version": 1, "text": DUPLICATE },
        },
    }));
    session.did_change(TEST_URI, DUPLICATE, 2);
    session.send_completion(37, TEST_URI, 0, 0);
    let next = session.read_message();
    assert_eq!(
        next["error"]["code"],
        serde_json::json!(-32602),
        "neither notification opened the document, got: {next}",
    );

    session.did_open(DUPLICATE, 3);
    let published = session.read_until_method("textDocument/publishDiagnostics");
    assert!(
        !diagnostics_of(&published).is_empty(),
        "reopening republishes the fixture's findings",
    );
    session.shutdown();
}

// ----------------------------------------------------------- ranged changes

/// A `didChange` for [`TEST_URI`] carrying `events` as its `contentChanges`.
fn change_events(version: i32, events: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": TEST_URI, "version": version },
            "contentChanges": events,
        },
    })
}

/// A `contentChanges` event inserting `text` at `line:character`.
fn insert_at(line: u32, character: u32, text: &str) -> serde_json::Value {
    serde_json::json!({
        "range": {
            "start": { "line": line, "character": character },
            "end": { "line": line, "character": character },
        },
        "text": text,
    })
}

#[test]
fn a_ranged_change_is_applied_to_the_stored_text() {
    let session = Session::start();
    session.did_open("@cairn 2026.06\nstruct s size=3x3\n  bogus!!\n", 1);
    let first = session.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_of(&first)[0]["code"], "E_PARSE");
    session.send(&change_events(
        2,
        &serde_json::json!([insert_at(0, 0, "# note\n")]),
    ));
    let second = session.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(second["params"]["version"], 2);
    let diagnostics = diagnostics_of(&second);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["code"], "E_PARSE");
    assert_eq!(
        diagnostics[0]["range"]["start"]["line"], 3,
        "the error moved down one line with the inserted comment",
    );
    session.shutdown();
}

#[test]
fn a_ranged_change_outside_the_document_is_dropped() {
    let session = Session::start();
    session.did_open("struct s size=2x2\n", 1);
    session.read_until_method("textDocument/publishDiagnostics");
    session.send(&change_events(
        2,
        &serde_json::json!([insert_at(5, 0, "bogus!!")]),
    ));
    session.send_completion(39, TEST_URI, 1, 0);
    let response = session.read_message();
    assert_eq!(
        response["id"], 39,
        "a refused revision publishes nothing, got: {response}"
    );
    assert!(
        response["result"]
            .as_array()
            .is_some_and(|items| !items.is_empty()),
        "the refusal leaves the URI open, with a line 1 to complete on, got: {response}",
    );
    session.shutdown();
}

#[test]
fn a_did_change_with_no_events_publishes_nothing() {
    // Unlike in `malformed_notifications_do_not_end_the_loop`, the document is open.
    let session = Session::start();
    session.did_open(CLEAN, 1);
    session.read_until_method("textDocument/publishDiagnostics");
    session.send(&change_events(2, &serde_json::json!([])));
    session.send_completion(40, TEST_URI, 0, 0);
    let next = session.read_message();
    assert_eq!(
        next["id"], 40,
        "a didChange with no events publishes nothing, got: {next}"
    );
    session.shutdown();
}

#[test]
fn a_refused_revision_keeps_none_of_its_events() {
    let session = Session::start();
    session.did_open("@cairn 2026.06\nstruct s size=3x3\n  bogus!!\n", 1);
    let first = session.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_of(&first)[0]["code"], "E_PARSE");
    session.send(&change_events(
        2,
        &serde_json::json!([insert_at(0, 0, "# note\n"), insert_at(9, 0, "bogus!!")]),
    ));
    session.send_completion(41, TEST_URI, 0, 0);
    let next = session.read_message();
    assert_eq!(
        next["id"], 41,
        "a refused revision publishes nothing, got: {next}"
    );
    session.send(&change_events(
        3,
        &serde_json::json!([{
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 6 },
            },
            "text": "struct",
        }]),
    ));
    let third = session.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(third["params"]["version"], 3);
    assert_eq!(
        diagnostics_of(&third),
        diagnostics_of(&first),
        "version 3 is version 1's text, so it has version 1's diagnostics",
    );
    session.shutdown();
}

// ------------------------------------------ the window between shutdown and exit

/// Messages an editor sends between `shutdown` and `exit`.
fn interlopers() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "$/cancelRequest",
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "$/cancelRequest",
                "params": { "id": 1 },
            }),
        ),
        (
            "didClose",
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didClose",
                "params": { "textDocument": { "uri": TEST_URI } },
            }),
        ),
        (
            "didChange",
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": { "uri": TEST_URI, "version": 2 },
                    "contentChanges": [{ "text": "struct s size=3x3\n" }],
                },
            }),
        ),
        (
            "completion request",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "textDocument/completion",
                "params": {
                    "textDocument": { "uri": TEST_URI },
                    "position": { "line": 0, "character": 0 },
                },
            }),
        ),
    ]
}

#[test]
fn a_message_between_shutdown_and_exit_still_ends_the_loop_cleanly() {
    for (label, interloper) in interlopers() {
        let session = Session::start();
        session.did_open(CLEAN, 1);
        session.read_until_method("textDocument/publishDiagnostics");
        session.request_shutdown();
        session.send(&interloper);
        if interloper.get("id").is_some() {
            session.read_response(7);
        }
        assert_eq!(
            session.exit(),
            Ok(Teardown::AfterShutdown),
            "interloper: {label}"
        );
    }
}

#[test]
fn a_request_after_shutdown_is_refused_as_invalid() {
    // -32600 is InvalidRequest, which the LSP spec asks for in this window.
    let session = Session::start();
    session.did_open(CLEAN, 1);
    session.read_until_method("textDocument/publishDiagnostics");
    session.request_shutdown();
    let response = session.request_completion(31, TEST_URI, 0, 0);
    assert_eq!(response["error"]["code"], serde_json::json!(-32600));
    assert_eq!(session.exit(), Ok(Teardown::AfterShutdown));
}

#[test]
fn a_second_shutdown_is_refused_rather_than_answered_again() {
    let session = Session::start();
    session.request_shutdown();
    session.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 32,
        "method": "shutdown",
    }));
    let response = session.read_response(32);
    assert_eq!(response["error"]["code"], serde_json::json!(-32600));
    assert_eq!(response.get("result"), None);
    assert_eq!(session.exit(), Ok(Teardown::AfterShutdown));
}

#[test]
fn a_notification_after_shutdown_publishes_nothing() {
    let session = Session::start();
    session.did_open(CLEAN, 1);
    session.read_until_method("textDocument/publishDiagnostics");
    session.request_shutdown();
    session.did_change(TEST_URI, "struct s size=2x2 size=3x3\n", 2);
    session.send_completion(33, TEST_URI, 0, 0);
    let next = session.read_message();
    assert_eq!(
        next.get("id"),
        Some(&serde_json::json!(33)),
        "nothing may reach the client between the ignored notification and \
         the refused request, got: {next}",
    );
    assert_eq!(session.exit(), Ok(Teardown::AfterShutdown));
}

// ------------------------------------------------------- how the loop ends

#[test]
fn exit_before_shutdown_ends_the_loop_with_an_error() {
    let session = Session::start();
    assert_eq!(
        session.exit(),
        Err("exit notification received before shutdown request".to_owned()),
    );
}

#[test]
fn a_closed_connection_ends_the_loop_without_an_error() {
    let session = Session::start();
    session.did_open(CLEAN, 1);
    session.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(session.hang_up(), Ok(Teardown::WithoutShutdown));

    let session = Session::start();
    session.request_shutdown();
    assert_eq!(session.hang_up(), Ok(Teardown::AfterShutdown));
}
