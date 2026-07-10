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
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": TEST_URI,
                    "languageId": "cairn",
                    "version": version,
                    "text": text,
                },
            },
        }));
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
    assert_eq!(
        message["params"]["uri"].as_str(),
        Some(TEST_URI),
        "diagnostics should target the test document",
    );
    message["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array")
}

#[test]
fn lsp_1_initialize_advertises_full_sync_and_shuts_down_clean() {
    // AC1 + AC9.
    let (server, response) = Server::start();
    assert_eq!(response.get("id"), Some(&serde_json::json!(1)));
    let sync = &response["result"]["capabilities"]["textDocumentSync"];
    assert_eq!(sync["openClose"], serde_json::json!(true));
    assert_eq!(sync["change"], serde_json::json!(1), "change kind FULL");
    server.shutdown();
}

#[test]
fn lsp_2_did_open_duplicate_publishes_e_duplicate_size() {
    // AC2 + AC3: the duplicate fixture surfaces the stable code with
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
    // AC4: parser-rejected content (tab indentation) yields exactly one
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
    // AC5.
    let (mut server, _) = Server::start();
    server.did_open(CLEAN, 1);
    let message = server.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_of(&message).len(), 0);
    server.shutdown();
}

#[test]
fn lsp_5_did_change_clears_diagnostics() {
    // AC6: replacing broken content with clean content via full-sync
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
    // AC7.
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
    // AC8: the column of a finding behind a 😀 counts UTF-16 code units.
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
