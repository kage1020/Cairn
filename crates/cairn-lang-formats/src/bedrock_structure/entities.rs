//! The IR's entities as a `.mcstructure` saves them (`spec/entities` "Two tiers"): an item frame
//! as the frame block Bedrock has for one, with its block entity, and every other entity in the
//! structure's `entities`, each with its point in the structure's coordinates.

use cairn_lang_core::block_array::{Compass, Entity, EntityData, Face, Item, Profession, Trade};
use cairn_lang_nbt::tag::{Compound, List, Tag};

use super::BedrockStructureError;
use crate::nbt_text::{compound_of, first_unencodable, put_over};
use crate::villager::saved_experience;

/// How many times a trade sells before its villager restocks.
const TRADE_USES: i32 = 12;

/// How much a trade's price moves with demand.
const TRADE_PRICE_MULTIPLIER: f32 = 0.05;

/// Each enchantment by its Java id, without the namespace, with the number Bedrock saves it as.
const ENCHANTMENTS: [(&str, i16); 41] = [
    ("protection", 0),
    ("fire_protection", 1),
    ("feather_falling", 2),
    ("blast_protection", 3),
    ("projectile_protection", 4),
    ("thorns", 5),
    ("respiration", 6),
    ("depth_strider", 7),
    ("aqua_affinity", 8),
    ("sharpness", 9),
    ("smite", 10),
    ("bane_of_arthropods", 11),
    ("knockback", 12),
    ("fire_aspect", 13),
    ("looting", 14),
    ("efficiency", 15),
    ("silk_touch", 16),
    ("unbreaking", 17),
    ("fortune", 18),
    ("power", 19),
    ("punch", 20),
    ("flame", 21),
    ("infinity", 22),
    ("luck_of_the_sea", 23),
    ("lure", 24),
    ("frost_walker", 25),
    ("mending", 26),
    ("binding_curse", 27),
    ("vanishing_curse", 28),
    ("impaling", 29),
    ("riptide", 30),
    ("loyalty", 31),
    ("channeling", 32),
    ("multishot", 33),
    ("piercing", 34),
    ("quick_charge", 35),
    ("soul_speed", 36),
    ("swift_sneak", 37),
    ("wind_burst", 38),
    ("density", 39),
    ("breach", 40),
];

/// An item frame as the block Bedrock builds one of.
pub(super) struct Frame {
    /// The entity's id, `minecraft:item_frame` or `minecraft:glow_item_frame`.
    pub(super) entity: String,
    /// The frame's cell.
    pub(super) pos: (u32, u32, u32),
    /// The block's id, `minecraft:frame` or `minecraft:glow_frame`.
    pub(super) id: &'static str,
    /// The block's states.
    pub(super) states: Compound,
    /// Its block entity, without its position.
    pub(super) block_entity: Compound,
}

/// The block Bedrock builds `entity` as, when it is an item frame.
pub(super) fn frame_block_id(entity: &Entity) -> Option<&'static str> {
    matches!(entity.data, EntityData::ItemFrame { .. }).then(|| {
        if entity.id == "minecraft:glow_item_frame" {
            "minecraft:glow_frame"
        } else {
            "minecraft:frame"
        }
    })
}

/// The entities of `entities` as Bedrock saves them: the frames, then the `entities` list.
pub(super) fn bedrock_entities(
    entities: &[Entity],
) -> Result<(Vec<Frame>, Vec<Compound>), BedrockStructureError> {
    let mut frames = Vec::new();
    let mut listed = Vec::new();
    for entity in entities {
        let facing = match &entity.data {
            EntityData::ItemFrame { facing, item } => {
                frames.push(frame(entity, *facing, item.as_ref())?);
                continue;
            }
            EntityData::Painting { facing, .. }
            | EntityData::ArmorStand { facing }
            | EntityData::Villager { facing, .. }
            | EntityData::TextDisplay { facing, .. }
            | EntityData::BlockDisplay { facing, .. }
            | EntityData::ItemDisplay { facing, .. }
            | EntityData::Spawn { facing } => *facing,
        };
        let mut nbt = entity_nbt(entity, facing, listed.len())?;
        if let Some(written) = &entity.nbt {
            let tags = compound_of(written).ok_or_else(|| BedrockStructureError::MalformedNbt {
                pos: entity.block_pos,
                text: written.clone(),
            })?;
            put_over(&mut nbt, tags);
        }
        check_strings(entity, &nbt)?;
        listed.push(nbt);
    }
    Ok((frames, listed))
}

/// Refuses the first string of `nbt` the NBT encoder could not write.
fn check_strings(entity: &Entity, nbt: &Compound) -> Result<(), BedrockStructureError> {
    match first_unencodable(nbt) {
        Some((text, source)) => Err(BedrockStructureError::UnencodableEntity {
            id: entity.id.clone(),
            pos: entity.block_pos,
            text,
            source,
        }),
        None => Ok(()),
    }
}

fn frame(
    entity: &Entity,
    facing: Face,
    item: Option<&Item>,
) -> Result<Frame, BedrockStructureError> {
    let glowing = entity.id == "minecraft:glow_item_frame";
    let mut states = Compound::new();
    states.insert("facing_direction", Tag::Int(direction_id(facing)));
    states.insert("item_frame_map_bit", Tag::Byte(0));
    states.insert("item_frame_photo_bit", Tag::Byte(0));
    let mut block_entity = Compound::new();
    block_entity.insert(
        "id",
        Tag::String(
            if glowing {
                "GlowItemFrame"
            } else {
                "ItemFrame"
            }
            .to_owned(),
        ),
    );
    if let Some(item) = item {
        block_entity.insert("Item", Tag::Compound(item_stack(item, entity)?));
    }
    block_entity.insert("ItemRotation", Tag::Float(0.0));
    block_entity.insert("ItemDropChance", Tag::Float(1.0));
    if let Some(written) = &entity.nbt {
        let tags = compound_of(written).ok_or_else(|| BedrockStructureError::MalformedNbt {
            pos: entity.block_pos,
            text: written.clone(),
        })?;
        put_over(&mut block_entity, tags);
    }
    check_strings(entity, &block_entity)?;
    Ok(Frame {
        entity: entity.id.clone(),
        pos: entity.block_pos,
        id: frame_block_id(entity).expect("an item frame's entity"),
        states,
        block_entity,
    })
}

/// One entry of `entities` for `entity` facing `facing`, the `index`th the list holds.
fn entity_nbt(
    entity: &Entity,
    facing: Compass,
    index: usize,
) -> Result<Compound, BedrockStructureError> {
    let identifier = match &entity.data {
        EntityData::Villager { .. } => "minecraft:villager_v2",
        EntityData::TextDisplay { .. }
        | EntityData::BlockDisplay { .. }
        | EntityData::ItemDisplay { .. } => {
            return Err(BedrockStructureError::UnmappedEntity {
                id: entity.id.clone(),
                pos: entity.block_pos,
            });
        }
        _ => entity.id.as_str(),
    };
    let mut definitions = vec![Tag::String(format!("+{identifier}"))];
    let mut nbt = Compound::new();
    nbt.insert("identifier", Tag::String(identifier.to_owned()));
    let (x, y, z) = entity.pos;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Bedrock saves an entity's point as floats"
    )]
    let pos = [x as f32, y as f32, z as f32];
    nbt.insert("Pos", floats(&pos));
    nbt.insert("Rotation", floats(&[yaw(facing), 0.0]));
    nbt.insert("Motion", floats(&[0.0, 0.0, 0.0]));
    nbt.insert(
        "UniqueID",
        Tag::Long(i64::try_from(index).map_or(i64::MAX, |index| index + 1)),
    );
    match &entity.data {
        EntityData::Painting {
            variant, spelling, ..
        } => {
            let motive =
                spelling
                    .clone()
                    .ok_or_else(|| BedrockStructureError::UnspelledPainting {
                        pos: entity.block_pos,
                        variant: variant.clone(),
                    })?;
            nbt.insert("Motive", Tag::String(motive));
            nbt.insert("Direction", Tag::Byte(horizontal_id(facing)));
        }
        EntityData::Villager {
            profession,
            level,
            trades,
            ..
        } => {
            if *profession != Profession::None {
                definitions.push(Tag::String(format!("+{}", profession.as_str())));
            }
            let tier = i32::from(level.number()) - 1;
            nbt.insert("Variant", Tag::Int(variant(*profession)));
            nbt.insert(
                "PreferredProfession",
                Tag::String(profession.as_str().to_owned()),
            );
            nbt.insert("TradeTier", Tag::Int(tier));
            nbt.insert("TradeExperience", Tag::Int(saved_experience(*level)));
            if !trades.is_empty() {
                let recipes = trades
                    .iter()
                    .map(|trade| offer(trade, tier, entity))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut offers = Compound::new();
                offers.insert("Recipes", Tag::List(List::of_compounds(recipes)));
                nbt.insert("Offers", Tag::Compound(offers));
            }
        }
        _ => {}
    }
    nbt.insert("definitions", Tag::List(List::of_tags(8, definitions)));
    Ok(nbt)
}

/// The number Bedrock's villager saves as its `Variant` for `profession`.
const fn variant(profession: Profession) -> i32 {
    match profession {
        Profession::None => 0,
        Profession::Farmer => 1,
        Profession::Fisherman => 2,
        Profession::Shepherd => 3,
        Profession::Fletcher => 4,
        Profession::Librarian => 5,
        Profession::Cartographer => 6,
        Profession::Cleric => 7,
        Profession::Armorer => 8,
        Profession::Weaponsmith => 9,
        Profession::Toolsmith => 10,
        Profession::Butcher => 11,
        Profession::Leatherworker => 12,
        Profession::Mason => 13,
        Profession::Nitwit => 14,
    }
}

fn floats(values: &[f32]) -> Tag {
    Tag::List(List::of_tags(
        5,
        values.iter().copied().map(Tag::Float).collect(),
    ))
}

/// The yaw an entity facing `facing` turns to: south 0, west 90, north 180, east 270.
const fn yaw(facing: Compass) -> f32 {
    match facing {
        Compass::South => 0.0,
        Compass::West => 90.0,
        Compass::North => 180.0,
        Compass::East => 270.0,
    }
}

/// The number a painting's `Direction` saves: south 0, west 1, north 2, east 3.
const fn horizontal_id(facing: Compass) -> i8 {
    match facing {
        Compass::South => 0,
        Compass::West => 1,
        Compass::North => 2,
        Compass::East => 3,
    }
}

/// The number a frame's `facing_direction` saves: down 0, up 1, north 2, south 3, west 4, east 5.
fn direction_id(facing: Face) -> i32 {
    match facing {
        Face::Down => 0,
        Face::Up => 1,
        Face::North => 2,
        Face::South => 3,
        Face::West => 4,
        Face::East => 5,
    }
}

/// `item`, which `holder` holds or trades, as Bedrock saves an item stack, its enchantment under
/// `tag.ench` by number.
fn item_stack(item: &Item, holder: &Entity) -> Result<Compound, BedrockStructureError> {
    let mut stack = Compound::new();
    stack.insert("Name", Tag::String(item.id.clone()));
    stack.insert("Count", Tag::Byte(item.count.as_i8()));
    stack.insert("Damage", Tag::Short(0));
    stack.insert("WasPickedUp", Tag::Byte(0));
    if let Some(enchantment) = &item.enchantment {
        let name = enchantment
            .id
            .strip_prefix("minecraft:")
            .unwrap_or(&enchantment.id);
        let Some(&(_, number)) = ENCHANTMENTS.iter().find(|(known, _)| *known == name) else {
            return Err(BedrockStructureError::UnmappedEnchantment {
                holder: holder.id.clone(),
                pos: holder.block_pos,
                id: enchantment.id.clone(),
                known: ENCHANTMENTS.map(|(known, _)| known).join(", "),
            });
        };
        let mut entry = Compound::new();
        entry.insert("id", Tag::Short(number));
        entry.insert("lvl", Tag::Short(i16::from(enchantment.level)));
        let mut tag = Compound::new();
        tag.insert("ench", Tag::List(List::of_compounds(vec![entry])));
        stack.insert("tag", Tag::Compound(tag));
    }
    Ok(stack)
}

/// One of the `Recipes` of `villager` at trade tier `tier`.
fn offer(trade: &Trade, tier: i32, villager: &Entity) -> Result<Compound, BedrockStructureError> {
    let mut offer = Compound::new();
    offer.insert("buyA", Tag::Compound(item_stack(&trade.buy, villager)?));
    offer.insert("buyCountA", Tag::Int(i32::from(trade.buy.count.get())));
    offer.insert("buyCountB", Tag::Int(0));
    offer.insert("sell", Tag::Compound(item_stack(&trade.sell, villager)?));
    offer.insert("uses", Tag::Int(0));
    offer.insert("maxUses", Tag::Int(TRADE_USES));
    offer.insert("rewardExp", Tag::Byte(1));
    offer.insert("tier", Tag::Int(tier));
    offer.insert("traderExp", Tag::Int(1));
    offer.insert("priceMultiplierA", Tag::Float(TRADE_PRICE_MULTIPLIER));
    offer.insert("priceMultiplierB", Tag::Float(0.0));
    offer.insert("demand", Tag::Int(0));
    Ok(offer)
}
