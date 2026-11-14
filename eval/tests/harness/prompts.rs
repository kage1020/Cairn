//! The prompt set: the one this checkout ships, and what a set must be to load.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use eval::prompts::{PromptSet, Topic};
use eval::run::prompt_set_hash;

fn set_of(files: &[(&str, &str)]) -> Result<PromptSet, String> {
    let dir = tempfile::tempdir().expect("a scratch directory");
    for (name, text) in files {
        fs::write(dir.path().join(name), text).expect("the prompt is written");
    }
    let prefix = format!("{}{}", dir.path().display(), std::path::MAIN_SEPARATOR);
    PromptSet::load(dir.path()).map_err(|error| error.to_string().replace(&prefix, "<dir>/"))
}

#[test]
fn the_shipped_set_covers_every_topic_with_generation_prompts_and_edits() {
    let set = PromptSet::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("prompts"))
        .expect("the shipped prompt set loads");
    let mut counts: BTreeMap<(Topic, bool), usize> = BTreeMap::new();
    for prompt in set.prompts() {
        *counts
            .entry((prompt.topic, prompt.base.is_some()))
            .or_default() += 1;
    }
    let expected: BTreeMap<(Topic, bool), usize> = [
        (Topic::Walls, 4, 1),
        (Topic::Openings, 4, 1),
        (Topic::Roofs, 4, 1),
        (Topic::Levels, 4, 1),
        (Topic::Members, 4, 1),
        (Topic::Themes, 4, 1),
        (Topic::DefRepeat, 4, 1),
        (Topic::Redstone, 4, 1),
        (Topic::ColumnsArches, 3, 2),
        (Topic::Interiors, 3, 2),
    ]
    .into_iter()
    .flat_map(|(topic, generation, edits)| [((topic, false), generation), ((topic, true), edits)])
    .collect();
    assert_eq!(counts, expected);
    let arched: Vec<_> = set
        .prompts()
        .iter()
        .filter(|prompt| prompt.prompt == "Make only the second-floor windows arched.")
        .map(|prompt| (prompt.topic, prompt.base.is_some()))
        .collect();
    assert_eq!(arched, [(Topic::ColumnsArches, true)]);
}

#[test]
fn an_edit_of_an_edit_is_refused() {
    let error = set_of(&[
        ("a.toml", "topic = \"walls\"\nprompt = \"A shed.\"\n"),
        (
            "b.toml",
            "topic = \"walls\"\nbase = \"a\"\nprompt = \"Taller.\"\n",
        ),
        (
            "c.toml",
            "topic = \"walls\"\nbase = \"b\"\nprompt = \"Wider.\"\n",
        ),
    ]);
    assert_eq!(
        error,
        Err("prompt `c` edits `b`, which is itself an edit; name a prompt with no `base`".into())
    );
}

#[test]
fn an_edit_of_a_prompt_the_set_lacks_is_refused() {
    let error = set_of(&[(
        "b.toml",
        "topic = \"walls\"\nbase = \"a\"\nprompt = \"Taller.\"\n",
    )]);
    assert_eq!(
        error,
        Err("prompt `b` edits `a`, which the set does not hold; name a prompt it holds".into())
    );
}

#[test]
fn a_file_named_other_than_an_id_is_refused() {
    let error = set_of(&[("Shed_1.toml", "topic = \"walls\"\nprompt = \"A shed.\"\n")]);
    assert_eq!(
        error,
        Err(
            "<dir>/Shed_1.toml: a prompt id is lowercase letters and digits in words joined by \
             `-`; rename the file"
                .into()
        )
    );
}

#[test]
fn an_empty_prompt_is_refused() {
    let error = set_of(&[("a.toml", "topic = \"walls\"\nprompt = \"  \"\n")]);
    assert_eq!(
        error,
        Err("prompt `a` is empty; write the request under `prompt`".into())
    );
}

#[test]
fn a_key_a_prompt_does_not_take_is_refused() {
    let error = set_of(&[(
        "a.toml",
        "topic = \"walls\"\nprompt = \"A shed.\"\nprogram = \"struct a\"\n",
    )])
    .expect_err("the key is unknown");
    assert!(
        error.starts_with("<dir>/a.toml: TOML parse error at line 3"),
        "{error}"
    );
    assert!(error.contains("unknown field `program`"), "{error}");
}

#[test]
fn selecting_an_edit_selects_its_base() {
    let set = set_of(&[
        ("a.toml", "topic = \"walls\"\nprompt = \"A shed.\"\n"),
        (
            "b.toml",
            "topic = \"walls\"\nbase = \"a\"\nprompt = \"Taller.\"\n",
        ),
        ("c.toml", "topic = \"roofs\"\nprompt = \"A roof.\"\n"),
    ])
    .expect("the set loads");
    let chosen = set
        .select(&["b".to_owned()])
        .expect("the prompt is in the set");
    let ids: Vec<&str> = chosen.prompts().iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(
        set.select(&["d".to_owned()])
            .map_err(|error| error.to_string()),
        Err("the set holds no prompt `d`".to_owned())
    );
}

#[test]
fn the_hash_follows_what_a_prompt_says_and_not_how_its_file_ends_lines() {
    let unix = set_of(&[(
        "a.toml",
        "topic = \"walls\"\nprompt = \"\"\"\nA shed.\nOf stone.\n\"\"\"\n",
    )]);
    let windows = set_of(&[(
        "a.toml",
        "topic = \"walls\"\r\nprompt = \"\"\"\r\nA shed.\r\nOf stone.\r\n\"\"\"\r\n",
    )]);
    let other = set_of(&[("a.toml", "topic = \"walls\"\nprompt = \"A barn.\"\n")]);
    let hash = |set: Result<PromptSet, String>| prompt_set_hash(&set.expect("the set loads"));
    let unix = hash(unix);
    assert_eq!(unix, hash(windows));
    assert_ne!(unix, hash(other));
}
