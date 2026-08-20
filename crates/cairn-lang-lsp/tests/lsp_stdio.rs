//! End-to-end tests driving the `cairn-lsp` binary over stdio with raw
//! Content-Length framed JSON-RPC, the same wire format an editor speaks.
//! Every test finishes with the `shutdown`/`exit` handshake so no server
//! process outlives its test on CI.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const DUPLICATE: &str = include_str!("../../cairn-lang-core/tests/fixtures/check/duplicate.crn");
const CLEAN: &str = include_str!("../../cairn-lang-core/tests/fixtures/check/clean.crn");
const TEST_URI: &str = "file:///test.crn";

/// A spawned `cairn-lsp` with framed stdin/stdout access.
struct Server {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
}

impl Server {
    /// Spawn the binary and run the `initialize`/`initialized` handshake,
    /// returning the server plus the raw `initialize` response.
    fn start() -> (Self, serde_json::Value) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cairn-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn cairn-lsp");
        let stdin = child.stdin.take().expect("piped stdin");
        let reader = BufReader::new(child.stdout.take().expect("piped stdout"));
        let mut server = Self {
            child,
            stdin,
            reader,
        };
        server.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "capabilities": {} },
        }));
        let response = server.read_message();
        server.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {},
        }));
        (server, response)
    }

    /// Write one Content-Length framed message.
    fn send(&mut self, message: &serde_json::Value) {
        let body = serde_json::to_string(message).expect("serialise message");
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body)
            .expect("write to server stdin");
        self.stdin.flush().expect("flush server stdin");
    }

    /// Read one Content-Length framed message.
    fn read_message(&mut self) -> serde_json::Value {
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            self.reader
                .read_line(&mut line)
                .expect("read header line from server");
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                content_length = Some(value.parse().expect("numeric Content-Length"));
            }
        }
        let length = content_length.expect("Content-Length header present");
        let mut body = vec![0_u8; length];
        self.reader
            .read_exact(&mut body)
            .expect("read message body from server");
        serde_json::from_slice(&body).expect("parse message body as JSON")
    }

    /// Read messages until one with the given method arrives, skipping
    /// unrelated server chatter (e.g. window/logMessage).
    fn read_until_method(&mut self, method: &str) -> serde_json::Value {
        loop {
            let message = self.read_message();
            if message.get("method").and_then(serde_json::Value::as_str) == Some(method) {
                return message;
            }
        }
    }

    fn did_open(&mut self, text: &str, version: i32) {
        self.did_open_uri(TEST_URI, text, version);
    }

    fn did_open_uri(&mut self, uri: &str, text: &str, version: i32) {
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

    /// Send a `textDocument/completion` request and return the response
    /// carrying `id`, skipping interleaved diagnostics pushes.
    fn request_completion(
        &mut self,
        id: i64,
        uri: &str,
        line: u32,
        character: u32,
    ) -> serde_json::Value {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            },
        }));
        loop {
            let message = self.read_message();
            if message.get("id") == Some(&serde_json::json!(id)) {
                break message;
            }
        }
    }

    /// Send `shutdown` and consume its response, asserting the `null`
    /// result the spec requires. Leaves the server running so a test can
    /// drive the window between `shutdown` and `exit`.
    fn request_shutdown(&mut self) {
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

    /// Read messages until the response carrying `id` arrives, skipping
    /// server-initiated notifications.
    fn read_response(&mut self, id: i64) -> serde_json::Value {
        loop {
            let message = self.read_message();
            if message.get("id") == Some(&serde_json::json!(id)) {
                return message;
            }
        }
    }

    /// Send `exit` and assert the process leaves with `code`.
    fn exit_expecting(mut self, code: i32) {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "exit",
        }));
        let status = self.child.wait().expect("wait for server exit");
        assert_eq!(status.code(), Some(code), "unexpected exit code");
    }

    /// Run the `shutdown`/`exit` handshake and assert the process exits 0.
    fn shutdown(mut self) {
        self.request_shutdown();
        self.exit_expecting(0);
    }
}

/// Extract the diagnostics array from a `publishDiagnostics` notification.
fn diagnostics_of(message: &serde_json::Value) -> &Vec<serde_json::Value> {
    diagnostics_for(message, TEST_URI)
}

/// Extract the diagnostics array, asserting the notification targets `uri`.
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

#[test]
fn lsp_1_initialize_advertises_full_sync_and_shuts_down_clean() {
    let (server, response) = Server::start();
    assert_eq!(response.get("id"), Some(&serde_json::json!(1)));
    let sync = &response["result"]["capabilities"]["textDocumentSync"];
    assert_eq!(sync["openClose"], serde_json::json!(true));
    assert_eq!(sync["change"], serde_json::json!(1), "change kind FULL");
    server.shutdown();
}

#[test]
fn lsp_2_did_open_duplicate_publishes_e_duplicate_size() {
    // The duplicate fixture surfaces the stable code with
    // ERROR severity and a relatedInformation pointer at the first
    // declaration.
    let (mut server, _) = Server::start();
    server.did_open(DUPLICATE, 1);
    let message = server.read_until_method("textDocument/publishDiagnostics");
    let diagnostics = diagnostics_of(&message);
    let dup = diagnostics
        .iter()
        .find(|d| d["code"] == serde_json::json!("E_DUPLICATE_SIZE"))
        .expect("E_DUPLICATE_SIZE in published diagnostics");
    assert_eq!(dup["severity"], serde_json::json!(1));
    assert_eq!(dup["source"], serde_json::json!("cairn"));
    assert_eq!(dup["range"]["start"]["line"], serde_json::json!(0));
    let second_size = DUPLICATE.find("size=5x5").expect("second size");
    assert_eq!(
        dup["range"]["start"]["character"],
        serde_json::json!(second_size),
    );
    let related = dup["relatedInformation"]
        .as_array()
        .expect("relatedInformation present");
    assert_eq!(related[0]["location"]["uri"].as_str(), Some(TEST_URI));
    server.shutdown();
}

#[test]
fn lsp_3_parse_error_publishes_single_error() {
    // Parser-rejected content (tab indentation) yields exactly one
    // ERROR diagnostic on the offending line.
    let (mut server, _) = Server::start();
    server.did_open("struct s size=2x2\n\tfloor\n", 1);
    let message = server.read_until_method("textDocument/publishDiagnostics");
    let diagnostics = diagnostics_of(&message);
    assert_eq!(diagnostics.len(), 1, "exactly one diagnostic expected");
    let d = &diagnostics[0];
    assert_eq!(d["severity"], serde_json::json!(1));
    assert_eq!(d["range"]["start"]["line"], serde_json::json!(1));
    assert_ne!(d["range"]["start"], d["range"]["end"], "non-empty range");
    server.shutdown();
}

#[test]
fn lsp_4_clean_file_publishes_empty() {
    let (mut server, _) = Server::start();
    server.did_open(CLEAN, 1);
    let message = server.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_of(&message).len(), 0);
    server.shutdown();
}

#[test]
fn lsp_5_did_change_clears_diagnostics() {
    // Replacing broken content with clean content via full-sync
    // didChange publishes an empty set and echoes the version.
    let (mut server, _) = Server::start();
    server.did_open(DUPLICATE, 1);
    let first = server.read_until_method("textDocument/publishDiagnostics");
    assert!(!diagnostics_of(&first).is_empty(), "broken file diagnosed");
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": TEST_URI, "version": 2 },
            "contentChanges": [ { "text": CLEAN } ],
        },
    }));
    let second = server.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_of(&second).len(), 0);
    assert_eq!(second["params"]["version"], serde_json::json!(2));
    server.shutdown();
}

#[test]
fn lsp_6_did_close_clears_diagnostics() {
    let (mut server, _) = Server::start();
    server.did_open(DUPLICATE, 1);
    let first = server.read_until_method("textDocument/publishDiagnostics");
    assert!(!diagnostics_of(&first).is_empty(), "broken file diagnosed");
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didClose",
        "params": {
            "textDocument": { "uri": TEST_URI },
        },
    }));
    let second = server.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_of(&second).len(), 0);
    server.shutdown();
}

#[test]
fn lsp_7_utf16_range_on_non_ascii_line() {
    // The column of a finding behind a 😀 counts UTF-16 code units.
    let (mut server, _) = Server::start();
    let source = "struct s size=2x2\n  door id=\"😀\" id=x\n";
    server.did_open(source, 1);
    let message = server.read_until_method("textDocument/publishDiagnostics");
    let diagnostics = diagnostics_of(&message);
    let dup = diagnostics
        .iter()
        .find(|d| d["code"] == serde_json::json!("E_DUPLICATE_ARG"))
        .expect("E_DUPLICATE_ARG in published diagnostics");
    let line = "  door id=\"😀\" id=x";
    let utf16_col = line[..line.find("id=x").expect("second id")]
        .encode_utf16()
        .count();
    assert_eq!(dup["range"]["start"]["line"], serde_json::json!(1));
    assert_eq!(
        dup["range"]["start"]["character"],
        serde_json::json!(utf16_col),
    );
    server.shutdown();
}

#[test]
fn lsp_8_unknown_request_gets_method_not_found() {
    // An unsupported request must be answered — a silent server leaves the
    // client blocked on the response id forever. -32601 is the JSON-RPC
    // MethodNotFound code.
    let (mut server, _) = Server::start();
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "textDocument/hover",
        "params": {},
    }));
    let response = loop {
        let message = server.read_message();
        if message.get("id") == Some(&serde_json::json!(42)) {
            break message;
        }
    };
    assert_eq!(response["error"]["code"], serde_json::json!(-32601));
    server.shutdown();
}

#[test]
fn lsp_9_exit_without_shutdown_exits_nonzero() {
    // The LSP spec requires `exit` without a preceding `shutdown` request
    // to terminate the process with a non-zero code.
    let (mut server, _) = Server::start();
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "exit",
    }));
    let status = server.child.wait().expect("wait for server exit");
    assert_eq!(
        status.code(),
        Some(1),
        "exit before shutdown should exit non-zero",
    );
}

#[test]
fn lsp_10_documents_publish_under_their_own_uris() {
    // Two open documents must not bleed into each other: each publish
    // targets the URI of the notification that triggered it, and a
    // didChange on one leaves the other's diagnostics untouched.
    let broken_uri = "file:///broken.crn";
    let clean_uri = "file:///clean.crn";
    let (mut server, _) = Server::start();
    server.did_open_uri(broken_uri, DUPLICATE, 1);
    let first = server.read_until_method("textDocument/publishDiagnostics");
    assert!(!diagnostics_for(&first, broken_uri).is_empty());
    server.did_open_uri(clean_uri, CLEAN, 1);
    let second = server.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_for(&second, clean_uri).len(), 0);
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": broken_uri, "version": 2 },
            "contentChanges": [ { "text": CLEAN } ],
        },
    }));
    let third = server.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_for(&third, broken_uri).len(), 0);
    server.shutdown();
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
fn lsp_12_initialize_advertises_completion_with_trigger_characters() {
    let (server, response) = Server::start();
    let provider = &response["result"]["capabilities"]["completionProvider"];
    assert_eq!(
        provider["triggerCharacters"],
        serde_json::json!(["@", "=", "."]),
    );
    server.shutdown();
}

#[test]
fn lsp_13_completion_at_mat_slot_returns_declared_slot_names() {
    // A completion request against the opened document resolves through the
    // document store and answers with the theme's slot names — on a document
    // whose cursor line does not parse.
    let (mut server, _) = Server::start();
    let source = "theme a:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=";
    server.did_open(source, 1);
    let response = server.request_completion(7, TEST_URI, 3, 17);
    assert_eq!(completion_labels(&response), vec!["floor"]);
    server.shutdown();
}

#[test]
fn lsp_14_completion_reflects_did_change_revisions() {
    // The store must serve the *latest* synced revision: a slot added via
    // didChange shows up in the next completion answer.
    let (mut server, _) = Server::start();
    let source = "theme a:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=";
    server.did_open(source, 1);
    let revised = "theme a:\n  slot floor -> @oak_planks\n  slot wall -> @cobblestone\n\
                   struct s size=2x2\n  floor mat_slot=";
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": TEST_URI, "version": 2 },
            "contentChanges": [ { "text": revised } ],
        },
    }));
    let response = server.request_completion(8, TEST_URI, 4, 17);
    assert_eq!(completion_labels(&response), vec!["floor", "wall"]);
    server.shutdown();
}

#[test]
fn lsp_15_completion_on_unopened_document_is_invalid_params() {
    // Requests are always answered; asking about a document the client
    // never opened is a protocol violation on the client's side, surfaced
    // loud as -32602 InvalidParams — and the server keeps serving.
    let (mut server, _) = Server::start();
    let response = server.request_completion(9, "file:///never-opened.crn", 0, 0);
    assert_eq!(response["error"]["code"], serde_json::json!(-32602));
    server.shutdown();
}

#[test]
fn lsp_11_malformed_notifications_do_not_kill_the_server() {
    // Notifications are fire-and-forget: a payload that does not match the
    // method's schema (or a didChange with empty contentChanges) must be
    // logged and skipped, not crash the loop — one buggy client message
    // would otherwise take every open document's diagnostics down with it.
    let (mut server, _) = Server::start();
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": { "textDocument": { "uri": TEST_URI } },
    }));
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": TEST_URI, "version": 2 },
            "contentChanges": [],
        },
    }));
    // The server must still be alive and serving: a valid didOpen after
    // the malformed traffic publishes as usual.
    server.did_open(CLEAN, 3);
    let message = server.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_of(&message).len(), 0);
    server.shutdown();
}

#[test]
fn lsp_16_deeply_nested_document_is_diagnosed_not_fatal() {
    // A Rust stack overflow aborts the process, so unbounded parser
    // recursion did not merely produce a bad diagnostic here — it killed the
    // language server, which re-parses on every keystroke. A user typing
    // brackets would have watched the editor report a crashed server.
    //
    // Asserting the *next* request still gets an answer is what makes this a
    // liveness test rather than a diagnostics test: a dead server publishes
    // nothing at all, which an "expect diagnostics" assertion alone could
    // not tell apart from a parse that succeeded.
    // Both recursive shapes reach here: brackets nest a value, indentation
    // nests a body. The second one stayed unguarded after the first was
    // fixed, so covering only brackets would have declared this closed while
    // the server still died on an indented document.
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
        let (mut server, _) = Server::start();
        server.did_open(&deep, 1);
        let published = server.read_until_method("textDocument/publishDiagnostics");
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

        // The liveness half, and the reason this is not just a diagnostics
        // test: a dead server publishes nothing, which the assertions above
        // cannot tell apart from a parse that succeeded.
        server.did_open(CLEAN, 2);
        let after = server.read_until_method("textDocument/publishDiagnostics");
        assert_eq!(
            diagnostics_of(&after).len(),
            0,
            "{shape}: the server must still serve the next document",
        );
        server.shutdown();
    }
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
fn lsp_17_a_message_between_shutdown_and_exit_still_exits_zero() {
    // `Connection::handle_shutdown` demanded that the very next message be
    // `exit`; anything else became a protocol error that ended the process
    // with code 1 *without reading the `exit` behind it*. An editor reads a
    // non-zero exit as "the language server crashed" and restarts it.
    for (label, interloper) in interlopers() {
        let (mut server, _) = Server::start();
        server.did_open(CLEAN, 1);
        server.read_until_method("textDocument/publishDiagnostics");
        server.request_shutdown();
        server.send(&interloper);
        // The completion request is answered before `exit`; draining it
        // here keeps the assertion about the exit code alone.
        if interloper.get("id").is_some() {
            server.read_response(7);
        }
        eprintln!("interloper: {label}");
        server.exit_expecting(0);
    }
}

#[test]
fn lsp_18_a_request_after_shutdown_is_refused_as_invalid() {
    // The spec's own words for this window: answer further requests with
    // `InvalidRequest` (-32600). Not `MethodNotFound` — the method exists —
    // and not silence, which leaves the client blocked on the id.
    let (mut server, _) = Server::start();
    server.did_open(CLEAN, 1);
    server.read_until_method("textDocument/publishDiagnostics");
    server.request_shutdown();
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 31,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": TEST_URI },
            "position": { "line": 0, "character": 0 },
        },
    }));
    let response = server.read_response(31);
    assert_eq!(response["error"]["code"], serde_json::json!(-32600));
    server.exit_expecting(0);
}

#[test]
fn lsp_19_a_second_shutdown_is_refused_rather_than_answered_again() {
    // The state check runs before the method check, so `shutdown` itself
    // is a request like any other once the server is shutting down. A
    // second `result: null` would tell the client the handshake restarted.
    let (mut server, _) = Server::start();
    server.request_shutdown();
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 32,
        "method": "shutdown",
    }));
    let response = server.read_response(32);
    assert_eq!(response["error"]["code"], serde_json::json!(-32600));
    assert_eq!(response.get("result"), None);
    server.exit_expecting(0);
}

#[test]
fn lsp_20_a_notification_after_shutdown_publishes_nothing() {
    // "Ignore notifications" is only testable as an ordering claim: send a
    // `didChange` that would publish, then a request that must be answered,
    // and require the *next* message on the wire to be that answer. A
    // `publishDiagnostics` arriving first is the failure signature, and it
    // is what a flag checked inside the notification handler — after the
    // diagnostics are computed and pushed — would produce.
    let (mut server, _) = Server::start();
    server.did_open(CLEAN, 1);
    server.read_until_method("textDocument/publishDiagnostics");
    server.request_shutdown();
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": TEST_URI, "version": 2 },
            "contentChanges": [{ "text": "struct s size=2x2 size=3x3\n" }],
        },
    }));
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 33,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": TEST_URI },
            "position": { "line": 0, "character": 0 },
        },
    }));
    let next = server.read_message();
    assert_eq!(
        next.get("id"),
        Some(&serde_json::json!(33)),
        "nothing may reach the client between the ignored notification and \
         the refused request, got: {next}",
    );
    server.exit_expecting(0);
}

// ------------------------------------- the store mirrors the client's open set

#[test]
fn lsp_21_did_change_after_did_close_leaves_the_document_closed() {
    // `didClose` publishes an empty set so the editor clears its squiggles.
    // A `didChange` behind it used to re-insert the URI and publish a fresh
    // set — a permanent marker on a file with no buffer to clear it, and a
    // store that only ever grew.
    let (mut server, _) = Server::start();
    server.did_open(DUPLICATE, 1);
    let opened = server.read_until_method("textDocument/publishDiagnostics");
    assert!(!diagnostics_of(&opened).is_empty(), "the fixture is broken");
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didClose",
        "params": { "textDocument": { "uri": TEST_URI } },
    }));
    let closed = server.read_until_method("textDocument/publishDiagnostics");
    assert!(diagnostics_of(&closed).is_empty(), "close clears the marks");
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": TEST_URI, "version": 2 },
            "contentChanges": [{ "text": DUPLICATE }],
        },
    }));
    // The ordering assertion again: the next message has to be the answer
    // to the request behind the change, not a publish for a closed file.
    let response = server.request_completion(34, TEST_URI, 0, 0);
    assert_eq!(
        response["error"]["code"],
        serde_json::json!(-32602),
        "the document is closed, so completion has nothing to answer from",
    );
    server.shutdown();
}

#[test]
fn lsp_22_did_change_for_a_never_opened_document_is_ignored() {
    // The same rule from the other side: a `didChange` is a revision of a
    // document, and there is no document. Inserting one made completion
    // available for a URI the client never opened.
    const OTHER_URI: &str = "file:///never-opened.crn";
    let (mut server, _) = Server::start();
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": OTHER_URI, "version": 1 },
            "contentChanges": [{ "text": CLEAN }],
        },
    }));
    let response = server.request_completion(35, OTHER_URI, 0, 0);
    assert_eq!(response["error"]["code"], serde_json::json!(-32602));
    server.shutdown();
}

#[test]
fn lsp_23_a_position_one_line_past_the_document_is_refused() {
    // The clamp that admitted this produced four items anchored on the
    // previous line: every `textEdit` covered `line 0, 0..2`, a range that
    // does not contain the requested `1:0`, so an editor discards them all.
    // Refusing is the answer the client can act on.
    let (mut server, _) = Server::start();
    server.did_open("st", 1);
    server.read_until_method("textDocument/publishDiagnostics");
    let response = server.request_completion(36, TEST_URI, 1, 0);
    assert_eq!(response["error"]["code"], serde_json::json!(-32602));
    server.shutdown();
}
