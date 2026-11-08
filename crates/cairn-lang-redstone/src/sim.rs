//! A redstone tick simulator over a circuit's voxels, on Java's scheduling model: a torch and a
//! comparator change a tick after their input does, a repeater its delay after, and dust at once
//! (`spec/redstone` "Verification").

use std::collections::HashMap;

use cairn_lang_core::block_array::Face;

use crate::geometry::{SIDES, Voxel};
use crate::part::Part;

/// The six faces of a voxel.
const FACES: [Face; 6] = [
    Face::East,
    Face::South,
    Face::West,
    Face::North,
    Face::Up,
    Face::Down,
];

/// The order the components due on one tick take their turn in, by voxel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Order {
    /// Lowest voxel first.
    Forward,
    /// Highest voxel first.
    Reverse,
}

/// How dust joins the cell on one side: not at all, along the floor, or up the block there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Connection {
    /// Not joined.
    None,
    /// Joined at its own height, or down to dust below the cell.
    Side,
    /// Joined up the block to dust on top of it.
    Up,
}

/// A world of parts, the power each holds and the ticks each has pending.
#[derive(Debug, Clone)]
pub(crate) struct Sim {
    at: HashMap<Voxel, usize>,
    voxels: Vec<Voxel>,
    parts: Vec<Part>,
    /// A torch lit or a repeater powered.
    on: Vec<bool>,
    /// A comparator's output strength.
    output: Vec<u8>,
    /// Dust's power level.
    power: Vec<u8>,
    /// Each dust's links to the dust it reads.
    links: Vec<Vec<usize>>,
    /// Each dust's connection toward [`SIDES`].
    sides: Vec<[Connection; 4]>,
    /// Torches, repeaters and comparators, in [`Order::Forward`].
    components: Vec<usize>,
    pending: Vec<Option<u32>>,
    /// A source strongly powering the cell under it, by its own voxel.
    inputs: HashMap<Voxel, u8>,
    now: u32,
    order: Order,
}

impl Sim {
    /// A world of `parts` at rest: every torch lit and every diode off, except where `seeds`
    /// says otherwise, then each part reading what powers it.
    pub(crate) fn new(
        parts: impl IntoIterator<Item = (Voxel, Part)>,
        seeds: impl IntoIterator<Item = (Voxel, bool)>,
        order: Order,
    ) -> Self {
        let mut placed: Vec<(Voxel, Part)> = parts.into_iter().collect();
        placed.sort_by_key(|(voxel, _)| *voxel);
        placed.dedup_by_key(|(voxel, _)| *voxel);
        let at = placed
            .iter()
            .enumerate()
            .map(|(index, (voxel, _))| (*voxel, index))
            .collect();
        let (voxels, parts): (Vec<Voxel>, Vec<Part>) = placed.into_iter().unzip();
        let count = parts.len();
        let mut sim = Self {
            at,
            on: parts
                .iter()
                .map(|part| matches!(part, Part::Torch { .. }))
                .collect(),
            components: (0..count)
                .filter(|&i| {
                    matches!(
                        parts[i],
                        Part::Torch { .. } | Part::Repeater { .. } | Part::Comparator { .. }
                    )
                })
                .collect(),
            voxels,
            parts,
            output: vec![0; count],
            power: vec![0; count],
            links: Vec::new(),
            sides: Vec::new(),
            pending: vec![None; count],
            inputs: HashMap::new(),
            now: 0,
            order,
        };
        sim.links = (0..count).map(|i| sim.dust_links(i)).collect();
        sim.sides = (0..count).map(|i| sim.dust_sides(i)).collect();
        for (voxel, on) in seeds {
            if let Some(&i) = sim.at.get(&voxel) {
                sim.on[i] = on;
            }
        }
        sim.recompute();
        sim.schedule();
        sim
    }

    /// The part at `voxel`.
    pub(crate) fn part(&self, voxel: Voxel) -> Option<Part> {
        self.index(voxel).map(|i| self.parts[i])
    }

    /// Dust's power at `voxel`, 0 for anything else.
    pub(crate) fn power(&self, voxel: Voxel) -> u8 {
        self.index(voxel).map_or(0, |i| self.power[i])
    }

    /// Whether the torch at `voxel` is lit or the repeater there powered.
    pub(crate) fn is_on(&self, voxel: Voxel) -> bool {
        self.index(voxel).is_some_and(|i| self.on[i])
    }

    /// The comparator's output at `voxel`.
    pub(crate) fn output(&self, voxel: Voxel) -> u8 {
        self.index(voxel).map_or(0, |i| self.output[i])
    }

    /// Whether the repeater at `voxel` is locked.
    pub(crate) fn is_locked(&self, voxel: Voxel) -> bool {
        match self.part(voxel) {
            Some(Part::Repeater { facing, .. }) => self.locked(voxel, facing),
            _ => false,
        }
    }

    /// The dust at `voxel`'s connection toward each of [`SIDES`].
    pub(crate) fn sides(&self, voxel: Voxel) -> [Connection; 4] {
        self.index(voxel)
            .map_or([Connection::None; 4], |i| self.sides[i])
    }

    /// Whether no part has a tick pending.
    pub(crate) fn is_settled(&self) -> bool {
        self.pending.iter().all(Option::is_none)
    }

    /// Sets the source at `voxel`, which strongly powers the cell under it, to `level`.
    pub(crate) fn set_input(&mut self, voxel: Voxel, level: u8) {
        if level == 0 {
            self.inputs.remove(&voxel);
        } else {
            self.inputs.insert(voxel, level.min(15));
        }
        self.recompute();
        self.schedule();
    }

    /// Runs one tick: each part due takes its turn, in [`Order`], seeing every change before it.
    pub(crate) fn step(&mut self) {
        self.now += 1;
        let mut due: Vec<usize> = self
            .components
            .iter()
            .copied()
            .filter(|&i| self.pending[i] == Some(self.now))
            .collect();
        if self.order == Order::Reverse {
            due.reverse();
        }
        for i in due {
            self.pending[i] = None;
            if self.tick(i) {
                self.recompute();
            }
            self.schedule();
        }
    }

    /// Runs ticks until none is pending or `limit` have run; whether the world came to rest.
    pub(crate) fn settle(&mut self, limit: u32) -> bool {
        for _ in 0..limit {
            if self.is_settled() {
                return true;
            }
            self.step();
        }
        self.is_settled()
    }

    fn index(&self, voxel: Voxel) -> Option<usize> {
        self.at.get(&voxel).copied()
    }

    fn conducts(&self, voxel: Voxel) -> bool {
        self.part(voxel).is_some_and(Part::conducts)
    }

    fn is_dust(&self, voxel: Voxel) -> bool {
        self.part(voxel) == Some(Part::Dust)
    }

    /// The dust a dust reads: beside it, up a block beside it under no block over it, and down
    /// past a cell beside it that is not a block.
    fn dust_links(&self, i: usize) -> Vec<usize> {
        if self.parts[i] != Part::Dust {
            return Vec::new();
        }
        let dust = self.voxels[i];
        let open_above = !self.conducts(dust.up());
        let mut links = Vec::new();
        for face in SIDES {
            let beside = dust.step(face);
            if self.is_dust(beside) {
                links.extend(self.index(beside));
            } else if self.conducts(beside) {
                if open_above && self.is_dust(beside.up()) {
                    links.extend(self.index(beside.up()));
                }
            } else if self.is_dust(beside.down()) {
                links.extend(self.index(beside.down()));
            }
        }
        links
    }

    /// The way dust joins each side, a lone join running on through the side opposite it and
    /// no join running all four ways.
    fn dust_sides(&self, i: usize) -> [Connection; 4] {
        let mut sides = [Connection::None; 4];
        if self.parts[i] != Part::Dust {
            return sides;
        }
        let dust = self.voxels[i];
        let open_above = !self.conducts(dust.up());
        for (side, face) in sides.iter_mut().zip(SIDES) {
            let beside = dust.step(face);
            *side = if open_above && self.conducts(beside) && self.is_dust(beside.up()) {
                Connection::Up
            } else if self.joins(beside, face)
                || (!self.conducts(beside) && self.is_dust(beside.down()))
            {
                Connection::Side
            } else {
                Connection::None
            };
        }
        // `SIDES` is east, south, west, north.
        let along_z = sides[1] != Connection::None || sides[3] != Connection::None;
        let along_x = sides[0] != Connection::None || sides[2] != Connection::None;
        for (k, side) in sides.iter_mut().enumerate() {
            let runs_on = if k % 2 == 0 { !along_z } else { !along_x };
            if runs_on && *side == Connection::None {
                *side = Connection::Side;
            }
        }
        sides
    }

    /// Whether dust joins the part at `voxel` on its side toward `face`.
    fn joins(&self, voxel: Voxel, face: Face) -> bool {
        match self.part(voxel) {
            Some(Part::Dust | Part::Torch { .. } | Part::Comparator { .. }) => true,
            Some(Part::Repeater { facing, .. }) => facing == face || facing == face.opposite(),
            Some(Part::Block) | None => false,
        }
    }

    /// Whether the dust `i` powers the cell toward `face`.
    fn points(&self, i: usize, face: Face) -> bool {
        SIDES
            .iter()
            .position(|side| *side == face)
            .is_some_and(|k| self.sides[i][k] != Connection::None)
    }

    /// The power a block holds: from a lit torch under it, a diode driving into it and a source
    /// over it, and with `dust`, from dust on it or pointing into it.
    fn block_power(&self, block: Voxel, dust: bool) -> u8 {
        let mut best = self.inputs.get(&block.up()).copied().unwrap_or(0);
        for face in FACES {
            let from = block.step(face);
            let Some(i) = self.index(from) else {
                continue;
            };
            let signal = match self.parts[i] {
                Part::Torch { .. } if face == Face::Down && self.on[i] => 15,
                Part::Repeater { .. } if self.parts[i].drives_into(from, block) && self.on[i] => 15,
                Part::Comparator { .. } if self.parts[i].drives_into(from, block) => self.output[i],
                Part::Dust if dust && (face == Face::Up || self.points(i, face.opposite())) => {
                    self.power[i]
                }
                _ => 0,
            };
            best = best.max(signal);
        }
        best
    }

    /// The power dust takes from what is not dust: a lit torch beside it, a diode driving into
    /// it, a block holding power from anything but dust, and a source over it.
    fn dust_source(&self, dust: Voxel) -> u8 {
        let mut best = self.inputs.get(&dust.up()).copied().unwrap_or(0);
        for face in FACES {
            let from = dust.step(face);
            let Some(i) = self.index(from) else {
                continue;
            };
            let signal = match self.parts[i] {
                Part::Torch { .. } if self.on[i] => 15,
                Part::Repeater { .. } if self.parts[i].drives_into(from, dust) && self.on[i] => 15,
                Part::Comparator { .. } if self.parts[i].drives_into(from, dust) => self.output[i],
                Part::Block => self.block_power(from, false),
                _ => 0,
            };
            best = best.max(signal);
        }
        best
    }

    /// Each dust's power: the most any source gives it, less one for each link from there.
    fn recompute(&mut self) {
        let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); 16];
        for i in 0..self.parts.len() {
            if self.parts[i] != Part::Dust {
                continue;
            }
            let level = self.dust_source(self.voxels[i]);
            self.power[i] = level;
            if level > 0 {
                buckets[usize::from(level)].push(i);
            }
        }
        for level in (2..=15u8).rev() {
            while let Some(i) = buckets[usize::from(level)].pop() {
                if self.power[i] != level {
                    continue;
                }
                for k in 0..self.links[i].len() {
                    let next = self.links[i][k];
                    if self.power[next] < level - 1 {
                        self.power[next] = level - 1;
                        buckets[usize::from(level - 1)].push(next);
                    }
                }
            }
        }
    }

    /// What a diode at `at` driving toward `facing` reads from the cell behind it.
    fn rear_signal(&self, at: Voxel, facing: Face) -> u8 {
        let rear = at.step(facing.opposite());
        let Some(i) = self.index(rear) else {
            return 0;
        };
        match self.parts[i] {
            Part::Dust => self.power[i],
            Part::Block => self.block_power(rear, true),
            Part::Torch { .. } if self.on[i] => 15,
            Part::Repeater { .. } if self.parts[i].drives_into(rear, at) && self.on[i] => 15,
            Part::Comparator { .. } if self.parts[i].drives_into(rear, at) => self.output[i],
            _ => 0,
        }
    }

    /// The cells on either side of a diode at `at` driving toward `facing`.
    fn diode_sides(at: Voxel, facing: Face) -> [Voxel; 2] {
        let side = facing.clockwise().unwrap_or(Face::North);
        [at.step(side), at.step(side.opposite())]
    }

    /// What a diode at `at` reads from its sides: dust's power, and a diode driving into it;
    /// with `diodes_only`, the diodes alone.
    fn side_signal(&self, at: Voxel, facing: Face, diodes_only: bool) -> u8 {
        Self::diode_sides(at, facing)
            .into_iter()
            .filter_map(|side| self.index(side).map(|i| (side, i)))
            .map(|(side, i)| match self.parts[i] {
                Part::Dust if !diodes_only => self.power[i],
                Part::Repeater { .. } if self.parts[i].drives_into(side, at) && self.on[i] => 15,
                Part::Comparator { .. } if self.parts[i].drives_into(side, at) => self.output[i],
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }

    fn locked(&self, at: Voxel, facing: Face) -> bool {
        self.side_signal(at, facing, true) > 0
    }

    fn torch_powered(&self, at: Voxel, facing: Face) -> bool {
        let block = at.step(facing.opposite());
        self.conducts(block) && self.block_power(block, true) > 0
    }

    fn comparator_output(&self, at: Voxel, facing: Face, subtract: bool) -> u8 {
        let rear = self.rear_signal(at, facing);
        let side = self.side_signal(at, facing, false);
        if rear == 0 || side > rear {
            0
        } else if subtract {
            rear - side
        } else {
            rear
        }
    }

    /// Whether the part `i` would change if it took a tick now.
    fn wants_change(&self, i: usize) -> bool {
        let at = self.voxels[i];
        match self.parts[i] {
            Part::Torch { facing } => self.on[i] == self.torch_powered(at, facing),
            Part::Repeater { facing, .. } => {
                !self.locked(at, facing) && self.on[i] != (self.rear_signal(at, facing) > 0)
            }
            Part::Comparator { facing, subtract } => {
                self.comparator_output(at, facing, subtract) != self.output[i]
            }
            Part::Block | Part::Dust => false,
        }
    }

    /// Schedules a tick for each part that wants a change and has none pending.
    fn schedule(&mut self) {
        for k in 0..self.components.len() {
            let i = self.components[k];
            if self.pending[i].is_some() || !self.wants_change(i) {
                continue;
            }
            let delay = match self.parts[i] {
                Part::Repeater { delay, .. } => u32::from(delay),
                _ => 1,
            };
            self.pending[i] = Some(self.now + delay);
        }
    }

    /// Runs the part `i`'s tick; whether it changed.
    fn tick(&mut self, i: usize) -> bool {
        let at = self.voxels[i];
        match self.parts[i] {
            Part::Torch { facing } => {
                let lit = !self.torch_powered(at, facing);
                let changed = lit != self.on[i];
                self.on[i] = lit;
                changed
            }
            Part::Repeater { facing, delay } => {
                if self.locked(at, facing) {
                    return false;
                }
                let input = self.rear_signal(at, facing) > 0;
                if self.on[i] {
                    if input {
                        return false;
                    }
                    self.on[i] = false;
                } else {
                    self.on[i] = true;
                    // Turned on, a repeater stays on for its delay at least.
                    if !input {
                        self.pending[i] = Some(self.now + u32::from(delay));
                    }
                }
                true
            }
            Part::Comparator { facing, subtract } => {
                let output = self.comparator_output(at, facing, subtract);
                let changed = output != self.output[i];
                self.output[i] = output;
                changed
            }
            Part::Block | Part::Dust => false,
        }
    }
}

#[cfg(test)]
mod tests;
