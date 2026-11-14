//! The entities a body places, in no edition's shape: each writer saves them in its own
//! (`spec/entities` "Two tiers").

use serde::Serialize;

use super::BlockState;
use super::joins::Face;

/// An entity on the grid: a painting, an item frame, an armor stand, a villager, a display or one
/// a `spawn` places.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entity {
    /// Entity id, as `minecraft:painting`, or the `type=` a `spawn` writes.
    pub id: String,
    /// Where it stands, in the block array's coordinates.
    pub pos: (f64, f64, f64),
    /// The cell it belongs to.
    pub block_pos: (u32, u32, u32),
    /// The way it faces.
    pub facing: Face,
    /// What its kind holds.
    #[serde(flatten)]
    pub data: EntityData,
    /// The `nbt={...}` compound its line writes, as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbt: Option<String>,
}

/// What an [`Entity`] holds beside its place.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntityData {
    /// A painting, hung by its hanging cell (`spec/entities` "Hanging entities").
    Painting {
        /// The variant, as the target spells it.
        variant: String,
        /// Its width in blocks, along the wall to the right of the way it faces.
        width: u32,
        /// Its height in blocks.
        height: u32,
    },
    /// An item frame, or a glow item frame by its id.
    ItemFrame {
        /// The item it holds, if any.
        #[serde(skip_serializing_if = "Option::is_none")]
        item: Option<Item>,
    },
    /// An armor stand, without arms.
    ArmorStand,
    /// A villager and the trades it offers, in the order written.
    Villager {
        /// Its profession, as `librarian`.
        profession: String,
        /// Its level, 1 for novice to 5 for master.
        level: u8,
        /// Its trades.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        trades: Vec<Trade>,
    },
    /// A text display.
    TextDisplay {
        /// Its lines, top first.
        lines: Vec<String>,
        /// How it turns toward the viewer, as `fixed`.
        billboard: String,
        /// Its scale on every axis.
        scale: f64,
    },
    /// A block display.
    BlockDisplay {
        /// The block it shows.
        block: BlockState,
        /// Its scale on every axis.
        scale: f64,
    },
    /// An item display.
    ItemDisplay {
        /// The item it shows.
        item: Item,
        /// Its scale on every axis.
        scale: f64,
    },
    /// An entity a `spawn` places, which holds only what its `nbt=` writes.
    Spawn,
}

/// An item stack an entity holds or a trade names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    /// Item id, as `minecraft:diamond`.
    pub id: String,
    /// How many, 1 to 64.
    pub count: u8,
    /// The enchantment it carries, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enchantment: Option<Enchantment>,
}

/// An enchantment on an item; an enchanted book keeps it as a stored enchantment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Enchantment {
    /// Enchantment id, as `minecraft:mending`.
    pub id: String,
    /// Its level, from 1.
    pub level: u8,
}

/// One trade a villager offers: what it buys and what it sells for that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Trade {
    /// What the player pays.
    pub buy: Item,
    /// What the player gets.
    pub sell: Item,
}
