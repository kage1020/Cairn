//! Every workspace crate takes its lints from `[workspace.lints]`, which a crate has to opt into.
//! [`UNSAFE_EXEMPT`] keeps a copy with `unsafe_code` at `deny`, since `forbid` cannot be lifted.

use std::path::Path;

use toml::Value;

use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "manifest",
    summary: "every crate inherits the workspace lints, which forbid unsafe_code",
    check,
};

/// The crate allowed to lower `unsafe_code` to `deny`.
pub const UNSAFE_EXEMPT: &str = "cairn-lang-tree-sitter";

fn level(lint: Option<&Value>) -> Option<&str> {
    match lint? {
        Value::String(level) => Some(level),
        Value::Table(table) => table.get("level")?.as_str(),
        _ => None,
    }
}

fn check(repo: &Repo) -> Vec<Finding> {
    let mut findings = Vec::new();
    let root = Path::new("Cargo.toml");
    let workspace_lints = match repo.manifest(root) {
        Ok(manifest) => manifest
            .get("workspace")
            .and_then(|workspace| workspace.get("lints"))
            .cloned(),
        Err(error) => return vec![Finding::new(root, None, error)],
    };
    let Some(workspace_lints) = workspace_lints else {
        return vec![Finding::new(root, None, "no `[workspace.lints]` table")];
    };
    let unsafe_level = level(
        workspace_lints
            .get("rust")
            .and_then(|rust| rust.get("unsafe_code")),
    );
    if unsafe_level != Some("forbid") {
        findings.push(Finding::new(
            root,
            None,
            format!(
                "`[workspace.lints.rust] unsafe_code` is {unsafe_level:?}; it must be \"forbid\" \
                 (the {UNSAFE_EXEMPT} crate lowers it in its own manifest)"
            ),
        ));
    }

    let mut exempt_copy = workspace_lints.clone();
    if let Some(Value::Table(rust)) = exempt_copy.get_mut("rust") {
        rust.insert("unsafe_code".into(), Value::String("deny".into()));
    }
    let inherited = Value::Table(toml::Table::from_iter([(
        "workspace".to_owned(),
        Value::Boolean(true),
    )]));

    let members = match repo.members() {
        Ok(members) => members,
        Err(error) => return vec![Finding::new(root, None, error)],
    };
    for member in members {
        let path = member.dir.join("Cargo.toml");
        let manifest = match repo.manifest(&path) {
            Ok(manifest) => manifest,
            Err(error) => {
                findings.push(Finding::new(&path, None, error));
                continue;
            }
        };
        let lints = manifest.get("lints");
        if member.name == UNSAFE_EXEMPT {
            if lints != Some(&exempt_copy) {
                findings.push(Finding::new(
                    &path,
                    None,
                    "`[lints]` must be a copy of `[workspace.lints]` with `unsafe_code = \"deny\"` \
                     and nothing else changed",
                ));
            }
        } else if lints != Some(&inherited) {
            findings.push(Finding::new(
                &path,
                None,
                "must inherit the workspace lints with `[lints]` `workspace = true` and set none \
                 of its own",
            ));
        }
    }
    findings
}
