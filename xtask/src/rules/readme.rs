//! Crate READMEs that keep an inventory keep it exact, in both directions.
//!
//! crates.io shows a crate's README as its front page, and a table nothing compares with the
//! code drifts one merge at a time. Three inventories are held:
//!
//! - `cairn-lang-core`'s `## Modules` table names exactly its `pub mod`s;
//! - the `## Public API` tables (exactly the crates in `PUBLIC_API_CRATES`) name exactly the
//!   crate root's `pub use` re-exports, each as its full `module::item` path;
//! - `cairn-lang-cli`'s `## Subcommands` table names exactly what `cairn --help` lists.
//!
//! What a row says is not checked, only which names the table holds.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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

/// The crates whose README carries a `## Public API` table. Listed rather than discovered, so
/// deleting the heading cannot switch the check off unnoticed.
const PUBLIC_API_CRATES: &[&str] = &["cairn-lang-formats", "cairn-lang-nbt"];

/// The spans in the first column of a README section's table, with the line each is on.
fn table_names(
    rule: &'static str,
    readme: &Path,
    text: &str,
    title: &str,
    findings: &mut Vec<Finding>,
) -> Vec<(usize, String)> {
    let Some(lines) = section(text, 2, |heading| heading == title) else {
        findings.push(Finding::new(
            rule,
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
            Err(error) => findings.push(Finding::new(rule, readme, Some(line), error)),
        }
    }
    if names.is_empty() {
        findings.push(Finding::new(
            rule,
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
    rule: &'static str,
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
            rule,
            readme,
            None,
            format!("{what} `{missing}` ships but has no row"),
        ));
    }
    for (line, phantom) in named.iter().filter(|(_, name)| !shipped.contains(name)) {
        findings.push(Finding::new(
            rule,
            readme,
            Some(*line),
            format!("the row names `{phantom}`, which is not a shipped {what}"),
        ));
    }
}

fn parse_rust(rule: &'static str, repo: &Repo, path: &Path) -> Result<syn::File, Finding> {
    let source = repo
        .read(path)
        .map_err(|error| Finding::new(rule, path, None, error))?;
    syn::parse_file(&source).map_err(|error| {
        Finding::new(
            rule,
            path,
            Some(error.span().start().line),
            error.to_string(),
        )
    })
}

fn check_modules(repo: &Repo) -> Vec<Finding> {
    let rule = MODULES.name;
    let lib = Path::new("crates/cairn-lang-core/src/lib.rs");
    let readme = Path::new("crates/cairn-lang-core/README.md");
    let mut findings = Vec::new();
    let file = match parse_rust(rule, repo, lib) {
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
            let named = table_names(rule, readme, &text, "Modules", &mut findings);
            compare(rule, readme, &modules, &named, "module", &mut findings);
        }
        Err(error) => findings.push(Finding::new(rule, readme, None, error)),
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
    let rule = PUBLIC_API.name;
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
            rule,
            unlisted,
            None,
            "has a `## Public API` table but its crate is not in xtask's PUBLIC_API_CRATES",
        ));
    }

    for name in PUBLIC_API_CRATES {
        let dir = Path::new("crates").join(name);
        let lib = dir.join("src/lib.rs");
        let readme = dir.join("README.md");
        let file = match parse_rust(rule, repo, &lib) {
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
                        findings.push(Finding::new(rule, &lib, Some(line), error));
                    }
                }
            }
        }
        match repo.read(&readme) {
            Ok(text) => {
                let named = table_names(rule, &readme, &text, "Public API", &mut findings);
                compare(rule, &readme, &exported, &named, "re-export", &mut findings);
            }
            Err(error) => findings.push(Finding::new(rule, &readme, None, error)),
        }
    }
    findings
}

/// The subcommands clap lists under `Commands:`, one per line at a two-space indent, without
/// clap's own `help`.
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

fn check_subcommands(repo: &Repo) -> Vec<Finding> {
    let rule = SUBCOMMANDS.name;
    let readme = Path::new("crates/cairn-lang-cli/README.md");
    let output = repo
        .cargo()
        .args([
            "run",
            "--quiet",
            "--locked",
            "--package",
            "cairn-lang-cli",
            "--bin",
            "cairn",
            "--",
            "--help",
        ])
        .output();
    let help = match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        Ok(output) => {
            return vec![Finding::new(
                rule,
                readme,
                None,
                format!(
                    "`cairn --help` failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            )];
        }
        Err(error) => {
            return vec![Finding::new(
                rule,
                readme,
                None,
                format!("cannot run cargo: {error}"),
            )];
        }
    };
    let Some(offered) = listed_subcommands(&help) else {
        return vec![Finding::new(
            rule,
            readme,
            None,
            "`cairn --help` printed no `Commands:` block",
        )];
    };

    let mut findings = Vec::new();
    let text = match repo.read(readme) {
        Ok(text) => text,
        Err(error) => return vec![Finding::new(rule, readme, None, error)],
    };
    // A row's first span is an invocation, `cairn <name> …`; the name is its second word.
    let named: Vec<(usize, String)> =
        table_names(rule, readme, &text, "Subcommands", &mut findings)
            .into_iter()
            .filter_map(|(line, invocation)| {
                let mut words = invocation.split_whitespace();
                (words.next() == Some("cairn"))
                    .then(|| words.next().map(|name| (line, name.to_owned())))
                    .flatten()
            })
            .collect();
    compare(rule, readme, &offered, &named, "subcommand", &mut findings);
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exported(source: &str) -> Vec<Result<String, String>> {
        let file = syn::parse_file(source).unwrap();
        let mut paths = Vec::new();
        for item in file.items {
            if let syn::Item::Use(item) = item
                && matches!(item.vis, Visibility::Public(_))
            {
                expand_use(&item.tree, &mut Vec::new(), &mut paths);
            }
        }
        paths
    }

    #[test]
    fn re_exports_expand_to_full_paths() {
        let source = "\
pub use tag::{Compound, List};
pub use crate::registry::{load::PackSource, PackEdition};
pub use self::bedrock::write;
pub(crate) use hidden::Item;
use private::Item;
";
        let paths: Vec<_> = exported(source).into_iter().map(Result::unwrap).collect();
        assert_eq!(
            paths,
            [
                "tag::Compound",
                "tag::List",
                "registry::load::PackSource",
                "registry::PackEdition",
                "bedrock::write",
            ]
        );
    }

    #[test]
    fn shapes_a_row_cannot_name_are_refused() {
        for source in [
            "pub use tag::*;",
            "pub use tag::Tag as Tree;",
            "pub use registry::{self, PackEdition};",
            "pub use crate::Tag;",
        ] {
            assert!(
                exported(source).iter().any(Result::is_err),
                "{source} was accepted"
            );
        }
    }

    #[test]
    fn clap_help_lists_its_subcommands() {
        let help = "Usage: cairn <COMMAND>\n\nCommands:\n  parse  Lex and parse\n  check  Run checks\n  help   Print this message\n\nOptions:\n  -h, --help\n";
        let names: Vec<_> = listed_subcommands(help).unwrap().into_iter().collect();
        assert_eq!(names, ["check", "parse"]);
    }
}
