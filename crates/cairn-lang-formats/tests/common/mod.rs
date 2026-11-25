//! Helpers shared by the `cairn-lang-formats` integration test binaries.

use std::path::PathBuf;

use cairn_lang_core::block_array::{
    BlockArray, BlockArrayIr, BlockEntity, BlockEntityData, BlockState, Dims, Palette,
    PaletteIndex, lower_to_block_array,
};
use cairn_lang_core::resolve::{DroppedIntent, declared_version_floors, versions_satisfying};
use cairn_lang_core::{Edition, Severity, lower, parse, resolve};
use cairn_lang_formats::bedrock_structure::{BedrockVolume, ParityNote};
use cairn_lang_formats::build::finding::Finding;
use cairn_lang_formats::build::lowering::{BuildRefusal, lower_for_target};
use cairn_lang_formats::data_version::{BedrockTarget, JavaTarget, resolve_target};
use cairn_lang_formats::java_structure::build_structure_tag;
use cairn_lang_formats::registry::{RegistryPack, VersionStates, builtin_pack};
use cairn_lang_formats::volume::{BlockVolume, StoredBlockEntity};
use cairn_lang_nbt::tag::{Compound, Tag};
use indexmap::IndexMap;

/// The repository's `examples/` directory.
pub fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

/// The number of `.crn` files `examples/` ships; bump it with the next example.
pub const SHIPPED_EXAMPLES: usize = 17;

/// Every `.crn` under `examples/`, as `(file name, source)` sorted by name, refusing any set but
/// the shipped one, since a sweep over nothing passes.
///
/// # Panics
///
/// When `examples/` cannot be read or holds other than [`SHIPPED_EXAMPLES`] files.
pub fn examples() -> Vec<(String, String)> {
    let dir = examples_dir();
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()));
    let mut found: Vec<(String, String)> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|err| panic!("cannot read an entry: {err}"))
                .path()
        })
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("crn"))
        .map(|path| {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
            (
                path.file_name().expect("named").to_string_lossy().into(),
                source,
            )
        })
        .collect();
    found.sort();
    assert_eq!(
        found.len(),
        SHIPPED_EXAMPLES,
        "found {} examples under {}, not the {SHIPPED_EXAMPLES} shipped; bump SHIPPED_EXAMPLES \
         when adding one",
        found.len(),
        dir.display(),
    );
    found
}

/// The source of the shipped example `name` (`"cottage.crn"`).
///
/// # Panics
///
/// When the file cannot be read.
pub fn example_source(name: &str) -> String {
    let path = examples_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

/// The version `--target latest` pins on `edition`.
pub fn latest(edition: Edition) -> &'static str {
    &builtin_pack(edition).data_versions().latest
}

/// The block-array IR `cairn compile --edition <edition>` lowers `source` to; a `mc_version` of
/// `None` pins no version, as `cairn info` does.
///
/// # Panics
///
/// When `source` does not parse.
pub fn lower_source(source: &str, edition: Edition, mc_version: Option<&str>) -> BlockArrayIr {
    let module = parse(source).unwrap_or_else(|err| panic!("source does not parse: {err}"));
    let ir = lower(&module);
    let resolution = resolve(&ir, Some(edition));
    lower_to_block_array(
        &ir,
        &resolution,
        Some(&builtin_pack(edition).view(mc_version)),
    )
}

/// The block-array IR `cairn compile` builds from `source` for `edition` at `mc_version`, its
/// circuits laid, or what refuses the build.
///
/// # Errors
///
/// The parse failure's message, else the refusing codes, `"a run-level refusal"` for one with none.
pub fn compile_source(
    source: &str,
    edition: Edition,
    mc_version: &str,
) -> Result<BlockArrayIr, Vec<String>> {
    let module = parse(source).map_err(|err| vec![err.user_message()])?;
    let lowered = lower_for_target(&module, edition, resolve_target(edition, mc_version));
    let codes = match lowered.verdict() {
        Ok(_) => return Ok(lowered.block_ir),
        Err(BuildRefusal::Findings) => {
            let core = lowered
                .block_ir
                .diagnostics
                .iter()
                .map(|d| d as &dyn Finding);
            let circuits = lowered.circuits.iter().map(|d| d as &dyn Finding);
            core.chain(circuits)
                .filter(|finding| finding.severity() == Severity::Error)
                .map(|finding| finding.code_str().to_owned())
                .collect()
        }
        Err(BuildRefusal::Conflicts) => lowered
            .conflicts
            .iter()
            .filter_map(|message| message.kind.code())
            .map(str::to_owned)
            .collect(),
        Err(BuildRefusal::RunLevel(message)) => {
            vec![
                message
                    .kind
                    .code()
                    .unwrap_or("a run-level refusal")
                    .to_owned(),
            ]
        }
        Err(BuildRefusal::Floor(finding)) => vec![finding.code.as_str().to_owned()],
    };
    Err(codes)
}

/// [`lower_source`] over the shipped example `name`.
pub fn lower_example(name: &str, edition: Edition, mc_version: Option<&str>) -> BlockArrayIr {
    lower_source(&example_source(name), edition, mc_version)
}

/// The id at `(x, y, z)` of `ba`, or `None` past the array.
pub fn id_at(ba: &BlockArray, x: u32, y: u32, z: u32) -> Option<&str> {
    let i = ba.dims.index(x, y, z)?;
    Some(&ba.palette.entries[usize::from(ba.voxels[i].0)].id)
}

/// Every version the pack can build for.
pub fn supported_versions(pack: &RegistryPack) -> Vec<&str> {
    pack.targetable()
        .into_iter()
        .map(|e| e.mc_version.as_str())
        .collect()
}

/// The versions `pack` targets that `source`'s floors for `edition` admit, the rest being ones a
/// pinned build refuses at the floor.
///
/// # Panics
///
/// When `source` does not parse.
pub fn admitted_versions(pack: &RegistryPack, source: &str, edition: Edition) -> Vec<String> {
    let module = parse(source).unwrap_or_else(|err| panic!("source does not parse: {err}"));
    let floors = declared_version_floors(&module, edition);
    let considered: Vec<String> = supported_versions(pack)
        .into_iter()
        .map(str::to_owned)
        .collect();
    versions_satisfying(&pack.version_order(), &floors, &considered)
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// A `.mcstructure` cell as read: its block and its second layer's, each a name and its states.
pub type BedrockCell = (Option<(String, Compound)>, Option<(String, Compound)>);

/// Each cell of `volume`, in its voxel order.
pub fn bedrock_cells(volume: &BedrockVolume) -> Vec<BedrockCell> {
    let named = |slot: &Option<PaletteIndex>| {
        slot.map(|slot| {
            let block = &volume.palette()[usize::from(slot.0)];
            (block.name.clone(), block.states.clone())
        })
    };
    volume
        .blocks()
        .iter()
        .zip(volume.liquids())
        .map(|(block, liquid)| (named(block), named(liquid)))
        .collect()
}

/// The `structure` compound of a `.mcstructure` root.
///
/// # Panics
///
/// When the root has no `structure` compound.
pub fn structure_compound(root: &Compound) -> &Compound {
    match root.entries.get("structure") {
        Some(Tag::Compound(c)) => c,
        other => panic!("structure is not a Compound: {other:?}"),
    }
}

/// The `structure.palette.default` compound of a `.mcstructure` root.
///
/// # Panics
///
/// When the root has no `structure.palette.default` compound.
pub fn default_palette(root: &Compound) -> &Compound {
    match structure_compound(root).entries.get("palette") {
        Some(Tag::Compound(p)) => match p.entries.get("default") {
            Some(Tag::Compound(d)) => d,
            other => panic!("palette.default is not a Compound: {other:?}"),
        },
        other => panic!("palette is not a Compound: {other:?}"),
    }
}

/// The `structure.palette.default.block_palette` entries of a `.mcstructure` root.
///
/// # Panics
///
/// When the root has no `block_palette` list.
pub fn block_palette(root: &Compound) -> &[Tag] {
    match default_palette(root).entries.get("block_palette") {
        Some(Tag::List(l)) => &l.items,
        other => panic!("block_palette is not a List: {other:?}"),
    }
}

/// `tag` in SNBT, its compounds' entries in the order the writer inserted them.
pub fn snbt(tag: &Tag) -> String {
    let joined = |items: Vec<String>| items.join(",");
    match tag {
        Tag::Byte(value) => format!("{value}b"),
        Tag::Short(value) => format!("{value}s"),
        Tag::Int(value) => value.to_string(),
        Tag::Long(value) => format!("{value}L"),
        Tag::Float(value) => format!("{value}f"),
        Tag::Double(value) => format!("{value}d"),
        Tag::String(value) => format!("{value:?}"),
        Tag::List(list) => format!("[{}]", joined(list.items.iter().map(snbt).collect())),
        Tag::Compound(compound) => format!(
            "{{{}}}",
            joined(
                compound
                    .entries
                    .iter()
                    .map(|(name, value)| format!("{name}:{}", snbt(value)))
                    .collect()
            )
        ),
        Tag::ByteArray(values) => format!("[B;{values:?}]"),
        Tag::IntArray(values) => format!("[I;{values:?}]"),
        Tag::LongArray(values) => format!("[L;{values:?}]"),
    }
}

/// Each `block_position_data` entry of a `.mcstructure` root, its key and its block entity in SNBT.
///
/// # Panics
///
/// When the root holds no `block_position_data` compound.
pub fn position_data(root: &Compound) -> Vec<(String, String)> {
    match default_palette(root).entries.get("block_position_data") {
        Some(Tag::Compound(data)) => data
            .entries
            .iter()
            .map(|(index, block)| (index.clone(), snbt(block)))
            .collect(),
        other => panic!("block_position_data is not a Compound: {other:?}"),
    }
}

/// Each `block_palette` entry of a `.mcstructure` root as `name[key=value,…]`, its states sorted
/// by key and the entries sorted.
///
/// # Panics
///
/// When an entry is not a compound with a `name` and `states`, or a state is not a byte, an int or
/// a string.
pub fn written_palette(root: &Compound) -> Vec<String> {
    let mut written: Vec<String> = block_palette(root)
        .iter()
        .map(|entry| {
            let Tag::Compound(entry) = entry else {
                panic!("palette entry: {entry:?}");
            };
            let (Some(Tag::String(name)), Some(Tag::Compound(states))) =
                (entry.entries.get("name"), entry.entries.get("states"))
            else {
                panic!("palette entry: {entry:?}");
            };
            let mut states: Vec<String> = states
                .entries
                .iter()
                .map(|(key, value)| match value {
                    Tag::Byte(v) => format!("{key}={v}"),
                    Tag::Int(v) => format!("{key}={v}"),
                    Tag::String(v) => format!("{key}={v}"),
                    other => panic!("{key}: {other:?}"),
                })
                .collect();
            states.sort();
            format!("{name}[{}]", states.join(","))
        })
        .collect();
    written.sort();
    written
}

/// Each `palette` entry of a Java structure root as `Name[key=value,…]`, its properties sorted by
/// key and the entries sorted.
///
/// # Panics
///
/// When the root has no `palette` list, or an entry is not a compound with a `Name` whose
/// `Properties`, if any, are strings.
pub fn written_java_palette(root: &Compound) -> Vec<String> {
    let Some(Tag::List(palette)) = root.entries.get("palette") else {
        panic!("palette is not a List: {:?}", root.entries.get("palette"));
    };
    let mut written: Vec<String> = palette
        .items
        .iter()
        .map(|entry| {
            let Tag::Compound(entry) = entry else {
                panic!("palette entry: {entry:?}");
            };
            let Some(Tag::String(name)) = entry.entries.get("Name") else {
                panic!("palette entry: {entry:?}");
            };
            let mut properties: Vec<String> = match entry.entries.get("Properties") {
                None => Vec::new(),
                Some(Tag::Compound(properties)) => properties
                    .entries
                    .iter()
                    .map(|(key, value)| match value {
                        Tag::String(v) => format!("{key}={v}"),
                        other => panic!("{key}: {other:?}"),
                    })
                    .collect(),
                Some(other) => panic!("Properties: {other:?}"),
            };
            properties.sort();
            format!("{name}[{}]", properties.join(","))
        })
        .collect();
    written.sort();
    written
}

/// A blockstate property map from `(key, value)` pairs, in the order given.
pub fn props<const N: usize>(pairs: [(&str, &str); N]) -> IndexMap<String, String> {
    pairs
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// Stair properties in the order lowering inserts them.
pub fn stair_props(facing: &str, half: &str, shape: &str) -> IndexMap<String, String> {
    props([("facing", facing), ("half", half), ("shape", shape)])
}

/// Each note's `(id, states, dropped)`: a `#[non_exhaustive]` note cannot be built to compare.
pub fn note_fields(notes: &[ParityNote]) -> Vec<(&str, &str, &DroppedIntent)> {
    notes
        .iter()
        .map(|note| (note.id.as_str(), note.states.as_str(), &note.dropped))
        .collect()
}

/// Why a Bedrock version without a stair `shape` state drops one, as the built-in pack words it.
pub const NO_SHAPE: &str = "this Bedrock version has no stair `shape` state; its stairs render \
                            straight, so corners show visual gaps";

/// The intent a stair drops at `mc_versions`, which have no form for its `shape=value`.
pub fn dropped_shape(value: &str, mc_versions: &[&str]) -> DroppedIntent {
    DroppedIntent {
        key: "shape".to_owned(),
        value: value.to_owned(),
        mc_versions: mc_versions.iter().map(|v| (*v).to_owned()).collect(),
        reason: NO_SHAPE.to_owned(),
    }
}

/// The built-in Bedrock pack's state map at `target`'s version.
///
/// # Panics
///
/// When the pack maps no states there, which its loader refuses for a targetable version.
pub fn states_for(target: &BedrockTarget) -> VersionStates<'_> {
    builtin_pack(Edition::Bedrock)
        .states_at(&target.mc_version)
        .expect("the built-in Bedrock pack maps every targetable version")
}

/// A block-array IR holding `array` alone, under its `source_scope`.
pub fn ir_of(array: BlockArray) -> BlockArrayIr {
    BlockArrayIr {
        structures: IndexMap::from([(array.source_scope.clone(), array)]),
        placements: IndexMap::new(),
        walkways: IndexMap::new(),
        members: IndexMap::new(),
        diagnostics: Vec::new(),
    }
}

/// An array of `dims` holding `ids` after air, its voxels the palette slots `slots` in `(y, z, x)`
/// order, under `struct::<name>`.
///
/// # Panics
///
/// When `slots` is not one slot per voxel.
pub fn array_of(name: &str, dims: Dims, ids: &[&str], slots: &[u16]) -> BlockArray {
    let mut palette = Palette::new_with_air();
    for id in ids {
        palette.intern(BlockState::bare(*id));
    }
    assert_eq!(slots.len(), dims.volume(), "one slot per voxel");
    BlockArray {
        dims,
        palette,
        voxels: slots.iter().map(|&slot| PaletteIndex(slot)).collect(),
        block_entities: vec![],
        entities: vec![],
        source_scope: format!("struct::{name}"),
        derived: Vec::new(),
    }
}

/// A 2×2×3 array of stone under a comparator spelled `id` at (1, 1, 1), its block entity resting
/// at strength 3, where Java's `blocks` and Bedrock's `block_indices` order it apart.
///
/// # Panics
///
/// When a cell it sets lies outside its dimensions, which none does.
pub fn comparator_array(id: &str) -> BlockArray {
    let mut palette = Palette::new_with_air();
    let stone = palette.intern(BlockState::bare("minecraft:stone"));
    let mut comparator = BlockState::bare(id);
    for (key, value) in [("facing", "east"), ("mode", "compare"), ("powered", "true")] {
        comparator
            .properties
            .insert(key.to_owned(), value.to_owned());
    }
    let comparator = palette.intern(comparator);
    let dims = Dims { x: 2, y: 2, z: 3 };
    let mut voxels = vec![PaletteIndex::AIR; dims.volume()];
    for z in 0..dims.z {
        for x in 0..dims.x {
            voxels[dims.index(x, 0, z).expect("in dims")] = stone;
        }
    }
    voxels[dims.index(1, 1, 1).expect("in dims")] = comparator;
    BlockArray {
        dims,
        palette,
        voxels,
        block_entities: vec![BlockEntity {
            pos: (1, 1, 1),
            data: BlockEntityData::Fields {
                id: "minecraft:comparator".to_owned(),
                fields: IndexMap::from([("OutputSignal".to_owned(), 3)]),
            },
        }],
        entities: vec![],
        source_scope: "struct::comparator".to_owned(),
        derived: Vec::new(),
    }
}

/// A row of one voxel per entry of `slots`, under `struct::wide`, whose palette after air is
/// `minecraft:b1` up to `minecraft:b<len - 1>`.
///
/// # Panics
///
/// When `slots` is longer than a `u32` counts.
pub fn wide_array(len: usize, slots: &[u16]) -> BlockArray {
    let mut entries = vec![BlockState::air()];
    entries.extend((1..len).map(|i| BlockState::bare(format!("minecraft:b{i}"))));
    BlockArray {
        dims: Dims {
            x: u32::try_from(slots.len()).expect("a row a u32 counts"),
            y: 1,
            z: 1,
        },
        palette: Palette { entries },
        voxels: slots.iter().map(|&slot| PaletteIndex(slot)).collect(),
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::wide".to_owned(),
        derived: Vec::new(),
    }
}

/// `array` as a reader returns it at `target`: its palette, its grid, and the block entities its
/// `.nbt` keeps, which a `.litematic` and a `.schem` keep as well.
///
/// # Panics
///
/// When `array` does not build as a structure, which no written array fails to.
pub fn volume_of(array: &BlockArray, target: &JavaTarget) -> BlockVolume {
    let root = build_structure_tag(array, target).expect("a written array builds");
    let Some(Tag::List(blocks)) = root.entries.get("blocks") else {
        panic!("blocks is a list");
    };
    let block_entities = blocks
        .items
        .iter()
        .filter_map(|block| {
            let Tag::Compound(block) = block else {
                panic!("a block entry is a compound");
            };
            let Some(Tag::Compound(nbt)) = block.entries.get("nbt") else {
                return None;
            };
            let Some(Tag::List(pos)) = block.entries.get("pos") else {
                panic!("a block entry has a pos");
            };
            let [x, y, z] = &pos.items[..] else {
                panic!("a pos has three axes");
            };
            let axis = |tag: &Tag| match tag {
                Tag::Int(value) => u32::try_from(*value).expect("a voxel of the grid"),
                other => panic!("an axis is an int: {other:?}"),
            };
            let mut data = nbt.clone();
            let Some(Tag::String(id)) = data.entries.shift_remove("id") else {
                panic!("a block entity has an id");
            };
            Some(StoredBlockEntity {
                pos: (axis(x), axis(y), axis(z)),
                id,
                data,
            })
        })
        .collect();
    BlockVolume::new(
        array.dims,
        array.palette.entries.clone(),
        array.voxels.clone(),
        block_entities,
    )
    .expect("a written array is a volume")
}

/// `bytes` as one gzip member.
///
/// # Panics
///
/// When compressing into memory fails, which it does not.
pub fn gzip(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(bytes).expect("the bytes compress");
    gz.finish().expect("the member closes")
}
