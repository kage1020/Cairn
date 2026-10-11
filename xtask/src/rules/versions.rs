//! Every copy of the release version agrees with `[workspace.package] version`, and with `--tag`.

use std::path::Path;

use regex::Regex;
use toml::Value;

use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "versions",
    summary: "every copy of the version, and --tag if given, matches [workspace.package]",
    check,
};

/// JSON files that carry the version, with the pointer to it.
const JSON_COPIES: &[(&str, &str)] = &[
    ("crates/cairn-lang-tree-sitter/package.json", "/version"),
    (
        "crates/cairn-lang-tree-sitter/tree-sitter.json",
        "/metadata/version",
    ),
    ("editors/vscode/package.json", "/version"),
];

const DEPENDENCY_TABLES: &[&str] = &["dependencies", "dev-dependencies", "build-dependencies"];

fn check(repo: &Repo) -> Vec<Finding> {
    let root = Path::new("Cargo.toml");
    let manifest = match repo.manifest(root) {
        Ok(manifest) => manifest,
        Err(error) => return vec![Finding::new(RULE.name, root, None, error)],
    };
    let Some(version) = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(Value::as_str)
    else {
        return vec![Finding::new(
            RULE.name,
            root,
            None,
            "no `[workspace.package] version`",
        )];
    };
    let calver = Regex::new(r"^[0-9]{4}\.(0|[1-9][0-9]*)(\.[0-9]+)?$").expect("valid regex");
    if !calver.is_match(version) {
        return vec![Finding::new(
            RULE.name,
            root,
            None,
            format!("`[workspace.package] version` is {version:?}, which is not YYYY.M[.PATCH]"),
        )];
    }

    let mut findings = Vec::new();
    let mut compare = |path: &Path, what: &str, found: Option<&str>| {
        if found != Some(version) {
            findings.push(Finding::new(
                RULE.name,
                path,
                None,
                format!("{what} is {found:?}, but the workspace version is {version:?}"),
            ));
        }
    };

    if let Some(tag) = &repo.tag {
        compare(root, "--tag", tag.strip_prefix('v'));
    }

    for (file, pointer) in JSON_COPIES {
        let path = Path::new(file);
        let found = repo
            .read(path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        compare(
            path,
            &format!("`{pointer}`"),
            found
                .as_ref()
                .and_then(|json| json.pointer(pointer)?.as_str()),
        );
    }

    let workspace_dependencies = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(Value::as_table);
    for (name, dependency) in workspace_dependencies.into_iter().flatten() {
        if dependency.get("path").is_some() {
            compare(
                root,
                &format!("`[workspace.dependencies] {name}` version"),
                dependency.get("version").and_then(Value::as_str),
            );
        }
    }

    match repo.members() {
        Ok(members) => {
            for member in members {
                let path = member.dir.join("Cargo.toml");
                let Ok(crate_manifest) = repo.manifest(&path) else {
                    continue;
                };
                for table in DEPENDENCY_TABLES {
                    let dependencies = crate_manifest.get(*table).and_then(Value::as_table);
                    for (name, dependency) in dependencies.into_iter().flatten() {
                        if dependency.get("path").is_some()
                            && let Some(declared) = dependency.get("version")
                        {
                            compare(
                                &path,
                                &format!("`[{table}] {name}` version"),
                                declared.as_str(),
                            );
                        }
                    }
                }
            }
        }
        Err(error) => findings.push(Finding::new(RULE.name, root, None, error)),
    }
    findings
}
