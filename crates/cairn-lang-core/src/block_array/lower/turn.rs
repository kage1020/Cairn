//! A def placed in a body, turned about the vertical axis with the blocks it builds
//! (`spec/components-editing-sites` "A `def` placed in a body").

use indexmap::IndexMap;

use crate::block_array::BlockState;
use crate::block_array::joins::Face;

use super::cell_box::Cell;

/// Whole quarter turns clockwise, seen from above, from 0 to 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Turn(u8);

/// The compass words of the four sideways faces, clockwise from north.
const COMPASS: [&str; 4] = ["north", "east", "south", "west"];

impl Turn {
    /// The turn that brings a front written toward `south` to look toward `face`, or `None` for
    /// `up` and `down`.
    pub(super) fn to_face(face: Face) -> Option<Self> {
        let at = |face: Face| Face::SIDES.iter().position(|side| *side == face);
        let (to, from) = (at(face)?, at(Face::South)?);
        u8::try_from((to + 4 - from) % 4).ok().map(Self)
    }

    /// The `(x, z)` extent a `(width, depth)` footprint takes once turned.
    pub(super) fn footprint(self, (width, depth): (u32, u32)) -> (u32, u32) {
        if self.0.is_multiple_of(2) {
            (width, depth)
        } else {
            (depth, width)
        }
    }

    /// Where `cell` of a `(width, depth)` footprint lands once turned, measured from the turned
    /// footprint's low corner.
    pub(super) fn cell(self, (x, y, z): Cell, (width, depth): (u32, u32)) -> Cell {
        let far = |extent: u32, at: u32| extent.saturating_sub(1).saturating_sub(at);
        match self.0 {
            1 => (far(depth, z), y, x),
            2 => (far(width, x), y, far(depth, z)),
            3 => (z, y, far(width, x)),
            _ => (x, y, z),
        }
    }

    /// The compass word `word` turns to, or `None` for a word that names none.
    fn compass(self, word: &str) -> Option<&'static str> {
        let at = COMPASS.iter().position(|name| *name == word)?;
        Some(COMPASS[(at + usize::from(self.0)) % 4])
    }

    /// `state` turned: a sideways `facing`, an `axis` of `x` or `z`, a `rotation`, each of
    /// `north`, `east`, `south` and `west`, and the compass words of a rail's `shape`.
    pub(super) fn state(self, state: &BlockState) -> BlockState {
        if self.0 == 0 {
            return state.clone();
        }
        let mut properties = IndexMap::with_capacity(state.properties.len());
        for (key, value) in &state.properties {
            let turned = match key.as_str() {
                "facing" => self.compass(value).map(str::to_owned),
                "axis" if self.0 % 2 == 1 => match value.as_str() {
                    "x" => Some("z".to_owned()),
                    "z" => Some("x".to_owned()),
                    _ => None,
                },
                "rotation" => value
                    .parse::<u8>()
                    .ok()
                    .filter(|rotation| *rotation < 16)
                    .map(|rotation| ((rotation + 4 * self.0) % 16).to_string()),
                "shape" => self.rail_shape(value),
                _ => None,
            };
            let key = self.compass(key).map_or_else(|| key.clone(), str::to_owned);
            properties.insert(key, turned.unwrap_or_else(|| value.clone()));
        }
        BlockState {
            id: state.id.clone(),
            properties,
        }
    }

    /// A rail's `shape` turned, or `None` for a value that names no compass direction, as a
    /// stair's does.
    fn rail_shape(self, shape: &str) -> Option<String> {
        if let Some(toward) = shape.strip_prefix("ascending_") {
            return Some(format!("ascending_{}", self.compass(toward)?));
        }
        let (a, b) = shape.split_once('_')?;
        let mut ends = [self.compass(a)?, self.compass(b)?];
        // The game names a rail by its ends in this order.
        ends.sort_by_key(|end| {
            ["north", "south", "east", "west"]
                .iter()
                .position(|n| n == end)
        });
        Some(ends.join("_"))
    }
}

#[cfg(test)]
mod tests;
