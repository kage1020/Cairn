//! A model that answers from replies written down before, so a run needs no network.
//!
//! The file is TOML, with a `[[conversation]]` table for each prompt naming its `id` and listing
//! its `replies` in turn order: the first answers the task, each later one the feedback on the
//! reply before it.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::json;

use super::{Conversation, Message, Model, ModelError, Reply, Usage};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayFile {
    conversation: Vec<ConversationFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConversationFile {
    id: String,
    replies: Vec<String>,
}

/// Replies for each conversation, by prompt id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    conversations: BTreeMap<String, Vec<String>>,
}

impl Replay {
    /// The replies a replay file at `path` lists.
    ///
    /// # Errors
    ///
    /// [`ModelError`] when the file cannot be read or is not a replay file.
    pub fn load(path: &Path) -> Result<Self, ModelError> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| ModelError(format!("cannot read {}: {error}", path.display())))?;
        Self::parse(&text)
            .map_err(|ModelError(message)| ModelError(format!("{}: {message}", path.display())))
    }

    /// The replies the replay file `text` lists.
    ///
    /// # Errors
    ///
    /// [`ModelError`] when `text` is not a replay file or lists a conversation twice.
    pub fn parse(text: &str) -> Result<Self, ModelError> {
        let file: ReplayFile = toml::from_str(text)
            .map_err(|error| ModelError(error.to_string().trim_end().to_owned()))?;
        let mut conversations = BTreeMap::new();
        for conversation in file.conversation {
            if conversations
                .insert(conversation.id.clone(), conversation.replies)
                .is_some()
            {
                return Err(ModelError(format!(
                    "conversation `{}` is listed twice; merge its replies into one table",
                    conversation.id
                )));
            }
        }
        Ok(Self { conversations })
    }
}

impl Model for Replay {
    fn reply(&mut self, conversation: &Conversation<'_>) -> Result<Reply, ModelError> {
        let turn = conversation
            .messages
            .iter()
            .filter(|message| matches!(message, Message::Assistant(_)))
            .count();
        let text = self
            .conversations
            .get(conversation.id)
            .and_then(|replies| replies.get(turn))
            .ok_or_else(|| {
                ModelError(format!(
                    "the replay has no reply {} for conversation `{}`",
                    turn + 1,
                    conversation.id
                ))
            })?;
        Ok(Reply {
            text: text.clone(),
            content: json!([{"type": "text", "text": text}]),
            stop_reason: "end_turn".to_owned(),
            usage: Usage::default(),
        })
    }
}
