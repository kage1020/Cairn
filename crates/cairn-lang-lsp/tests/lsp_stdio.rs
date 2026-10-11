//! The `cairn-lsp` binary over stdio, spoken to in Content-Length framed
//! JSON-RPC: the `initialize` handshake, the exit code and stderr of each
//! way a session ends (a transport failure included), the stderr line for
//! each notification the server drops, and the `--stdio` flag a client
//! appends. What the dispatch loop answers is tested in-process, beside
//! `server.rs`.

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

/// How long a test waits for a message before declaring the server stuck.
///
/// The regressions here are about messages that never arrive — a request
/// left unanswered, a notification silently dropped — and a blocking read
/// turns every one of those into a hang. On CI a hang surfaces as a job
/// timeout with no failing test name, which is the least useful shape a
/// failure can take. A bounded wait turns it back into a named assertion.
const READ_TIMEOUT: Duration = Duration::from_secs(20);

/// A spawned `cairn-lsp` with framed stdin access and its two output
/// streams drained by threads.
///
/// Both streams need a reader of their own: stdout because a blocking
/// read is what the timeout exists to avoid, and stderr because the drop
/// paths this crate takes report themselves there and nowhere else — a
/// test that cannot see stderr cannot tell "ignored, and said so" from
/// "ignored in silence". Draining stderr also keeps a chatty server from
/// filling the pipe buffer and blocking on its own log line.
struct Server {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<serde_json::Value>,
    stderr: Receiver<String>,
}

impl Server {
    /// Spawn the binary and run the `initialize`/`initialized` handshake,
    /// returning the server plus the raw `initialize` response.
    fn start() -> (Self, serde_json::Value) {
        Self::start_with_args(&[])
    }

    /// [`Server::start`] with `args` on the command line, for the argv an
    /// LSP client builds rather than the bare one a test would pick.
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
            // Stops on the first short read: the server has exited, and
            // every pending `recv_timeout` then fails immediately with
            // `Disconnected` instead of waiting out the timeout.
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

    /// Write `bytes` to stdin as they are, framing and all, for a frame a
    /// conforming client would never send.
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

    /// Every stderr line that has already reached the channel.
    ///
    /// Best-effort, and only for panic messages: a failing test says what
    /// the server had managed to log, and a snapshot that misses a line
    /// still in flight costs nothing there. It is not a way to assert
    /// anything. To wait for a line, use [`Server::read_stderr_until`]; to
    /// assert a finished session logged nothing, collect the channel
    /// (`server.stderr.iter().collect()`), which blocks until the reader
    /// thread disconnects — `child.wait()` returning says nothing about
    /// whether that thread has finished sending.
    fn drain_stderr(&mut self) -> Vec<String> {
        self.stderr.try_iter().collect()
    }

    /// Wait for a stderr line containing `needle`, and return every line
    /// seen up to and including it.
    ///
    /// The server writes stderr and stdout down two different pipes, and a
    /// reader thread carries each to its own channel, so reading a response
    /// says nothing about whether a line logged *before* it has been
    /// delivered yet. Taking whatever happened to have arrived made these
    /// assertions a race that lost on a loaded runner.
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
fn malformed_notifications_are_logged_and_skipped() {
    // Notifications are fire-and-forget: a payload that does not match the
    // method's schema, or a didChange with empty contentChanges, is skipped
    // with a line on stderr rather than taking the server down.
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
    // Still serving: a valid didOpen after the malformed traffic publishes
    // as usual.
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
    // One line names the method, the URI and the reason.
    // `read_stderr_until` returns the matching line last, so asserting on
    // `last()` keeps all three on the same line — `any()` would accept a
    // URI mentioned by some earlier line about a different document.
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
    // An editor that was killed rather than one that quit. There is nobody
    // left to answer, so this is not a failure, but it is not the orderly
    // teardown either, and stderr says which one happened.
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
    // `run` checks for a missing `shutdown` once the I/O threads are joined.
    // A client that says `shutdown` and then closes stdin reaches that check
    // with the flag set, so it has to stay quiet here; without this test,
    // "always warn" passes the one above.
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
    // The control for the abnormal-end line: it has to be absent from the
    // handshake it is meant to distinguish.
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
    // vscode-languageclient appends `--stdio` to an `Executable`'s argv
    // whenever its transport is `TransportKind.stdio`. Stdio is the only
    // transport this server speaks, so the flag must start the same session
    // the bare command does — including the diagnostics that are the reason
    // an editor starts it at all.
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
    // A revision whose range the text does not have is dropped with one
    // line on stderr naming the event it refused. The second revision is a
    // valid insert followed by a refused range: the line count is the one
    // the insert left, five lines where the opened text has four, because
    // the first event was applied before the second was read.
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
    // An unreadable frame ends `lsp-server`'s reader with an error, and the
    // dispatch loop sees that exactly as it sees end of input. The reader's
    // error is the only line: a "client closed stdin" line would say the
    // opposite of what happened.
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
    // The editor is gone with a message in flight: the server's write to
    // stdout fails, then stdin closes. `lsp-server` hands the writer's
    // error back from the same join as the reader's, and without saying
    // which thread it came from, so the one line the session gets has to
    // say what is true of both: the error, and that `shutdown` never came.
    let mut child = Command::new(env!("CARGO_BIN_EXE_cairn-lsp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cairn-lsp");
    // Closing the read end before sending anything makes the server's
    // first write, the `initialize` response, fail.
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
