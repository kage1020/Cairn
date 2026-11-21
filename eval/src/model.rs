//! The models a run asks: the Anthropic Messages API, or a replay of replies written down before.

pub mod anthropic;
pub mod replay;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The tokens a request spent, as the Messages API counts them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Input tokens read neither from nor into the prompt cache.
    pub input_tokens: u64,
    /// Output tokens, thinking included.
    pub output_tokens: u64,
    /// Input tokens written to the prompt cache.
    pub cache_creation_input_tokens: u64,
    /// Input tokens read from the prompt cache.
    pub cache_read_input_tokens: u64,
}

impl std::ops::AddAssign for Usage {
    fn add_assign(&mut self, other: Self) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation_input_tokens += other.cache_creation_input_tokens;
        self.cache_read_input_tokens += other.cache_read_input_tokens;
    }
}

/// A model's reply to a conversation.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    /// The text of the reply's text blocks, a line apart.
    pub text: String,
    /// The reply's content blocks as they came, which the next turn sends back unchanged.
    pub content: serde_json::Value,
    /// Why the model stopped, such as `end_turn` or `max_tokens`.
    pub stop_reason: String,
    /// What the request spent.
    pub usage: Usage,
}

/// One turn of a conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// What the harness said.
    User(String),
    /// What the model replied.
    Assistant(Reply),
}

/// A conversation as a model is asked to continue it.
#[derive(Debug, Clone, Copy)]
pub struct Conversation<'a> {
    /// The prompt the conversation is for.
    pub id: &'a str,
    /// The system prompt.
    pub system: &'a str,
    /// The turns so far, the last one the harness's.
    pub messages: &'a [Message],
}

/// Why a model gave no reply.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct ModelError(pub String);

/// Something that replies to a conversation.
pub trait Model {
    /// The model's next turn in `conversation`.
    ///
    /// # Errors
    ///
    /// [`ModelError`] when no reply comes.
    fn reply(&mut self, conversation: &Conversation<'_>) -> Result<Reply, ModelError>;
}
