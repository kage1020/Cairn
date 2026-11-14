//! The Messages API backend, against a local stand-in for the API that answers what it is told.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use eval::model::anthropic::{Anthropic, parse_reply, request_body};
use eval::model::{Conversation, Message, Model, Reply, Usage};
use serde_json::{Value, json};

/// A request as the stand-in received it.
struct Received {
    line: String,
    headers: BTreeMap<String, String>,
    body: Value,
}

fn read_request(stream: &mut TcpStream) -> Received {
    let mut data = Vec::new();
    let mut buffer = [0; 4096];
    let head_end = loop {
        if let Some(end) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        let read = stream.read(&mut buffer).expect("the request reads");
        assert!(read > 0, "the client closed the connection mid-request");
        data.extend_from_slice(&buffer[..read]);
    };
    let head = String::from_utf8(data[..head_end].to_vec()).expect("the head is UTF-8");
    let mut lines = head.split("\r\n");
    let line = lines.next().expect("a request line").to_owned();
    let headers: BTreeMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length: usize = headers["content-length"].parse().expect("a content length");
    let mut body = data[head_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut buffer).expect("the body reads");
        body.extend_from_slice(&buffer[..read]);
    }
    Received {
        line,
        headers,
        body: serde_json::from_slice(&body).expect("the body is JSON"),
    }
}

/// A stand-in answering one connection per response, in order, and the URL it listens at.
fn serve(responses: Vec<(u16, &'static str, String)>) -> (String, JoinHandle<Vec<Received>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let url = format!(
        "http://{}/v1/messages",
        listener.local_addr().expect("a bound address")
    );
    let handle = thread::spawn(move || {
        responses
            .into_iter()
            .map(|(status, headers, body)| {
                let (mut stream, _) = listener.accept().expect("the client connects");
                let received = read_request(&mut stream);
                let response = format!(
                    "HTTP/1.1 {status} Answer\r\ncontent-type: application/json\r\n\
                     content-length: {}\r\nconnection: close\r\n{headers}\r\n{body}",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("the response writes");
                received
            })
            .collect()
    });
    (url, handle)
}

fn thinking_reply() -> Reply {
    Reply {
        text: "Here.".to_owned(),
        content: json!([
            {"type": "thinking", "thinking": "A shed.", "signature": "c2lnbmF0dXJl"},
            {"type": "text", "text": "Here."},
        ]),
        stop_reason: "end_turn".to_owned(),
        usage: Usage::default(),
    }
}

fn conversation_messages() -> Vec<Message> {
    vec![
        Message::User("task".to_owned()),
        Message::Assistant(thinking_reply()),
        Message::User("feedback".to_owned()),
    ]
}

fn expected_body() -> Value {
    json!({
        "model": "test-model",
        "max_tokens": 1024,
        "system": [{"type": "text", "text": "spec", "cache_control": {"type": "ephemeral"}}],
        "messages": [
            {"role": "user", "content": "task"},
            {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "A shed.", "signature": "c2lnbmF0dXJl"},
                    {"type": "text", "text": "Here."},
                ],
            },
            {"role": "user", "content": "feedback"},
        ],
    })
}

const ANSWER: &str = r#"{
    "id": "msg_01",
    "type": "message",
    "role": "assistant",
    "content": [
        {"type": "thinking", "thinking": "The roof.", "signature": "c2ln"},
        {"type": "text", "text": "One."},
        {"type": "text", "text": "Two."}
    ],
    "stop_reason": "end_turn",
    "usage": {
        "input_tokens": 10,
        "output_tokens": 20,
        "cache_creation_input_tokens": 30,
        "cache_read_input_tokens": 40
    }
}"#;

fn answered() -> Reply {
    Reply {
        text: "One.\nTwo.".to_owned(),
        content: json!([
            {"type": "thinking", "thinking": "The roof.", "signature": "c2ln"},
            {"type": "text", "text": "One."},
            {"type": "text", "text": "Two."},
        ]),
        stop_reason: "end_turn".to_owned(),
        usage: Usage {
            input_tokens: 10,
            output_tokens: 20,
            cache_creation_input_tokens: 30,
            cache_read_input_tokens: 40,
        },
    }
}

#[test]
fn a_request_carries_the_model_the_cached_spec_and_each_turn_as_it_came() {
    let messages = conversation_messages();
    let conversation = Conversation {
        id: "a",
        system: "spec",
        messages: &messages,
    };
    assert_eq!(
        request_body("test-model", 1024, &conversation),
        expected_body()
    );
}

#[test]
fn a_reply_keeps_every_block_and_joins_the_text_of_its_text_blocks() {
    assert_eq!(parse_reply(ANSWER).ok(), Some(answered()));
}

#[test]
fn an_overloaded_api_is_asked_again_with_the_same_request() {
    let overloaded =
        r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
    let (url, server) = serve(vec![
        (529, "retry-after: 0\r\n", overloaded.to_owned()),
        (200, "", ANSWER.to_owned()),
    ]);
    let mut model = Anthropic::new("test-key".to_owned(), "test-model".to_owned(), 1024)
        .with_endpoint(url)
        .with_backoff(Duration::ZERO);
    let messages = conversation_messages();
    let reply = model.reply(&Conversation {
        id: "a",
        system: "spec",
        messages: &messages,
    });
    assert_eq!(reply.ok(), Some(answered()));
    let received = server.join().expect("the stand-in ends");
    assert_eq!(received.len(), 2);
    for request in received {
        assert_eq!(request.line, "POST /v1/messages HTTP/1.1");
        let sent: BTreeMap<&str, &str> = request
            .headers
            .iter()
            .filter(|(name, _)| {
                [
                    "x-api-key",
                    "anthropic-version",
                    "content-type",
                    "authorization",
                ]
                .contains(&name.as_str())
            })
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        assert_eq!(
            sent,
            BTreeMap::from([
                ("anthropic-version", "2026-10-21"),
                ("content-type", "application/json"),
                ("x-api-key", "test-key"),
            ])
        );
        assert_eq!(request.body, expected_body());
    }
}

#[test]
fn a_refused_request_is_reported_and_not_sent_again() {
    let refused = r#"{"type":"error","error":{"type":"invalid_request_error","message":"bad"}}"#;
    let (url, server) = serve(vec![(400, "", refused.to_owned())]);
    let mut model = Anthropic::new("test-key".to_owned(), "test-model".to_owned(), 1024)
        .with_endpoint(url)
        .with_backoff(Duration::ZERO);
    let messages = vec![Message::User("task".to_owned())];
    let reply = model.reply(&Conversation {
        id: "a",
        system: "spec",
        messages: &messages,
    });
    assert_eq!(
        reply.map_err(|error| error.to_string()),
        Err(format!("the API answered 400: {refused}"))
    );
    assert_eq!(server.join().expect("the stand-in ends").len(), 1);
}
