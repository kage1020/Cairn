//! The primary metrics of `spec/evaluation` "Primary metrics", measured on a program, on an edit
//! and on a whole run.

use std::collections::BTreeMap;

use cairn_lang_core::{LexError, Module, ParseError, TokenKind};
use serde_json::Value;

use crate::compiler::Cells;
use crate::json::canonical;
use crate::record::{
    AstDiff, BlockDiff, EditStability, Metrics, Outcome, PromptRecord, Rate, TokenEfficiency,
    TopicCounts,
};

/// The DSL tokens of `program`: the tokens Cairn's lexer reads, with no end of line, indent or
/// dedent among them.
///
/// # Errors
///
/// [`LexError`] when `program` does not lex.
pub fn tokens(program: &str) -> Result<u64, LexError> {
    let tokens = cairn_lang_core::lex(program)?;
    Ok(tokens
        .iter()
        .filter(|token| {
            !matches!(
                token.kind,
                TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent
            )
        })
        .count() as u64)
}

/// The objects of a node's own that hold its children, which become nodes of their own.
const NESTED: [&str; 2] = ["body", "children"];

/// The arrays of `key=value` arguments, whose order a node does not keep.
const ARGUMENTS: [&str; 2] = ["args", "selector"];

fn flatten(mut value: Value, depth: usize, nodes: &mut Vec<String>) {
    let mut children = Vec::new();
    if let Value::Object(map) = &mut value {
        for key in NESTED {
            if let Some(Value::Array(nested)) = map.remove(key) {
                children.extend(nested);
            }
        }
        for key in ARGUMENTS {
            if let Some(Value::Array(arguments)) = map.get_mut(key) {
                arguments.sort_by_key(|argument| argument["key"].as_str().map(str::to_owned));
            }
        }
    }
    nodes.push(format!("{depth} {}", canonical(&value)));
    for child in children {
        flatten(child, depth + 1, nodes);
    }
}

/// The AST nodes of `module` in source order: each header, each item with its arguments, and
/// each statement of a body, at its depth, with its arguments sorted by key.
///
/// # Panics
///
/// Never: an AST always serializes to JSON.
#[must_use]
pub fn ast_nodes(module: &Module) -> Vec<String> {
    let value = serde_json::to_value(module).expect("an AST serializes to JSON");
    let mut nodes = Vec::new();
    for key in ["headers", "items"] {
        for node in value[key].as_array().into_iter().flatten() {
            flatten(node.clone(), 0, &mut nodes);
        }
    }
    nodes
}

/// The length of the longest common subsequence of `a` and `b`.
fn common(a: &[String], b: &[String]) -> usize {
    let mut row = vec![0; b.len() + 1];
    for left in a {
        let mut diagonal = 0;
        for (j, right) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if left == right {
                diagonal + 1
            } else {
                above.max(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a count of nodes, cells or prompts is far below 2^52"
)]
fn ratio(part: u64, whole: u64) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

/// The AST diff from `before` to `after`.
///
/// # Errors
///
/// [`ParseError`] when either program does not parse.
pub fn ast_diff(before: &str, after: &str) -> Result<AstDiff, ParseError> {
    let before = ast_nodes(&cairn_lang_core::parse(before)?);
    let after = ast_nodes(&cairn_lang_core::parse(after)?);
    let kept = common(&before, &after);
    let (removed, added) = ((before.len() - kept) as u64, (after.len() - kept) as u64);
    let (before, after) = (before.len() as u64, after.len() as u64);
    Ok(AstDiff {
        before,
        after,
        removed,
        added,
        ratio: ratio(removed + added, before + after).unwrap_or_default(),
    })
}

/// The block diff from `before` to `after`.
#[must_use]
pub fn block_diff(before: &Cells, after: &Cells) -> BlockDiff {
    let cells = before.0.len()
        + after
            .0
            .keys()
            .filter(|key| !before.0.contains_key(*key))
            .count();
    let changed = before
        .0
        .iter()
        .filter(|(key, block)| after.0.get(*key) != Some(*block))
        .count()
        + after
            .0
            .keys()
            .filter(|key| !before.0.contains_key(*key))
            .count();
    let (cells, changed) = (cells as u64, changed as u64);
    BlockDiff {
        cells,
        changed,
        ratio: ratio(changed, cells).unwrap_or_default(),
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a count of edits is far below 2^52"
)]
fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

/// The run's metrics over its prompts' conversations.
#[must_use]
pub fn summarize(prompts: &[PromptRecord]) -> Metrics {
    let generation: Vec<&PromptRecord> = prompts.iter().filter(|p| p.base.is_none()).collect();
    let edits: Vec<&PromptRecord> = prompts.iter().filter(|p| p.base.is_some()).collect();
    let zero_shot =
        |prompt: &&PromptRecord| prompt.outcome == Outcome::Passed && prompt.attempts.len() == 1;
    let converged = |prompt: &&PromptRecord| prompt.outcome == Outcome::Passed;
    let count = |filter: &dyn Fn(&&PromptRecord) -> bool| {
        generation.iter().filter(|prompt| filter(prompt)).count() as u64
    };
    let of = generation.len() as u64;
    let rate = |passed: u64| Rate {
        passed,
        of,
        rate: ratio(passed, of),
    };

    let builds: Vec<_> = generation.iter().filter_map(|p| p.build.as_ref()).collect();
    let blocks: u64 = builds.iter().map(|build| build.blocks).sum();
    let tokens: u64 = builds.iter().map(|build| build.tokens).sum();

    let diffs: Vec<_> = edits.iter().filter_map(|p| p.edit.as_ref()).collect();
    let ast: Vec<f64> = diffs.iter().map(|diff| diff.ast.ratio).collect();
    let changed: Vec<f64> = diffs.iter().map(|diff| diff.blocks.ratio).collect();

    let mut topics: BTreeMap<_, TopicCounts> = BTreeMap::new();
    for prompt in &generation {
        let counts = topics.entry(prompt.topic).or_insert(TopicCounts {
            prompts: 0,
            zero_shot: 0,
            converged: 0,
        });
        counts.prompts += 1;
        counts.zero_shot += u64::from(zero_shot(prompt));
        counts.converged += u64::from(converged(prompt));
    }

    let mut usage = crate::model::Usage::default();
    let mut requests = 0;
    for attempt in prompts.iter().flat_map(|prompt| &prompt.attempts) {
        usage += attempt.usage;
        requests += 1;
    }

    Metrics {
        zero_shot_compile_rate: rate(count(&zero_shot)),
        fix_convergence_rate: rate(count(&converged)),
        token_efficiency: TokenEfficiency {
            programs: builds.len() as u64,
            blocks,
            tokens,
            ratio: ratio(blocks, tokens),
        },
        edit_stability: EditStability {
            measured: diffs.len() as u64,
            of: edits.len() as u64,
            ast: mean(&ast),
            blocks: mean(&changed),
        },
        topics,
        requests,
        usage,
    }
}
