//! Crate READMEs that keep an inventory keep it exact, in both directions: crates.io shows a
//! crate's README as its front page. Only which names an inventory holds is checked, and, for
//! `cairn synth`, that its stages follow `--stage`'s order and that its prose names the stage
//! `--edition` is required from.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;
use syn::{UseTree, Visibility};

use super::{Finding, Rule};
use crate::markdown::{code_spans, first_cell_spans, section};
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
    summary: "cairn-lang-cli's `cairn synth` section lists what `--stage` offers, in its order, \
              and where `--edition` starts",
    check: check_synth_stages,
};

/// The crates whose README carries a `## Public API` table, listed so deleting one is seen.
const PUBLIC_API_CRATES: &[&str] = &["cairn-lang-formats", "cairn-lang-nbt"];

/// The README that holds `cairn`'s subcommand table and its `cairn synth` section.
const CLI_README: &str = "crates/cairn-lang-cli/README.md";

/// The source whose clap attributes print `cairn synth --help`.
const CLI_SOURCE: &str = "crates/cairn-lang-cli/src/cli.rs";

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

/// How far `line` is indented.
fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The subcommands clap lists under `Commands:`, without its own `help`.
fn listed_subcommands(help: &str) -> Option<BTreeSet<String>> {
    let block = help.split_once("\nCommands:\n")?.1.split("\n\n").next()?;
    Some(
        block
            .lines()
            .filter(|line| indent(line) == 2)
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
    let readme = Path::new(CLI_README);
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

/// The lines clap prints under `option` in a long `--help`, or `None` when it has no such option.
fn option_help<'a>(help: &'a str, option: &str) -> Option<Vec<&'a str>> {
    let header = Regex::new(&format!(
        r"^(\s*(?:-[^\s,]+, )?){}(?:\s|$)",
        regex::escape(option)
    ))
    .expect("valid regex");
    let mut lines = help.lines();
    // clap starts each long flag at one column, short flag or not: the next header is no deeper.
    let column = lines.find_map(|line| header.captures(line).map(|found| found[1].len()))?;
    Some(
        lines
            .take_while(|line| line.trim().is_empty() || indent(line) > column)
            .collect(),
    )
}

/// The values `option`'s long help lists under `Possible values:`, in its order, or why
/// `cairn synth --help` gives none to read.
fn possible_values(option: &str, lines: &[&str]) -> Result<Vec<String>, String> {
    let no_values = || {
        format!(
            "`cairn synth --help` lists no values under `{option}`; restore its \
             `Possible values:` list in `{CLI_SOURCE}`, or update this rule"
        )
    };
    let Some(start) = lines
        .iter()
        .position(|line| line.trim() == "Possible values:")
    else {
        if lines.iter().any(|line| line.contains("[possible values: ")) {
            return Err(format!(
                "`cairn synth --help` prints `{option}`'s values inline, as \
                 `[possible values: …]`, which clap does when no value has help; give each value \
                 its help again in `{CLI_SOURCE}`, or update this rule"
            ));
        }
        return Err(no_values());
    };
    let depth = indent(lines[start]);
    let mut values = Vec::new();
    for line in lines[start + 1..]
        .iter()
        .take_while(|line| !line.trim().is_empty())
    {
        // A value's help that wraps carries on deeper than its `- `.
        if indent(line) > depth {
            continue;
        }
        let item = line.trim_start().strip_prefix("- ").ok_or_else(|| {
            format!(
                "`cairn synth --help` has {:?} among `{option}`'s possible values, which is \
                 neither a `- <value>: <help>` line nor one wrapped from it; restore that shape \
                 in `{CLI_SOURCE}`, or update this rule",
                line.trim()
            )
        })?;
        values.push(
            item.split_once(':')
                .map_or(item, |(value, _)| value)
                .to_owned(),
        );
    }
    if values.is_empty() {
        return Err(no_values());
    }
    Ok(values)
}

/// The stages `--edition`'s long help names in its "Required when `--stage …` is set", or why
/// `cairn synth --help` gives none to read.
fn edition_tagged(lines: &[&str]) -> Result<Vec<String>, String> {
    let text = lines
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join(" ");
    let Some((clause, _)) = text
        .split_once("Required when ")
        .and_then(|(_, rest)| rest.split_once(" is set"))
    else {
        return Err(format!(
            "`cairn synth --help` has no \"Required when … is set\" under `--edition` to read the \
             stages that require it from; restore that sentence in `{CLI_SOURCE}`, or update \
             this rule"
        ));
    };
    let tagged = code_spans(clause)
        .map(|span| {
            span.strip_prefix("--stage ")
                .map(str::to_owned)
                .ok_or_else(|| {
                    format!(
                        "`--edition`'s \"Required when … is set\" in `cairn synth --help` names \
                         `{span}`, which is not a `--stage <stage>`; write each stage that \
                         requires `--edition` as one in `{CLI_SOURCE}`, or update this rule"
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if tagged.is_empty() {
        return Err(format!(
            "`--edition`'s \"Required when … is set\" in `cairn synth --help` names no \
             `--stage <stage>`; name each stage that requires `--edition` as one in \
             `{CLI_SOURCE}`, or update this rule"
        ));
    }
    Ok(tagged)
}

/// The `--stage` values in `cairn synth --help`'s order and the ones it requires `--edition` for,
/// or what in the help keeps them from being read.
fn synth_help(help: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let no_option = |option: &str| {
        format!(
            "`cairn synth --help` lists no `{option}` option; restore it in `{CLI_SOURCE}`, or \
             update this rule"
        )
    };
    let stage = option_help(help, "--stage").ok_or_else(|| no_option("--stage"))?;
    let stages = possible_values("--stage", &stage)?;
    let edition = option_help(help, "--edition").ok_or_else(|| no_option("--edition"))?;
    Ok((stages, edition_tagged(&edition)?))
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
    let unpaired: Vec<Finding> = lines
        .iter()
        .filter(|(_, row)| row.matches('`').count() % 2 != 0)
        .map(|&(line, _)| {
            Finding::new(
                readme,
                Some(line),
                "unpaired backtick; close the code span on this line, since the stages are read \
                 from this section's code spans",
            )
        })
        .collect();
    if !unpaired.is_empty() {
        return unpaired;
    }
    let mut findings = Vec::new();
    match stage_list(&lines, stages) {
        Some((line, listed)) => listed_stage_findings(readme, line, &listed, stages, &mut findings),
        None => findings.push(Finding::new(
            readme,
            None,
            format!(
                "the section has no comma-separated list of the stages; list them as \
                 `cairn synth --help` does: {}",
                spans(stages)
            ),
        )),
    }
    edition_start_findings(readme, &lines, stages, tagged, &mut findings);
    findings
}

/// The section's comma-separated run of one-word code spans that names the most `--stage` values,
/// with its line.
fn stage_list<'a>(lines: &[(usize, &'a str)], stages: &[String]) -> Option<(usize, Vec<&'a str>)> {
    let span = "`[a-z][a-z0-9-]*`";
    let run =
        Regex::new(&format!("(?:{span}, )+(?:{span},? (?:or|and) )?{span}")).expect("valid regex");
    let stage_count = |spans: &[&str]| {
        spans
            .iter()
            .filter(|span| stages.iter().any(|stage| stage == *span))
            .count()
    };
    let mut best: Option<(usize, Vec<&str>)> = None;
    for &(line, row) in lines {
        for found in run.find_iter(row) {
            let listed: Vec<&str> = code_spans(found.as_str()).collect();
            if stage_count(&listed) > best.as_ref().map_or(0, |(_, list)| stage_count(list)) {
                best = Some((line, listed));
            }
        }
    }
    best
}

/// Report each of `stages` that the section's list of stages, on `line`, leaves out, each name in
/// it that is not a stage, and a list out of `stages`' order.
fn listed_stage_findings(
    readme: &Path,
    line: usize,
    listed: &[&str],
    stages: &[String],
    findings: &mut Vec<Finding>,
) {
    let is_stage = |name: &str| stages.iter().any(|stage| stage == name);
    for stage in stages
        .iter()
        .filter(|stage| !listed.contains(&stage.as_str()))
    {
        findings.push(Finding::new(
            readme,
            Some(line),
            format!(
                "`--stage {stage}` ships but the `cairn synth` section does not list it among its \
                 stages; add `{stage}` to that list"
            ),
        ));
    }
    for phantom in listed.iter().filter(|name| !is_stage(name)) {
        findings.push(Finding::new(
            readme,
            Some(line),
            format!(
                "`{phantom}` is in the section's list of stages but is not a `--stage` value; \
                 `cairn synth --help` lists {}, so drop it or name one of those",
                spans(stages)
            ),
        ));
    }
    let in_readme: Vec<&str> = listed
        .iter()
        .copied()
        .filter(|name| is_stage(name))
        .collect();
    let in_help: Vec<&str> = stages
        .iter()
        .map(String::as_str)
        .filter(|stage| in_readme.contains(stage))
        .collect();
    if in_readme != in_help {
        findings.push(Finding::new(
            readme,
            Some(line),
            format!(
                "the section lists the stages as {}, but `--stage` offers them as {}, the order \
                 \"from … onwards\" reads against; list them in that order",
                spans(&in_readme),
                spans(&in_help)
            ),
        ));
    }
}

/// Report `tagged` stages that are not the last of `stages`, in its order, or a section that does
/// not say `--edition` is required from the first of them onwards.
fn edition_start_findings(
    readme: &Path,
    lines: &[(usize, &str)],
    stages: &[String],
    tagged: &[String],
    findings: &mut Vec<Finding>,
) {
    let Some(first) = tagged.first().filter(|_| stages.ends_with(tagged)) else {
        findings.push(Finding::new(readme, None, not_the_last(stages, tagged)));
        return;
    };
    let onwards =
        Regex::new(r"`--edition` is required from `([^`]*)` onwards").expect("valid regex");
    let starts: Vec<(usize, &str)> = lines
        .iter()
        .flat_map(|&(line, row)| {
            onwards
                .captures_iter(row)
                .filter_map(move |found| Some((line, found.get(1)?.as_str())))
        })
        .collect();
    if starts.is_empty() {
        findings.push(Finding::new(
            readme,
            None,
            format!(
                "the section does not say which stage `--edition` starts at; write \"`--edition` \
                 is required from `{first}` onwards\""
            ),
        ));
    }
    for (line, start) in starts
        .into_iter()
        .filter(|&(_, start)| start != first.as_str())
    {
        findings.push(Finding::new(
            readme,
            Some(line),
            format!(
                "\"from `{start}` onwards\" names `{start}`, but the first stage `cairn synth \
                 --help` requires `--edition` for is `{first}`; write \"from `{first}` onwards\""
            ),
        ));
    }
}

/// Why `tagged`, which `stages` does not end with, cannot be read as "from … onwards".
fn not_the_last(stages: &[String], tagged: &[String]) -> String {
    let last = &stages[stages.len().saturating_sub(tagged.len())..];
    if last.iter().collect::<BTreeSet<_>>() == tagged.iter().collect::<BTreeSet<_>>() {
        format!(
            "`cairn synth --help` names the stages that require `--edition` as {}, but `--stage` \
             orders them {}; name them in that order in `{CLI_SOURCE}`",
            spans(tagged),
            spans(last)
        )
    } else {
        format!(
            "the stages `cairn synth --help` requires `--edition` for, {}, do not run to the end \
             of `--stage`'s order ({}), so \"from … onwards\" cannot describe them; reword the \
             section, and this rule with it",
            spans(tagged),
            spans(stages)
        )
    }
}

fn check_synth_stages(repo: &Repo) -> Vec<Finding> {
    let readme = Path::new(CLI_README);
    let help = match help_text(repo, &["synth"], readme) {
        Ok(help) => help,
        Err(finding) => return vec![finding],
    };
    let (stages, tagged) = match synth_help(&help) {
        Ok(read) => read,
        Err(fault) => return vec![Finding::new(readme, None, fault)],
    };
    match repo.read(readme) {
        Ok(text) => synth_section(readme, &text, &stages, &tagged),
        Err(error) => vec![Finding::new(readme, None, error)],
    }
}

#[cfg(test)]
mod tests;
