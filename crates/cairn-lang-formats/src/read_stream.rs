//! What the readers share over `cairn-lang-nbt`'s pull reader.

use std::io::Read;

use cairn_lang_nbt::tag::{Compound, Tag};
use cairn_lang_nbt::{Event, NbtReadError, NbtReader};

use crate::volume::MAX_HELD_NBT;

/// The type id of `TAG_Compound`.
const COMPOUND_ID: u8 = 10;

/// The type id an empty list reads with.
const END_ID: u8 = 0;

/// Whether a list whose items carry `element_type_id` holds compounds, or nothing.
pub(crate) fn lists_compounds(element_type_id: u8) -> bool {
    element_type_id == COMPOUND_ID || element_type_id == END_ID
}

/// The next event inside a compound or list that is open.
pub(crate) fn next_inside<R: Read>(nbt: &mut NbtReader<R>) -> Result<Event, NbtReadError> {
    Ok(nbt
        .next_event()?
        .expect("an open compound or list closes before the root does"))
}

/// What is left of [`MAX_HELD_NBT`] while one file is read.
#[derive(Debug)]
pub(crate) struct Held {
    left: usize,
}

impl Held {
    /// All of [`MAX_HELD_NBT`].
    pub(crate) const fn new() -> Self {
        Self { left: MAX_HELD_NBT }
    }

    /// What is left of the open compound or list, read whole within what is left here, and the
    /// memory it takes; `None` when it would take more.
    pub(crate) fn read<R: Read>(
        &self,
        nbt: &mut NbtReader<R>,
    ) -> Result<Option<(Tag, usize)>, NbtReadError> {
        match nbt.read_rest_within(self.left) {
            Ok(read) => Ok(Some(read)),
            Err(NbtReadError::HeldTooMuch { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Keeps `bytes` of what is left.
    pub(crate) fn keep(&mut self, bytes: usize) {
        self.left = self.left.saturating_sub(bytes);
    }

    /// [`Self::read`], what it reads kept.
    pub(crate) fn rest<R: Read>(
        &mut self,
        nbt: &mut NbtReader<R>,
    ) -> Result<Option<Tag>, NbtReadError> {
        Ok(self.read(nbt)?.map(|(tag, taken)| {
            self.keep(taken);
            tag
        }))
    }

    /// [`Self::rest`] of the open compound.
    pub(crate) fn rest_of_compound<R: Read>(
        &mut self,
        nbt: &mut NbtReader<R>,
    ) -> Result<Option<Compound>, NbtReadError> {
        Ok(self.rest(nbt)?.map(|tag| {
            let Tag::Compound(compound) = tag else {
                unreachable!("the rest of a compound reads as a compound");
            };
            compound
        }))
    }
}
