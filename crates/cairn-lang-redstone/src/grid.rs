//! The voxels of a `circuit` reservation as placement and routing see them: what stands in each,
//! and what each must stay clear of (`spec/redstone` "Place-and-route").

use cairn_lang_core::block_array::Face;

use crate::geometry::{SIDES, Voxel};
use crate::part::Part;
use crate::placement_ir::{CircuitLayout, PlacedTile, Reservation, Wire};
use crate::tile::Template;

/// Whose dust a voxel holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Label {
    /// A net's, which its wire may join.
    Net(usize),
    /// A tile's own, between its ports and its output, which no wire may run beside.
    Internal(usize),
}

/// What one voxel holds or must stay clear of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    /// Nothing, and free to take.
    Open,
    /// Nothing yet, and never dust or a diode: a torch or a powered block beside it would drive one.
    NoDust,
    /// Nothing, ever: a climb runs past it or a torch under it would power a block there.
    Air,
    /// A tile's part other than dust.
    Part(Part),
    /// A block laid under dust, or the bottom layer.
    Support,
    /// Dust, a tile's or a wire's.
    Dust(Label),
}

impl Slot {
    /// Whether the voxel holds a block that passes power on.
    pub(crate) const fn conducts(self) -> bool {
        matches!(self, Self::Support | Self::Part(Part::Block))
    }
}

/// Why a tile or a wire cannot take the voxels it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Collision {
    pub(crate) at: Voxel,
}

/// Every voxel of one reservation.
#[derive(Debug, Clone)]
pub(crate) struct Grid {
    pub(crate) region: Reservation,
    slots: Vec<Slot>,
    /// What each voxel held before [`Self::trial`] wrote it, while one runs.
    journal: Option<Vec<(usize, Slot)>>,
}

impl Grid {
    /// The reservation with its bottom layer laid and nothing else.
    pub(crate) fn new(region: &Reservation) -> Self {
        let size = (region.width as usize) * (region.layers as usize) * (region.depth as usize);
        let mut grid = Self {
            region: region.clone(),
            slots: vec![Slot::Open; size],
            journal: None,
        };
        for z in 0..region.depth {
            for x in 0..region.width {
                grid.set(Voxel::new(coord(x), 0, coord(z)), Slot::Support);
            }
        }
        grid
    }

    /// The layout's tiles and its wires so far.
    pub(crate) fn of(layout: &CircuitLayout) -> Result<Self, Collision> {
        let mut grid = Self::new(&layout.region);
        for (index, tile) in layout.tiles.iter().enumerate() {
            grid.lay_tile(index, tile, &tile.kind.template(), &labels(layout, index))?;
        }
        for (net, circuit_net) in layout.nets.iter().enumerate() {
            grid.lay_wire(net, &circuit_net.wire)?;
        }
        Ok(grid)
    }

    fn index(&self, at: Voxel) -> Option<usize> {
        self.region.contains(at).then(|| {
            let (width, layers) = (self.region.width as usize, self.region.layers as usize);
            let [x, y, z] = [at.x, at.y, at.z].map(|c| c.unsigned_abs() as usize);
            (z * layers + y) * width + x
        })
    }

    /// What `at` holds, or `None` outside the reservation.
    pub(crate) fn get(&self, at: Voxel) -> Option<Slot> {
        self.index(at).and_then(|i| self.slots.get(i)).copied()
    }

    /// Puts `slot` at `at`, whatever it held.
    pub(crate) fn replace(&mut self, at: Voxel, slot: Slot) {
        self.set(at, slot);
    }

    fn set(&mut self, at: Voxel, slot: Slot) {
        if let Some(i) = self.index(at) {
            if let Some(journal) = &mut self.journal {
                journal.push((i, self.slots[i]));
            }
            self.slots[i] = slot;
        }
    }

    /// Whether `tile` lays as [`Self::lay_tile`] lays it and `holds` of the grid it leaves, the
    /// grid put back as it was either way.
    pub(crate) fn trial(
        &mut self,
        index: usize,
        tile: &PlacedTile,
        template: &Template,
        labels: &[(crate::geometry::Rel, usize)],
        holds: impl FnOnce(&Self) -> bool,
    ) -> bool {
        self.journal = Some(Vec::new());
        let laid = self.lay_tile(index, tile, template, labels).is_ok();
        let journal = self.journal.take().unwrap_or_default();
        let held = laid && holds(self);
        for (i, slot) in journal.into_iter().rev() {
            self.slots[i] = slot;
        }
        held
    }

    /// Whether `at` holds a block that passes power on.
    pub(crate) fn conducts(&self, at: Voxel) -> bool {
        self.get(at).is_some_and(Slot::conducts)
    }

    /// Keeps dust and diodes off `at`; outside the reservation nothing is kept.
    fn keep_dust_off(&mut self, at: Voxel) -> Result<(), Collision> {
        match self.get(at) {
            Some(Slot::Open) => {
                self.set(at, Slot::NoDust);
                Ok(())
            }
            Some(Slot::Dust(_) | Slot::Part(Part::Repeater { .. } | Part::Comparator { .. })) => {
                Err(Collision { at })
            }
            None | Some(_) => Ok(()),
        }
    }

    /// Keeps `at` empty; outside the reservation nothing is kept.
    pub(crate) fn keep_empty(&mut self, at: Voxel) -> Result<(), Collision> {
        match self.get(at) {
            None | Some(Slot::Air) => Ok(()),
            Some(Slot::Open | Slot::NoDust) => {
                self.set(at, Slot::Air);
                Ok(())
            }
            Some(_) => Err(Collision { at }),
        }
    }

    /// Lays a block under dust at `at`.
    pub(crate) fn lay_support(&mut self, at: Voxel) -> Result<(), Collision> {
        match self.get(at) {
            Some(Slot::Open | Slot::NoDust) => {
                self.set(at, Slot::Support);
                Ok(())
            }
            Some(slot) if slot.conducts() => Ok(()),
            _ => Err(Collision { at }),
        }
    }

    fn lay_part(&mut self, at: Voxel, part: Part, label: Label) -> Result<(), Collision> {
        let slot = self.get(at).ok_or(Collision { at })?;
        let fits = match part {
            Part::Block | Part::Torch { .. } => matches!(slot, Slot::Open | Slot::NoDust),
            Part::Dust | Part::Repeater { .. } | Part::Comparator { .. } => slot == Slot::Open,
        };
        if !fits {
            return Err(Collision { at });
        }
        self.set(
            at,
            match part {
                Part::Dust => Slot::Dust(label),
                other => Slot::Part(other),
            },
        );
        Ok(())
    }

    /// Lays `tile`, the layout's tile `index`, with its reservations around it and the blocks
    /// under its parts above the bottom layers; `labels` names the net of each port and output.
    pub(crate) fn lay_tile(
        &mut self,
        index: usize,
        tile: &PlacedTile,
        template: &Template,
        labels: &[(crate::geometry::Rel, usize)],
    ) -> Result<(), Collision> {
        let parts = tile.parts(template);
        let mine: Vec<Voxel> = parts.iter().map(|(at, _)| *at).collect();
        for ((rel, _), (at, part)) in template.parts.iter().zip(&parts) {
            let label = labels
                .iter()
                .find_map(|(port, net)| (port == rel).then_some(Label::Net(*net)))
                .unwrap_or(Label::Internal(index));
            if *part == Part::Dust
                && self.joins_dust(*at, |cell, other| !mine.contains(&cell) && other != label)
            {
                return Err(Collision { at: *at });
            }
            self.lay_part(*at, *part, label)?;
        }
        let keepout = template.keepout();
        for rel in keepout.no_dust {
            self.keep_dust_off(tile.at(rel))?;
        }
        for rel in keepout.air {
            self.keep_empty(tile.at(rel))?;
        }
        for (at, part) in &parts {
            if at.y >= 2 && !matches!(part, Part::Block | Part::Torch { .. }) {
                let under = at.down();
                if !parts.iter().any(|(v, _)| *v == under) {
                    self.lay_support(under)?;
                }
            }
        }
        Ok(())
    }

    /// Whether dust at `at` would join dust beside it, level or a step up or down, that `foreign`
    /// holds is another signal's.
    fn joins_dust(&self, at: Voxel, foreign: impl Fn(Voxel, Label) -> bool) -> bool {
        SIDES.into_iter().any(|face| {
            let beside = at.step(face);
            [beside, beside.up(), beside.down()].into_iter().any(
                |cell| matches!(self.get(cell), Some(Slot::Dust(label)) if foreign(cell, label)),
            )
        })
    }

    /// Lays `wire`, the dust of net `net`.
    pub(crate) fn lay_wire(&mut self, net: usize, wire: &Wire) -> Result<(), Collision> {
        for at in &wire.supports {
            self.lay_support(*at)?;
        }
        for at in &wire.air {
            self.keep_empty(*at)?;
        }
        for at in &wire.dust {
            match self.get(*at) {
                Some(Slot::Dust(Label::Net(own))) if own == net => {}
                Some(Slot::Open) => self.set(*at, Slot::Dust(Label::Net(net))),
                _ => return Err(Collision { at: *at }),
            }
        }
        for buffer in &wire.buffers {
            self.set(
                buffer.at,
                Slot::Part(Part::Repeater {
                    facing: buffer.facing,
                    delay: 1,
                }),
            );
        }
        Ok(())
    }

    /// Whether a repeater or comparator drives into `at`.
    pub(crate) fn driven(&self, at: Voxel) -> bool {
        SIDES.into_iter().any(|face| {
            let from = at.step(face);
            matches!(self.get(from), Some(Slot::Part(part)) if part.drives_into(from, at))
        })
    }

    /// Whether a torch stands at `at`.
    pub(crate) fn torch(&self, at: Voxel) -> bool {
        matches!(self.get(at), Some(Slot::Part(Part::Torch { .. })))
    }

    /// The face of `at` toward `to`, beside it in its own layer.
    pub(crate) fn toward(at: Voxel, to: Voxel) -> Option<Face> {
        SIDES.into_iter().find(|face| at.step(*face) == to)
    }
}

/// The nets the layout's tile `index` reads and drives, by the dust that carries each.
pub(crate) fn labels(layout: &CircuitLayout, index: usize) -> Vec<(crate::geometry::Rel, usize)> {
    let tile = &layout.tiles[index];
    let template = tile.kind.template();
    let mut labels = Vec::new();
    for input in &tile.inputs {
        if let Some(port) = template.port(input.port) {
            labels.push((port.at, input.net));
        }
    }
    if let Some(net) = tile.output {
        for out in &template.outs {
            labels.push((*out, net));
        }
    }
    labels
}

fn coord(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}
