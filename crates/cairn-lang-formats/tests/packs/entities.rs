//! What the built-in packs say of the entities a build places: each version's paintings, the
//! entities an edition lacks, and the boxes and solid blocks the constraint checks read, pinned or
//! not (`spec/entities` "Hanging entities").

use std::num::NonZeroU32;

use cairn_lang_core::block_array::{
    AbsentEntity, BlockState, EntityBox, PaintingLookup, PaintingVariant, TargetRegistry,
};
use cairn_lang_core::{Diagnostic, Edition, lower, parse};
use cairn_lang_formats::build::lowering::check_lowering;
use cairn_lang_formats::registry::{builtin_pack, builtin_view};

/// A painting of `width` by `height` that its target spells `spelling`.
fn known(spelling: &str, width: u32, height: u32) -> PaintingLookup {
    PaintingLookup::Known(PaintingVariant {
        spelling: spelling.to_owned(),
        width: NonZeroU32::new(width).expect("a painting's width"),
        height: NonZeroU32::new(height).expect("a painting's height"),
    })
}

#[test]
fn each_pack_hangs_a_painting_at_its_size_pinned_or_not() {
    for pinned in [Some("26.3"), None] {
        assert_eq!(
            builtin_view(Edition::Java, pinned).painting("pointer"),
            known("minecraft:pointer", 4, 4),
            "{pinned:?}",
        );
    }
    for pinned in [Some("1.21.0"), Some("1.26.60"), None] {
        let bedrock = builtin_view(Edition::Bedrock, pinned);
        assert_eq!(
            bedrock.painting("pointer"),
            known("Pointer", 4, 4),
            "{pinned:?}"
        );
        assert_eq!(bedrock.painting("orb"), known("orb", 4, 4), "{pinned:?}");
    }
}

#[test]
fn a_painting_the_target_lacks_is_unknown_with_the_versions_that_have_it() {
    let paintings = builtin_pack(Edition::Bedrock).paintings();
    assert_eq!(
        builtin_view(Edition::Bedrock, Some("1.21.0")).painting("dennis"),
        PaintingLookup::Unknown {
            registry: "bedrock 1.21.0".to_owned(),
            known: paintings.variants_at("1.21.0").map(str::to_owned).collect(),
            elsewhere: vec!["1.26.60".to_owned()],
        },
    );
    assert_eq!(
        builtin_view(Edition::Bedrock, None).painting("kebabb"),
        PaintingLookup::Unknown {
            registry: "bedrock".to_owned(),
            known: paintings
                .every_variant()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            elsewhere: Vec::new(),
        },
    );
}

#[test]
fn the_bedrock_pack_lacks_the_display_entities_pinned_or_not() {
    let reason = "a display entity, which draws text, a block or an item where it stands without \
                  being one, with no box and no collision; Bedrock has no display entity";
    for (pinned, registry) in [(Some("1.26.60"), "bedrock 1.26.60"), (None, "bedrock")] {
        for id in [
            "minecraft:text_display",
            "minecraft:block_display",
            "minecraft:item_display",
        ] {
            assert_eq!(
                builtin_view(Edition::Bedrock, pinned).absent_entity(id),
                Some(AbsentEntity {
                    registry: registry.to_owned(),
                    declared_by: Edition::Java,
                    reason: reason.to_owned(),
                }),
                "{pinned:?} {id}",
            );
        }
        assert_eq!(
            builtin_view(Edition::Bedrock, pinned).absent_entity("minecraft:villager"),
            None
        );
    }
    for pinned in [Some("26.3"), None] {
        assert_eq!(
            builtin_view(Edition::Java, pinned).absent_entity("minecraft:text_display"),
            None
        );
    }
}

#[test]
fn a_pinned_view_answers_what_is_solid_and_how_big_an_entity_is_and_an_unpinned_one_does_not() {
    let block = |id: &str| BlockState::bare(format!("minecraft:{id}"));
    let pinned = builtin_view(Edition::Java, Some("26.3"));
    assert_eq!(pinned.solid(&block("cobblestone")), Some(true));
    assert_eq!(pinned.solid(&block("glass_pane")), Some(false));
    assert_eq!(
        pinned.entity_box("minecraft:villager"),
        Some(EntityBox {
            width: 0.6,
            height: 1.95,
            depth: None,
        }),
    );
    assert_eq!(
        pinned.entity_box("minecraft:item_frame"),
        Some(EntityBox {
            width: 0.75,
            height: 0.75,
            depth: Some(0.0625),
        }),
    );
    let unpinned = builtin_view(Edition::Java, None);
    assert_eq!(unpinned.solid(&block("cobblestone")), None);
    assert_eq!(unpinned.entity_box("minecraft:villager"), None);
}

/// A 7x5 room with 3-row walls and `member` in it.
fn room_with(member: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n\n\
         struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  {member}\n"
    )
}

/// What `cairn check` with `edition` and no `--target` finds on `source`, each finding as its code,
/// the text it covers and its message.
fn checked(source: &str, edition: Option<Edition>) -> Vec<(&'static str, String, String)> {
    let ir = lower(&parse(source).expect("the source parses"));
    check_lowering(&ir, edition, None)
        .diagnostics
        .into_iter()
        .map(|d: Diagnostic| (d.code.as_str(), source[d.span].to_owned(), d.primary))
        .collect()
}

#[test]
fn a_check_with_an_edition_and_no_target_weighs_its_entities_against_every_version() {
    let painting = "painting at=inside.front offset=2 y=1 variant=kebabb";
    let source = room_with(painting);
    assert_eq!(
        checked(&source, Some(Edition::Java)),
        [(
            "E_INVALID_MEMBER",
            painting.to_owned(),
            "painting `variant=kebabb` is no painting `java` has".to_owned(),
        )],
    );
    let display = "text_display at=3,1,2 text=\"Inn\"";
    let source = room_with(display);
    assert_eq!(
        checked(&source, Some(Edition::Bedrock)),
        [(
            "E_PARITY_UNSUPPORTED",
            display.to_owned(),
            "`minecraft:text_display` is an entity only Java has, so `bedrock` cannot build it"
                .to_owned(),
        )],
    );
    assert_eq!(checked(&source, Some(Edition::Java)), []);
    assert_eq!(checked(&source, None), []);
}
