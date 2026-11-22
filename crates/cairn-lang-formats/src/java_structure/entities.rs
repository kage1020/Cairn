//! The IR's entities as a Java structure's `entities` saves them: each with its point, its cell
//! and its `nbt`, in the shape the target's `DataVersion` reads (`spec/entities` "Two tiers").

use cairn_lang_core::block_array::{Billboard, Compass, Entity, EntityData, Face, Item, Trade};
use cairn_lang_nbt::tag::{Compound, List, Tag};

use super::{JavaStructureError, palette_entry, palette_entry_keys, text_component};
use crate::nbt_text::{compound_of, first_unencodable, put_over};
use crate::villager::saved_experience;

/// The first `DataVersion` whose items are saved as data components, 1.20.5's.
const ITEM_COMPONENTS_FROM: i32 = 3837;

/// The first `DataVersion` whose enchantments map each id to its level straight, 1.21.5's.
const BARE_ENCHANTMENT_LEVELS_FROM: i32 = 4325;

/// The item that keeps its enchantment as a stored one.
const ENCHANTED_BOOK: &str = "minecraft:enchanted_book";

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
        EntityData::Painting {
            facing,
            variant,
            spelling,
            ..
        } => {
            let id = spelling
                .clone()
                .unwrap_or_else(|| format!("minecraft:{variant}"));
            nbt.insert("variant", Tag::String(id));
            nbt.insert("facing", Tag::Byte(horizontal_id(*facing)));
        }
        EntityData::ItemFrame { facing, item } => {
            nbt.insert("Facing", Tag::Byte(direction_id(*facing)));
            if let Some(item) = item {
                nbt.insert("Item", Tag::Compound(item_stack(item, data_version)));
            }
        }
        EntityData::ArmorStand { facing } | EntityData::Spawn { facing } => {
            nbt.insert("Rotation", rotation(*facing));
        }
        EntityData::Villager {
            facing,
            profession,
            level,
            trades,
        } => {
            nbt.insert("Rotation", rotation(*facing));
            let mut data = Compound::new();
            data.insert("type", Tag::String("minecraft:plains".to_owned()));
            data.insert(
                "profession",
                Tag::String(format!("minecraft:{}", profession.as_str())),
            );
            data.insert("level", Tag::Int(i32::from(level.number())));
            nbt.insert("VillagerData", Tag::Compound(data));
            nbt.insert("Xp", Tag::Int(saved_experience(*level)));
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
            facing,
            lines,
            billboard,
            scale,
        } => {
            display(&mut nbt, *facing, *billboard, *scale);
            nbt.insert(
                "text",
                Tag::String(text_component(&lines.join("\n"), data_version)),
            );
        }
        EntityData::BlockDisplay {
            facing,
            block,
            scale,
        } => {
            display(&mut nbt, *facing, Billboard::Fixed, *scale);
            let keys = palette_entry_keys(data_version);
            nbt.insert("block_state", Tag::Compound(palette_entry(block, keys)));
        }
        EntityData::ItemDisplay {
            facing,
            item,
            scale,
        } => {
            display(&mut nbt, *facing, Billboard::Fixed, *scale);
            nbt.insert("item", Tag::Compound(item_stack(item, data_version)));
        }
    }
    nbt
}

/// What a display's `nbt` holds beside what it shows: its yaw, its billboard and its scale.
fn display(nbt: &mut Compound, facing: Compass, billboard: Billboard, scale: f32) {
    nbt.insert("Rotation", rotation(facing));
    nbt.insert("billboard", Tag::String(billboard.as_str().to_owned()));
    let floats = |values: &[f32]| {
        Tag::List(List::of_tags(
            5,
            values.iter().copied().map(Tag::Float).collect(),
        ))
    };
    let mut transformation = Compound::new();
    transformation.insert("left_rotation", floats(&[0.0, 0.0, 0.0, 1.0]));
    transformation.insert("right_rotation", floats(&[0.0, 0.0, 0.0, 1.0]));
    transformation.insert("translation", floats(&[0.0, 0.0, 0.0]));
    transformation.insert("scale", floats(&[scale, scale, scale]));
    nbt.insert("transformation", Tag::Compound(transformation));
}

/// `Rotation` for an entity turned to face `facing`: its yaw, then a level pitch.
fn rotation(facing: Compass) -> Tag {
    let yaw = match facing {
        Compass::South => 0.0,
        Compass::West => 90.0,
        Compass::North => 180.0,
        Compass::East => 270.0,
    };
    Tag::List(List::of_tags(5, vec![Tag::Float(yaw), Tag::Float(0.0)]))
}

/// The number a painting's `facing` saves: south 0, west 1, north 2, east 3.
const fn horizontal_id(facing: Compass) -> i8 {
    match facing {
        Compass::South => 0,
        Compass::West => 1,
        Compass::North => 2,
        Compass::East => 3,
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
        stack.insert("count", Tag::Int(i32::from(item.count.get())));
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
        stack.insert("Count", Tag::Byte(item.count.as_i8()));
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
