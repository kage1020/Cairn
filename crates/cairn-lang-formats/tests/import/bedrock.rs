//! Importing a `.mcstructure`: its blocks spelled in the canonical vocabulary, its water layer,
//! how it is stamped, and what it loses.

use cairn_lang_core::CAIRN_VERSION;
use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockArray, BlockState, Dims, Palette, PaletteIndex};
use cairn_lang_formats::bedrock_structure::{
    build_mcstructure_tag, read_mcstructure, write_mcstructure,
};
use cairn_lang_formats::data_version::{BedrockTarget, resolve_bedrock_target};
use cairn_lang_formats::import::{
    ImportError, ImportFormat, ImportStamp, RawImport, SemanticLoss, StampFit, import_raw,
};
use cairn_lang_nbt::tag::{Compound, Tag};

use crate::common::{BedrockCell, bedrock_cells, compile_source, states_for};

fn target(version: &str) -> BedrockTarget {
    resolve_bedrock_target(version).expect("a shipped target")
}

/// An array named `name` of `dims` whose voxels, in `(y, z, x)` order, are the states `cells`
/// spells, `.` for air.
fn array(name: &str, dims: Dims, cells: &[&str]) -> BlockArray {
    let mut palette = Palette::new_with_air();
    let voxels = cells
        .iter()
        .map(|spelled| {
            if *spelled == "." {
                return PaletteIndex::AIR;
            }
            let (id, rest) = spelled.split_once('[').unwrap_or((spelled, ""));
            let mut state = BlockState::bare(id);
            for pair in rest
                .trim_end_matches(']')
                .split(',')
                .filter(|p| !p.is_empty())
            {
                let (key, value) = pair.split_once('=').expect("key=value");
                state.properties.insert(key.to_owned(), value.to_owned());
            }
            palette.intern(state)
        })
        .collect();
    BlockArray {
        dims,
        palette,
        voxels,
        block_entities: vec![],
        entities: vec![],
        source_scope: format!("struct::{name}"),
        derived: Vec::new(),
    }
}

/// The root `array` writes as at `version`.
fn root(array: &BlockArray, version: &str) -> Compound {
    let target = target(version);
    build_mcstructure_tag(array, &target, &states_for(&target))
        .expect("the array writes")
        .0
}

fn bytes(root: &Compound) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_mcstructure(&mut bytes, root).expect("the root writes");
    bytes
}

fn imported(root: &Compound, name: &str) -> RawImport {
    import_raw(bytes(root).as_slice(), ImportFormat::Mcstructure, name)
        .unwrap_or_else(|error| panic!("{name} does not import: {error}"))
}

/// The `structure.palette.default` compound of `root`, to edit.
fn default_palette(root: &mut Compound) -> &mut Compound {
    let Some(Tag::Compound(structure)) = root.entries.get_mut("structure") else {
        panic!("a structure");
    };
    let Some(Tag::Compound(palette)) = structure.entries.get_mut("palette") else {
        panic!("a palette");
    };
    let Some(Tag::Compound(default)) = palette.entries.get_mut("default") else {
        panic!("a default palette");
    };
    default
}

/// The `block_palette` entries of `root`, to edit.
fn block_palette(root: &mut Compound) -> &mut Vec<Tag> {
    let Some(Tag::List(list)) = default_palette(root).entries.get_mut("block_palette") else {
        panic!("a block palette");
    };
    &mut list.items
}

/// The `block_indices` layer `layer` of `root`, to edit.
fn layer(root: &mut Compound, layer: usize) -> &mut Vec<Tag> {
    let Some(Tag::Compound(structure)) = root.entries.get_mut("structure") else {
        panic!("a structure");
    };
    let Some(Tag::List(layers)) = structure.entries.get_mut("block_indices") else {
        panic!("block indices");
    };
    let Tag::List(layer) = &mut layers.items[layer] else {
        panic!("a layer");
    };
    &mut layer.items
}

fn header(file: &str, saved: &str, version: &str, reason: &str) -> String {
    format!(
        "# imported from `{file}`, a Bedrock structure saved at {saved}\n\
         # stamped bedrock {version}: {reason}\n\
         # raw transliteration: blocks and their regularity, not the author's intent \
         (W_SEMANTIC_LOSS)\n\
         @cairn {CAIRN_VERSION}\n\
         @requires bedrock version>={version}\n\
         @intended_targets [\"{version}\"]\n"
    )
}

fn loss(unit: &str, what: &str) -> SemanticLoss {
    SemanticLoss {
        unit: Some(unit.to_owned()),
        what: what.to_owned(),
    }
}

/// Each cell of a `.mcstructure`'s bytes.
fn cells(bytes: &[u8]) -> Vec<BedrockCell> {
    bedrock_cells(&read_mcstructure(bytes).expect("the file reads").volume)
}

#[test]
fn a_bedrock_structure_imports_in_the_canonical_vocabulary_and_builds_the_same_file() {
    let built = array(
        "stoop",
        Dims { x: 3, y: 2, z: 1 },
        &[
            "minecraft:stone",
            "minecraft:stone",
            "minecraft:stone",
            "minecraft:oak_stairs[facing=east,half=top,waterlogged=true]",
            "minecraft:redstone_torch[lit=false]",
            "minecraft:oak_slab[type=double]",
        ],
    );
    let root = root(&built, "1.21.60");
    let import = imported(&root, "stoop.mcstructure");
    assert_eq!(
        import.transliteration.source,
        format!(
            "{}\n\
             struct stoop size=3x1\n  \
               raw_fill mat=minecraft:stone from=0,0,0 to=2,0,0 origin=imported\n  \
               raw_block mat=minecraft:oak_stairs[facing=east,half=top,waterlogged=true] \
             at=0,1,0 origin=imported\n  \
               raw_block mat=minecraft:redstone_torch[lit=false] at=1,1,0 origin=imported\n  \
               raw_block mat=minecraft:oak_slab[type=double] at=2,1,0 origin=imported\n",
            header(
                "stoop.mcstructure",
                "block version 1.21.60.33",
                "1.21.60",
                "a shipped target that saves that block version"
            )
        )
    );
    assert_eq!(
        import.decoded.losses.len(),
        1,
        "{:?}",
        import.decoded.losses
    );
    let compiled = compile_source(&import.transliteration.source, Edition::Bedrock, "1.21.60")
        .expect("the import builds");
    let again = self::root(&compiled.structures["struct::stoop"], "1.21.60");
    assert_eq!(cells(&bytes(&again)), cells(&bytes(&root)));
}

#[test]
fn a_shared_block_version_is_stamped_with_the_oldest_target_whose_map_spells_every_block() {
    let corner = array(
        "corner",
        Dims { x: 1, y: 1, z: 1 },
        &["minecraft:oak_stairs[shape=outer_left]"],
    );
    let plain = array("plain", Dims { x: 1, y: 1, z: 1 }, &["minecraft:stone"]);
    let stamped = |array: &BlockArray| {
        imported(&root(array, "1.26.60"), "a.mcstructure")
            .decoded
            .stamp
            .mc_version
    };
    assert_eq!(
        (stamped(&corner), stamped(&plain)),
        ("1.26.60".to_owned(), "1.21.60".to_owned())
    );
}

#[test]
fn a_block_version_between_shipped_targets_is_stamped_with_the_next() {
    let mut root = root(
        &array("one", Dims { x: 1, y: 1, z: 1 }, &["minecraft:stone"]),
        "1.21.40",
    );
    for entry in block_palette(&mut root) {
        let Tag::Compound(entry) = entry else {
            panic!("a compound");
        };
        entry.insert("version", Tag::Int(18_160_000));
    }
    let import = imported(&root, "one.mcstructure");
    assert_eq!(
        import.decoded.stamp,
        ImportStamp {
            edition: Edition::Bedrock,
            mc_version: "1.21.40".to_owned(),
            saved: Some(18_160_000),
            fit: StampFit::Later,
        }
    );
    assert!(import.transliteration.source.starts_with(&header(
        "one.mcstructure",
        "block version 1.21.25.128",
        "1.21.40",
        "the nearest shipped target after the version the file was saved at"
    )));
}

#[test]
fn a_palette_with_no_version_is_stamped_with_the_latest_target() {
    let mut root = root(
        &array("one", Dims { x: 1, y: 1, z: 1 }, &["minecraft:stone"]),
        "1.21.60",
    );
    for entry in block_palette(&mut root) {
        let Tag::Compound(entry) = entry else {
            panic!("a compound");
        };
        entry.entries.shift_remove("version");
    }
    let import = imported(&root, "one.mcstructure");
    assert!(import.transliteration.source.starts_with(&header(
        "one.mcstructure",
        "no block version",
        "1.26.60",
        "the latest shipped target, since the file names no version"
    )));
}

#[test]
fn a_block_version_past_every_shipped_target_is_refused() {
    let mut root = root(
        &array("one", Dims { x: 1, y: 1, z: 1 }, &["minecraft:stone"]),
        "1.21.60",
    );
    for entry in block_palette(&mut root) {
        let Tag::Compound(entry) = entry else {
            panic!("a compound");
        };
        entry.insert("version", Tag::Int(0x0140_0000));
    }
    let error = import_raw(
        bytes(&root).as_slice(),
        ImportFormat::Mcstructure,
        "one.mcstructure",
    )
    .expect_err("no target checks a file from the future");
    assert!(matches!(error, ImportError::Unshipped(_)), "{error:?}");
    assert_eq!(
        error.to_string(),
        "the file was saved at block version 1.64.0.0, past every bedrock target this build \
         ships (the newest is 1.26.60), so no target can check its blocks. Fix: save it from \
         1.26.60 or older, or import it with a newer cairn"
    );
}

#[test]
fn what_no_raw_member_writes_is_reported_lost() {
    let mut root = root(
        &array(
            "shed",
            Dims { x: 4, y: 1, z: 1 },
            &[
                "minecraft:stone",
                "minecraft:stone",
                "minecraft:dirt",
                "minecraft:dirt",
            ],
        ),
        "1.21.60",
    );
    let mut snow = Compound::new();
    snow.insert("name", Tag::String("minecraft:snow_layer".to_owned()));
    snow.insert("states", Tag::Compound(Compound::new()));
    snow.insert("version", Tag::Int(target("1.21.60").block_version));
    let palette = block_palette(&mut root);
    palette.push(Tag::Compound(snow));
    let snow = i32::try_from(palette.len() - 1).expect("a small palette");
    let water = {
        let mut water = Compound::new();
        water.insert("name", Tag::String("minecraft:water".to_owned()));
        let mut states = Compound::new();
        states.insert("liquid_depth", Tag::Int(0));
        water.insert("states", Tag::Compound(states));
        water.insert("version", Tag::Int(target("1.21.60").block_version));
        water
    };
    palette.push(Tag::Compound(water));
    let water = snow + 1;
    let Tag::Compound(dirt) = &mut palette[2] else {
        panic!("a compound");
    };
    let mut odd = Compound::new();
    odd.insert("bogus", Tag::Int(1));
    dirt.insert("states", Tag::Compound(odd));
    // z fastest then y then x, so each voxel of a 4x1x1 row is its x.
    layer(&mut root, 0)[1] = Tag::Int(-1);
    let liquids = layer(&mut root, 1);
    liquids[0] = Tag::Int(water);
    liquids[3] = Tag::Int(snow);
    let import = imported(&root, "shed.mcstructure");
    assert!(import.transliteration.source.ends_with(
        "struct shed size=4x1\n  \
           raw_block mat=minecraft:stone at=0,0,0 origin=imported\n  \
           raw_fill mat=minecraft:dirt from=2,0,0 to=3,0,0 origin=imported\n"
    ));
    assert_eq!(
        import.decoded.losses[1..],
        [
            loss(
                "shed",
                "1 cell holds a structure void, which a placement leaves as the world has it, \
                 and the import writes air there, which a placement clears"
            ),
            loss(
                "shed",
                "`minecraft:dirt[bogus=1]` (2 cells) has no canonical state at bedrock 1.21.60, \
                 so it is written by its id alone and builds that block's default states"
            ),
            loss(
                "shed",
                "water stands at 1 cell whose block cannot hold it as `waterlogged`; the water \
                 is not written"
            ),
            loss(
                "shed",
                "the second layer holds `minecraft:snow_layer` at 1 cell, which no raw member \
                 writes; it is not written"
            ),
        ]
    );
}

#[test]
fn a_block_that_always_stands_in_water_is_reported_where_the_file_holds_none() {
    let mut root = root(
        &array("bed", Dims { x: 1, y: 1, z: 1 }, &["minecraft:seagrass"]),
        "1.21.60",
    );
    layer(&mut root, 1)[0] = Tag::Int(-1);
    let import = imported(&root, "bed.mcstructure");
    assert_eq!(
        import.decoded.losses[1..],
        [loss(
            "bed",
            "the written block stands in water at 1 cell where the file's second layer holds \
             none, since that block always does"
        )]
    );
}
