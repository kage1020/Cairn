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
    /// The member as an `edit` line names it, else by its keyword.
    #[must_use]
    pub fn target(&self) -> String {
        edit_target(&self.keyword, self.id.as_deref(), self.address.as_deref())
            .unwrap_or_else(|| self.keyword.clone())
    }
}

/// A member as an `edit` line names it: by its `id=` when it has one, else by its address; `None`
/// when it has neither.
pub(super) fn edit_target(
    keyword: &str,
    id: Option<&str>,
    address: Option<&str>,
) -> Option<String> {
    match (id, address) {
        (Some(id), _) => Some(format!("{keyword}[id={id}]")),
        (None, Some(address)) => Some(address.to_owned()),
        (None, None) => None,
    }
}

/// What one `edit` line changes, as a region's cause reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Fix {
    /// The member, as the line names it.
    pub(super) target: String,
    /// Each key it sets, with the value the member held before, if its line wrote one.
    pub(super) changes: Vec<(String, Option<String>, String)>,
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
pub(super) fn regions(comparison: &Comparison, source: &str) -> Vec<Region> {
    let lines = LineStarts::new(source);
    let mut out = Vec::new();
    for (b, body) in comparison.bodies.iter().enumerate() {
        let member_ref = |m: usize| {
            let owned = &body.built.members[m];
            MemberRef {
                keyword: owned.role.clone(),
                id: owned.id.clone(),
                address: owned.address.as_ref().map(ToString::to_string),
                line: lines.position(source, owned.span.start).line.get(),
            }
        };
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
                out.push((b, region(body, b, kind, mat, built, &cluster, &member_ref)));
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

/// The region `cells` of pair `b` make, with its cause, whose member `member_ref` names.
fn region(
    body: &BodyComparison,
    b: usize,
    kind: RegionKind,
    mat: &str,
    built: &str,
    cells: &[&Difference],
    member_ref: &impl Fn(usize) -> MemberRef,
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
    let found = cause(body, kind, mat, cells, from, to);
    let culprit = match found {
        Found::BuiltBy(m) | Found::ClearedBy(m) | Found::ShortOf(m) => Some(m),
        Found::Walkway | Found::NoMember | Found::Unpaired => None,
    };
    let cause = match found {
        Found::BuiltBy(m) => Cause::BuiltBy {
            member: member_ref(m),
        },
        Found::ClearedBy(m) => Cause::ClearedBy {
            member: member_ref(m),
        },
        Found::ShortOf(m) => Cause::ShortOf {
            member: member_ref(m),
        },
        Found::Walkway => Cause::Walkway,
        Found::NoMember => Cause::NoMember,
        Found::Unpaired => Cause::Unpaired,
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

/// A cause, naming its member by its index in the pair's built members.
enum Found {
    BuiltBy(usize),
    ClearedBy(usize),
    ShortOf(usize),
    Walkway,
    NoMember,
    Unpaired,
}

/// What likely made `cells` differ.
fn cause(
    body: &BodyComparison,
    kind: RegionKind,
    mat: &str,
    cells: &[&Difference],
    from: [u32; 3],
    to: [u32; 3],
) -> Found {
    if body.structure.is_none() || body.source.is_none() {
        return Found::Unpaired;
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
    let members = &body.built.members;
    let most = tally
        .iter()
        .max_by_key(|(m, n)| (**n, Reverse(members[**m].span.start), Reverse(**m)))
        .map(|(m, _)| *m);
    match (kind, most) {
        (RegionKind::Missing, Some(m)) => Found::ClearedBy(m),
        (RegionKind::Missing, None) => match nearest(members, mat, from, to) {
            Some(m) => Found::ShortOf(m),
            None => Found::NoMember,
        },
        (_, Some(m)) => Found::BuiltBy(m),
        (_, None) if walkway => Found::Walkway,
        (_, None) => Found::NoMember,
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
            (d <= REACH).then_some((d, member.span.start, m))
        })
        .min()
        .map(|(_, _, m)| m)
}
