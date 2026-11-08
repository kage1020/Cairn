//! Setting the connection states of one body's panes, fences, walls and fence gates from the
//! blocks beside them once every member is painted (`spec/blockstate` "Connections").

use std::collections::HashMap;

use crate::block_array::joins::{Face, JoinKind, Joins};
use crate::block_array::material::TargetRegistry;
use crate::block_array::{BlockState, PaletteIndex};

use super::canvas::{Canvas, ScopePalette};

/// A cell, `(x, y, z)`.
type Cell = (u32, u32, u32);

/// How high a wall's side stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Height {
    None,
    Low,
    Tall,
}

impl Height {
    fn from_value(value: &str) -> Option<Self> {
        Some(match value {
            "none" => Self::None,
            "low" => Self::Low,
            "tall" => Self::Tall,
            _ => return None,
        })
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Tall => "tall",
        }
    }
}

/// A connecting block, with what has been settled about it so far.
struct Joiner {
    cell: Cell,
    kind: JoinKind,
    state: BlockState,
    /// Per [`Face::SIDES`], whether it joins the block on that side.
    joined: [bool; 4],
    /// A wall's side heights per [`Face::SIDES`], once settled.
    heights: [Height; 4],
    /// A wall's `up`, once settled.
    up: Option<bool>,
}

/// What the rules read of a body: its cells' states and each palette slot's [`Joins`].
struct Grid<'a> {
    canvas: &'a Canvas,
    palette: &'a ScopePalette,
    joins: Vec<Joins>,
}

impl Grid<'_> {
    /// The block at `cell` stepped once toward `face`, as its slot; `None` outside the body.
    fn beside(&self, cell: Cell, face: Face) -> Option<(Cell, PaletteIndex)> {
        let (dx, dy, dz) = face.step();
        let next = (
            cell.0.checked_add_signed(dx)?,
            cell.1.checked_add_signed(dy)?,
            cell.2.checked_add_signed(dz)?,
        );
        Some((next, self.canvas.slot_at(next)?))
    }

    /// What the block at `cell` stepped toward `face` is to the rules; nothing outside the body.
    fn joins_beside(&self, cell: Cell, face: Face) -> (Option<Cell>, Joins) {
        self.beside(cell, face)
            .map_or((None, Joins::default()), |(next, slot)| {
                (Some(next), self.joins[usize::from(slot.0)])
            })
    }
}

/// Set the connection states of every pane, fence, wall and fence gate on `canvas` that its state
/// does not write, each repainted under the member that painted it.
pub(super) fn derive_connections(
    canvas: &mut Canvas,
    palette: &mut ScopePalette,
    registry: &dyn TargetRegistry,
) {
    let joins: Vec<Joins> = (0..palette.len())
        .map(|slot| {
            let state = palette.state(slot);
            registry.joins(state).unwrap_or_default()
        })
        .collect();
    let grid = Grid {
        canvas,
        palette,
        joins,
    };
    let mut joiners = gather(&grid);
    if joiners.is_empty() {
        return;
    }
    let at: HashMap<Cell, usize> = joiners
        .iter()
        .enumerate()
        .map(|(i, joiner)| (joiner.cell, i))
        .collect();
    for i in 0..joiners.len() {
        if matches!(joiners[i].kind, JoinKind::Wall) {
            joiners[i].heights = heights(&grid, &joiners, &at, &joiners[i]);
        }
    }
    // A wall's post reads the post of the wall above it, so walls settle from the top down.
    let mut walls: Vec<usize> = (0..joiners.len())
        .filter(|&i| matches!(joiners[i].kind, JoinKind::Wall))
        .collect();
    walls.sort_by_key(|&i| std::cmp::Reverse(joiners[i].cell.1));
    for i in walls {
        joiners[i].up = Some(raises_post(&grid, &joiners, &at, &joiners[i]));
    }
    let derived: Vec<(Cell, BlockState)> = joiners
        .iter()
        .filter_map(|joiner| {
            let state = derived_state(&grid, joiner);
            (state != joiner.state).then_some((joiner.cell, state))
        })
        .collect();
    for (cell, state) in derived {
        let Some(owner) = canvas.owner_at(cell) else {
            continue;
        };
        canvas
            .for_member(owner)
            .paint(cell, || palette.intern(state));
    }
}

/// Every connecting block of the body, each with the sides it joins.
fn gather(grid: &Grid<'_>) -> Vec<Joiner> {
    let mut joiners = Vec::new();
    for (cell, slot) in grid.canvas.cells() {
        let Some(kind) = grid.joins[usize::from(slot.0)].kind else {
            continue;
        };
        let state = grid.palette.state(usize::from(slot.0)).clone();
        let joined = Face::SIDES.map(|side| match written_side(&state, kind, side) {
            Some(written) => written,
            None => joins_toward(grid, cell, kind, side),
        });
        joiners.push(Joiner {
            cell,
            kind,
            state,
            joined,
            heights: [Height::None; 4],
            up: None,
        });
    }
    joiners
}

/// Whether the state writes that it joins the block toward `side`, or `None` when it does not say.
fn written_side(state: &BlockState, kind: JoinKind, side: Face) -> Option<bool> {
    let value = state.properties.get(side.as_str())?;
    match kind {
        JoinKind::Pane | JoinKind::Fence { .. } => value.parse().ok(),
        JoinKind::Wall => Height::from_value(value).map(|height| height != Height::None),
        JoinKind::FenceGate { .. } => None,
    }
}

/// Whether a `kind` block at `cell` joins the block beside it toward `side`.
fn joins_toward(grid: &Grid<'_>, cell: Cell, kind: JoinKind, side: Face) -> bool {
    let (_, other) = grid.joins_beside(cell, side);
    let by_face = other.sturdy.contains(side.opposite()) && !other.never_joined_by_face;
    let gate = matches!(
        other.kind,
        Some(JoinKind::FenceGate { facing, .. }) if across(facing, side)
    );
    match kind {
        JoinKind::Pane => by_face || matches!(other.kind, Some(JoinKind::Pane | JoinKind::Wall)),
        JoinKind::Fence { wooden } => {
            by_face || gate || other.kind == Some(JoinKind::Fence { wooden })
        }
        JoinKind::Wall => {
            by_face || gate || matches!(other.kind, Some(JoinKind::Pane | JoinKind::Wall))
        }
        JoinKind::FenceGate { .. } => false,
    }
}

/// Whether two sideways faces lie on different axes, as a gate facing one spans the other.
fn across(a: Face, b: Face) -> bool {
    matches!(a, Face::North | Face::South) != matches!(b, Face::North | Face::South)
}

/// What the bottom of a block above a wall covers: the wall's post, or one of its sides.
#[derive(Clone, Copy)]
enum Under {
    Post,
    Side(usize),
}

/// Whether the block above `cell` covers the part of the wall there that `under` names.
fn covered(
    grid: &Grid<'_>,
    joiners: &[Joiner],
    at: &HashMap<Cell, usize>,
    cell: Cell,
    under: Under,
) -> bool {
    let (above, joins) = grid.joins_beside(cell, Face::Up);
    let Some(above) = above.and_then(|above| at.get(&above)).map(|&i| &joiners[i]) else {
        return match under {
            Under::Post => joins.covers.post,
            Under::Side(side) => joins.covers.sides.contains(Face::SIDES[side]),
        };
    };
    match (above.kind, under) {
        (JoinKind::FenceGate { open: true, .. }, _) => false,
        (JoinKind::FenceGate { .. } | JoinKind::Pane | JoinKind::Fence { .. }, Under::Post) => true,
        (JoinKind::Wall, Under::Post) => above.up == Some(true) || above.joined.contains(&true),
        (JoinKind::FenceGate { facing, .. }, Under::Side(side)) => {
            across(facing, Face::SIDES[side])
        }
        (JoinKind::Pane | JoinKind::Fence { .. } | JoinKind::Wall, Under::Side(side)) => {
            above.joined[side]
        }
    }
}

/// A wall's side heights: none where it joins nothing, tall where the block above covers it.
fn heights(
    grid: &Grid<'_>,
    joiners: &[Joiner],
    at: &HashMap<Cell, usize>,
    wall: &Joiner,
) -> [Height; 4] {
    std::array::from_fn(|side| {
        if let Some(written) = wall
            .state
            .properties
            .get(Face::SIDES[side].as_str())
            .and_then(|value| Height::from_value(value))
        {
            return written;
        }
        if !wall.joined[side] {
            Height::None
        } else if covered(grid, joiners, at, wall.cell, Under::Side(side)) {
            Height::Tall
        } else {
            Height::Low
        }
    })
}

/// A wall's `up`: whether it raises its post, read as the game's `WallBlock.shouldRaisePost`.
fn raises_post(
    grid: &Grid<'_>,
    joiners: &[Joiner],
    at: &HashMap<Cell, usize>,
    wall: &Joiner,
) -> bool {
    if let Some(written) = wall.state.properties.get("up").and_then(|v| v.parse().ok()) {
        return written;
    }
    let (above, joins) = grid.joins_beside(wall.cell, Face::Up);
    let above_wall = above.and_then(|above| at.get(&above)).map(|&i| &joiners[i]);
    if above_wall
        .is_some_and(|above| matches!(above.kind, JoinKind::Wall) && above.up == Some(true))
    {
        return true;
    }
    let [north, east, south, west] = wall.heights.map(|height| height == Height::None);
    if (north && east && south && west) || north != south || east != west {
        return true;
    }
    let [n, e, s, w] = wall.heights;
    if (n == Height::Tall && s == Height::Tall) || (e == Height::Tall && w == Height::Tall) {
        return false;
    }
    joins.raises_post || covered(grid, joiners, at, wall.cell, Under::Post)
}

/// The state `joiner` is repainted with: its own, with each connection state it does not write.
fn derived_state(grid: &Grid<'_>, joiner: &Joiner) -> BlockState {
    let mut state = joiner.state.clone();
    let mut set = |key: &str, value: String| {
        state.properties.entry(key.to_owned()).or_insert(value);
    };
    match joiner.kind {
        JoinKind::Pane | JoinKind::Fence { .. } => {
            for (side, joined) in Face::SIDES.iter().zip(joiner.joined) {
                set(side.as_str(), joined.to_string());
            }
        }
        JoinKind::Wall => {
            for (side, height) in Face::SIDES.iter().zip(joiner.heights) {
                set(side.as_str(), height.as_str().to_owned());
            }
            if let Some(up) = joiner.up {
                set("up", up.to_string());
            }
        }
        JoinKind::FenceGate { facing, .. } => {
            // A gate sits lower where its posts meet walls: on its sides, across the way it faces.
            let in_wall = Face::SIDES
                .iter()
                .filter(|&&side| across(facing, side))
                .any(|&side| {
                    matches!(
                        grid.joins_beside(joiner.cell, side).1.kind,
                        Some(JoinKind::Wall)
                    )
                });
            set("in_wall", in_wall.to_string());
        }
    }
    state.properties.sort_keys();
    state
}
