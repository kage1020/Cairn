//! Bedrock's conductor table is written by hand from the Java conductors of the release each
//! version's state map follows (`spec/versioning-editions` "Backend = data tables"), so a state
//! map regenerated for a new release names Java conductors the table has not yet classified. Each
//! one the map names must have a row in the table, conducting or `unsettled`, or be recorded below
//! as a block a source shows Bedrock makes pass no power.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;

use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "bedrock-conductors",
    summary: "each Java conductor Bedrock's state map names has a Bedrock conductor row or is \
              recorded as passing no power there",
    check,
};

const PACK: &str = "crates/cairn-lang-formats/registry-data/bedrock/pack.json";
const STATES: &str = "crates/cairn-lang-formats/registry-data/bedrock/states.json";
const JAVA: &str = "crates/cairn-lang-formats/registry-data/java/conductors.json";
const BEDROCK: &str = "crates/cairn-lang-formats/registry-data/bedrock/conductors.json";
const RULE_FILE: &str = "xtask/src/rules/bedrock_conductors.rs";

/// A Java conductor a source shows Bedrock makes pass no power at some of its versions.
struct Insulating {
    id: &'static str,
    mc_versions: &'static [&'static str],
    why: &'static str,
}

/// The Java conductors Bedrock makes pass no power, which its table leaves out.
const INSULATING: &[Insulating] = &[Insulating {
    id: "mangrove_roots",
    mc_versions: &["1.21.0"],
    why: "Bedrock's mangrove roots pass power only from 1.21.40",
}];

/// The texts the rule reads, each of the file its field is named for.
struct Texts<'a> {
    pack: &'a str,
    states: &'a str,
    java: &'a str,
    bedrock: &'a str,
}

fn check(repo: &Repo) -> Vec<Finding> {
    let mut read = Vec::new();
    let mut findings = Vec::new();
    for file in [PACK, STATES, JAVA, BEDROCK] {
        match repo.read(Path::new(file)) {
            Ok(text) => read.push(text),
            Err(error) => findings.push(Finding::new(file, None, error)),
        }
    }
    if !findings.is_empty() {
        return findings;
    }
    let texts = Texts {
        pack: &read[0],
        states: &read[1],
        java: &read[2],
        bedrock: &read[3],
    };
    compare(&texts, INSULATING)
}

/// The findings for `texts`, with `insulating` the record of the blocks Bedrock makes pass no
/// power.
fn compare(texts: &Texts<'_>, insulating: &[Insulating]) -> Vec<Finding> {
    let parsed = (|| {
        Ok::<_, Finding>((
            follows(texts.pack)?,
            named_by_map(texts.states)?,
            rows_by_version(JAVA, texts.java)?,
            rows_by_version(BEDROCK, texts.bedrock)?,
        ))
    })();
    let (follows, named, java, bedrock) = match parsed {
        Ok(parsed) => parsed,
        Err(finding) => return vec![finding],
    };
    let mut findings = Vec::new();
    let mut classified: BTreeSet<(&str, &str)> = BTreeSet::new();
    for (version, named) in &named {
        let Some(release) = follows.get(version.as_str()) else {
            findings.push(Finding::new(
                PACK,
                None,
                format!("`extracted.states` names no Java release the {version} state map follows"),
            ));
            continue;
        };
        let Some((at, conductors)) = java
            .iter()
            .filter(|(java, _)| order(java) <= order(release))
            .max_by_key(|(java, _)| order(java))
        else {
            findings.push(Finding::new(
                JAVA,
                None,
                format!(
                    "no table at or before Java {release}, which the Bedrock {version} state map \
                     follows"
                ),
            ));
            continue;
        };
        let rows = bedrock.get(version.as_str());
        for id in conductors.intersection(named) {
            let excused = insulating.iter().any(|entry| {
                entry.id == id.as_str() && entry.mc_versions.contains(&version.as_str())
            });
            if excused {
                classified.insert((id.as_str(), version.as_str()));
            } else if !rows.is_some_and(|rows| rows.contains(id)) {
                findings.push(Finding::new(
                    BEDROCK,
                    None,
                    format!(
                        "`{id}` conducts in Java {at}, and the Bedrock {version} state map names \
                         it, but no row names it at {version}: give it a row, conducting or \
                         `unsettled`, or record in {RULE_FILE} the source that shows Bedrock \
                         makes it pass no power"
                    ),
                ));
            }
        }
    }
    for entry in insulating {
        for version in entry.mc_versions {
            let listed = bedrock
                .get(*version)
                .is_some_and(|rows| rows.contains(entry.id));
            if listed || !classified.contains(&(entry.id, *version)) {
                findings.push(Finding::new(
                    RULE_FILE,
                    None,
                    format!(
                        "records `{}` as passing no power on Bedrock {version} ({}), but it is no \
                         Java conductor that version's state map names without a row; drop the \
                         record",
                        entry.id, entry.why,
                    ),
                ));
            }
        }
    }
    findings
}

/// `id` without its `minecraft:` namespace.
fn bare(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

/// `version`'s numbers, to order versions by.
fn order(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|part| part.parse().unwrap_or(u64::MAX))
        .collect()
}

fn parsed(file: &str, text: &str) -> Result<Value, Finding> {
    serde_json::from_str(text)
        .map_err(|error| Finding::new(file, None, format!("is not valid JSON: {error}")))
}

/// The strings of the array at `value`, or a finding on `file` naming `what`.
fn strings<'v>(file: &str, value: &'v Value, what: &str) -> Result<Vec<&'v str>, Finding> {
    value
        .as_array()
        .and_then(|items| items.iter().map(Value::as_str).collect())
        .ok_or_else(|| Finding::new(file, None, format!("{what} is no array of strings")))
}

/// Each Bedrock version with the Java release its state map follows, from `pack.json`.
fn follows(text: &str) -> Result<BTreeMap<String, String>, Finding> {
    let pack = parsed(PACK, text)?;
    let sources = pack["extracted"]["states"]
        .as_array()
        .ok_or_else(|| Finding::new(PACK, None, "`extracted.states` is no array"))?;
    sources
        .iter()
        .map(
            |source| match (source["mc_version"].as_str(), source["java"].as_str()) {
                (Some(version), Some(java)) => Ok((version.to_owned(), java.to_owned())),
                _ => Err(Finding::new(
                    PACK,
                    None,
                    "an `extracted.states` entry gives no `mc_version` and `java`",
                )),
            },
        )
        .collect()
}

/// Each Bedrock version with the Java blocks its state map names, from `states.json`.
fn named_by_map(text: &str) -> Result<BTreeMap<String, BTreeSet<String>>, Finding> {
    let states = parsed(STATES, text)?;
    let families = states["families"]
        .as_array()
        .ok_or_else(|| Finding::new(STATES, None, "`families` is no array"))?;
    let mut named: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for family in families {
        let versions = strings(STATES, &family["mc_versions"], "a family's `mc_versions`")?;
        let pairs = strings(STATES, &family["ids"], "a family's `ids`")?;
        for version in versions {
            let ids = named.entry(version.to_owned()).or_default();
            for pair in &pairs {
                let (bedrock, java) = pair.split_once('=').unwrap_or((pair, pair));
                ids.insert(bare(if java.is_empty() { bedrock } else { java }).to_owned());
            }
        }
    }
    Ok(named)
}

/// Each version of the conductor table in `text`, read from `file`, with the ids its rows name.
fn rows_by_version(file: &str, text: &str) -> Result<BTreeMap<String, BTreeSet<String>>, Finding> {
    let table = parsed(file, text)?;
    let rows = table["rows"]
        .as_array()
        .ok_or_else(|| Finding::new(file, None, "`rows` is no array"))?;
    let mut by_version: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in rows {
        let ids = strings(file, &row["ids"], "a row's `ids`")?;
        for version in strings(file, &row["mc_versions"], "a row's `mc_versions`")? {
            by_version
                .entry(version.to_owned())
                .or_default()
                .extend(ids.iter().map(|id| bare(id).to_owned()));
        }
    }
    Ok(by_version)
}

#[cfg(test)]
mod tests;
