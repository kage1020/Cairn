//! What a reader holds whole of one file: NBT past [`MAX_HELD_NBT`] is refused by the field that
//! passes it, and a palette past a block array's is refused before an entry of it is read.

use cairn_lang_core::block_array::PALETTE_CAPACITY;
use cairn_lang_formats::MAX_HELD_NBT;
use cairn_lang_formats::bedrock_structure::{McStructureReadError, read_mcstructure};
use cairn_lang_formats::java_structure::{StructureReadError, read_structure};
use cairn_lang_formats::litematic::{LitematicReadError, read_litematic};
use cairn_lang_formats::schem::{SchemReadError, read_schem};
use cairn_lang_nbt::tag::Tag;
use cairn_lang_nbt::{
    CompoundStream, NbtIoError, stream_bedrock_uncompressed, stream_java_uncompressed,
};

use crate::common::gzip;

/// Writes `junk`, a list of one more one-byte tag than [`MAX_HELD_NBT`] holds.
fn junk<W: std::io::Write>(compound: &mut CompoundStream<'_, W>) -> Result<(), NbtIoError> {
    let len = MAX_HELD_NBT / size_of::<Tag>() + 1;
    compound.list("junk", 1, len, |list| {
        (0..len).try_for_each(|_| list.item(&Tag::Byte(0)))
    })
}

/// The gzip-wrapped Java root `body` writes.
fn java(body: impl FnOnce(&mut CompoundStream<'_, Vec<u8>>) -> Result<(), NbtIoError>) -> Vec<u8> {
    let mut bytes = Vec::new();
    stream_java_uncompressed(&mut bytes, "", body).expect("the root writes");
    gzip(&bytes)
}

/// The Bedrock root `body` writes.
fn bedrock(
    body: impl FnOnce(&mut CompoundStream<'_, Vec<u8>>) -> Result<(), NbtIoError>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    stream_bedrock_uncompressed(&mut bytes, "", body).expect("the root writes");
    bytes
}

/// The refusal of the field `path`.
fn held_too_much(path: &str) -> String {
    format!(
        "`{path}` holds more NBT than the {MAX_HELD_NBT} bytes a reader holds whole of one file"
    )
}

#[test]
fn a_structure_file_holding_more_nbt_than_a_reader_holds_is_refused_by_the_field() {
    let entities = java(|root| root.list("entities", 10, 1, |list| list.compound(junk)));
    let block_entity = java(|root| {
        root.list("blocks", 10, 1, |list| {
            list.compound(|block| block.compound("nbt", junk))
        })
    });
    for (bytes, path) in [(entities, "entities"), (block_entity, "blocks[0]")] {
        let err = read_structure(bytes.as_slice()).expect_err("the file is refused");
        assert!(
            matches!(&err, StructureReadError::HeldTooMuch { path: at, limit: MAX_HELD_NBT } if at == path),
            "{err:?}"
        );
        assert_eq!(err.to_string(), held_too_much(path));
    }
}

#[test]
fn a_mcstructure_holding_more_nbt_than_a_reader_holds_is_refused_by_the_field() {
    let entities = bedrock(|root| {
        root.compound("structure", |structure| {
            structure.list("entities", 10, 1, |list| list.compound(junk))
        })
    });
    let block_entity = bedrock(|root| {
        root.compound("structure", |structure| {
            structure.compound("palette", |palettes| {
                palettes.compound("default", |default| {
                    default.compound("block_position_data", |data| {
                        data.compound("0", |entry| entry.compound("block_entity_data", junk))
                    })
                })
            })
        })
    });
    for (bytes, path) in [
        (entities, "structure.entities"),
        (
            block_entity,
            "structure.palette.default.block_position_data",
        ),
    ] {
        let err = read_mcstructure(bytes.as_slice()).expect_err("the file is refused");
        assert!(
            matches!(&err, McStructureReadError::HeldTooMuch { path: at, limit: MAX_HELD_NBT } if at == path),
            "{err:?}"
        );
        assert_eq!(err.to_string(), held_too_much(path));
    }
}

#[test]
fn a_litematic_holding_more_nbt_than_a_reader_holds_is_refused_by_the_field() {
    let region = |list_name: &'static str| {
        java(move |root| {
            root.compound("Regions", |regions| {
                regions.compound("house", |region| {
                    region.list(list_name, 10, 1, |list| list.compound(junk))
                })
            })
        })
    };
    for (bytes, path) in [
        (region("Entities"), "Regions.house.Entities[0]"),
        (region("TileEntities"), "Regions.house.TileEntities[0]"),
    ] {
        let err = read_litematic(bytes.as_slice()).expect_err("the file is refused");
        assert!(
            matches!(&err, LitematicReadError::HeldTooMuch { path: at, limit: MAX_HELD_NBT } if at == path),
            "{err:?}"
        );
        assert_eq!(err.to_string(), held_too_much(path));
    }
}

#[test]
fn a_schem_holding_more_nbt_than_a_reader_holds_is_refused_by_the_field() {
    let entities = java(|root| {
        root.compound("Schematic", |schematic| {
            schematic.list("Entities", 10, 1, |list| list.compound(junk))
        })
    });
    let block_entity = java(|root| {
        root.compound("Schematic", |schematic| {
            schematic.compound("Blocks", |blocks| {
                blocks.list("BlockEntities", 10, 1, |list| list.compound(junk))
            })
        })
    });
    let metadata = java(|root| {
        root.compound("Schematic", |schematic| {
            schematic.compound("Metadata", junk)
        })
    });
    for (bytes, path) in [
        (entities, "Schematic.Entities[0]"),
        (block_entity, "Schematic.Blocks.BlockEntities[0]"),
        (metadata, "Schematic.Metadata"),
    ] {
        let err = read_schem(bytes.as_slice()).expect_err("the file is refused");
        assert!(
            matches!(&err, SchemReadError::HeldTooMuch { path: at, limit: MAX_HELD_NBT } if at == path),
            "{err:?}"
        );
        assert_eq!(err.to_string(), held_too_much(path));
    }
}

/// The bytes that open a list of compounds declaring `len` items, its name `name` if it has one,
/// with nothing after them.
fn compounds_declared(name: Option<&str>, len: i32, big_endian: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(name) = name {
        bytes.push(9);
        let len = u16::try_from(name.len()).expect("a short name");
        bytes.extend(if big_endian {
            len.to_be_bytes()
        } else {
            len.to_le_bytes()
        });
        bytes.extend(name.as_bytes());
    }
    bytes.push(10);
    bytes.extend(if big_endian {
        len.to_be_bytes()
    } else {
        len.to_le_bytes()
    });
    bytes
}

#[test]
fn a_palette_past_a_block_array_is_refused_before_an_entry_of_it_is_read() {
    let declared = 1 << 24;
    let refused = |err: &StructureReadError| {
        matches!(
            err,
            StructureReadError::PaletteTooLarge { len, limit: PALETTE_CAPACITY } if *len == 1 << 24
        )
    };
    let mut palette = vec![10, 0, 0];
    palette.extend(compounds_declared(Some("palette"), declared, true));
    let err = read_structure(gzip(&palette).as_slice()).expect_err("the file is refused");
    assert!(refused(&err), "{err:?}");

    let mut palettes = vec![10, 0, 0, 9, 0, 8];
    palettes.extend(b"palettes");
    palettes.extend([9, 0, 0, 0, 1]);
    palettes.extend(compounds_declared(None, declared, true));
    let err = read_structure(gzip(&palettes).as_slice()).expect_err("the file is refused");
    assert!(refused(&err), "{err:?}");

    let mut mcstructure = vec![10, 0, 0];
    for name in ["structure", "palette", "default"] {
        mcstructure.push(10);
        mcstructure.extend(u16::try_from(name.len()).expect("short").to_le_bytes());
        mcstructure.extend(name.as_bytes());
    }
    mcstructure.extend(compounds_declared(Some("block_palette"), declared, false));
    let err = read_mcstructure(mcstructure.as_slice()).expect_err("the file is refused");
    assert!(
        matches!(
            err,
            McStructureReadError::PaletteTooLarge { len, limit: PALETTE_CAPACITY } if len == 1 << 24
        ),
        "{err:?}"
    );
}
