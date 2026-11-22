//! The primary metrics of `spec/evaluation` "Primary metrics", measured on a program, on an edit
//! and on a whole run.

use std::collections::BTreeMap;

use cairn_lang_core::{LexError, Module, ParseError, TokenKind};
use serde::Serialize;
use serde_json::Value;

use crate::compiler::Cells;
use crate::json::canonical;
use crate::record::{
    AstDiff, EditStability, Metrics, NbtDiff, Outcome, PromptRecord, Rate, TokenEfficiency,
    TopicRates, Verdict,
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

/// The arrays of a node's own that hold the lines nested under it, which become nodes of their
/// own: a body's `requires` lines, its other lines, a member's children and a truth table's rows.
const NESTED: [&str; 4] = ["requires", "body", "children", "rows"];

/// The arrays of `key=value` arguments, whose order a node does not keep.
const ARGUMENTS: [&str; 5] = ["args", "selector", "attrs", "bindings", "set"];

/// `value` with every array of [`ARGUMENTS`] in it sorted by key.
fn sort_arguments(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, nested) in map.iter_mut() {
                if let Value::Array(arguments) = nested
                    && ARGUMENTS.contains(&key.as_str())
                {
                    arguments.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()));
                }
                sort_arguments(nested);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(sort_arguments),
        _ => {}
    }
}

fn flatten(mut value: Value, depth: usize, nodes: &mut Vec<String>) {
    let mut children = Vec::new();
    if let Value::Object(map) = &mut value {
        for key in NESTED {
            if let Some(Value::Array(nested)) = map.remove(key) {
                children.extend(nested);
            }
        }
        // A member's address counts its siblings, so it is not what its line writes.
        map.remove("address");
    }
    sort_arguments(&mut value);
    nodes.push(format!("{depth} {}", canonical(value)));
    for child in children {
        flatten(child, depth + 1, nodes);
    }
}

/// The AST nodes of `module`: each header, then each item and top-level `edit` line in source
/// order, and under each the lines nested in it, at its depth, with its arguments sorted by key.
///
/// # Panics
///
/// Never: an AST always serializes to JSON.
#[must_use]
pub fn ast_nodes(module: &Module) -> Vec<String> {
    fn value(node: &impl Serialize) -> Value {
        serde_json::to_value(node).expect("an AST serializes to JSON")
    }
    let mut lines: Vec<(usize, Value)> = module
        .items
        .iter()
        .map(|item| (item.span().start, value(item)))
        .chain(
            module
                .edits
                .iter()
                .map(|edit| (edit.span.start, value(edit))),
        )
        .collect();
    lines.sort_by_key(|(start, _)| *start);
    let mut nodes = Vec::new();
    for header in &module.headers {
        flatten(value(header), 0, &mut nodes);
    }
    for (_, line) in lines {
        flatten(line, 0, &mut nodes);
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

/// `part / whole`, with no value when `whole` is 0.
#[expect(
    clippy::cast_precision_loss,
    reason = "a count of nodes, cells, blocks, tokens or prompts is far below 2^52"
)]
pub(crate) fn ratio(part: u64, whole: u64) -> Option<f64> {
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

/// The NBT diff from `before` to `after`.
#[must_use]
pub fn nbt_diff(before: &Cells, after: &Cells) -> NbtDiff {
    let added = after
        .0
        .keys()
        .filter(|key| !before.0.contains_key(*key))
        .count();
    let cells = before.0.len() + added;
    let changed = before
        .0
        .iter()
        .filter(|(key, block)| after.0.get(*key) != Some(*block))
        .count()
        + added;
    let (cells, changed) = (cells as u64, changed as u64);
    NbtDiff {
        cells,
        changed,
        ratio: ratio(changed, cells).unwrap_or_default(),
    }
}

/// The mean of `values`, with no value when there are none.
#[expect(
    clippy::cast_precision_loss,
    reason = "a count of edits is far below 2^52"
)]
pub(crate) fn mean(values: impl IntoIterator<Item = f64>) -> Option<f64> {
    let (sum, count) = values
        .into_iter()
        .fold((0.0, 0_u64), |(sum, count), value| (sum + value, count + 1));
    (count > 0).then(|| sum / count as f64)
}

/// The prompts of `prompts` that `counted` counts, over the ones `cut` does not leave out.
fn rate(
    prompts: &[&PromptRecord],
    counted: impl Fn(&PromptRecord) -> bool,
    cut: impl Fn(&PromptRecord) -> bool,
) -> Rate {
    let cut_short = prompts.iter().filter(|prompt| cut(prompt)).count() as u64;
    let of = prompts.len() as u64 - cut_short;
    let count = prompts.iter().filter(|prompt| counted(prompt)).count() as u64;
    Rate {
        count,
        of,
        cut_short,
        rate: ratio(count, of),
    }
}

/// The zero-shot compile rate of `prompts`.
fn zero_shot(prompts: &[&PromptRecord]) -> Rate {
    let first = |prompt: &PromptRecord| prompt.attempts.first().map(|attempt| attempt.verdict);
    rate(
        prompts,
        |prompt| first(prompt) == Some(Verdict::Pass),
        |prompt| first(prompt).is_some_and(Verdict::cuts_short),
    )
}

/// The fix convergence rate of `prompts`.
fn convergence(prompts: &[&PromptRecord]) -> Rate {
    rate(
        prompts,
        |prompt| prompt.outcome.build().is_some(),
        |prompt| prompt.outcome == Outcome::CutShort,
    )
}

/// The run's metrics over its prompts' conversations.
#[must_use]
pub fn summarize(prompts: &[PromptRecord]) -> Metrics {
    let generation: Vec<&PromptRecord> = prompts.iter().filter(|p| p.base.is_none()).collect();
    let edits: Vec<&PromptRecord> = prompts.iter().filter(|p| p.base.is_some()).collect();

    let builds: Vec<_> = generation
        .iter()
        .filter_map(|p| p.outcome.build())
        .collect();
    let blocks: u64 = builds.iter().map(|build| build.blocks).sum();
    let tokens: u64 = builds.iter().map(|build| build.tokens).sum();

    let diffs: Vec<_> = edits.iter().filter_map(|p| p.outcome.edit()).collect();

    let mut by_topic: BTreeMap<_, Vec<&PromptRecord>> = BTreeMap::new();
    for prompt in &generation {
        by_topic.entry(prompt.topic).or_default().push(prompt);
    }
    let topics = by_topic
        .into_iter()
        .map(|(topic, prompts)| {
            (
                topic,
                TopicRates {
                    zero_shot_compile_rate: zero_shot(&prompts),
                    fix_convergence_rate: convergence(&prompts),
                },
            )
        })
        .collect();

    let mut usage = crate::model::Usage::default();
    let mut requests = 0;
    for attempt in prompts.iter().flat_map(|prompt| &prompt.attempts) {
        usage += attempt.usage;
        requests += 1;
    }

    Metrics {
        zero_shot_compile_rate: zero_shot(&generation),
        fix_convergence_rate: convergence(&generation),
        token_efficiency: TokenEfficiency {
            programs: builds.len() as u64,
            blocks,
            tokens,
            ratio: ratio(blocks, tokens),
        },
        edit_stability: EditStability {
            measured: diffs.len() as u64,
            of: edits.len() as u64,
            ast: mean(diffs.iter().map(|diff| diff.ast.ratio)),
            nbt: mean(diffs.iter().map(|diff| diff.nbt.ratio)),
        },
        topics,
        requests,
        usage,
    }
}
