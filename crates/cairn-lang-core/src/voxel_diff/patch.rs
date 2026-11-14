//! The `edit` lines that bring a build nearer the structure, each found by building the source with
//! it and comparing again (`spec/ecosystem-interop` "The suggested patch").

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::ast::ValueKind;
use crate::intent::{Member, MemberRole};
use crate::transliterate::spelled;

use super::regions::{Fix, Region};
use super::{Comparison, Counts, DiffInput, Metrics, RegionKind, compare};

/// The most lines a patch holds.
pub const MAX_EDITS: usize = 3;

/// The most candidate sources one patch search builds.
pub const BUILD_BUDGET: usize = 48;

/// The `edit` lines found to bring the build nearer the structure, and what it reaches with them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Patch {
    /// The lines, in the order they are appended to the source.
    pub edits: Vec<String>,
    /// The metrics the source reaches with every line appended.
    #[serde(flatten)]
    pub metrics: Metrics,
    /// The cells those metrics are ratios of.
    pub counts: Counts,
}

/// One candidate line, with what it changes.
struct Candidate {
    line: String,
    fix: Fix,
}

/// The patch for `regions` of `comparison`: for each region in turn whose cause names a member
/// no earlier region named, the candidate line that most lowers the cells that differ, kept when
/// it lowers them; each kept line is written into its region.
#[must_use]
pub fn suggest_patch(
    input: &DiffInput<'_>,
    comparison: &Comparison,
    regions: &mut [Region],
) -> Option<Patch> {
    let members: HashMap<usize, &Member> = input
        .module
        .addressed_members()
        .into_iter()
        .map(|(_, _, member)| (member.span.start, member))
        .collect();
    let mut kept: Vec<String> = Vec::new();
    let mut reached = comparison.counts;
    let mut edited: HashSet<usize> = HashSet::new();
    let mut builds = 0;
    for region in regions.iter_mut() {
        if kept.len() == MAX_EDITS || builds == BUILD_BUDGET || reached.differing() == 0 {
            break;
        }
        let Some((b, m)) = region.culprit else {
            continue;
        };
        let owned = &comparison.bodies[b].built.members[m];
        let Some(member) = members.get(&owned.span.start) else {
            continue;
        };
        if !edited.insert(owned.span.start) {
            continue;
        }
        let slots = slots_building(comparison, b, &members, &region.mat);
        let mut best: Option<(Candidate, Counts)> = None;
        for candidate in candidates(member, region, input.source, &slots) {
            if builds == BUILD_BUDGET {
                break;
            }
            builds += 1;
            let text = appended(input.source, kept.iter().chain([&candidate.line]));
            let Some(built) = (input.build)(&text) else {
                continue;
            };
            let counts = compare(input.units, &built).counts;
            let bar = best.as_ref().map_or(reached, |(_, counts)| *counts);
            if counts.differing() < bar.differing() {
                best = Some((candidate, counts));
            }
        }
        if let Some((candidate, counts)) = best {
            region.patch = Some(candidate.line.clone());
            region.fix = Some(candidate.fix);
            kept.push(candidate.line);
            reached = counts;
        }
    }
    (!kept.is_empty()).then(|| Patch {
        edits: kept,
        metrics: reached.metrics(),
        counts: reached,
    })
}

/// `source` with each of `lines` appended on a line of its own.
fn appended<'a>(source: &str, lines: impl Iterator<Item = &'a String>) -> String {
    let mut out = source.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The slots the members of pair `b` read whose blocks include `mat`, in member order.
fn slots_building(
    comparison: &Comparison,
    b: usize,
    members: &HashMap<usize, &Member>,
    mat: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    for owned in &comparison.bodies[b].built.members {
        if !owned.blocks.iter().any(|(id, _)| id == mat) {
            continue;
        }
        let slot = members
            .get(&owned.span.start)
            .and_then(|member| member.read_slot());
        if let Some(slot) = slot
            && !out.iter().any(|kept| kept == slot)
        {
            out.push(slot.to_owned());
        }
    }
    out
}

/// The candidate lines for `member`, the member `region`'s cause names, in the order they are
/// tried: its block or states set to the structure's, then each whole number its line writes one
/// and two either way, then a `repeat=` its line leaves out, then a `from=`/`to=` box grown over
/// a missing region or cut back from an extra one.
fn candidates(member: &Member, region: &Region, source: &str, slots: &[String]) -> Vec<Candidate> {
    let target = match (&member.id, &member.address) {
        (Some(id), _) => format!("{}[id={id}]", member.role.keyword()),
        (None, Some(address)) => address.to_string(),
        (None, None) => return Vec::new(),
    };
    let written = |key: &str| {
        member
            .written(key)
            .and_then(|value| source.get(value.span.clone()))
            .map(str::to_owned)
    };
    let mut out = Vec::new();
    let mut push = |changes: Vec<(String, String)>| {
        let sets: Vec<String> = changes.iter().map(|(k, v)| format!("{k}={v}")).collect();
        out.push(Candidate {
            line: format!("edit {target} set {}", sets.join(" ")),
            fix: Fix {
                target: target.clone(),
                changes: changes
                    .into_iter()
                    .map(|(key, to)| {
                        let was = written(&key);
                        (key, was, to)
                    })
                    .collect(),
            },
        });
    };
    let raw = matches!(member.role, MemberRole::RawBlock | MemberRole::RawFill);
    let spells = region.sample.as_ref().map(spelled);
    match region.kind {
        RegionKind::State | RegionKind::Replaced if raw => {
            if let Some(mat) = spells {
                push(vec![("mat".to_owned(), mat)]);
            }
        }
        RegionKind::State => {
            let keys = member.role.state_arguments();
            let changes: Vec<(String, String)> = region
                .states
                .iter()
                .filter(|state| keys.contains(&state.key.as_str()))
                .filter_map(|state| Some((state.key.clone(), state.structure.clone()?)))
                .collect();
            if !changes.is_empty() {
                push(changes);
            }
        }
        RegionKind::Replaced => {
            for slot in slots {
                if member.read_slot().is_some_and(|own| own != slot) {
                    push(vec![("mat_slot".to_owned(), slot.clone())]);
                }
            }
        }
        RegionKind::Missing | RegionKind::Extra => {}
    }
    for (key, value) in member.arguments.iter().chain(member.intent_state.iter()) {
        let ValueKind::Int(n) = value.value.kind else {
            continue;
        };
        for delta in [1, -1, 2, -2] {
            let to = n + delta;
            if to >= 0 {
                push(vec![(key.clone(), to.to_string())]);
            }
        }
    }
    let takes_repeat = member
        .role
        .own_keys()
        .is_some_and(|keys| keys.contains(&"repeat"));
    if takes_repeat && member.written("repeat").is_none() {
        for to in [2, 3] {
            push(vec![("repeat".to_owned(), to.to_string())]);
        }
    }
    if let Some(changes) = moved_box(member, region) {
        push(changes);
    }
    out
}

/// The `from=`/`to=` of `member`'s box grown over a missing `region` beside it, or cut back from
/// an extra one at one end of it; `None` when neither applies.
fn moved_box(member: &Member, region: &Region) -> Option<Vec<(String, String)>> {
    let corner = |key: &str| match member.written(key)?.value.kind {
        ValueKind::Coord { x, y, z } => {
            Some([x, y, z].map(|v| u32::try_from(v).unwrap_or(u32::MAX)))
        }
        _ => None,
    };
    let (a, b) = (corner("from")?, corner("to")?);
    let low: [u32; 3] = std::array::from_fn(|i| a[i].min(b[i]));
    let high: [u32; 3] = std::array::from_fn(|i| a[i].max(b[i]));
    let (new_low, new_high) = match region.kind {
        RegionKind::Missing => (
            std::array::from_fn(|i| low[i].min(region.from[i])),
            std::array::from_fn(|i| high[i].max(region.to[i])),
        ),
        RegionKind::Extra => cut_back(low, high, region.from, region.to)?,
        RegionKind::Replaced | RegionKind::State => return None,
    };
    if (new_low, new_high) == (low, high) {
        return None;
    }
    let text = |c: [u32; 3]| format!("{},{},{}", c[0], c[1], c[2]);
    Some(vec![
        ("from".to_owned(), text(new_low)),
        ("to".to_owned(), text(new_high)),
    ])
}

/// The box `low..high` without the slab `from..to`, when the slab spans it across two axes and
/// lies at one end of the third.
fn cut_back(
    low: [u32; 3],
    high: [u32; 3],
    from: [u32; 3],
    to: [u32; 3],
) -> Option<([u32; 3], [u32; 3])> {
    for axis in 0..3 {
        let spans = (0..3)
            .filter(|&other| other != axis)
            .all(|other| from[other] <= low[other] && to[other] >= high[other]);
        if !spans {
            continue;
        }
        let (mut new_low, mut new_high) = (low, high);
        if from[axis] <= low[axis] && to[axis] < high[axis] {
            new_low[axis] = to[axis] + 1;
        } else if to[axis] >= high[axis] && from[axis] > low[axis] {
            new_high[axis] = from[axis] - 1;
        } else {
            continue;
        }
        return Some((new_low, new_high));
    }
    None
}
