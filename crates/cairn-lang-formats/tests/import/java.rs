//! Importing Java's three formats: what each writes, how each is stamped, and what each loses.

use cairn_lang_core::CAIRN_VERSION;
use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockArray, BlockState, Dims, PaletteIndex};
use cairn_lang_core::transliterate::{TransliterateError, WrittenBody};
use cairn_lang_formats::data_version::{JavaTarget, resolve_java_target};
use cairn_lang_formats::import::{
    ImportError, ImportFormat, LossKind, RawImport, SavedVersion, SemanticLoss, StampFit,
    import_raw,
};
use cairn_lang_formats::java_structure::{build_structure_tag, write_compound_gzip};
use cairn_lang_formats::litematic::{litematic_files, prepare_litematic};
use cairn_lang_formats::schem::{prepare_schem, schem_files};
use cairn_lang_nbt::tag::{Compound, List, Tag};

use crate::common::{array_of, comparator_array, compile_source, ir_of, lower_example};

fn target(version: &str) -> JavaTarget {
    resolve_java_target(version).expect("a shipped target")
}

/// `array` as a `.nbt` saved at `data_version`, its `DataVersion` set apart from the target that
/// writes it.
fn nbt_bytes(array: &BlockArray, version: &str, data_version: i32) -> Vec<u8> {
    let mut root = build_structure_tag(array, &target(version)).expect("the array writes");
    root.insert("DataVersion", Tag::Int(data_version));
    gzip(&root)
}

fn gzip(root: &Compound) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_compound_gzip(&mut bytes, root).expect("the root writes");
    bytes
}

fn imported(bytes: &[u8], format: ImportFormat, name: &str) -> RawImport {
    import_raw(bytes, format, name)
        .unwrap_or_else(|error| panic!("{name} does not import: {error}"))
}

/// The source's header for a Java file saved as `saved` and stamped `version` for `reason`.
fn header(file: &str, kind: &str, saved: &str, version: &str, reason: &str) -> String {
    format!(
        "# imported from `{file}`, {kind} saved at {saved}\n\
         # stamped java {version}: {reason}\n\
         # raw transliteration: blocks and their regularity, not the author's intent \
         (W_SEMANTIC_LOSS)\n\
         @cairn {CAIRN_VERSION}\n\
         @requires java version>={version}\n\
         @intended_targets [\"{version}\"]\n"
    )
}

/// The loss every import reports first.
fn intent_loss() -> SemanticLoss {
    SemanticLoss {
        unit: None,
        kind: LossKind::Intent,
        what: "an import recovers blocks and their regularity, not what the author meant: every \
               member it writes is a raw block or fill, and none names a wall, a window or a \
               material slot"
            .to_owned(),
    }
}

fn loss(unit: &str, kind: LossKind, what: &str) -> SemanticLoss {
    SemanticLoss {
        unit: Some(unit.to_owned()),
        kind,
        what: what.to_owned(),
    }
}

#[test]
fn a_structure_file_imports_as_one_struct_of_raw_members_with_its_block_entities_lost() {
    let bytes = nbt_bytes(&comparator_array("minecraft:comparator"), "1.20.4", 3700);
    let import = imported(&bytes, ImportFormat::Nbt, "comparator.nbt");
    assert_eq!(
        import.transliteration.source,
        format!(
            "{}\n\
             struct comparator size=2x3\n  \
               raw_fill mat=minecraft:stone from=0,0,0 to=1,0,2 origin=imported\n  \
               raw_block mat=minecraft:comparator[facing=east,mode=compare,powered=true] \
             at=1,1,1 origin=imported\n",
            header(
                "comparator.nbt",
                "a Java structure file",
                "DataVersion 3700 (1.20.4)",
                "1.20.4",
                "the shipped target the file was saved at"
            )
        )
    );
    assert_eq!(
        import.decoded.losses,
        [
            intent_loss(),
            loss(
                "comparator",
                LossKind::BlockEntities,
                "1 block entity is not written, since an import writes no block entity data, \
                 such as a chest's contents or a command block's command: 1 \
                 `minecraft:comparator`"
            )
        ]
    );
    assert_eq!(import.transliteration.warnings, []);
}

#[test]
fn a_file_saved_between_two_shipped_targets_is_stamped_with_the_later() {
    let array = array_of("one", Dims { x: 1, y: 1, z: 1 }, &["minecraft:stone"], &[1]);
    let import = imported(
        &nbt_bytes(&array, "1.21.4", 3955),
        ImportFormat::Nbt,
        "one.nbt",
    );
    let stamp = &import.decoded.stamp;
    assert_eq!(
        (stamp.edition(), stamp.mc_version(), stamp.saved()),
        (
            Edition::Java,
            "1.21.4",
            Some(SavedVersion {
                version: 3955,
                fit: StampFit::Later,
            })
        )
    );
    assert!(import.transliteration.source.starts_with(&header(
        "one.nbt",
        "a Java structure file",
        "DataVersion 3955 (1.21.1)",
        "1.21.4",
        "the nearest shipped target after the version the file was saved at"
    )));
}

#[test]
fn a_file_saved_before_every_shipped_target_is_stamped_with_the_earliest() {
    let array = array_of("one", Dims { x: 1, y: 1, z: 1 }, &["minecraft:stone"], &[1]);
    let import = imported(
        &nbt_bytes(&array, "1.20.4", 3000),
        ImportFormat::Nbt,
        "one.nbt",
    );
    let stamp = &import.decoded.stamp;
    assert_eq!(
        (stamp.mc_version(), stamp.saved()),
        (
            "1.20.4",
            Some(SavedVersion {
                version: 3000,
                fit: StampFit::Oldest,
            })
        )
    );
    assert!(import.transliteration.source.starts_with(&header(
        "one.nbt",
        "a Java structure file",
        "DataVersion 3000",
        "1.20.4",
        "the earliest shipped target, since the file was saved before every one"
    )));
}

#[test]
fn a_file_saved_past_every_shipped_target_is_refused() {
    let array = array_of("one", Dims { x: 1, y: 1, z: 1 }, &["minecraft:stone"], &[1]);
    let error = import_raw(
        nbt_bytes(&array, "26.3", 9000).as_slice(),
        ImportFormat::Nbt,
        "one.nbt",
    )
    .expect_err("no target checks a file from the future");
    assert!(matches!(error, ImportError::Unshipped(_)), "{error:?}");
    assert_eq!(
        error.to_string(),
        "the file was saved at DataVersion 9000, past every java target this build ships (the \
         newest is 26.3), so no target can check its blocks. Fix: save it from 26.3 or older, \
         or import it with a newer cairn"
    );
}

#[test]
fn a_litematic_with_no_region_is_refused() {
    let mut root = Compound::new();
    root.insert("Version", Tag::Int(7));
    root.insert("MinecraftDataVersion", Tag::Int(4189));
    root.insert("Metadata", Tag::Compound(Compound::new()));
    root.insert("Regions", Tag::Compound(Compound::new()));
    let error = import_raw(
        gzip(&root).as_slice(),
        ImportFormat::Litematic,
        "empty.litematic",
    )
    .expect_err("a schematic of no region writes nothing");
    assert!(matches!(error, ImportError::NoRegions), "{error:?}");
    assert_eq!(
        error.to_string(),
        "the schematic holds no region, so there is nothing to write"
    );
}

#[test]
fn a_block_the_stamped_target_does_not_declare_is_refused_with_the_compile_error() {
    let array = array_of(
        "odd",
        Dims { x: 1, y: 1, z: 1 },
        &["minecraft:not_a_block"],
        &[1],
    );
    let error = import_raw(
        nbt_bytes(&array, "1.21.4", 4189).as_slice(),
        ImportFormat::Nbt,
        "odd.nbt",
    )
    .expect_err("the target refuses the block");
    let ImportError::Transliterate(TransliterateError::Refused {
        target,
        source,
        errors,
    }) = &error
    else {
        panic!("expected the compile's refusal, got {error:?}");
    };
    let found: Vec<(&str, &str)> = errors
        .iter()
        .map(|e| (e.code.as_str(), &source[e.span.clone()]))
        .collect();
    assert_eq!(
        (target.as_str(), found),
        (
            "java 1.21.4",
            vec![("E_UNKNOWN_ID", "minecraft:not_a_block")]
        )
    );
}

#[test]
fn a_structure_void_extra_palettes_and_entities_are_reported_lost() {
    let mut root = Compound::new();
    root.insert("DataVersion", Tag::Int(4189));
    root.insert("size", Tag::List(List::of_ints([3, 1, 1])));
    let palette = |id: &str| {
        let mut entry = Compound::new();
        entry.insert("Name", Tag::String(id.to_owned()));
        List::of_compounds(vec![entry])
    };
    root.insert(
        "palettes",
        Tag::List(List::of_tags(
            9,
            vec![
                Tag::List(palette("minecraft:stone")),
                Tag::List(palette("minecraft:dirt")),
            ],
        )),
    );
    let block = |x: i32| {
        let mut entry = Compound::new();
        entry.insert("state", Tag::Int(0));
        entry.insert("pos", Tag::List(List::of_ints([x, 0, 0])));
        entry
    };
    root.insert(
        "blocks",
        Tag::List(List::of_compounds(vec![block(0), block(2)])),
    );
    let mut cow = Compound::new();
    let mut nbt = Compound::new();
    nbt.insert("id", Tag::String("minecraft:cow".to_owned()));
    cow.insert("nbt", Tag::Compound(nbt));
    root.insert("entities", Tag::List(List::of_compounds(vec![cow])));
    let import = imported(&gzip(&root), ImportFormat::Nbt, "Odd Shed.nbt");
    assert_eq!(
        import.transliteration.bodies,
        [WrittenBody {
            name: "odd_shed".to_owned(),
            chunks: Vec::new(),
            lines: 2,
        }]
    );
    assert!(import.transliteration.source.ends_with(
        "struct odd_shed size=3x1\n  \
           raw_repeat count=2 step=2,0,0\n    \
             raw_block mat=minecraft:stone at=0,0,0 origin=imported\n"
    ));
    assert_eq!(
        import.decoded.losses,
        [
            intent_loss(),
            loss(
                "odd_shed",
                LossKind::Entities,
                "1 entity is not written, since an import writes blocks only: 1 `minecraft:cow`"
            ),
            loss(
                "odd_shed",
                LossKind::StructureVoids,
                "1 cell holds a structure void, which a placement leaves as the world has it, \
                 and the import writes air there, which a placement clears"
            ),
            loss(
                "odd_shed",
                LossKind::ExtraPalettes,
                "the file holds 2 palettes, of which a placement picks one at random; only the \
                 first is written"
            ),
        ]
    );
}

#[test]
fn a_structure_past_the_chunk_footprint_imports_as_a_site_of_chunks_that_builds_it() {
    let dims = Dims { x: 50, y: 1, z: 2 };
    let slots: Vec<u16> = (0..dims.volume())
        .map(|i| u16::from(i % 50 != 49))
        .collect();
    let array = array_of("long", dims, &["minecraft:stone"], &slots);
    let import = imported(
        &nbt_bytes(&array, "1.21.4", 4189),
        ImportFormat::Nbt,
        "long.nbt",
    );
    let source = &import.transliteration.source;
    assert_eq!(
        import.transliteration.bodies,
        [WrittenBody {
            name: "long".to_owned(),
            chunks: vec!["long_x0_z0".to_owned(), "long_x1_z0".to_owned()],
            lines: 2,
        }]
    );
    let built = compile_source(source, Edition::Java, "1.21.4").expect("the import builds");
    let placed: Vec<(&str, (i32, i32, i32), Dims)> = built
        .placements
        .iter()
        .map(|(key, placement)| (key.as_str(), placement.origin, built.structures[key].dims))
        .collect();
    assert_eq!(
        placed,
        [
            (
                "site::long::long_x0_z0",
                (0, 0, 0),
                Dims { x: 48, y: 1, z: 2 }
            ),
            (
                "site::long::long_x1_z0",
                (48, 0, 0),
                Dims { x: 2, y: 1, z: 2 }
            ),
        ]
    );
    let east = &built.structures["site::long::long_x1_z0"];
    let ids: Vec<&str> = east
        .voxels
        .iter()
        .map(|slot: &PaletteIndex| east.palette.entries[usize::from(slot.0)].id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "minecraft:stone",
            "minecraft:air",
            "minecraft:stone",
            "minecraft:air"
        ]
    );
}

#[test]
fn a_litematics_regions_import_as_structs_each_with_its_corner() {
    let ir = lower_example("village.crn", Edition::Java, Some("1.21.4"));
    let file = litematic_files(&ir).remove(0);
    let name = format!("{}.litematic", file.stem);
    let mut bytes = Vec::new();
    prepare_litematic(file, &target("1.21.4"))
        .expect("the file prepares")
        .write_gzip(&mut bytes)
        .expect("the file writes");
    let import = imported(&bytes, ImportFormat::Litematic, &name);
    let comments: Vec<&str> = import
        .transliteration
        .source
        .lines()
        .filter(|line| line.starts_with("# region"))
        .collect();
    assert_eq!(
        comments,
        [
            "# region `home1`, its minimum corner at 0,0,0 of the schematic",
            "# region `home2`, its minimum corner at 15,0,0 of the schematic",
            "# region `home3`, its minimum corner at 0,0,-14 of the schematic",
            "# region `hamlet_walkway_home1_entry__home2_entry`, its minimum corner at 5,0,8 of \
             the schematic",
            "# region `hamlet_walkway_home1_entry__home3_entry`, its minimum corner at 5,0,-6 of \
             the schematic",
        ]
    );
    let names: Vec<&str> = import
        .transliteration
        .bodies
        .iter()
        .map(|body| body.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "home1",
            "home2",
            "home3",
            "hamlet_walkway_home1_entry_home2_entry",
            "hamlet_walkway_home1_entry_home3_entry",
        ]
    );
}

#[test]
fn a_schematics_offset_is_kept_as_a_comment() {
    let ir = lower_example("village.crn", Edition::Java, Some("1.21.4"));
    let file = schem_files(&ir)
        .into_iter()
        .find(|file| file.offset != (0, 0, 0))
        .expect("a placement away from the origin");
    let name = format!("{}.schem", file.stem);
    let (x, y, z) = file.offset;
    let mut bytes = Vec::new();
    prepare_schem(file, &target("1.21.4"))
        .expect("the file prepares")
        .write_gzip(&mut bytes)
        .expect("the file writes");
    let import = imported(&bytes, ImportFormat::Schem, &name);
    let source = &import.transliteration.source;
    let body = &source[source.find("\n\n").expect("a header and a body") + 2..];
    assert_eq!(
        body.lines().next(),
        Some(format!("# pasted at an offset of {x},{y},{z}").as_str())
    );
}

#[test]
fn a_version_2_schematics_offset_is_kept_without_calling_it_a_paste() {
    let mut palette = Compound::new();
    palette.insert("minecraft:stone", Tag::Int(0));
    let mut root = Compound::new();
    for (name, tag) in [
        ("Version", Tag::Int(2)),
        ("DataVersion", Tag::Int(3700)),
        ("Width", Tag::Short(1)),
        ("Height", Tag::Short(1)),
        ("Length", Tag::Short(1)),
        ("Offset", Tag::IntArray(vec![1000, 64, -200])),
        ("Palette", Tag::Compound(palette)),
        ("BlockData", Tag::ByteArray(vec![0])),
    ] {
        root.insert(name, tag);
    }
    let import = imported(&gzip(&root), ImportFormat::Schem, "copied.schem");
    let source = &import.transliteration.source;
    let body = &source[source.find("\n\n").expect("a header and a body") + 2..];
    assert_eq!(
        body.lines().next(),
        Some("# stored with an `Offset` of 1000,64,-200")
    );
}

#[test]
fn a_comparator_in_each_java_format_reports_its_block_entity_lost() {
    let ir = ir_of(comparator_array("minecraft:comparator"));
    let litematic = litematic_files(&ir).remove(0);
    let schem = schem_files(&ir).remove(0);
    let mut litematic_bytes = Vec::new();
    prepare_litematic(litematic, &target("1.21.4"))
        .expect("the file prepares")
        .write_gzip(&mut litematic_bytes)
        .expect("the file writes");
    let mut schem_bytes = Vec::new();
    prepare_schem(schem, &target("1.21.4"))
        .expect("the file prepares")
        .write_gzip(&mut schem_bytes)
        .expect("the file writes");
    for (bytes, format) in [
        (litematic_bytes, ImportFormat::Litematic),
        (schem_bytes, ImportFormat::Schem),
    ] {
        let import = imported(
            &bytes,
            format,
            &format!("comparator.{}", format.extension()),
        );
        assert_eq!(
            import.decoded.losses[1..],
            [loss(
                "comparator",
                LossKind::BlockEntities,
                "1 block entity is not written, since an import writes no block entity data, \
                 such as a chest's contents or a command block's command: 1 \
                 `minecraft:comparator`"
            )],
            "{format:?}"
        );
        let state: BlockState = import.decoded.units[0].grid.palette()[2].state.clone();
        assert_eq!(state.id, "minecraft:comparator", "{format:?}");
    }
}
