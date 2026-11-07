//! The Java writer names a palette entry's block and properties `id` and `properties` from the
//! same `DataVersion` the game-load check reads them by. Each side tests itself, so a value moved
//! on one side alone passes both and builds structures the check then reads differently.

use std::path::Path;

use regex::Regex;

use super::{Finding, Rule};
use crate::repo::{Repo, line_of};

pub const RULE: Rule = Rule {
    name: "state-keys",
    summary: "the Java writer and the game-load check switch palette keys at one DataVersion",
    check,
};

/// Each file that defines the switch, with the line that does.
const COPIES: &[(&str, &str)] = &[
    (
        "crates/cairn-lang-formats/src/java_structure.rs",
        r"(?m)^const LOWERCASE_STATE_KEYS_FROM: i32 = (\d+);$",
    ),
    (
        ".github/scripts/minecraft_load.py",
        r"(?m)^LOWERCASE_STATE_KEYS_FROM = (\d+)$",
    ),
];

fn check(repo: &Repo) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut texts = Vec::new();
    for (file, _) in COPIES {
        let path = Path::new(file);
        match repo.read(path) {
            Ok(text) => texts.push(text),
            Err(error) => findings.push(Finding::new(path, None, error)),
        }
    }
    if findings.is_empty() {
        findings = compare(&texts);
    }
    findings
}

/// The findings for `texts`, each the text of the [`COPIES`] file at the same index.
fn compare(texts: &[String]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut first: Option<(&str, &str)> = None;
    for ((file, pattern), text) in COPIES.iter().zip(texts) {
        let Some(found) = Regex::new(pattern).expect("valid regex").captures(text) else {
            findings.push(Finding::new(
                *file,
                None,
                "defines no `LOWERCASE_STATE_KEYS_FROM`",
            ));
            continue;
        };
        let value = found
            .get(1)
            .expect("the pattern captures the value")
            .as_str();
        match first {
            None => first = Some((file, value)),
            Some((other, expected)) if value != expected => findings.push(Finding::new(
                *file,
                Some(line_of(text, found.get(0).expect("a match").start())),
                format!(
                    "`LOWERCASE_STATE_KEYS_FROM` is {value}, but `{other}` switches at {expected}"
                ),
            )),
            Some(_) => {}
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(rust: &str, python: &str) -> Vec<String> {
        vec![
            format!("//! a module\n\nconst LOWERCASE_STATE_KEYS_FROM: i32 = {rust};\n"),
            format!("import os\n\nLOWERCASE_STATE_KEYS_FROM = {python}\n"),
        ]
    }

    fn rendered(findings: &[Finding]) -> Vec<String> {
        findings
            .iter()
            .map(|finding| finding.render("state-keys"))
            .collect()
    }

    #[test]
    fn one_value_on_both_sides_passes() {
        assert!(compare(&texts("5006", "5006")).is_empty());
    }

    #[test]
    fn a_value_moved_on_one_side_is_reported_where_it_moved() {
        assert_eq!(
            rendered(&compare(&texts("5006", "5007"))),
            [Finding::new(
                COPIES[1].0,
                Some(3),
                "`LOWERCASE_STATE_KEYS_FROM` is 5007, but \
                 `crates/cairn-lang-formats/src/java_structure.rs` switches at 5006",
            )
            .render("state-keys")],
        );
    }

    #[test]
    fn a_side_that_no_longer_defines_it_is_reported() {
        let mut texts = texts("5006", "5006");
        texts[1] = "LOWERCASE_STATE_KEYS_FROM = compute()\n".to_owned();
        assert_eq!(
            rendered(&compare(&texts)),
            [
                Finding::new(COPIES[1].0, None, "defines no `LOWERCASE_STATE_KEYS_FROM`")
                    .render("state-keys")
            ],
        );
    }
}
