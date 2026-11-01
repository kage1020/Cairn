//! Repository checks for the Cairn workspace that neither rustc nor clippy can express.

mod markdown;
mod repo;
mod rules;

use std::path::PathBuf;
use std::process::ExitCode;

use repo::Repo;
use rules::RULES;

const USAGE: &str = "usage: cargo xtask lint [--list] [--tag vVERSION] [--site DIR] [RULE...]";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("lint") {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }

    let mut tag = None;
    let mut site = None;
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
            "--site" => {
                let Some(value) = args.next() else {
                    eprintln!("--site needs a directory\n{USAGE}");
                    return ExitCode::from(2);
                };
                site = Some(PathBuf::from(value));
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
    if tag.is_some() && !selected.iter().any(|rule| rule.name == "versions") {
        eprintln!("--tag is read only by the `versions` rule, which is not selected\n{USAGE}");
        return ExitCode::from(2);
    }
    if site.is_some() && !selected.iter().any(|rule| rule.name == "site-parity") {
        eprintln!("--site is read only by the `site-parity` rule, which is not selected\n{USAGE}");
        return ExitCode::from(2);
    }

    let repo = match Repo::open(tag, site) {
        Ok(repo) => repo,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };

    let annotate = std::env::var_os("GITHUB_ACTIONS").is_some();
    let mut count = 0;
    for rule in selected {
        for finding in (rule.check)(&repo) {
            println!("{}", finding.render(rule.name));
            if annotate {
                println!("{}", finding.annotation(rule.name));
            }
            count += 1;
        }
    }
    if count == 0 {
        ExitCode::SUCCESS
    } else {
        eprintln!("{count} finding(s)");
        ExitCode::FAILURE
    }
}
