//! The dispatch loop driven in-process over an in-memory connection: what
//! each notification publishes, how each request is answered, and the
//! state machine between `shutdown` and `exit`.

use std::str::FromStr;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::*;
use crate::test_fixtures::{CLEAN, DUPLICATE};

const TEST_URI: &str = "file:///test.crn";

/// How long a test waits for a message before declaring the loop stuck.
///
/// The failures here are messages that never arrive, and a blocking read
/// turns each of those into a hang with no failing test name.
const READ_TIMEOUT: Duration = Duration::from_secs(20);

/// [`main_loop`] running on its own thread, with the client end of its
/// connection.
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

    /// Send a `textDocument/completion` request without waiting for it, for
    /// a test that reads the next message itself: [`Session::read_response`]
    /// would step over an interloping `publishDiagnostics`, which is what
    /// some of these tests assert does not happen.
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

    /// Send `shutdown` and assert the `null` result the spec requires,
    /// leaving the loop running for the window before `exit`.
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

    /// Close the client's end without a word, and return what the loop
    /// ended with.
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

    /// Run the `shutdown`/`exit` handshake and assert it ends the loop
    /// cleanly.
    fn shutdown(self) {
        self.request_shutdown();
        assert_eq!(self.exit(), Ok(Teardown::AfterShutdown));
    }
}

/// The diagnostics array of a `publishDiagnostics` notification, asserting
/// it targets `uri`.
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
    // Replacing broken content with clean content via full-sync didChange
    // publishes an empty set and echoes the version.
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
    // Two open documents must not bleed into each other: each publish
    // targets the URI of the notification that triggered it, and a
    // didChange on one leaves the other's diagnostics untouched.
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
    // Notifications are fire-and-forget: a payload that does not match the
    // method's schema, or a didChange with empty contentChanges, is skipped
    // rather than taking every open document's diagnostics down with it.
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
    // The spec lets a server ignore a notification it does not implement,
    // and `$/cancelRequest` overwhelmingly arrives during a session rather
    // than in the shutdown window.
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
    // Unbounded parser recursion overflows the stack and aborts the
    // process, so the second half — the next document still publishes — is
    // what tells a diagnosed document from a dead server. Brackets nest a
    // value and indentation nests a body; both reach here.
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
    // A silent server leaves the client blocked on the response id forever.
    // -32601 is the JSON-RPC MethodNotFound code.
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
    // Resolved through the document store, on a document whose cursor line
    // does not parse.
    let session = Session::start();
    let source = "theme a:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=";
    session.did_open(source, 1);
    let response = session.request_completion(7, TEST_URI, 3, 17);
    assert_eq!(completion_labels(&response), vec!["floor"]);
    session.shutdown();
}

#[test]
fn completion_reflects_did_change_revisions() {
    // The store serves the latest synced revision: a slot added via
    // didChange shows up in the next completion answer.
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
    // Asking about a document the client never opened is the client's
    // protocol violation, refused loud as -32602 InvalidParams, and the
    // server keeps serving.
    let session = Session::start();
    let response = session.request_completion(9, "file:///never-opened.crn", 0, 0);
    assert_eq!(response["error"]["code"], serde_json::json!(-32602));
    session.shutdown();
}

#[test]
fn a_position_one_line_past_the_document_is_refused() {
    // Clamping it would anchor every `textEdit` on the previous line, a
    // range that does not contain the requested position, so an editor
    // discards them all. Refusing is the answer the client can act on.
    let session = Session::start();
    session.did_open("st", 1);
    session.read_until_method("textDocument/publishDiagnostics");
    let response = session.request_completion(36, TEST_URI, 1, 0);
    assert_eq!(response["error"]["code"], serde_json::json!(-32602));
    session.shutdown();
}

#[test]
fn completion_answers_at_the_end_of_a_newline_terminated_document() {
    // A file ending in a newline has a line after it, and the cursor sitting
    // there is an ordinary position, not one past the document.
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
    // A document with no text still has line 0, where the cursor sits the
    // instant a new file is opened.
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
    // `didClose` publishes an empty set so the editor clears its squiggles.
    // A publish for a `didChange` behind it would leave a marker on a file
    // with no buffer to clear it.
    let session = Session::start();
    session.did_open(DUPLICATE, 1);
    let opened = session.read_until_method("textDocument/publishDiagnostics");
    assert!(!diagnostics_of(&opened).is_empty(), "the fixture is broken");
    session.did_close(TEST_URI);
    let closed = session.read_until_method("textDocument/publishDiagnostics");
    assert!(diagnostics_of(&closed).is_empty(), "close clears the marks");
    session.did_change(TEST_URI, DUPLICATE, 2);
    // The next message has to be the answer to the request behind the
    // change; `request_completion` would skip a publish unseen.
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
    // A `didChange` is a revision of a document, and there is no document:
    // recording it would make completion available for a URI the client
    // never opened.
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
    // A `didOpen` whose payload does not match the method's schema is
    // discarded, so the document never enters the store, and a `didChange`
    // does not put it there: the file stays unknown until it is opened
    // again. That cost is paid only by a client that violated the protocol
    // on the way in.
    let session = Session::start();
    session.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            // No `languageId`: required by the protocol, so this payload
            // does not deserialise and the notification is dropped.
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

    // The way out: a well-formed `didOpen` restores the document.
    session.did_open(DUPLICATE, 3);
    let published = session.read_until_method("textDocument/publishDiagnostics");
    assert!(
        !diagnostics_of(&published).is_empty(),
        "reopening republishes the fixture's findings",
    );
    session.shutdown();
}

// ----------------------------------------------------------- ranged changes

/// A `didChange` for [`TEST_URI`] carrying `events` as its
/// `contentChanges`.
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
    // The server advertises FULL sync, so a conforming client never sends a
    // range. One that does has its event applied as an edit: stored as the
    // whole document, `# note\n` inserted at 0:0 would replace the file,
    // and version 2 would publish no diagnostics for a file that still has
    // its parse error.
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
    // A range on a line the document does not have names no edit. The
    // revision is dropped, as a malformed payload is, and the document
    // keeps the text it had.
    let session = Session::start();
    session.did_open("struct s size=2x2\n", 1);
    session.read_until_method("textDocument/publishDiagnostics");
    session.send(&change_events(
        2,
        &serde_json::json!([insert_at(5, 0, "bogus!!")]),
    ));
    // Nothing is published for the dropped revision: the next message is
    // the completion response. That the stored text is unchanged is
    // `a_refused_revision_keeps_none_of_its_events`'s to show.
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
    // The document is open, so the only thing that keeps this revision from
    // being published is the guard on empty `contentChanges`: applied, it
    // would hand back the stored text and republish the same diagnostics
    // under version 2. `malformed_notifications_do_not_end_the_loop` sends
    // one too, but to a document whose `didOpen` was malformed, so it would
    // pass without the guard.
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
    // Version 2 is a valid insert followed by a range on a line the text
    // does not have. The whole revision is refused, the insert with it, so
    // the store still holds version 1's text. Version 3 is a ranged edit
    // that rewrites `struct` as itself: its diagnostics are version 1's
    // only if version 2 left nothing behind, since the insert would have
    // moved the parse error down a line.
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

/// One message an editor really sends, per row: a cancellation that may
/// arrive at any time, the `didClose` a closing window emits, a `didChange`
/// from a buffer being disposed, and a request already in flight.
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
    // `Connection::handle_shutdown` demands that the very next message be
    // `exit`, and anything else is a protocol error that ends the process
    // with code 1 without reading the `exit` behind it. An editor reads a
    // non-zero exit as a crashed language server.
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
    // The spec's words for this window: answer further requests with
    // `InvalidRequest` (-32600). Not `MethodNotFound` — the method exists —
    // and not silence, which leaves the client blocked on the id.
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
    // The state check runs before the method check, so `shutdown` itself
    // is refused like any other request once the server is shutting down.
    // A second `result: null` would tell the client the handshake restarted.
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
    // "Ignore notifications" is only testable as an ordering claim: send a
    // `didChange` that would publish, then a request that must be answered,
    // and require the next message to be that answer.
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
    // The spec requires `exit` without a preceding `shutdown` request to
    // end the process with a non-zero code, and the error is what `main`
    // turns into one.
    let session = Session::start();
    assert_eq!(
        session.exit(),
        Err("exit notification received before shutdown request".to_owned()),
    );
}

#[test]
fn a_closed_connection_ends_the_loop_without_an_error() {
    // Nobody is left to answer, before `shutdown` or after it. Whether
    // `shutdown` came first is what `run` needs to say which ending it was.
    let session = Session::start();
    session.did_open(CLEAN, 1);
    session.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(session.hang_up(), Ok(Teardown::WithoutShutdown));

    let session = Session::start();
    session.request_shutdown();
    assert_eq!(session.hang_up(), Ok(Teardown::AfterShutdown));
}
