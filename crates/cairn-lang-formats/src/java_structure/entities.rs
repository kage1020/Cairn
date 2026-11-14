//! The IR's entities as a Java structure's `entities` saves them: each with its point, its cell
//! and its `nbt`, in the shape the target's `DataVersion` reads (`spec/entities` "Two tiers").

use cairn_lang_core::block_array::{Entity, EntityData, Face, Item, Trade};
use cairn_lang_nbt::tag::{Compound, List, Tag};

use super::{JavaStructureError, palette_entry, palette_entry_keys, text_component};
use crate::nbt_text::{compound_of, first_unencodable, put_over};

/// The first `DataVersion` whose items are saved as data components, 1.20.5's.
const ITEM_COMPONENTS_FROM: i32 = 3837;

/// The first `DataVersion` whose enchantments map each id to its level straight, 1.21.5's.
const BARE_ENCHANTMENT_LEVELS_FROM: i32 = 4325;

/// The item that keeps its enchantment as a stored one.
const ENCHANTED_BOOK: &str = "minecraft:enchanted_book";

/// The experience each villager level starts at, from level 1, as the game levels one up.
const LEVEL_XP: [i32; 5] = [0, 10, 70, 150, 250];

/// How many times a trade sells before its villager restocks.
const TRADE_USES: i32 = 12;

/// How much a trade's price moves with demand and reputation.
const TRADE_PRICE_MULTIPLIER: f32 = 0.05;

/// Each entity of `entities` as one entry of `entities`, for `data_version`.
pub(super) fn entities_list(
    entities: &[Entity],
    data_version: i32,
) -> Result<Vec<Compound>, JavaStructureError> {
    entities
        .iter()
        .map(|entity| entity_entry(entity, data_version))
        .collect()
}

fn entity_entry(entity: &Entity, data_version: i32) -> Result<Compound, JavaStructureError> {
    let mut nbt = entity_nbt(entity, data_version);
    if let Some(written) = &entity.nbt {
        let tags = compound_of(written).ok_or_else(|| JavaStructureError::MalformedNbt {
            pos: entity.block_pos,
            text: written.clone(),
        })?;
        put_over(&mut nbt, tags);
    }
    if let Some((text, source)) = first_unencodable(&nbt) {
        return Err(JavaStructureError::UnencodableEntity {
            id: entity.id.clone(),
            pos: entity.block_pos,
            text,
            source,
        });
    }
    let (x, y, z) = entity.block_pos;
    let mut cell = [0; 3];
    for (slot, (axis, value)) in cell.iter_mut().zip([("x", x), ("y", y), ("z", z)]) {
        *slot = i32::try_from(value)
            .map_err(|_| JavaStructureError::DimensionOverflow { axis, value })?;
    }
    let (px, py, pz) = entity.pos;
    let mut entry = Compound::new();
    entry.insert(
        "pos",
        Tag::List(List::of_tags(
            6,
            vec![Tag::Double(px), Tag::Double(py), Tag::Double(pz)],
        )),
    );
    entry.insert("blockPos", Tag::List(List::of_ints(cell)));
    entry.insert("nbt", Tag::Compound(nbt));
    Ok(entry)
}

/// What the game saves of `entity` beside its place, which the structure's placement sets.
fn entity_nbt(entity: &Entity, data_version: i32) -> Compound {
    let mut nbt = Compound::new();
    nbt.insert("id", Tag::String(entity.id.clone()));
    match &entity.data {
        EntityData::Painting { variant, .. } => {
            nbt.insert("variant", Tag::String(variant.clone()));
            nbt.insert("facing", Tag::Byte(horizontal_id(entity.facing)));
        }
        EntityData::ItemFrame { item } => {
            nbt.insert("Facing", Tag::Byte(direction_id(entity.facing)));
            if let Some(item) = item {
                nbt.insert("Item", Tag::Compound(item_stack(item, data_version)));
            }
        }
        EntityData::ArmorStand | EntityData::Spawn => {
            nbt.insert("Rotation", rotation(entity.facing));
        }
        EntityData::Villager {
            profession,
            level,
            trades,
        } => {
            nbt.insert("Rotation", rotation(entity.facing));
            let mut data = Compound::new();
            data.insert("type", Tag::String("minecraft:plains".to_owned()));
            data.insert("profession", Tag::String(format!("minecraft:{profession}")));
            data.insert("level", Tag::Int(i32::from(*level)));
            nbt.insert("VillagerData", Tag::Compound(data));
            nbt.insert("Xp", Tag::Int(villager_xp(*level)));
            if !trades.is_empty() {
                let recipes = trades
                    .iter()
                    .map(|trade| offer(trade, data_version))
                    .collect();
                let mut offers = Compound::new();
                offers.insert("Recipes", Tag::List(List::of_compounds(recipes)));
                nbt.insert("Offers", Tag::Compound(offers));
            }
        }
        EntityData::TextDisplay {
            lines,
            billboard,
            scale,
        } => {
            display(&mut nbt, entity.facing, billboard, *scale);
            nbt.insert(
                "text",
                Tag::String(text_component(&lines.join("\n"), data_version)),
            );
        }
        EntityData::BlockDisplay { block, scale } => {
            display(&mut nbt, entity.facing, "fixed", *scale);
            let keys = palette_entry_keys(data_version);
            nbt.insert("block_state", Tag::Compound(palette_entry(block, keys)));
        }
        EntityData::ItemDisplay { item, scale } => {
            display(&mut nbt, entity.facing, "fixed", *scale);
            nbt.insert("item", Tag::Compound(item_stack(item, data_version)));
        }
    }
    nbt
}

/// The experience a villager of `level` is saved with: its level's start, and 1 for a novice,
/// whom the game would otherwise take the profession of without a job site.
fn villager_xp(level: u8) -> i32 {
    let start = LEVEL_XP[usize::from(level.clamp(1, 5) - 1)];
    start.max(1)
}

/// What a display's `nbt` holds beside what it shows: its yaw, its billboard and its scale.
fn display(nbt: &mut Compound, facing: Face, billboard: &str, scale: f64) {
    nbt.insert("Rotation", rotation(facing));
    nbt.insert("billboard", Tag::String(billboard.to_owned()));
    let floats = |values: &[f32]| {
        Tag::List(List::of_tags(
            5,
            values.iter().copied().map(Tag::Float).collect(),
        ))
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the game saves a display's scale as a float"
    )]
    let scale = scale as f32;
    let mut transformation = Compound::new();
    transformation.insert("left_rotation", floats(&[0.0, 0.0, 0.0, 1.0]));
    transformation.insert("right_rotation", floats(&[0.0, 0.0, 0.0, 1.0]));
    transformation.insert("translation", floats(&[0.0, 0.0, 0.0]));
    transformation.insert("scale", floats(&[scale, scale, scale]));
    nbt.insert("transformation", Tag::Compound(transformation));
}

/// `Rotation` for an entity turned to face `facing`: its yaw, then a level pitch.
fn rotation(facing: Face) -> Tag {
    let yaw = match facing {
        Face::West => 90.0,
        Face::North => 180.0,
        Face::East => 270.0,
        _ => 0.0,
    };
    Tag::List(List::of_tags(5, vec![Tag::Float(yaw), Tag::Float(0.0)]))
}

/// The number a painting's `facing` saves: south 0, west 1, north 2, east 3.
fn horizontal_id(facing: Face) -> i8 {
    match facing {
        Face::West => 1,
        Face::North => 2,
        Face::East => 3,
        _ => 0,
    }
}

/// The number an item frame's `Facing` saves: down 0, up 1, north 2, south 3, west 4, east 5.
fn direction_id(facing: Face) -> i8 {
    match facing {
        Face::Down => 0,
        Face::Up => 1,
        Face::North => 2,
        Face::South => 3,
        Face::West => 4,
        Face::East => 5,
    }
}

/// `item` as an item stack: `Count` and a `tag` before 1.20.5, `count` and `components` from it.
fn item_stack(item: &Item, data_version: i32) -> Compound {
    let mut stack = Compound::new();
    stack.insert("id", Tag::String(item.id.clone()));
    let stored = item.id == ENCHANTED_BOOK;
    if data_version >= ITEM_COMPONENTS_FROM {
        stack.insert("count", Tag::Int(i32::from(item.count)));
        if let Some(enchantment) = &item.enchantment {
            let mut levels = Compound::new();
            levels.insert(
                enchantment.id.clone(),
                Tag::Int(i32::from(enchantment.level)),
            );
            let enchantments = if data_version >= BARE_ENCHANTMENT_LEVELS_FROM {
                levels
            } else {
                let mut wrapped = Compound::new();
                wrapped.insert("levels", Tag::Compound(levels));
                wrapped
            };
            let component = if stored {
                "minecraft:stored_enchantments"
            } else {
                "minecraft:enchantments"
            };
            let mut components = Compound::new();
            components.insert(component, Tag::Compound(enchantments));
            stack.insert("components", Tag::Compound(components));
        }
    } else {
        stack.insert(
            "Count",
            Tag::Byte(i8::try_from(item.count).unwrap_or(i8::MAX)),
        );
        if let Some(enchantment) = &item.enchantment {
            let mut entry = Compound::new();
            entry.insert("id", Tag::String(enchantment.id.clone()));
            entry.insert("lvl", Tag::Short(i16::from(enchantment.level)));
            let mut tag = Compound::new();
            tag.insert(
                if stored {
                    "StoredEnchantments"
                } else {
                    "Enchantments"
                },
                Tag::List(List::of_compounds(vec![entry])),
            );
            stack.insert("tag", Tag::Compound(tag));
        }
    }
    stack
}

/// One of a villager's `Recipes`: what it buys, what it sells, and how often.
fn offer(trade: &Trade, data_version: i32) -> Compound {
    let mut offer = Compound::new();
    offer.insert("buy", Tag::Compound(item_stack(&trade.buy, data_version)));
    offer.insert("sell", Tag::Compound(item_stack(&trade.sell, data_version)));
    offer.insert("uses", Tag::Int(0));
    offer.insert("maxUses", Tag::Int(TRADE_USES));
    offer.insert("rewardExp", Tag::Byte(1));
    offer.insert("xp", Tag::Int(1));
    offer.insert("priceMultiplier", Tag::Float(TRADE_PRICE_MULTIPLIER));
    offer.insert("specialPrice", Tag::Int(0));
    offer.insert("demand", Tag::Int(0));
    offer
}
