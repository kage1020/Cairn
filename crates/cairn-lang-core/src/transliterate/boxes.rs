//! Cutting a grid into boxes of one block, and folding boxes repeated at a fixed step.

use std::collections::{BTreeMap, HashSet};

use crate::block_array::Dims;

/// A cell, `(x, y, z)`.
pub(super) type Cell = (u32, u32, u32);

/// A box of cells that all hold one written block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Cuboid {
    /// The low corner.
    pub(super) low: Cell,
    /// The extents along `x`, `y` and `z`, each at least 1.
    pub(super) size: Cell,
    /// The written block every cell holds.
    pub(super) slot: u32,
}

impl Cuboid {
    /// The high corner.
    pub(super) fn high(self) -> Cell {
        (
            self.low.0 + self.size.0 - 1,
            self.low.1 + self.size.1 - 1,
            self.low.2 + self.size.2 - 1,
        )
    }
}

/// A box written `count` times, each copy `step` past the one before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Folded {
    /// The first copy.
    pub(super) cuboid: Cuboid,
    /// How many copies, `1` for a box written once.
    pub(super) count: u32,
    /// The distance from one copy to the next, `(0, 0, 0)` for a box written once.
    pub(super) step: Cell,
}

impl Folded {
    /// The highest row the last copy reaches.
    pub(super) fn top(self) -> u32 {
        self.cuboid.high().1 + (self.count - 1) * self.step.1
    }
}

/// The boxes `slots` cuts into, `None` being a cell left empty: each grown from its first cell in
/// `y`, `z`, `x` order along `x`, then `z`, then `y`, as far as the cells hold its block.
pub(super) fn cut(dims: Dims, slots: &[Option<u32>]) -> Vec<Cuboid> {
    let at = |x: u32, y: u32, z: u32| dims.index(x, y, z).expect("the cut stays inside the grid");
    let mut taken = vec![false; slots.len()];
    let mut out = Vec::new();
    for y in 0..dims.y {
        for z in 0..dims.z {
            for x in 0..dims.x {
                let first = at(x, y, z);
                let Some(slot) = slots[first].filter(|_| !taken[first]) else {
                    continue;
                };
                let free = |x: u32, y: u32, z: u32| {
                    let i = at(x, y, z);
                    !taken[i] && slots[i] == Some(slot)
                };
                let mut width = 1;
                while x + width < dims.x && free(x + width, y, z) {
                    width += 1;
                }
                let row = |y: u32, z: u32| (x..x + width).all(|x| free(x, y, z));
                let mut depth = 1;
                while z + depth < dims.z && row(y, z + depth) {
                    depth += 1;
                }
                let mut height = 1;
                while y + height < dims.y && (z..z + depth).all(|z| row(y + height, z)) {
                    height += 1;
                }
                for cy in y..y + height {
                    for cz in z..z + depth {
                        for cx in x..x + width {
                            taken[at(cx, cy, cz)] = true;
                        }
                    }
                }
                out.push(Cuboid {
                    low: (x, y, z),
                    size: (width, height, depth),
                    slot,
                });
            }
        }
    }
    out
}

/// How many later boxes of a kind a box tries as the second copy of a run.
const PARTNERS: usize = 32;

/// `cuboids` with each run of two or more boxes of one block and size, at a fixed step, folded
/// into one, in the order of each run's first box.
pub(super) fn fold(cuboids: Vec<Cuboid>) -> Vec<Folded> {
    let mut kinds: BTreeMap<(u32, Cell), Vec<Cuboid>> = BTreeMap::new();
    for cuboid in cuboids {
        kinds
            .entry((cuboid.slot, cuboid.size))
            .or_default()
            .push(cuboid);
    }
    let mut out = Vec::new();
    for mut kind in kinds.into_values() {
        kind.sort_by_key(|c| (c.low.1, c.low.2, c.low.0));
        let mut left: HashSet<Cell> = kind.iter().map(|c| c.low).collect();
        for (i, first) in kind.iter().enumerate() {
            if !left.contains(&first.low) {
                continue;
            }
            let mut best = (1, (0, 0, 0));
            for second in kind[i + 1..].iter().take(PARTNERS) {
                let Some(step) = step_between(first.low, second.low) else {
                    continue;
                };
                if !left.contains(&second.low) {
                    continue;
                }
                let count = run(first.low, step, &left);
                if count > best.0 {
                    best = (count, step);
                }
            }
            let (count, step) = best;
            for k in 0..count {
                left.remove(&shifted(first.low, step, k));
            }
            out.push(Folded {
                cuboid: *first,
                count,
                step,
            });
        }
    }
    out.sort_by_key(|f| (f.cuboid.low.1, f.cuboid.low.2, f.cuboid.low.0));
    out
}

/// The step from `a` to `b` when no component of it is negative.
fn step_between(a: Cell, b: Cell) -> Option<Cell> {
    Some((
        b.0.checked_sub(a.0)?,
        b.1.checked_sub(a.1)?,
        b.2.checked_sub(a.2)?,
    ))
}

/// `low` moved `k` steps.
fn shifted(low: Cell, step: Cell, k: u32) -> Cell {
    (low.0 + k * step.0, low.1 + k * step.1, low.2 + k * step.2)
}

/// How many boxes of `left` stand at `low` and each step past it, without a gap.
fn run(low: Cell, step: Cell, left: &HashSet<Cell>) -> u32 {
    let mut count = 0;
    while left.contains(&shifted(low, step, count)) {
        count += 1;
    }
    count
}
