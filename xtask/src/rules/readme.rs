//! Crate READMEs that keep an inventory keep it exact, in both directions: crates.io shows a
//! crate's README as its front page. Only which names an inventory holds is checked, and, for
//! `cairn synth`, which stage its prose says `--edition` starts at.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;
use syn::{UseTree, Visibility};

use super::{Finding, Rule};
use crate::markdown::{first_cell_spans, section};
use crate::repo::Repo;

pub const MODULES: Rule = Rule {
    name: "readme-modules",
    summary: "cairn-lang-core's module table names exactly its pub mods",
    check: check_modules,
};

pub const PUBLIC_API: Rule = Rule {
    name: "readme-public-api",
    summary: "each `## Public API` table names exactly the crate root's re-exports",
    check: check_public_api,
};

pub const SUBCOMMANDS: Rule = Rule {
    name: "readme-subcommands",
    summary: "cairn-lang-cli's subcommand table names exactly what `cairn --help` lists",
    check: check_subcommands,
};

pub const SYNTH_STAGES: Rule = Rule {
    name: "readme-synth-stages",
    summary: "cairn-lang-cli's `cairn synth` section names what `--stage` offers and where \
              `--edition` starts",
    check: check_synth_stages,
};

/// The crates whose README carries a `## Public API` table, listed so deleting one is seen.
const PUBLIC_API_CRATES: &[&str] = &["cairn-lang-formats", "cairn-lang-nbt"];

/// The spans in the first column of a README section's table, with the line each is on.
fn table_names(
    readme: &Path,
    text: &str,
    title: &str,
    findings: &mut Vec<Finding>,
) -> Vec<(usize, String)> {
    let Some(lines) = section(text, 2, |heading| heading == title) else {
        findings.push(Finding::new(
            readme,
            None,
            format!("no `## {title}` section"),
        ));
        return Vec::new();
    };
    let mut names = Vec::new();
    for (line, row) in lines {
        match first_cell_spans(row) {
            Ok(spans) => names.extend(
                spans
                    .into_iter()
                    .flatten()
                    .map(|span| (line, span.to_owned())),
            ),
            Err(error) => findings.push(Finding::new(readme, Some(line), error)),
        }
    }
    if names.is_empty() {
        findings.push(Finding::new(
            readme,
            None,
            format!(
                "`## {title}` has no table rows; a row is a line starting with `|` whose first \
                 cell holds code spans"
            ),
        ));
    }
    names
}

/// Report what one side holds and the other does not.
fn compare(
    readme: &Path,
    shipped: &BTreeSet<String>,
    named: &[(usize, String)],
    what: &str,
    findings: &mut Vec<Finding>,
) {
    let named_set: BTreeSet<&str> = named.iter().map(|(_, name)| name.as_str()).collect();
    for missing in shipped
        .iter()
        .filter(|name| !named_set.contains(name.as_str()))
    {
        findings.push(Finding::new(
            readme,
            None,
            format!("{what} `{missing}` ships but has no row"),
        ));
    }
    for (line, phantom) in named.iter().filter(|(_, name)| !shipped.contains(name)) {
        findings.push(Finding::new(
            readme,
            Some(*line),
            format!("the row names `{phantom}`, which is not a shipped {what}"),
        ));
    }
}

fn parse_rust(repo: &Repo, path: &Path) -> Result<syn::File, Finding> {
    let source = repo
        .read(path)
        .map_err(|error| Finding::new(path, None, error))?;
    syn::parse_file(&source)
        .map_err(|error| Finding::new(path, Some(error.span().start().line), error.to_string()))
}

fn check_modules(repo: &Repo) -> Vec<Finding> {
    let lib = Path::new("crates/cairn-lang-core/src/lib.rs");
    let readme = Path::new("crates/cairn-lang-core/README.md");
    let mut findings = Vec::new();
    let file = match parse_rust(repo, lib) {
        Ok(file) => file,
        Err(finding) => return vec![finding],
    };
    let modules: BTreeSet<String> = file
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module) if matches!(module.vis, Visibility::Public(_)) => {
                Some(module.ident.to_string())
            }
            _ => None,
        })
        .collect();
    match repo.read(readme) {
        Ok(text) => {
            let named = table_names(readme, &text, "Modules", &mut findings);
            compare(readme, &modules, &named, "module", &mut findings);
        }
        Err(error) => findings.push(Finding::new(readme, None, error)),
    }
    findings
}

/// Every path a `use` tree publishes, as `module::item`, or why it cannot be listed.
fn expand_use(tree: &UseTree, prefix: &mut Vec<String>, out: &mut Vec<Result<String, String>>) {
    match tree {
        UseTree::Path(path) => {
            let segment = path.ident.to_string();
            let pushed = !(prefix.is_empty() && (segment == "crate" || segment == "self"));
            if pushed {
                prefix.push(segment);
            }
            expand_use(&path.tree, prefix, out);
            if pushed {
                prefix.pop();
            }
        }
        UseTree::Name(name) if name.ident == "self" => out.push(Err(format!(
            "`{}::self` re-exports a module, not an item; the table lists items",
            prefix.join("::")
        ))),
        UseTree::Name(name) if prefix.is_empty() => out.push(Err(format!(
            "`{}` is re-exported with no module path for a row to name",
            name.ident
        ))),
        UseTree::Name(name) => out.push(Ok(format!("{}::{}", prefix.join("::"), name.ident))),
        UseTree::Rename(rename) => out.push(Err(format!(
            "`{} as {}` renames on re-export; decide how its row is spelled first",
            rename.ident, rename.rename
        ))),
        UseTree::Glob(_) => out.push(Err(format!(
            "`{}::*` cannot be listed item by item",
            prefix.join("::")
        ))),
        UseTree::Group(group) => {
            for tree in &group.items {
                expand_use(tree, prefix, out);
            }
        }
    }
}

fn check_public_api(repo: &Repo) -> Vec<Finding> {
    let mut findings = Vec::new();

    let tabled: BTreeSet<PathBuf> = repo
        .files()
        .iter()
        .filter(|path| {
            path.starts_with("crates")
                && path.file_name().is_some_and(|name| name == "README.md")
                && repo.read(path).is_ok_and(|text| {
                    section(&text, 2, |heading| heading == "Public API").is_some()
                })
        })
        .cloned()
        .collect();
    let listed: BTreeSet<PathBuf> = PUBLIC_API_CRATES
        .iter()
        .map(|name| Path::new("crates").join(name).join("README.md"))
        .collect();
    for unlisted in tabled.difference(&listed) {
        findings.push(Finding::new(
            unlisted,
            None,
            "has a `## Public API` table but its crate is not in xtask's PUBLIC_API_CRATES",
        ));
    }

    for name in PUBLIC_API_CRATES {
        let dir = Path::new("crates").join(name);
        let lib = dir.join("src/lib.rs");
        let readme = dir.join("README.md");
        let file = match parse_rust(repo, &lib) {
            Ok(file) => file,
            Err(finding) => {
                findings.push(finding);
                continue;
            }
        };
        let mut exported = BTreeSet::new();
        for item in &file.items {
            let syn::Item::Use(item) = item else {
                continue;
            };
            if !matches!(item.vis, Visibility::Public(_)) {
                continue;
            }
            let mut paths = Vec::new();
            expand_use(&item.tree, &mut Vec::new(), &mut paths);
            for path in paths {
                match path {
                    Ok(path) => {
                        exported.insert(path);
                    }
                    Err(error) => {
                        let line = item.use_token.span.start().line;
                        findings.push(Finding::new(&lib, Some(line), error));
                    }
                }
            }
        }
        match repo.read(&readme) {
            Ok(text) => {
                let named = table_names(&readme, &text, "Public API", &mut findings);
                compare(&readme, &exported, &named, "re-export", &mut findings);
            }
            Err(error) => findings.push(Finding::new(&readme, None, error)),
        }
    }
    findings
}

/// The subcommands clap lists under `Commands:`, without its own `help`.
fn listed_subcommands(help: &str) -> Option<BTreeSet<String>> {
    let block = help.split_once("\nCommands:\n")?.1.split("\n\n").next()?;
    Some(
        block
            .lines()
            .filter(|line| line.len() - line.trim_start().len() == 2)
            .filter_map(|line| line.split_whitespace().next())
            .filter(|name| *name != "help")
            .map(str::to_owned)
            .collect(),
    )
}

/// The subcommand a row's `cairn <name> …` span runs.
fn subcommand_of(invocation: &str) -> Option<&str> {
    let mut words = invocation.split_whitespace();
    (words.next() == Some("cairn"))
        .then(|| words.next())
        .flatten()
}

/// `cairn <subcommand…> --help`, run through cargo with colour off.
fn help_command(repo: &Repo, subcommand: &[&str]) -> Command {
    let mut command = repo.cargo();
    command
        .args([
            "run",
            "--quiet",
            "--locked",
            "--package",
            "cairn-lang-cli",
            "--bin",
            "cairn",
            "--",
        ])
        .args(subcommand)
        .arg("--help")
        // `CLICOLOR_FORCE` styles clap's help even into a pipe; `NO_COLOR` outranks it.
        .env("NO_COLOR", "1");
    command
}

/// What `cairn <subcommand…> --help` prints, or a finding against `readme` saying why it did not.
fn help_text(repo: &Repo, subcommand: &[&str], readme: &Path) -> Result<String, Finding> {
    let output = help_command(repo, subcommand)
        .output()
        .map_err(|error| Finding::new(readme, None, format!("cannot run cargo: {error}")))?;
    if !output.status.success() {
        let invocation = [&["cairn"], subcommand, &["--help"]].concat().join(" ");
        return Err(Finding::new(
            readme,
            None,
            format!(
                "`{invocation}` failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn check_subcommands(repo: &Repo) -> Vec<Finding> {
    let readme = Path::new("crates/cairn-lang-cli/README.md");
    let help = match help_text(repo, &[], readme) {
        Ok(help) => help,
        Err(finding) => return vec![finding],
    };
    let Some(offered) = listed_subcommands(&help) else {
        return vec![Finding::new(
            readme,
            None,
            "`cairn --help` printed no `Commands:` block",
        )];
    };

    let mut findings = Vec::new();
    let text = match repo.read(readme) {
        Ok(text) => text,
        Err(error) => return vec![Finding::new(readme, None, error)],
    };
    let mut named = Vec::new();
    for (line, invocation) in table_names(readme, &text, "Subcommands", &mut findings) {
        match subcommand_of(&invocation) {
            Some(name) => named.push((line, name.to_owned())),
            None => findings.push(Finding::new(
                readme,
                Some(line),
                format!("`{invocation}` is not an invocation; start the row with `cairn <name>`"),
            )),
        }
    }
    compare(readme, &offered, &named, "subcommand", &mut findings);
    findings
}

/// The lines clap prints under `option` in a long `--help`, each trimmed.
fn option_help<'a>(help: &'a str, option: &str) -> Option<Vec<&'a str>> {
    let indent = |line: &str| line.len() - line.trim_start().len();
    let mut lines = help.lines();
    let header = lines.find(|line| line.split_whitespace().next() == Some(option))?;
    let depth = indent(header);
    Some(
        lines
            .take_while(|line| line.trim().is_empty() || indent(line) > depth)
            .map(str::trim)
            .collect(),
    )
}

/// The values an option's long help lists under `Possible values:`, in its order.
fn possible_values(option_help: &[&str]) -> Vec<String> {
    option_help
        .iter()
        .skip_while(|line| **line != "Possible values:")
        .skip(1)
        .map_while(|line| line.strip_prefix("- "))
        .map(|item| {
            item.split_once(':')
                .map_or(item, |(value, _)| value)
                .to_owned()
        })
        .collect()
}

/// The stages `--edition`'s long help names in its "Required when `--stage …` is set".
fn edition_tagged(option_help: &[&str]) -> Option<Vec<String>> {
    let text = option_help.join(" ");
    let (_, rest) = text.split_once("Required when ")?;
    let (clause, _) = rest.split_once(" is set")?;
    clause
        .split('`')
        .skip(1)
        .step_by(2)
        .map(|span| span.strip_prefix("--stage ").map(str::to_owned))
        .collect::<Option<Vec<_>>>()
        .filter(|stages| !stages.is_empty())
}

/// `names` as code spans, comma-separated.
fn spans<S: AsRef<str>>(names: &[S]) -> String {
    names
        .iter()
        .map(|name| format!("`{}`", name.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The faults in `text`'s `cairn synth` section, given the `--stage` values in order and the
/// `tagged` ones that require `--edition`.
fn synth_section(readme: &Path, text: &str, stages: &[String], tagged: &[String]) -> Vec<Finding> {
    let Some(lines) = section(text, 2, |title| title.starts_with("`cairn synth`")) else {
        return vec![Finding::new(
            readme,
            None,
            "no `##` section whose title starts with `cairn synth`",
        )];
    };
    let mut findings = Vec::new();
    let (listed, starts) = synth_spans(readme, &lines, &mut findings);
    listed_stage_findings(readme, &listed, stages, &mut findings);
    edition_start_findings(readme, &starts, stages, tagged, &mut findings);
    findings
}

/// Names a README holds, each with the line it is on.
type LinedNames = Vec<(usize, String)>;

/// The section's code spans of one lowercase word outside a "from `…` onwards", each at its first
/// line, and the stage each "from `…` onwards" names, at its line.
fn synth_spans(
    readme: &Path,
    lines: &[(usize, &str)],
    findings: &mut Vec<Finding>,
) -> (LinedNames, LinedNames) {
    let onwards = Regex::new(r"from `([^`]*)` onwards").expect("valid regex");
    let is_stage_like = |span: &str| {
        span.starts_with(|c: char| c.is_ascii_lowercase())
            && span
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    let mut listed: LinedNames = Vec::new();
    let mut starts = Vec::new();
    for (line, row) in lines {
        if row.matches('`').count() % 2 != 0 {
            findings.push(Finding::new(
                readme,
                Some(*line),
                "unpaired backtick; the stages are read from this section's code spans",
            ));
            continue;
        }
        starts.extend(
            onwards
                .captures_iter(row)
                .map(|capture| (*line, capture[1].to_owned())),
        );
        let rest = onwards.replace_all(row, "");
        for span in rest.split('`').skip(1).step_by(2) {
            if is_stage_like(span) && !listed.iter().any(|(_, name)| name == span) {
                listed.push((*line, span.to_owned()));
            }
        }
    }
    (listed, starts)
}

/// Report each of `stages` that `listed` leaves out, each name in it that is not a stage, and a
/// list out of `stages`' order.
fn listed_stage_findings(
    readme: &Path,
    listed: &[(usize, String)],
    stages: &[String],
    findings: &mut Vec<Finding>,
) {
    for stage in stages {
        if !listed.iter().any(|(_, name)| name == stage) {
            findings.push(Finding::new(
                readme,
                None,
                format!(
                    "`--stage {stage}` ships but the `cairn synth` section does not name it; add \
                     `{stage}` to its list of stages"
                ),
            ));
        }
    }
    for (line, phantom) in listed.iter().filter(|(_, name)| !stages.contains(name)) {
        findings.push(Finding::new(
            readme,
            Some(*line),
            format!(
                "`{phantom}` is not a `--stage` value; a one-word code span in this section \
                 names a stage, and `cairn synth --help` lists {}",
                spans(stages)
            ),
        ));
    }
    let in_readme: Vec<&str> = listed
        .iter()
        .map(|(_, name)| name.as_str())
        .filter(|name| stages.iter().any(|stage| stage == name))
        .collect();
    let in_help: Vec<&str> = stages
        .iter()
        .map(String::as_str)
        .filter(|stage| in_readme.contains(stage))
        .collect();
    if in_readme != in_help {
        findings.push(Finding::new(
            readme,
            None,
            format!(
                "the section lists the stages as {}, but `--stage` offers them as {}, the order \
                 \"from … onwards\" reads against; list them in that order",
                spans(&in_readme),
                spans(&in_help)
            ),
        ));
    }
}

/// Report a section whose "from `…` onwards" is missing or names another stage than the first of
/// `tagged`, or `tagged` stages that are not the last of `stages`.
fn edition_start_findings(
    readme: &Path,
    starts: &[(usize, String)],
    stages: &[String],
    tagged: &[String],
    findings: &mut Vec<Finding>,
) {
    let first = match stages.iter().position(|stage| tagged.contains(stage)) {
        Some(start) if stages[start..] == *tagged => &stages[start],
        _ => {
            findings.push(Finding::new(
                readme,
                None,
                format!(
                    "`cairn synth --help` requires `--edition` for {}, which are not the last \
                     stages in `--stage`'s order ({}), so \"from … onwards\" cannot describe \
                     them; reword the section, and this rule with it",
                    spans(tagged),
                    spans(stages)
                ),
            ));
            return;
        }
    };
    if starts.is_empty() {
        findings.push(Finding::new(
            readme,
            None,
            format!(
                "the section does not say which stage `--edition` starts at; say it is required \
                 from `{first}` onwards"
            ),
        ));
    }
    for (line, start) in starts.iter().filter(|(_, start)| start != first) {
        findings.push(Finding::new(
            readme,
            Some(*line),
            format!(
                "\"from `{start}` onwards\" names `{start}`, but the first stage `cairn synth \
                 --help` requires `--edition` for is `{first}`; write \"from `{first}` onwards\""
            ),
        ));
    }
}

fn check_synth_stages(repo: &Repo) -> Vec<Finding> {
    let readme = Path::new("crates/cairn-lang-cli/README.md");
    let help = match help_text(repo, &["synth"], readme) {
        Ok(help) => help,
        Err(finding) => return vec![finding],
    };
    let stages = option_help(&help, "--stage")
        .map(|lines| possible_values(&lines))
        .unwrap_or_default();
    if stages.is_empty() {
        return vec![Finding::new(
            readme,
            None,
            "`cairn synth --help` lists no `Possible values:` under `--stage`",
        )];
    }
    let Some(tagged) = option_help(&help, "--edition").and_then(|lines| edition_tagged(&lines))
    else {
        return vec![Finding::new(
            readme,
            None,
            "`cairn synth --help` has no \"Required when `--stage …` is set\" under `--edition` \
             to read the stages that require it from",
        )];
    };
    match repo.read(readme) {
        Ok(text) => synth_section(readme, &text, &stages, &tagged),
        Err(error) => vec![Finding::new(readme, None, error)],
    }
}

#[cfg(test)]
mod tests;
