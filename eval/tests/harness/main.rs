//! The evaluation harness, tested offline: every model here is a replay or a local stand-in for
//! the Messages API, so no test makes a request.

mod anthropic;
mod cli;
mod common;
mod compare;
mod compiler;
mod conversation;
mod metrics;
mod prompts;
mod replay;
mod run;
