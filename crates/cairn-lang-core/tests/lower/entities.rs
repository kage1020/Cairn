//! An entity member places one entity where its line says, facing the way it says, holding what
//! its keys write; a `trade` joins the villager it names, and a `hologram` is built as the member
//! form its slot binds (`spec/entities` "Two tiers").

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::block_array::{
    AbsentEntity, BlockIdSet, BlockState, Enchantment, Entity, EntityData, Face, Item,
    PaintingLookup, PaintingVariant, StateDomain, TargetRegistry, Trade,
};
use cairn_lang_core::{BlockArrayIr, DiagnosticCode, Edition, lower, parse, resolve};

use crate::common::{
    Finding, findings, lowered, lowered_with, lowered_with_resolver_diagnostics, only_structure,
};

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n\n";

/// A 7x5 struct with a floor and 3-row walls, `members` after them; with no overhang its front
/// wall is `z=4` and its left `x=0`.
fn room(members: &str) -> String {
    format!(
        "{THEME}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         {members}\n"
    )
}

/// The entities `out` places.
fn placed(out: &BlockArrayIr) -> Vec<Entity> {
    only_structure(out).entities.clone()
}

/// The entities `members` places in [`room`] with no target, which it must place without a
/// finding.
fn place(members: &str) -> Vec<Entity> {
    let src = room(members);
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), Vec::<Finding<'_>>::new(), "{members}");
    placed(&out)
}

fn entity(
    id: &str,
    pos: (f64, f64, f64),
    block_pos: (u32, u32, u32),
    facing: Face,
    data: EntityData,
) -> Entity {
    Entity {
        id: id.to_owned(),
        pos,
        block_pos,
        facing,
        data,
        nbt: None,
    }
}

fn item(id: &str, count: u8) -> Item {
    Item {
        id: format!("minecraft:{id}"),
        count,
        enchantment: None,
    }
}

/// A target whose painting table has `kebab` 1x1 and `pointer` 4x4, and `dennis` only at another
/// version; it lacks every display when `bedrock`.
struct Paintings {
    bedrock: bool,
}

impl TargetRegistry for Paintings {
    fn lookup(&self, _token: &str) -> Option<BlockState> {
        None
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        None
    }

    fn state_domain(&self, _state: &BlockState) -> StateDomain {
        StateDomain::Checked(Vec::new())
    }

    fn painting(&self, variant: &str) -> PaintingLookup {
        let known = |spelling: &str, side| {
            PaintingLookup::Known(PaintingVariant {
                spelling: spelling.to_owned(),
                width: side,
                height: side,
            })
        };
        match variant {
            "kebab" => known("Kebab", 1),
            "pointer" => known("Pointer", 4),
            other => PaintingLookup::Unknown {
                registry: "java 1.21".to_owned(),
                known: vec!["kebab".to_owned(), "pointer".to_owned()],
                elsewhere: if other == "dennis" {
                    vec!["26.3".to_owned()]
                } else {
                    Vec::new()
                },
            },
        }
    }

    fn absent_entity(&self, id: &str) -> Option<AbsentEntity> {
        (self.bedrock && id.ends_with("_display")).then(|| AbsentEntity {
            registry: "bedrock 1.21.60".to_owned(),
            declared_by: Edition::Java,
            reason: "Bedrock has no display entity".to_owned(),
        })
    }
}

#[test]
fn a_painting_by_a_wall_hangs_in_the_cell_inside_it_facing_away() {
    assert_eq!(
        place("painting at=inside.front offset=2 y=1 variant=kebab"),
        [entity(
            "minecraft:painting",
            (2.5, 1.5, 3.968_75),
            (2, 1, 3),
            Face::North,
            EntityData::Painting {
                variant: "minecraft:kebab".to_owned(),
                width: 1,
                height: 1,
            },
        )],
    );
}

#[test]
fn a_wide_painting_runs_right_and_up_from_its_anchor_and_hangs_by_the_game_s_cell() {
    let src = format!(
        "{THEME}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=4\n  \
         painting at=inside.front offset=4 y=1 variant=pointer\n"
    );
    let out = lowered_with(&src, &Paintings { bedrock: false });
    assert_eq!(findings(&src, &out), Vec::<Finding<'_>>::new());
    assert_eq!(
        placed(&out),
        [entity(
            "minecraft:painting",
            (3.5, 2.5, 3.968_75),
            (3, 2, 3),
            Face::North,
            EntityData::Painting {
                variant: "Pointer".to_owned(),
                width: 4,
                height: 4,
            },
        )],
    );
}

#[test]
fn a_painting_variant_the_target_lacks_is_refused_with_what_it_has() {
    let src = room(
        "painting at=inside.front offset=2 y=1 variant=kebabs\n  \
         painting at=inside.back offset=3 y=1 variant=dennis",
    );
    let out = lowered_with(&src, &Paintings { bedrock: false });
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::InvalidMember,
                "painting at=inside.front offset=2 y=1 variant=kebabs",
                "painting `variant=kebabs` is no painting `java 1.21` has",
                vec![
                    "did you mean `variant=kebab`?",
                    "`java 1.21` has the paintings kebab, pointer",
                ],
            ),
            (
                DiagnosticCode::InvalidMember,
                "painting at=inside.back offset=3 y=1 variant=dennis",
                "painting `variant=dennis` is no painting `java 1.21` has",
                vec![
                    "`dennis` is a painting of 26.3: pass a `--target` that has it, or pick another"
                ],
            ),
        ],
    );
    assert_eq!(placed(&out), []);
}

#[test]
fn an_item_frame_hangs_on_a_wall_or_on_the_face_its_facing_turns_from() {
    assert_eq!(
        place(
            "item_frame at=inside.back offset=3 y=2 item=diamond\n  \
             item_frame at=2,1,2 facing=up glowing=true"
        ),
        [
            entity(
                "minecraft:glow_item_frame",
                (2.5, 1.031_25, 2.5),
                (2, 1, 2),
                Face::Up,
                EntityData::ItemFrame { item: None },
            ),
            entity(
                "minecraft:item_frame",
                (3.5, 2.5, 1.031_25),
                (3, 2, 1),
                Face::South,
                EntityData::ItemFrame {
                    item: Some(item("diamond", 1)),
                },
            ),
        ],
    );
}

#[test]
fn a_hanging_entity_at_a_triple_needs_a_facing_and_one_by_a_wall_takes_none() {
    let src = room(
        "item_frame at=2,1,2\n  \
         painting at=inside.front offset=2 y=1 facing=south variant=kebab",
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::InvalidMember,
                "item_frame at=2,1,2",
                "item_frame at a coordinate triple has no `facing=`, so nothing says which block \
                 it hangs on",
                vec![
                    "write `facing=` with the way it faces, one of north, south, east, west, up, \
                     down: it hangs on the block behind it"
                ],
            ),
            (
                DiagnosticCode::InvalidMember,
                "painting at=inside.front offset=2 y=1 facing=south variant=kebab",
                "painting `facing=south` by a wall: it hangs on the wall and faces away from it, \
                 so `facing=` has nothing to choose",
                vec!["drop `facing=`, or place it at a coordinate triple, which takes one"],
            ),
        ],
    );
    assert_eq!(placed(&out), []);
}

#[test]
fn a_standing_entity_stands_on_its_cell_s_floor_facing_south_unless_told() {
    assert_eq!(
        place("armor_stand at=3,1,2 facing=east\n  armor_stand at=1,1,1"),
        [
            entity(
                "minecraft:armor_stand",
                (1.5, 1.0, 1.5),
                (1, 1, 1),
                Face::South,
                EntityData::ArmorStand,
            ),
            entity(
                "minecraft:armor_stand",
                (3.5, 1.0, 2.5),
                (3, 1, 2),
                Face::East,
                EntityData::ArmorStand,
            ),
        ],
    );
}

#[test]
fn a_villager_offers_the_trades_that_name_it_in_the_order_written() {
    assert_eq!(
        place(
            "villager id=lib at=2,1,2 profession=librarian level=expert\n  \
             trade villager=lib buy=emerald count=24 sell=enchanted_book enchant=mending \
             enchant_level=1\n  \
             trade villager=lib buy=book sell=emerald sell_count=2"
        ),
        [entity(
            "minecraft:villager",
            (2.5, 1.0, 2.5),
            (2, 1, 2),
            Face::South,
            EntityData::Villager {
                profession: "librarian".to_owned(),
                level: 4,
                trades: vec![
                    Trade {
                        buy: item("emerald", 24),
                        sell: Item {
                            enchantment: Some(Enchantment {
                                id: "minecraft:mending".to_owned(),
                                level: 1,
                            }),
                            ..item("enchanted_book", 1)
                        },
                    },
                    Trade {
                        buy: item("book", 1),
                        sell: item("emerald", 2),
                    },
                ],
            },
        )],
    );
}

#[test]
fn a_trade_no_villager_offers_is_refused() {
    let src = room("villager id=lib at=2,1,2\n  trade villager=libr buy=book sell=emerald");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::InvalidMember,
            "trade villager=libr buy=book sell=emerald",
            "trade `villager=libr`: no villager of this body has `id=libr`",
            vec![
                "did you mean `villager=lib`?",
                "this body's villagers are lib",
            ],
        )],
    );
}

#[test]
fn an_item_display_that_names_no_item_is_refused() {
    let src = room("item_display at=2,1,2\n  item_display at=3,1,2 item=\"sword\" scale=2");
    let out = lowered(&src);
    let fix = "write `item=` with the item's id, such as `item=diamond_sword`";
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::InvalidMember,
                "item_display at=2,1,2",
                "item_display has no `item=`, so nothing says what it shows",
                vec![fix],
            ),
            (
                DiagnosticCode::InvalidMember,
                "item_display at=3,1,2 item=\"sword\" scale=2",
                "item_display `item=` must be an id such as `diamond` or `minecraft:diamond`, not \
                 string `\"sword\"`",
                vec![fix],
            ),
        ],
    );
    assert_eq!(placed(&out), []);
}

/// The entities in cell order: by row, then z, then x.
#[test]
fn a_display_stands_on_its_cell_s_floor_showing_what_its_keys_name() {
    assert_eq!(
        place(
            "text_display at=3,2,2 text=[\"Inn\", \"rooms\"] billboard=center scale=1.5\n  \
             block_display at=1,1,1 mat_slot=wall scale=0.5\n  \
             item_display at=5,1,3 item=diamond_sword facing=west"
        ),
        [
            entity(
                "minecraft:block_display",
                (1.5, 1.0, 1.5),
                (1, 1, 1),
                Face::South,
                EntityData::BlockDisplay {
                    block: BlockState::bare("minecraft:cobblestone"),
                    scale: 0.5,
                },
            ),
            entity(
                "minecraft:item_display",
                (5.5, 1.0, 3.5),
                (5, 1, 3),
                Face::West,
                EntityData::ItemDisplay {
                    item: item("diamond_sword", 1),
                    scale: 1.0,
                },
            ),
            entity(
                "minecraft:text_display",
                (3.5, 2.0, 2.5),
                (3, 2, 2),
                Face::South,
                EntityData::TextDisplay {
                    lines: vec!["Inn".to_owned(), "rooms".to_owned()],
                    billboard: "center".to_owned(),
                    scale: 1.5,
                },
            ),
        ],
    );
}

#[test]
fn a_display_the_target_s_edition_lacks_is_refused_with_what_to_write() {
    let src = room("text_display at=3,2,2 text=\"Inn\"");
    let out = lowered_with(&src, &Paintings { bedrock: true });
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::ParityUnsupported,
            "text_display at=3,2,2 text=\"Inn\"",
            "`minecraft:text_display` is an entity only Java has, so `bedrock 1.21.60` cannot \
             build it",
            vec![
                "Bedrock has no display entity",
                "the semantic layer is edition-neutral: write floating text as a `hologram` whose \
                 slot a `theme NAME_java` binds to `text_display` and a `theme NAME_bedrock` to \
                 `sign`, or place a block or an `item_frame`, which both editions have",
            ],
        )],
    );
    assert_eq!(placed(&out), []);
}

#[test]
fn a_spawn_under_its_edition_s_guard_places_its_type_with_its_nbt_as_written() {
    let src = room("@edition java\n    spawn type=cat at=2,1,2 nbt={variant:\"minecraft:black\"}");
    let module = parse(&src).expect("parses");
    let ir = lower(&module);
    let out = lower_to_block_array(&ir, &resolve(&ir, Some(Edition::Java)), None);
    assert_eq!(findings(&src, &out), Vec::<Finding<'_>>::new());
    assert_eq!(
        placed(&out),
        [Entity {
            nbt: Some("{variant:\"minecraft:black\"}".to_owned()),
            ..entity(
                "minecraft:cat",
                (2.5, 1.0, 2.5),
                (2, 1, 2),
                Face::South,
                EntityData::Spawn,
            )
        }],
    );
}

/// A hologram in [`room`] whose slot `holo` `binding` binds, lowered with no target.
fn hologram(binding: &str) -> BlockArrayIr {
    let src = format!(
        "theme t:\n  slot wall -> @cobblestone\n  slot holo -> {binding}\n\n\
         struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         hologram at=3,1,2 facing=north text=\"Inn\" mat_slot=holo\n"
    );
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), Vec::<Finding<'_>>::new(), "{binding}");
    out
}

#[test]
fn a_hologram_is_built_as_the_member_form_its_slot_binds_with_the_form_s_keys() {
    assert_eq!(
        placed(&hologram("text_display billboard=vertical")),
        [entity(
            "minecraft:text_display",
            (3.5, 1.0, 2.5),
            (3, 1, 2),
            Face::North,
            EntityData::TextDisplay {
                lines: vec!["Inn".to_owned()],
                billboard: "vertical".to_owned(),
                scale: 1.0,
            },
        )],
    );
    let as_sign = hologram("sign glowing=true");
    assert_eq!(placed(&as_sign), []);
    let array = only_structure(&as_sign);
    assert_eq!(
        array
            .block_entities
            .iter()
            .map(|entity| (entity.pos, format!("{:?}", entity.data)))
            .collect::<Vec<_>>(),
        [(
            (3, 1, 2),
            "Sign { front: SignText { lines: [\"Inn\"], glowing: true }, back: SignText { lines: \
             [], glowing: true } }"
                .to_owned()
        )],
    );
}

#[test]
fn a_member_form_binds_only_a_hologram_and_takes_only_its_own_keys() {
    let src = "theme t:\n  slot wall -> @cobblestone\n  \
               slot holo -> text_display scale=2 glowing=true\n  slot blk -> @stone\n\n\
               struct s size=7x5\n  floor mat_slot=holo\n  walls mat_slot=wall height=3\n  \
               hologram at=3,1,2 text=\"Inn\" mat_slot=blk\n";
    let out = lowered_with_resolver_diagnostics(src);
    assert_eq!(
        findings(src, &out),
        [
            (
                DiagnosticCode::UnknownArgument,
                "true",
                "member form `text_display` takes no `glowing=`",
                vec![
                    "the member form `text_display` takes `billboard=` and `scale=`; the \
                     `hologram` line places it and writes its text",
                ],
            ),
            (
                DiagnosticCode::InvalidMember,
                "text_display",
                "floor `mat_slot=holo` binds the member form `text_display`, which only a \
                 `hologram` is built as",
                vec![
                    "bind `holo` to a block, as `slot holo -> @oak_planks`, or give the floor a \
                     slot that binds one",
                ],
            ),
            (
                DiagnosticCode::InvalidMember,
                "hologram at=3,1,2 text=\"Inn\" mat_slot=blk",
                "hologram `mat_slot=blk` binds token `@stone`, a block, where a hologram is built \
                 as the member form its slot binds",
                vec![
                    "bind the slot to a member form in the theme: `slot blk -> text_display`, or \
                     `slot blk -> sign`",
                ],
            ),
        ],
    );
    assert_eq!(placed(&out), []);
}
