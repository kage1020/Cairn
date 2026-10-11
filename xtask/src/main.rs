//! Repository checks for the Cairn workspace that neither rustc nor clippy can express: manifest
//! policy, documentation inventories, spec citations, and version agreement.
//!
//! ```text
//! cargo xtask lint                  # every rule
//! cargo xtask lint spec-citations   # one or more rules by name
//! cargo xtask lint --list           # the rules and what each holds
//! cargo xtask lint versions --tag v2026.10.0
//! ```
//!
//! Each finding prints as `path:line: rule: message`, and as a GitHub annotation when
//! `GITHUB_ACTIONS` is set. The exit code is 1 when anything was found.

mod markdown;
mod repo;
mod rules;

use std::process::ExitCode;

use repo::Repo;
use rules::{Finding, RULES};

const USAGE: &str = "usage: cargo xtask lint [--list] [--tag vVERSION] [RULE...]";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("lint") {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }

    let mut tag = None;
    let mut selected = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--list" => {
                for rule in RULES {
                    println!("{:<20} {}", rule.name, rule.summary);
                }
                return ExitCode::SUCCESS;
            }
            "--tag" => {
                let Some(value) = args.next() else {
                    eprintln!("--tag needs a value\n{USAGE}");
                    return ExitCode::from(2);
                };
                tag = Some(value);
            }
            name => {
                let Some(rule) = RULES.iter().find(|rule| rule.name == name) else {
                    let names: Vec<_> = RULES.iter().map(|rule| rule.name).collect();
                    eprintln!("unknown rule `{name}`; the rules are: {}", names.join(", "));
                    return ExitCode::from(2);
                };
                selected.push(rule);
            }
        }
    }
    if selected.is_empty() {
        selected.extend(RULES);
    }

    let repo = match Repo::open(tag) {
        Ok(repo) => repo,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };

    let mut findings: Vec<Finding> = Vec::new();
    for rule in selected {
        findings.extend((rule.check)(&repo));
    }
    let annotate = std::env::var_os("GITHUB_ACTIONS").is_some();
    for finding in &findings {
        println!("{finding}");
        if annotate {
            println!("{}", finding.annotation());
        }
    }
    if findings.is_empty() {
        ExitCode::SUCCESS
    } else {
        eprintln!("{} finding(s)", findings.len());
        ExitCode::FAILURE
    }
}
