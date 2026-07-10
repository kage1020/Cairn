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

    /// Run the `shutdown`/`exit` handshake and assert the process exits 0.
    fn shutdown(mut self) {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "shutdown",
        }));
        let response = loop {
            let message = self.read_message();
            if message.get("id") == Some(&serde_json::json!(99)) {
                break message;
            }
        };
        assert_eq!(
            response.get("result"),
            Some(&serde_json::Value::Null),
            "shutdown should return null, got: {response}",
        );
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "exit",
        }));
        let status = self.child.wait().expect("wait for server exit");
        assert_eq!(status.code(), Some(0), "server should exit cleanly");
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
