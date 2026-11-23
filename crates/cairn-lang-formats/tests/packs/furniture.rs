//! Every def of the furniture library builds on every target the built-in packs support, from
//! ids that target declares (`spec/components-editing-sites` "The furniture library").

use std::fmt::Write as _;

use cairn_lang_core::Edition;
use cairn_lang_core::furniture;
use cairn_lang_formats::bedrock_structure::build_mcstructure_tag;
use cairn_lang_formats::data_version::resolve_bedrock_target;
use cairn_lang_formats::registry::builtin_pack;
use cairn_lang_nbt::tag::Tag;

use crate::common::{block_palette, compile_source, states_for, supported_versions};

/// One room per library def, each standing its def against the back wall.
fn furnished() -> String {
    let mut src =
        String::from("theme t:\n  slot floor -> @oak_planks\n  slot wall -> @cobblestone\n\n");
    for def in furniture::defs() {
        writeln!(
            src,
            "struct {name} size=5x5\n  floor mat_slot=floor\n  walls mat_slot=wall height=4\n  \
             place use={name} at=inside.back offset=1 y=1\n",
            name = def.name,
        )
        .expect("a String takes every write");
    }
    src
}

#[test]
fn every_library_def_builds_on_every_target_from_ids_it_declares() {
    let src = furnished();
    for edition in [Edition::Java, Edition::Bedrock] {
        let pack = builtin_pack(edition);
        for version in supported_versions(pack) {
            let built = compile_source(&src, edition, version)
                .unwrap_or_else(|codes| panic!("{edition:?} {version} refuses it: {codes:?}"));
            assert_eq!(
                built
                    .diagnostics
                    .iter()
                    .map(|d| (d.code.as_str(), d.primary.as_str()))
                    .collect::<Vec<_>>(),
                [],
                "{edition:?} {version}",
            );
            assert_eq!(built.structures.len(), furniture::defs().len());
            let ids = pack.blocks().ids_for(version).expect("a block table");
            for (name, structure) in &built.structures {
                let written: Vec<String> = match edition {
                    Edition::Java => structure
                        .palette
                        .entries
                        .iter()
                        .map(|e| e.id.clone())
                        .collect(),
                    Edition::Bedrock => {
                        let target = resolve_bedrock_target(version).expect("a known target");
                        let (root, notes) =
                            build_mcstructure_tag(structure, &target, &states_for(&target))
                                .unwrap_or_else(|e| panic!("`{name}` at {version}: {e}"));
                        assert_eq!(notes, [], "`{name}` at Bedrock {version}");
                        block_palette(&root)
                            .iter()
                            .map(|entry| match entry {
                                Tag::Compound(c) => match c.entries.get("name") {
                                    Some(Tag::String(id)) => id.clone(),
                                    other => panic!("name: {other:?}"),
                                },
                                other => panic!("palette entry: {other:?}"),
                            })
                            .collect()
                    }
                };
                for id in written {
                    assert!(
                        ids.binary_search(&id).is_ok(),
                        "`{name}` writes `{id}`, which {edition:?} {version} does not declare",
                    );
                }
            }
        }
    }
}
