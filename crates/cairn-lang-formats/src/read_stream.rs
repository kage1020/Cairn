//! What the readers share over `cairn-lang-nbt`'s pull reader.

use std::io::Read;

use cairn_lang_nbt::tag::{Compound, Tag};
use cairn_lang_nbt::{Event, NbtReadError, NbtReader};

/// The type id of `TAG_Compound`.
pub(crate) const COMPOUND_ID: u8 = 10;

/// The type id an empty list reads with.
pub(crate) const END_ID: u8 = 0;

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

/// What is left of the open compound, read whole.
pub(crate) fn rest_of_compound<R: Read>(nbt: &mut NbtReader<R>) -> Result<Compound, NbtReadError> {
    let Tag::Compound(compound) = nbt.read_rest()? else {
        unreachable!("the rest of a compound reads as a compound");
    };
    Ok(compound)
}
