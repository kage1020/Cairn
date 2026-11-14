//! The cells that differ, clustered into regions, each with what likely caused it.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt;

use indexmap::IndexMap;
use serde::Serialize;

use crate::block_array::BlockState;
use crate::lines::LineStarts;
use crate::transliterate::spelled;

use super::bodies::{By, Cell, Owned};
use super::{BodyComparison, Comparison, Difference, RegionKind, StateDifference};

/// How far apart two cells of one kind and block may stand and still be one region: one cell
/// between them.
const LINK: u32 = 2;

/// How far from a region a member building its block may stand to be what stops short of it.
const REACH: u32 = 2;

/// A member of the source, as a region's cause names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemberRef {
    /// The keyword its line starts with.
    pub keyword: String,
    /// Its `id=`, if its line writes one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Its address in its body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// The 1-based line it is written on.
    pub line: u32,
}

impl MemberRef {
    /// The member as an `edit` line names it: by its `id=` when it has one, else by its address.
    #[must_use]
    pub fn target(&self) -> String {
        match (&self.id, &self.address) {
            (Some(id), _) => format!("{}[id={id}]", self.keyword),
            (None, Some(address)) => address.clone(),
            (None, None) => format!("{}[]", self.keyword),
        }
    }
}

/// What one `edit` line changes, as a region's cause reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    /// The member, as the line names it.
    pub target: String,
    /// Each key it sets, with the value the member held before, if its line wrote one.
    pub changes: Vec<(String, Option<String>, String)>,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let [(key, Some(was), to)] = self.changes.as_slice()
            && let (Ok(was_n), Ok(to_n)) = (was.parse::<i64>(), to.parse::<i64>())
        {
            let verdict = if to_n > was_n {
                "too small"
            } else {
                "too large"
            };
            return write!(f, "`{}` `{key}={was}` {verdict}", self.target);
        }
        let was: Vec<String> = self
            .changes
            .iter()
            .filter_map(|(key, was, _)| Some(format!("{key}={}", was.as_ref()?)))
            .collect();
        let to: Vec<String> = self
            .changes
            .iter()
            .map(|(key, _, to)| format!("{key}={to}"))
            .collect();
        if was.is_empty() {
            write!(f, "`{}` without `{}`", self.target, to.join(" "))
        } else {
            write!(
                f,
                "`{}` `{}` where `{}` builds it",
                self.target,
                was.join(" "),
                to.join(" ")
            )
        }
    }
}

/// What likely made a region differ (`spec/ecosystem-interop` "Regions and their causes").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Cause {
    /// A member built the block the region's cells hold in the build.
    BuiltBy {
        /// The member.
        member: MemberRef,
    },
    /// A member left air where the structure holds the region's block.
    ClearedBy {
        /// The member.
        member: MemberRef,
    },
    /// A member builds the region's block beside it and stops short.
    ShortOf {
        /// The member.
        member: MemberRef,
    },
    /// A walkway of the site laid the block.
    Walkway,
    /// No member builds the region's block there or beside it.
    NoMember,
    /// The region's body has no counterpart on the other side.
    Unpaired,
}

/// Cells of one body that differ in one way about one block, standing together.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Region {
    /// What the cells are.
    pub kind: RegionKind,
    /// The body, as the build names it, else as the structure does.
    pub body: String,
    /// The region's lowest corner, `[x, y, z]`.
    pub from: [u32; 3],
    /// Its highest corner.
    pub to: [u32; 3],
    /// How many of the cells in that box differ.
    pub cells: u64,
    /// The block id: the structure's, or the build's for an extra region.
    pub mat: String,
    /// For a replaced region, the build's block id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub built: Option<String>,
    /// For a state region, each property that differs.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<StateDifference>,
    /// What likely made it differ.
    pub cause: Cause,
    /// The `edit` line chosen for it, when one brings the build nearer the structure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    #[serde(skip)]
    pub(super) fix: Option<Fix>,
    /// The pair it lies in and the member its cause names, as indices.
    #[serde(skip)]
    pub(super) culprit: Option<(usize, usize)>,
    /// The structure's state the most cells hold.
    #[serde(skip)]
    pub(super) sample: Option<BlockState>,
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [fx, fy, fz] = self.from;
        let [tx, ty, tz] = self.to;
        let cells = if self.cells == 1 {
            "1 cell".to_owned()
        } else {
            format!("{} cells", self.cells)
        };
        write!(
            f,
            "{} {} ({fx},{fy},{fz})..({tx},{ty},{tz}): {cells} of {}",
            self.kind.as_str(),
            self.body,
            self.mat
        )?;
        if let Some(built) = &self.built {
            write!(f, " built as {built}")?;
        }
        if !self.states.is_empty() {
            let pairs: Vec<String> = self
                .states
                .iter()
                .map(|state| {
                    let value =
                        |v: &Option<String>| v.clone().unwrap_or_else(|| "unset".to_owned());
                    format!(
                        "{}={} built as {}",
                        state.key,
                        value(&state.structure),
                        value(&state.built)
                    )
                })
                .collect();
            write!(f, " ({})", pairs.join(", "))?;
        }
        f.write_str(" → ")?;
        match (&self.fix, &self.cause) {
            (Some(fix), _) => write!(f, "likely {fix}"),
            (None, Cause::BuiltBy { member }) => {
                write!(f, "built by `{}` (line {})", member.target(), member.line)
            }
            (None, Cause::ClearedBy { member }) => {
                write!(f, "cleared by `{}` (line {})", member.target(), member.line)
            }
            (None, Cause::ShortOf { member }) => write!(
                f,
                "likely `{}` (line {}) stops short of it",
                member.target(),
                member.line
            ),
            (None, Cause::Walkway) => f.write_str("laid by a walkway"),
            (None, Cause::NoMember) => f.write_str("no member builds it"),
            (None, Cause::Unpaired) => match self.kind {
                RegionKind::Extra => write!(f, "the structure holds no `{}`", self.body),
                _ => write!(f, "the source builds no `{}`", self.body),
            },
        }
    }
}

/// The differing cells of `comparison` clustered into regions, the largest first, each with its
/// likely cause; `source` gives each member its line.
#[must_use]
pub fn regions(comparison: &Comparison, source: &str) -> Vec<Region> {
    let lines = LineStarts::new(source);
    let mut out = Vec::new();
    for (b, body) in comparison.bodies.iter().enumerate() {
        let mut groups: IndexMap<(RegionKind, &str, &str), Vec<&Difference>> = IndexMap::new();
        for difference in &body.differences {
            let expected = difference.expected.as_ref().map_or("", |s| s.id.as_str());
            let built = difference.built.as_ref().map_or("", |s| s.id.as_str());
            let key = match difference.kind {
                RegionKind::Extra => (difference.kind, built, ""),
                RegionKind::Replaced => (difference.kind, expected, built),
                RegionKind::Missing | RegionKind::State => (difference.kind, expected, ""),
            };
            groups.entry(key).or_default().push(difference);
        }
        for ((kind, mat, built), cells) in groups {
            for cluster in clusters(&cells) {
                out.push((
                    b,
                    region(body, b, kind, mat, built, &cluster, source, &lines),
                ));
            }
        }
    }
    out.sort_by_key(|(b, region)| {
        (
            Reverse(region.cells),
            *b,
            region.kind,
            [region.from[1], region.from[2], region.from[0]],
            region.mat.clone(),
        )
    });
    out.into_iter().map(|(_, region)| region).collect()
}

/// `cells` split into the groups whose cells stand within [`LINK`] of one another, each in the
/// order its cells were found.
fn clusters<'a>(cells: &[&'a Difference]) -> Vec<Vec<&'a Difference>> {
    let at: HashMap<Cell, usize> = cells
        .iter()
        .enumerate()
        .map(|(i, difference)| (difference.cell, i))
        .collect();
    let mut parent: Vec<usize> = (0..cells.len()).collect();
    let reach = i64::from(LINK);
    for (i, difference) in cells.iter().enumerate() {
        let (cx, cy, cz) = difference.cell;
        for dx in -reach..=reach {
            for dy in -reach..=reach {
                for dz in -reach..=reach {
                    let near = (
                        u32::try_from(i64::from(cx) + dx),
                        u32::try_from(i64::from(cy) + dy),
                        u32::try_from(i64::from(cz) + dz),
                    );
                    let (Ok(nx), Ok(ny), Ok(nz)) = near else {
                        continue;
                    };
                    if let Some(&j) = at.get(&(nx, ny, nz)) {
                        let (first, second) = (root(&mut parent, i), root(&mut parent, j));
                        if first != second {
                            parent[first.max(second)] = first.min(second);
                        }
                    }
                }
            }
        }
    }
    let mut grouped: IndexMap<usize, Vec<&Difference>> = IndexMap::new();
    for (i, difference) in cells.iter().enumerate() {
        let r = root(&mut parent, i);
        grouped.entry(r).or_default().push(difference);
    }
    grouped.into_values().collect()
}

/// The cluster `i` belongs to, flattening the path to it.
fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// The region `cells` make, with its cause.
#[expect(
    clippy::too_many_arguments,
    reason = "the pair's parts are read once here, where one struct would be built to be undone"
)]
fn region(
    body: &BodyComparison,
    b: usize,
    kind: RegionKind,
    mat: &str,
    built: &str,
    cells: &[&Difference],
    source: &str,
    lines: &LineStarts,
) -> Region {
    let mut from = [u32::MAX; 3];
    let mut to = [0; 3];
    for difference in cells {
        let (x, y, z) = difference.cell;
        for (axis, value) in [x, y, z].into_iter().enumerate() {
            from[axis] = from[axis].min(value);
            to[axis] = to[axis].max(value);
        }
    }
    let mut states: Vec<StateDifference> = cells
        .iter()
        .flat_map(|difference| difference.states.iter().cloned())
        .collect();
    states.sort();
    states.dedup();
    let mut samples: IndexMap<String, (usize, &BlockState)> = IndexMap::new();
    for state in cells.iter().filter_map(|d| d.expected.as_ref()) {
        samples.entry(spelled(state)).or_insert((0, state)).0 += 1;
    }
    let sample = samples
        .values()
        .enumerate()
        .max_by_key(|(i, (n, _))| (*n, Reverse(*i)))
        .map(|(_, (_, state))| (*state).clone());
    let (cause, culprit) = cause(body, kind, mat, cells, from, to);
    let member_ref = |m: usize| {
        let owned = &body.built.members[m];
        MemberRef {
            keyword: owned.role.clone(),
            id: owned.id.clone(),
            address: owned.address.as_ref().map(ToString::to_string),
            line: lines.position(source, owned.span.start).line.get(),
        }
    };
    let cause = match (cause, culprit) {
        (Found::BuiltBy, Some(m)) => Cause::BuiltBy {
            member: member_ref(m),
        },
        (Found::ClearedBy, Some(m)) => Cause::ClearedBy {
            member: member_ref(m),
        },
        (Found::ShortOf, Some(m)) => Cause::ShortOf {
            member: member_ref(m),
        },
        (Found::Walkway, _) => Cause::Walkway,
        (Found::Unpaired, _) => Cause::Unpaired,
        _ => Cause::NoMember,
    };
    Region {
        kind,
        body: body.name().to_owned(),
        from,
        to,
        cells: cells.len() as u64,
        mat: mat.to_owned(),
        built: (kind == RegionKind::Replaced).then(|| built.to_owned()),
        states,
        cause,
        patch: None,
        fix: None,
        culprit: culprit.map(|m| (b, m)),
        sample,
    }
}

/// A cause before its member is named.
enum Found {
    BuiltBy,
    ClearedBy,
    ShortOf,
    Walkway,
    NoMember,
    Unpaired,
}

/// What likely made `cells` differ, and the member it names.
fn cause(
    body: &BodyComparison,
    kind: RegionKind,
    mat: &str,
    cells: &[&Difference],
    from: [u32; 3],
    to: [u32; 3],
) -> (Found, Option<usize>) {
    if body.structure.is_none() || body.source.is_none() {
        return (Found::Unpaired, None);
    }
    let mut tally: IndexMap<usize, usize> = IndexMap::new();
    let mut walkway = false;
    for difference in cells {
        match difference.by {
            By::Member(m) => *tally.entry(m).or_default() += 1,
            By::Walkway => walkway = true,
            By::Nothing => {}
        }
    }
    let most = tally
        .iter()
        .max_by_key(|(m, n)| (**n, Reverse(**m)))
        .map(|(m, _)| *m);
    match (kind, most) {
        (RegionKind::Missing, Some(m)) => (Found::ClearedBy, Some(m)),
        (RegionKind::Missing, None) => match nearest(&body.built.members, mat, from, to) {
            Some(m) => (Found::ShortOf, Some(m)),
            None => (Found::NoMember, None),
        },
        (_, Some(m)) => (Found::BuiltBy, Some(m)),
        (_, None) if walkway => (Found::Walkway, None),
        (_, None) => (Found::NoMember, None),
    }
}

/// The member building `mat` that stands nearest the box `from..to`, within [`REACH`] of it.
fn nearest(members: &[Owned], mat: &str, from: [u32; 3], to: [u32; 3]) -> Option<usize> {
    let distance = |(x, y, z): Cell| {
        [x, y, z]
            .into_iter()
            .enumerate()
            .map(|(axis, v)| from[axis].saturating_sub(v).max(v.saturating_sub(to[axis])))
            .max()
            .unwrap_or(0)
    };
    members
        .iter()
        .enumerate()
        .filter_map(|(m, member)| {
            let cells = &member.blocks.iter().find(|(id, _)| id == mat)?.1;
            let d = cells.iter().copied().map(distance).min()?;
            (d <= REACH).then_some((d, m))
        })
        .min()
        .map(|(_, m)| m)
}
