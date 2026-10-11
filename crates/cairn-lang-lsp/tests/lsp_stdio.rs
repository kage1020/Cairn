//! The `cairn-lsp` binary over stdio: how each way a session ends exits and logs, and the line each
//! dropped notification logs. What the dispatch loop answers is tested beside `server.rs`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread;
use std::time::Duration;

/// A document with a finding on every line.
const DUPLICATE: &str = "\
struct s size=4x4 size=5x5
  walls height=4 height=5
  door id=front
  door id=front
";
/// A document `check` has nothing to say about.
const CLEAN: &str = "\
struct s size=2x2
  floor mat_slot=wall
  walls height=4 mat_slot=wall
";
const TEST_URI: &str = "file:///test.crn";

/// How long a test waits for a message, so one that never arrives fails rather than hangs.
const READ_TIMEOUT: Duration = Duration::from_secs(20);

/// A spawned `cairn-lsp` whose stdout and stderr are drained by threads.
struct Server {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<serde_json::Value>,
    stderr: Receiver<String>,
}

impl Server {
    /// Spawn the binary and run the handshake, returning it and the raw `initialize` response.
    fn start() -> (Self, serde_json::Value) {
        Self::start_with_args(&[])
    }

    /// [`Server::start`] with `args` on the command line.
    fn start_with_args(args: &[&str]) -> (Self, serde_json::Value) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cairn-lsp"))
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn cairn-lsp");
        let stdin = child.stdin.take().expect("piped stdin");

        let (message_tx, messages) = channel();
        let mut out = BufReader::new(child.stdout.take().expect("piped stdout"));
        thread::spawn(move || {
            while let Some(message) = read_framed(&mut out) {
                if message_tx.send(message).is_err() {
                    return;
                }
            }
        });

        let (stderr_tx, stderr) = channel();
        let err = BufReader::new(child.stderr.take().expect("piped stderr"));
        thread::spawn(move || {
            for line in err.lines().map_while(Result::ok) {
                if stderr_tx.send(line).is_err() {
                    return;
                }
            }
        });

        let mut server = Self {
            child,
            stdin,
            messages,
            stderr,
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

    /// Write `bytes` to stdin as they are, for a frame no conforming client would send.
    fn send_raw(&mut self, bytes: &[u8]) {
        self.stdin.write_all(bytes).expect("write to server stdin");
        self.stdin.flush().expect("flush server stdin");
    }

    /// Take the next message the server sent, or fail the test.
    fn read_message(&mut self) -> serde_json::Value {
        match self.messages.recv_timeout(READ_TIMEOUT) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => {
                panic!(
                    "server sent nothing within {READ_TIMEOUT:?}; stderr: {:?}",
                    self.drain_stderr()
                )
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!(
                    "server exited without answering; stderr: {:?}",
                    self.drain_stderr()
                )
            }
        }
    }

    /// The stderr lines that have already arrived, for panic messages only: a line can still be in
    /// flight, so a test waits with [`Server::read_stderr_until`] or collects the channel.
    fn drain_stderr(&mut self) -> Vec<String> {
        self.stderr.try_iter().collect()
    }

    /// Wait for a stderr line containing `needle`, and return every line up to and including it.
    fn read_stderr_until(&mut self, needle: &str) -> Vec<String> {
        let mut seen = Vec::new();
        loop {
            match self.stderr.recv_timeout(READ_TIMEOUT) {
                Ok(line) => {
                    let matched = line.contains(needle);
                    seen.push(line);
                    if matched {
                        return seen;
                    }
                }
                Err(RecvTimeoutError::Timeout) => panic!(
                    "no stderr line containing {needle:?} within {READ_TIMEOUT:?}; saw: {seen:?}"
                ),
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("the server exited without logging {needle:?}; saw: {seen:?}")
                }
            }
        }
    }

    /// Read messages until one with the given method arrives.
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

    /// Send `shutdown` and assert its `null` result, leaving the server running until `exit`.
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

    /// Read messages until the response carrying `id` arrives.
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

/// Read one Content-Length framed message, or `None` at end of stream.
fn read_framed(reader: &mut impl BufRead) -> Option<serde_json::Value> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length: ") {
            content_length = Some(value.parse().expect("numeric Content-Length"));
        }
    }
    let mut body = vec![0_u8; content_length?];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

/// Extract the diagnostics array from a `publishDiagnostics` notification.
fn diagnostics_of(message: &serde_json::Value) -> &Vec<serde_json::Value> {
    assert_eq!(
        message["params"]["uri"].as_str(),
        Some(TEST_URI),
        "diagnostics should target the expected document",
    );
    message["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array")
}

#[test]
fn initialize_advertises_full_sync_and_completion_and_shuts_down_clean() {
    let (server, response) = Server::start();
    assert_eq!(response.get("id"), Some(&serde_json::json!(1)));
    let capabilities = &response["result"]["capabilities"];
    let sync = &capabilities["textDocumentSync"];
    assert_eq!(sync["openClose"], serde_json::json!(true));
    assert_eq!(sync["change"], serde_json::json!(1), "change kind FULL");
    assert_eq!(
        capabilities["completionProvider"]["triggerCharacters"],
        serde_json::json!(["@", "=", "."]),
    );
    server.shutdown();
}

#[test]
fn exit_without_shutdown_exits_nonzero() {
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
fn malformed_notifications_are_logged_and_skipped() {
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
    server.read_stderr_until("ignoring malformed `textDocument/didOpen`");
    server.read_stderr_until(
        "ignoring `textDocument/didChange` notification with empty contentChanges",
    );
    server.did_open(CLEAN, 3);
    let message = server.read_until_method("textDocument/publishDiagnostics");
    assert_eq!(diagnostics_of(&message).len(), 0);
    server.shutdown();
}

#[test]
fn a_change_to_a_closed_document_is_logged_with_its_uri() {
    let (mut server, _) = Server::start();
    server.did_open(DUPLICATE, 1);
    server.read_until_method("textDocument/publishDiagnostics");
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didClose",
        "params": { "textDocument": { "uri": TEST_URI } },
    }));
    server.read_until_method("textDocument/publishDiagnostics");
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": TEST_URI, "version": 2 },
            "contentChanges": [{ "text": DUPLICATE }],
        },
    }));
    // `last()` is the line with the reason, so the method and URI must be on it too.
    let logged = server.read_stderr_until("the document is not open");
    assert!(
        logged.last().is_some_and(|line| line.contains(&format!(
            "ignoring `textDocument/didChange` for {TEST_URI}: "
        ))),
        "the reported line should name the method and the URI, got: {logged:?}",
    );
    server.shutdown();
}

#[test]
fn closing_stdin_without_exit_says_the_session_ended_abnormally() {
    let (mut server, _) = Server::start();
    server.did_open(CLEAN, 1);
    server.read_until_method("textDocument/publishDiagnostics");
    drop(server.stdin);
    let status = server.child.wait().expect("wait for server exit");
    assert_eq!(
        status.code(),
        Some(0),
        "there is nobody to report an error to"
    );
    let logged: Vec<String> = server.stderr.iter().collect();
    assert!(
        logged
            .iter()
            .any(|line| line.contains("closed stdin without `shutdown`")),
        "the abnormal end should be on the record, got: {logged:?}",
    );
}

#[test]
fn closing_stdin_after_shutdown_is_not_abnormal() {
    let (mut server, _) = Server::start();
    server.did_open(CLEAN, 1);
    server.read_until_method("textDocument/publishDiagnostics");
    server.request_shutdown();
    drop(server.stdin);
    let status = server.child.wait().expect("wait for server exit");
    assert_eq!(status.code(), Some(0));
    let logged: Vec<String> = server.stderr.iter().collect();
    assert!(
        logged.is_empty(),
        "the client said `shutdown`, so the teardown was orderly, got: {logged:?}",
    );
}

#[test]
fn a_clean_session_says_nothing_on_stderr() {
    let (mut server, _) = Server::start();
    server.did_open(CLEAN, 1);
    server.read_until_method("textDocument/publishDiagnostics");
    server.request_shutdown();
    server.send(&serde_json::json!({ "jsonrpc": "2.0", "method": "exit" }));
    let status = server.child.wait().expect("wait for server exit");
    assert_eq!(status.code(), Some(0));
    let logged: Vec<String> = server.stderr.iter().collect();
    assert!(
        logged.is_empty(),
        "a clean session is quiet, got: {logged:?}"
    );
}

#[test]
fn the_stdio_flag_a_client_appends_still_starts_the_server() {
    let (mut server, response) = Server::start_with_args(&["--stdio"]);
    assert_eq!(response.get("id"), Some(&serde_json::json!(1)));
    assert_eq!(
        response["result"]["capabilities"]["textDocumentSync"]["change"],
        serde_json::json!(1),
        "`--stdio` should answer `initialize` like the bare command: {response}",
    );
    server.did_open(DUPLICATE, 1);
    let message = server.read_until_method("textDocument/publishDiagnostics");
    assert!(
        !diagnostics_of(&message).is_empty(),
        "`--stdio` session should publish the duplicate fixture's diagnostics",
    );
    server.shutdown();
}

#[test]
fn a_refused_revision_is_logged_with_the_event_and_the_lines_there_were() {
    // The second revision's insert is applied before its refused event, so the text has 5 lines.
    let (mut server, _) = Server::start();
    server.did_open("@cairn 2026.06\nstruct s size=3x3\n  bogus!!\n", 1);
    server.read_until_method("textDocument/publishDiagnostics");
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": TEST_URI, "version": 2 },
            "contentChanges": [{
                "range": {
                    "start": { "line": 5, "character": 0 },
                    "end": { "line": 5, "character": 0 },
                },
                "text": "bogus!!",
            }],
        },
    }));
    let logged = server.read_stderr_until("ignoring `textDocument/didChange`");
    assert!(
        logged
            .last()
            .is_some_and(|line| line.contains("contentChanges[0]")),
        "the line should name the refused event, got: {logged:?}",
    );
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": TEST_URI, "version": 3 },
            "contentChanges": [
                {
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 0 },
                    },
                    "text": "# note\n",
                },
                {
                    "range": {
                        "start": { "line": 9, "character": 0 },
                        "end": { "line": 9, "character": 0 },
                    },
                    "text": "bogus!!",
                },
            ],
        },
    }));
    let logged = server.read_stderr_until("ignoring `textDocument/didChange`");
    assert!(
        logged.last().is_some_and(|line| line.ends_with(
            "contentChanges[1] edits 9:0..9:0, but the text it applies to has 5 lines (0 to 4)"
        )),
        "the line should name the refused event and the lines there were, got: {logged:?}",
    );
    server.shutdown();
}

#[test]
fn an_unreadable_frame_is_not_reported_as_the_client_closing_stdin() {
    for frame in [
        &b"Content-Length: 10\r\n\r\n{not json}"[..],
        &b"Content-Length: abc\r\n\r\n"[..],
    ] {
        let (mut server, _) = Server::start();
        server.send_raw(frame);
        drop(server.stdin);
        let status = server.child.wait().expect("wait for server exit");
        assert_eq!(status.code(), Some(1), "the transport failed");
        let logged: Vec<String> = server.stderr.iter().collect();
        assert!(
            logged.len() == 1 && logged[0].starts_with("error: "),
            "the reader's error should be the one line, got: {logged:?}",
        );
        assert!(
            !logged.iter().any(|line| line.contains("closed stdin")),
            "the client did not close stdin, got: {logged:?}",
        );
    }
}

#[test]
fn a_writer_failure_before_shutdown_still_says_the_session_ended_without_it() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cairn-lsp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cairn-lsp");
    // The server's first write, the `initialize` response, fails.
    drop(child.stdout.take());
    let mut stdin = child.stdin.take().expect("piped stdin");
    for message in [
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "capabilities": {} },
        }),
        serde_json::json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
    ] {
        let body = serde_json::to_string(&message).expect("serialise message");
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body)
            .expect("write to server stdin");
    }
    drop(stdin);
    let output = child.wait_with_output().expect("wait for server exit");
    assert_eq!(output.status.code(), Some(1), "the transport failed");
    let stderr = String::from_utf8(output.stderr).expect("utf-8 stderr");
    let logged: Vec<&str> = stderr.lines().collect();
    assert!(
        logged.len() == 1
            && logged[0].starts_with("error: ")
            && logged[0].ends_with("; the session ended without `shutdown`"),
        "the writer's error should be the one line, and say `shutdown` never came, got: {logged:?}",
    );
}
