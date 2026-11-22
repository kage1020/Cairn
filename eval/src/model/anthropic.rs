//! The Anthropic Messages API, asked over HTTPS with the key in `ANTHROPIC_API_KEY`.
//!
//! A request carries the model, `max_tokens`, the system prompt as one cached block and the
//! conversation, and nothing else, so a run's replies depend on the model and what it is told.

use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use super::{Conversation, Message, Model, ModelError, Reply, Usage};

/// Where requests go.
pub const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";

/// The `anthropic-version` header every request sends.
pub const API_VERSION: &str = "2026-10-21";

/// The variable the key is read from.
pub const KEY_VARIABLE: &str = "ANTHROPIC_API_KEY";

/// How many times a request is sent again after a rate limit, an overload or a dropped connection.
const RETRIES: u32 = 6;

/// The longest wait before sending a request again.
const LONGEST_WAIT: Duration = Duration::from_secs(120);

/// A Messages API backend; it has no `Debug`, which would print the key.
pub struct Anthropic {
    agent: ureq::Agent,
    endpoint: String,
    key: String,
    model: String,
    max_tokens: u32,
    backoff: Duration,
    log: Box<dyn FnMut(&str)>,
}

fn agent(proxy: Option<ureq::Proxy>) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(600)))
        .proxy(proxy)
        .build()
        .into()
}

impl Anthropic {
    /// A backend asking `model` with `key`, through the proxy the environment names, if any.
    #[must_use]
    pub fn new(key: String, model: String, max_tokens: u32) -> Self {
        Self {
            agent: agent(ureq::Proxy::try_from_env()),
            endpoint: ENDPOINT.to_owned(),
            key,
            model,
            max_tokens,
            backoff: Duration::from_secs(2),
            log: Box::new(|line| eprintln!("{line}")),
        }
    }

    /// A backend asking `model` with the key [`KEY_VARIABLE`] holds.
    ///
    /// # Errors
    ///
    /// [`ModelError`] when the variable is unset or empty.
    pub fn from_env(model: String, max_tokens: u32) -> Result<Self, ModelError> {
        match std::env::var(KEY_VARIABLE) {
            Ok(key) if !key.trim().is_empty() => Ok(Self::new(key, model, max_tokens)),
            _ => Err(ModelError(format!(
                "{KEY_VARIABLE} is not set; set it to an Anthropic API key, or pass `--replay` to \
                 answer from recorded replies"
            ))),
        }
    }

    /// The same backend sending to `endpoint` directly, with no proxy: a local stand-in for the
    /// API, in tests.
    #[must_use]
    pub fn with_endpoint(self, endpoint: String) -> Self {
        Self {
            agent: agent(None),
            endpoint,
            ..self
        }
    }

    /// The same backend waiting `backoff` before its first retry, and twice as long before each
    /// one after it.
    #[must_use]
    pub fn with_backoff(self, backoff: Duration) -> Self {
        Self { backoff, ..self }
    }

    /// The same backend telling `log` each time it waits to send a request again, in place of
    /// standard error.
    #[must_use]
    pub fn with_log(self, log: impl FnMut(&str) + 'static) -> Self {
        Self {
            log: Box::new(log),
            ..self
        }
    }
}

/// The body of the request that asks `model` to continue `conversation`.
#[must_use]
pub fn request_body(model: &str, max_tokens: u32, conversation: &Conversation<'_>) -> Value {
    let messages: Vec<Value> = conversation
        .messages
        .iter()
        .map(|message| match message {
            Message::User(text) => json!({"role": "user", "content": text}),
            Message::Assistant(reply) => json!({"role": "assistant", "content": reply.content}),
        })
        .collect();
    json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": [{
            "type": "text",
            "text": conversation.system,
            "cache_control": {"type": "ephemeral"},
        }],
        "messages": messages,
    })
}

/// The reply a successful response's `body` carries.
///
/// # Errors
///
/// [`ModelError`] when the body is not a Messages API response, or lacks its `stop_reason` or
/// its input and output token counts.
pub fn parse_reply(body: &str) -> Result<Reply, ModelError> {
    let invalid = |what: &str| ModelError(format!("the API's response {what}: {body}"));
    let response: Value = serde_json::from_str(body).map_err(|_| invalid("is not JSON"))?;
    let content = response
        .get("content")
        .filter(|content| content.is_array())
        .ok_or_else(|| invalid("has no `content` array"))?
        .clone();
    let text = content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let stop_reason = response["stop_reason"]
        .as_str()
        .ok_or_else(|| invalid("has no `stop_reason`"))?
        .to_owned();
    let count = |key: &str| response["usage"][key].as_u64();
    let spent = |key: &str| count(key).ok_or_else(|| invalid(&format!("has no `usage.{key}`")));
    Ok(Reply {
        text,
        content,
        stop_reason,
        usage: Usage {
            input_tokens: spent("input_tokens")?,
            output_tokens: spent("output_tokens")?,
            cache_creation_input_tokens: count("cache_creation_input_tokens").unwrap_or_default(),
            cache_read_input_tokens: count("cache_read_input_tokens").unwrap_or_default(),
        },
    })
}

/// Whether a response with `status` is worth sending the request again for.
fn retryable(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504 | 529)
}

/// Whether a request that got no response may get one if sent again: a dropped or refused
/// connection or a timeout, never a TLS, proxy or request error.
fn transient(error: &ureq::Error) -> bool {
    matches!(
        error,
        ureq::Error::Io(_)
            | ureq::Error::Timeout(_)
            | ureq::Error::ConnectionFailed
            | ureq::Error::HostNotFound
            | ureq::Error::BodyStalled
    )
}

/// The `error.message` of an error response's `body`, or the body when it holds none.
fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|error| error["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| body.to_owned())
}

impl Anthropic {
    /// One round trip: the status, the `retry-after` header if any, and the body.
    fn send(&self, body: &str) -> Result<(u16, Option<String>, String), ureq::Error> {
        let mut response = self
            .agent
            .post(&self.endpoint)
            .header("x-api-key", &self.key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .send(body)?;
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .map(|value| value.trim().to_owned());
        let text = response.body_mut().read_to_string()?;
        Ok((response.status().as_u16(), retry_after, text))
    }
}

impl Model for Anthropic {
    fn reply(&mut self, conversation: &Conversation<'_>) -> Result<Reply, ModelError> {
        let body = request_body(&self.model, self.max_tokens, conversation).to_string();
        let mut backoff = self.backoff;
        let mut retry = 0;
        loop {
            let more = retry < RETRIES;
            let (wait, why) = match self.send(&body) {
                Ok((200, _, text)) => return parse_reply(&text),
                Ok((status, retry_after, text)) if retryable(status) && more => {
                    let wait = match retry_after.map(|value| (value.parse(), value)) {
                        None => backoff,
                        Some((Ok(seconds), _)) => Duration::from_secs(seconds),
                        Some((Err(_), value)) => {
                            (self.log)(&format!(
                                "the API's retry-after `{value}` is not a number of seconds, so \
                                 the harness waits as it does without one"
                            ));
                            backoff
                        }
                    };
                    (
                        wait,
                        format!("the API answered {status}: {}", error_message(&text)),
                    )
                }
                Ok((status, _, text)) => {
                    return Err(ModelError(format!("the API answered {status}: {text}")));
                }
                Err(error) if transient(&error) && more => {
                    (backoff, format!("no response from the API: {error}"))
                }
                Err(error) => {
                    return Err(ModelError(format!("no response from the API: {error}")));
                }
            };
            retry += 1;
            let wait = wait.min(LONGEST_WAIT);
            (self.log)(&format!(
                "{why}; asking again in {}s, retry {retry} of {RETRIES}",
                wait.as_secs()
            ));
            thread::sleep(wait);
            backoff *= 2;
        }
    }
}
