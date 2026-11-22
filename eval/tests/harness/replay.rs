//! The replay backend: replies in turn order, by conversation.

use eval::model::replay::Replay;
use eval::model::{Conversation, Message, Model, Reply, Usage};
use serde_json::json;

fn replied(text: &str) -> Reply {
    Reply {
        text: text.to_owned(),
        content: json!([{"type": "text", "text": text}]),
        stop_reason: "end_turn".to_owned(),
        usage: Usage::default(),
    }
}

#[test]
fn each_turn_takes_the_conversations_next_reply() {
    let mut replay = Replay::parse(
        "[[conversation]]\nid = \"a\"\nreplies = [\"first\", \"second\"]\n\n\
         [[conversation]]\nid = \"b\"\nreplies = [\"other\"]\n",
    )
    .expect("the replay parses");
    let first = vec![Message::User("task".to_owned())];
    let ask = |replay: &mut Replay, id: &str, messages: &[Message]| {
        replay.reply(&Conversation {
            id,
            system: "spec",
            messages,
        })
    };
    assert_eq!(ask(&mut replay, "a", &first).ok(), Some(replied("first")));
    assert_eq!(ask(&mut replay, "b", &first).ok(), Some(replied("other")));
    let second = vec![
        Message::User("task".to_owned()),
        Message::Assistant(replied("first")),
        Message::User("feedback".to_owned()),
    ];
    assert_eq!(ask(&mut replay, "a", &second).ok(), Some(replied("second")));
    assert_eq!(
        ask(&mut replay, "b", &second).map_err(|error| error.to_string()),
        Err("the replay has no reply 2 for conversation `b`".to_owned())
    );
}

#[test]
fn a_conversation_listed_twice_is_refused() {
    let error = Replay::parse(
        "[[conversation]]\nid = \"a\"\nreplies = []\n\n[[conversation]]\nid = \"a\"\nreplies = []\n",
    )
    .map_err(|error| error.to_string());
    assert_eq!(
        error,
        Err("conversation `a` is listed twice; merge its replies into one table".to_owned())
    );
}
