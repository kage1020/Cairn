//! The entries of an NBT compound that `cairn install` looks at, read off a pull reader so that
//! nothing else in the file is held.

use std::io::Read;

use cairn_lang_nbt::tag::Tag;
use cairn_lang_nbt::{Event, NbtReadError, NbtReader};
use flate2::read::MultiGzDecoder;

/// The most elements an array may declare, far past any the game saves in a `level.dat` or a
/// structure: a value is read whole, so a file declaring more is refused before it fills memory.
const MAX_ARRAY_LEN: usize = 1 << 24;

/// A reader of the gzip-wrapped Java NBT in `gzip`.
pub(super) fn java_gzip<R: Read>(gzip: R) -> NbtReader<MultiGzDecoder<R>> {
    NbtReader::java(MultiGzDecoder::new(gzip)).with_max_array_len(MAX_ARRAY_LEN)
}

/// A reader of the Bedrock NBT in `payload`.
pub(super) fn bedrock<R: Read>(payload: R) -> NbtReader<R> {
    NbtReader::bedrock(payload).with_max_array_len(MAX_ARRAY_LEN)
}

/// One entry of a compound.
pub(super) struct Entry {
    /// Its key.
    pub(super) name: String,
    /// Its tag, or which container opens.
    pub(super) body: Body,
}

/// An entry's tag, a compound or list being opened rather than read.
pub(super) enum Body {
    /// A compound whose entries are the reader's next events.
    Compound,
    /// A list whose items are the reader's next events.
    List,
    /// Any other tag, read whole.
    Value(Tag),
}

/// Opens the root compound of `nbt`.
///
/// # Errors
///
/// Those of [`NbtReader::next_event`].
pub(super) fn open_root<R: Read>(nbt: &mut NbtReader<R>) -> Result<(), NbtReadError> {
    match nbt.next_event()? {
        Some(Event::Compound { .. }) => Ok(()),
        _ => unreachable!("a root opens as a compound or fails to read"),
    }
}

/// Hands each entry of the compound `nbt` is in to `visit`, which may read a compound's or list's
/// events, and skips whatever of one it leaves.
///
/// # Errors
///
/// Those of [`NbtReader::next_event`], or the first `visit` returns.
pub(super) fn for_each_entry<R: Read>(
    nbt: &mut NbtReader<R>,
    mut visit: impl FnMut(&mut NbtReader<R>, Entry) -> Result<(), NbtReadError>,
) -> Result<(), NbtReadError> {
    let depth = nbt.depth();
    loop {
        let (name, body) = match nbt.next_event()? {
            Some(Event::Compound { name }) => (name, Body::Compound),
            Some(Event::List { name, .. }) => (name, Body::List),
            Some(Event::Value { name, tag }) => (name, Body::Value(tag)),
            Some(Event::End) | None => return Ok(()),
        };
        let name = name.expect("a compound's entries are named");
        visit(nbt, Entry { name, body })?;
        while nbt.depth() > depth {
            nbt.skip_rest()?;
        }
    }
}
