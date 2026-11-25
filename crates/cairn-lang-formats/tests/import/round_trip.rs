//! Every shipped example that builds, written in each format of its edition, imported, and the
//! import compiled for the same target: it builds the blocks the file holds, and the voxel diff
//! finds no cell of the file that it or the example builds otherwise.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockArray, BlockState, Dims};
use cairn_lang_core::transliterate::{CHUNK_FOOTPRINT, WrittenBody};
use cairn_lang_core::voxel_diff::{Metrics, compare};
use cairn_lang_formats::bedrock_structure::{
    frames_as_blocks, prepare_mcstructure, read_mcstructure,
};
use cairn_lang_formats::import::{ImportFormat, LossKind, RawImport, import_raw};
use cairn_lang_formats::java_structure::{prepare_structure, read_structure};
use cairn_lang_formats::litematic::{litematic_files, prepare_litematic, read_litematic};
use cairn_lang_formats::registry::builtin_pack;
use cairn_lang_formats::schem::{prepare_schem, read_schem, schem_files};

use cairn_lang_nbt::tag::Compound;

use crate::common::{
    BedrockCell, admitted_versions, bedrock_cells, compile_source, examples, examples_dir, latest,
    states_for,
};

/// One structure file an example writes: its name, its bytes, and the arrays it holds in order
/// with the `structures` key of each.
struct Written<'a> {
    name: String,
    bytes: Vec<u8>,
    arrays: Vec<&'a BlockArray>,
    scopes: Vec<&'a str>,
}

/// What a file saved at `version` says of it: its `DataVersion` on Java, its block version on
/// Bedrock, which several releases share.
fn stamp_key(edition: Edition, version: &str) -> Option<i32> {
    let row = builtin_pack(edition)
        .targetable()
        .into_iter()
        .find(|row| row.mc_version == version)?;
    match edition {
        Edition::Java => Some(row.data_version),
        Edition::Bedrock => row.block_version,
    }
}

/// Each file `arrays` writes in `format` at `version`.
fn written<'a>(
    ir: &'a cairn_lang_core::block_array::BlockArrayIr,
    format: ImportFormat,
    version: &str,
) -> Vec<Written<'a>> {
    let pack = builtin_pack(format.edition());
    match format {
        ImportFormat::Nbt => {
            let target = pack.resolve_java_target(version).expect("a shipped target");
            ir.structures
                .iter()
                .map(|(scope, array)| {
                    let mut bytes = Vec::new();
                    prepare_structure(array, &target)
                        .expect("the array prepares")
                        .write_gzip(&mut bytes)
                        .expect("the file writes");
                    Written {
                        name: format!("{}.nbt", ir.artifact_stem(scope)),
                        bytes,
                        arrays: vec![array],
                        scopes: vec![scope],
                    }
                })
                .collect()
        }
        ImportFormat::Litematic => {
            let target = pack.resolve_java_target(version).expect("a shipped target");
            litematic_files(ir)
                .into_iter()
                .map(|file| {
                    let name = format!("{}.litematic", file.stem);
                    let arrays = file.regions.iter().map(|r| r.block_array).collect();
                    let scopes = file.regions.iter().map(|r| r.scope).collect();
                    let mut bytes = Vec::new();
                    prepare_litematic(file, &target)
                        .expect("the file prepares")
                        .write_gzip(&mut bytes)
                        .expect("the file writes");
                    Written {
                        name,
                        bytes,
                        arrays,
                        scopes,
                    }
                })
                .collect()
        }
        ImportFormat::Schem => {
            let target = pack.resolve_java_target(version).expect("a shipped target");
            schem_files(ir)
                .into_iter()
                .map(|file| {
                    let name = format!("{}.schem", file.stem);
                    let arrays = vec![file.block_array];
                    let scopes = vec![file.scope];
                    let mut bytes = Vec::new();
                    prepare_schem(file, &target)
                        .expect("the file prepares")
                        .write_gzip(&mut bytes)
                        .expect("the file writes");
                    Written {
                        name,
                        bytes,
                        arrays,
                        scopes,
                    }
                })
                .collect()
        }
        ImportFormat::Mcstructure => ir
            .structures
            .iter()
            .map(|(scope, array)| Written {
                name: format!("{}.mcstructure", ir.artifact_stem(scope)),
                bytes: mcstructure_bytes(array, version),
                arrays: vec![array],
                scopes: vec![scope],
            })
            .collect(),
    }
}

fn mcstructure_bytes(array: &BlockArray, version: &str) -> Vec<u8> {
    let target = builtin_pack(Edition::Bedrock)
        .resolve_bedrock_target(version)
        .expect("a shipped target");
    let (prepared, _) =
        prepare_mcstructure(array, &target, &states_for(&target)).expect("the array prepares");
    let mut bytes = Vec::new();
    prepared.write(&mut bytes).expect("the file writes");
    bytes
}

/// The chunks a body was written as, each with its array where the import compiles and where
/// its `(0, 0, 0)` cell lies in the body.
fn rebuilt<'a>(
    ir: &'a cairn_lang_core::block_array::BlockArrayIr,
    body: &WrittenBody,
) -> Vec<(&'a BlockArray, (u32, u32, u32))> {
    if body.chunks.is_empty() {
        let key = format!("struct::{}", body.name);
        return vec![(&ir.structures[&key], (0, 0, 0))];
    }
    body.chunks
        .iter()
        .map(|chunk| {
            let (i, j) = chunk
                .rsplit_once("_z")
                .and_then(|(rest, j)| {
                    let (_, i) = rest.rsplit_once("_x")?;
                    Some((i.parse::<u32>().ok()?, j.parse::<u32>().ok()?))
                })
                .expect("a chunk named `<body>_x<i>_z<j>`");
            let key = format!("site::{}::{chunk}", body.name);
            (
                &ir.structures[&key],
                (i * CHUNK_FOOTPRINT, 0, j * CHUNK_FOOTPRINT),
            )
        })
        .collect()
}

/// A chunk's cells, its extents, and where its `(0, 0, 0)` cell lies in its body.
struct Part<T> {
    cells: Vec<T>,
    dims: Dims,
    at: (u32, u32, u32),
}

/// Each cell of `dims` filled from `parts`, each at its offset, `None` where none lies.
fn assembled<T: Clone>(dims: Dims, parts: &[Part<T>]) -> Vec<Option<T>> {
    let mut out = vec![None; dims.volume()];
    for Part {
        cells,
        dims: part,
        at: (ox, oy, oz),
    } in parts
    {
        for (i, cell) in cells.iter().enumerate() {
            let (x, y, z) = part.position(i).expect("a cell of the part");
            let at = dims
                .index(ox + x, oy + y, oz + z)
                .expect("a part lies inside its body");
            out[at] = Some(cell.clone());
        }
    }
    out
}

/// `cell` with an item frame's states dropped: Bedrock builds that entity as a block whose states no
/// canonical state spells, so the import writes it by its id alone.
fn frame_by_id((block, liquid): BedrockCell) -> BedrockCell {
    let block = block.map(|(name, states)| {
        if matches!(name.as_str(), "minecraft:frame" | "minecraft:glow_frame") {
            (name, Compound::new())
        } else {
            (name, states)
        }
    });
    (block, liquid)
}

fn states(array: &BlockArray) -> Vec<BlockState> {
    array
        .voxels
        .iter()
        .map(|slot| array.palette.entries[usize::from(slot.0)].clone())
        .collect()
}

/// The import of `file`, failing with its name.
fn imported(file: &Written<'_>, format: ImportFormat) -> RawImport {
    import_raw(file.bytes.as_slice(), format, &file.name)
        .unwrap_or_else(|error| panic!("{} does not import: {error}", file.name))
}

/// Whether `file`, read back in `format`, holds a block entity.
fn holds_block_entities(file: &Written<'_>, format: ImportFormat) -> bool {
    let bytes = file.bytes.as_slice();
    match format {
        ImportFormat::Nbt => !read_structure(bytes)
            .expect("a written file reads back")
            .volume
            .block_entities()
            .is_empty(),
        ImportFormat::Litematic => read_litematic(bytes)
            .expect("a written file reads back")
            .regions
            .iter()
            .any(|region| !region.volume.block_entities().is_empty()),
        ImportFormat::Schem => !read_schem(bytes)
            .expect("a written file reads back")
            .volume
            .block_entities()
            .is_empty(),
        ImportFormat::Mcstructure => !read_mcstructure(bytes)
            .expect("a written file reads back")
            .volume
            .block_entities()
            .is_empty(),
    }
}

/// Whether the decoded `file` reports its block entities lost exactly when it holds some.
fn block_entities_reported(import: &RawImport, file: &Written<'_>, format: ImportFormat) -> bool {
    let held = holds_block_entities(file, format);
    let reported = import
        .decoded
        .losses
        .iter()
        .any(|loss| loss.kind == LossKind::BlockEntities);
    held == reported
}

#[test]
fn every_example_that_builds_imports_to_a_source_that_builds_the_same_blocks() {
    let mut unbuilt: Vec<(String, Edition, String)> = Vec::new();
    let mut compared = 0;
    for (name, source) in examples() {
        for edition in [Edition::Java, Edition::Bedrock] {
            let versions = admitted_versions(builtin_pack(edition), &source, edition);
            if versions.is_empty() {
                unbuilt.push((name.clone(), edition, "no admitted version".to_owned()));
            }
            for version in versions {
                let Ok(ir) = compile_source(&source, edition, &version) else {
                    unbuilt.push((name.clone(), edition, version));
                    continue;
                };
                let formats: &[ImportFormat] = match edition {
                    Edition::Java => &[
                        ImportFormat::Nbt,
                        ImportFormat::Litematic,
                        ImportFormat::Schem,
                    ],
                    Edition::Bedrock => &[ImportFormat::Mcstructure],
                };
                for &format in formats {
                    for file in written(&ir, format, &version) {
                        let import = imported(&file, format);
                        let label = format!("{name} as {} at {edition} {version}", file.name);
                        assert_eq!(
                            stamp_key(edition, import.decoded.stamp.mc_version()),
                            stamp_key(edition, &version),
                            "{label}"
                        );
                        assert!(block_entities_reported(&import, &file, format), "{label}");
                        let built =
                            compile_source(&import.transliteration.source, edition, &version)
                                .unwrap_or_else(|codes| {
                                    panic!("{label}: the import refuses {codes:?}")
                                });
                        let bodies = &import.transliteration.bodies;
                        assert_eq!(bodies.len(), file.arrays.len(), "{label}");
                        compared += 1;
                        for (body, array) in bodies.iter().zip(&file.arrays) {
                            let parts = rebuilt(&built, body);
                            if edition == Edition::Java {
                                let parts: Vec<_> = parts
                                    .iter()
                                    .map(|(part, at)| Part {
                                        cells: states(part),
                                        dims: part.dims,
                                        at: *at,
                                    })
                                    .collect();
                                assert_eq!(
                                    assembled(array.dims, &parts),
                                    states(array).into_iter().map(Some).collect::<Vec<_>>(),
                                    "{label}: `{}`",
                                    body.name
                                );
                            } else {
                                let read = |array: &BlockArray| {
                                    let bytes = mcstructure_bytes(array, &version);
                                    read_mcstructure(bytes.as_slice()).expect("the file reads back")
                                };
                                let parts: Vec<_> = parts
                                    .iter()
                                    .map(|(part, at)| Part {
                                        cells: bedrock_cells(&read(part).volume),
                                        dims: part.dims,
                                        at: *at,
                                    })
                                    .collect();
                                assert_eq!(
                                    assembled(array.dims, &parts)
                                        .into_iter()
                                        .map(|cell| cell.map(frame_by_id))
                                        .collect::<Vec<_>>(),
                                    bedrock_cells(&read(array).volume)
                                        .into_iter()
                                        .map(|cell| Some(frame_by_id(cell)))
                                        .collect::<Vec<_>>(),
                                    "{label}: `{}`",
                                    body.name
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(unbuilt, Vec::<(String, Edition, String)>::new());
    assert!(compared > 0, "no file was compared");
}

#[test]
fn every_structure_file_an_example_writes_agrees_at_every_cell_with_its_import_and_its_source() {
    let mut compared = 0;
    for (name, source) in examples() {
        for edition in [Edition::Java, Edition::Bedrock] {
            for version in admitted_versions(builtin_pack(edition), &source, edition) {
                let Ok(ir) = compile_source(&source, edition, &version) else {
                    continue;
                };
                let formats: &[ImportFormat] = match edition {
                    Edition::Java => &[
                        ImportFormat::Nbt,
                        ImportFormat::Litematic,
                        ImportFormat::Schem,
                    ],
                    Edition::Bedrock => &[ImportFormat::Mcstructure],
                };
                for &format in formats {
                    for file in written(&ir, format, &version) {
                        let import = imported(&file, format);
                        let label = format!("{name} as {} at {edition} {version}", file.name);
                        let lifted =
                            compile_source(&import.transliteration.source, edition, &version)
                                .unwrap_or_else(|codes| {
                                    panic!("{label}: the import refuses {codes:?}")
                                });
                        let counts = compare(import.decoded.bodies(), &lifted).counts;
                        assert_eq!(
                            counts.metrics(),
                            Metrics {
                                block_iou: 1.0,
                                state_accuracy: 1.0,
                                residual_ratio: 1.0,
                            },
                            "{label}: the import"
                        );
                        // A file holds one struct, one site or one of a site's arrays: one body.
                        let mut held = ir.clone();
                        held.structures
                            .retain(|scope, _| file.scopes.contains(&scope.as_str()));
                        if edition == Edition::Bedrock {
                            held = frames_as_blocks(&held);
                        }
                        let comparison = compare(import.decoded.bodies(), &held);
                        let metrics = comparison.counts.metrics();
                        assert_eq!(
                            (
                                comparison.bodies.len(),
                                metrics.block_iou,
                                metrics.state_accuracy
                            ),
                            (1, 1.0, 1.0),
                            "{label}: the source"
                        );
                        compared += 1;
                    }
                }
            }
        }
    }
    assert!(compared > 0, "no file was compared");
}

#[test]
fn a_site_written_as_one_litematic_compares_with_the_site_as_one_body_named_after_the_file() {
    let path = examples_dir().join("l-walkway.crn");
    let source = std::fs::read_to_string(&path).expect("the example reads");
    let version = latest(Edition::Java);
    let ir = compile_source(&source, Edition::Java, version).expect("the example compiles");
    let [file] = written(&ir, ImportFormat::Litematic, version)
        .try_into()
        .unwrap_or_else(|files: Vec<Written<'_>>| panic!("{} files", files.len()));
    let import = imported(&file, ImportFormat::Litematic);
    assert_eq!(
        import.decoded.units.len(),
        4,
        "three placements and a walkway"
    );
    let comparison = compare(import.decoded.bodies(), &ir);
    let pairs: Vec<_> = comparison
        .bodies
        .iter()
        .map(|body| (body.structure.as_deref(), body.source.as_deref()))
        .collect();
    assert_eq!(pairs, [(Some("duo"), Some("duo"))]);
    assert_eq!(
        comparison.counts.metrics(),
        Metrics {
            block_iou: 1.0,
            state_accuracy: 1.0,
            residual_ratio: 0.0,
        }
    );
}
