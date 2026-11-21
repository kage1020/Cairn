//! The IR's entities as a `.mcstructure` saves them (`spec/entities` "Two tiers"): an item frame
//! as the frame block Bedrock has for one, with its block entity, and every other entity in the
//! structure's `entities`, each with its point in the structure's coordinates.

use cairn_lang_core::block_array::{Entity, EntityData, Face, Item, Trade};
use cairn_lang_nbt::tag::{Compound, List, Tag};

use super::BedrockStructureError;
use crate::nbt_text::{compound_of, first_unencodable, put_over};

/// The experience each villager level starts at, from level 1.
const LEVEL_XP: [i32; 5] = [0, 10, 70, 150, 250];

/// How many times a trade sells before its villager restocks.
const TRADE_USES: i32 = 12;

/// How much a trade's price moves with demand.
const TRADE_PRICE_MULTIPLIER: f32 = 0.05;

/// Each profession with the number Bedrock's villager saves as its `Variant`, `none` 0.
const PROFESSIONS: [(&str, i32); 15] = [
    ("none", 0),
    ("farmer", 1),
    ("fisherman", 2),
    ("shepherd", 3),
    ("fletcher", 4),
    ("librarian", 5),
    ("cartographer", 6),
    ("cleric", 7),
    ("armorer", 8),
    ("weaponsmith", 9),
    ("toolsmith", 10),
    ("butcher", 11),
    ("leatherworker", 12),
    ("mason", 13),
    ("nitwit", 14),
];

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
    /// The frame's cell.
    pub(super) pos: (u32, u32, u32),
    /// The block's id, `minecraft:frame` or `minecraft:glow_frame`.
    pub(super) id: &'static str,
    /// The block's states.
    pub(super) states: Compound,
    /// Its block entity, without its position.
    pub(super) block_entity: Compound,
}

/// The entities of `entities` as Bedrock saves them: the frames, then the `entities` list.
pub(super) fn bedrock_entities(
    entities: &[Entity],
) -> Result<(Vec<Frame>, Vec<Compound>), BedrockStructureError> {
    let mut frames = Vec::new();
    let mut listed = Vec::new();
    for entity in entities {
        if let EntityData::ItemFrame { item } = &entity.data {
            frames.push(frame(entity, item.as_ref())?);
            continue;
        }
        let mut nbt = entity_nbt(entity, listed.len())?;
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

fn frame(entity: &Entity, item: Option<&Item>) -> Result<Frame, BedrockStructureError> {
    let glowing = entity.id == "minecraft:glow_item_frame";
    let mut states = Compound::new();
    states.insert("facing_direction", Tag::Int(direction_id(entity.facing)));
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
        block_entity.insert("Item", Tag::Compound(item_stack(item)?));
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
        pos: entity.block_pos,
        id: if glowing {
            "minecraft:glow_frame"
        } else {
            "minecraft:frame"
        },
        states,
        block_entity,
    })
}

/// One entry of `entities` for `entity`, the `index`th the list holds.
fn entity_nbt(entity: &Entity, index: usize) -> Result<Compound, BedrockStructureError> {
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
    nbt.insert("Rotation", floats(&[yaw(entity.facing), 0.0]));
    nbt.insert("Motion", floats(&[0.0, 0.0, 0.0]));
    nbt.insert(
        "UniqueID",
        Tag::Long(i64::try_from(index).map_or(i64::MAX, |index| index + 1)),
    );
    match &entity.data {
        EntityData::Painting { variant, .. } => {
            nbt.insert("Motive", Tag::String(variant.clone()));
            nbt.insert("Direction", Tag::Byte(horizontal_id(entity.facing)));
        }
        EntityData::Villager {
            profession,
            level,
            trades,
        } => {
            let variant = PROFESSIONS
                .iter()
                .find(|(known, _)| known == profession)
                .map_or(0, |&(_, variant)| variant);
            if variant != 0 {
                definitions.push(Tag::String(format!("+{profession}")));
            }
            let tier = i32::from(level.clamp(&1, &5) - 1);
            nbt.insert("Variant", Tag::Int(variant));
            nbt.insert("PreferredProfession", Tag::String(profession.clone()));
            nbt.insert("TradeTier", Tag::Int(tier));
            nbt.insert(
                "TradeExperience",
                Tag::Int(LEVEL_XP[usize::try_from(tier).unwrap_or(0)].max(1)),
            );
            if !trades.is_empty() {
                let recipes = trades
                    .iter()
                    .map(|trade| offer(trade, tier))
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

fn floats(values: &[f32]) -> Tag {
    Tag::List(List::of_tags(
        5,
        values.iter().copied().map(Tag::Float).collect(),
    ))
}

/// The yaw an entity facing `facing` turns to: south 0, west 90, north 180, east 270.
fn yaw(facing: Face) -> f32 {
    match facing {
        Face::West => 90.0,
        Face::North => 180.0,
        Face::East => 270.0,
        _ => 0.0,
    }
}

/// The number a painting's `Direction` saves: south 0, west 1, north 2, east 3.
fn horizontal_id(facing: Face) -> i8 {
    match facing {
        Face::West => 1,
        Face::North => 2,
        Face::East => 3,
        _ => 0,
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

/// `item` as Bedrock saves an item stack, its enchantment under `tag.ench` by number.
fn item_stack(item: &Item) -> Result<Compound, BedrockStructureError> {
    let mut stack = Compound::new();
    stack.insert("Name", Tag::String(item.id.clone()));
    stack.insert(
        "Count",
        Tag::Byte(i8::try_from(item.count).unwrap_or(i8::MAX)),
    );
    stack.insert("Damage", Tag::Short(0));
    stack.insert("WasPickedUp", Tag::Byte(0));
    if let Some(enchantment) = &item.enchantment {
        let name = enchantment
            .id
            .strip_prefix("minecraft:")
            .unwrap_or(&enchantment.id);
        let Some(&(_, number)) = ENCHANTMENTS.iter().find(|(known, _)| *known == name) else {
            return Err(BedrockStructureError::UnmappedEnchantment {
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

/// One of a villager's `Recipes` at trade tier `tier`.
fn offer(trade: &Trade, tier: i32) -> Result<Compound, BedrockStructureError> {
    let mut offer = Compound::new();
    offer.insert("buyA", Tag::Compound(item_stack(&trade.buy)?));
    offer.insert("buyCountA", Tag::Int(i32::from(trade.buy.count)));
    offer.insert("buyCountB", Tag::Int(0));
    offer.insert("sell", Tag::Compound(item_stack(&trade.sell)?));
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
