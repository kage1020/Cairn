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
        /// The variant, as its line names it, without a namespace.
        variant: String,
        /// The variant as the target's painting table spells it; `None` where no table answers.
        #[serde(skip_serializing_if = "Option::is_none")]
        spelling: Option<String>,
        /// Its width in blocks, along the wall to the right as seen from in front of it.
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
        /// Its profession.
        profession: Profession,
        /// Its level.
        level: VillagerLevel,
        /// Its trades.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        trades: Vec<Trade>,
    },
    /// A text display.
    TextDisplay {
        /// Its lines, top first.
        lines: Vec<String>,
        /// How it turns toward the viewer.
        billboard: Billboard,
        /// Its scale on every axis, as the float the game saves.
        scale: f32,
    },
    /// A block display.
    BlockDisplay {
        /// The block it shows.
        block: BlockState,
        /// Its scale on every axis, as the float the game saves.
        scale: f32,
    },
    /// An item display.
    ItemDisplay {
        /// The item it shows.
        item: Item,
        /// Its scale on every axis, as the float the game saves.
        scale: f32,
    },
    /// An entity a `spawn` places, which holds only what its `nbt=` writes.
    Spawn,
}

/// Declares a field-less enum named by the words of its `ALL`, in the order its variants are listed.
macro_rules! named {
    ($(#[$meta:meta])* $name:ident { $($(#[$v_meta:meta])* $variant:ident => $word:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
        pub enum $name {
            $($(#[$v_meta])* #[serde(rename = $word)] $variant,)+
        }

        impl $name {
            /// Every value, in the order a key lists its words.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The word a line writes for it.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $word,)+
                }
            }
        }
    };
}

named! {
    /// A villager's profession, as `profession=` names it (`spec/entities` "Standing entities").
    Profession {
        /// No profession, the default.
        None => "none",
        /// An armorer.
        Armorer => "armorer",
        /// A butcher.
        Butcher => "butcher",
        /// A cartographer.
        Cartographer => "cartographer",
        /// A cleric.
        Cleric => "cleric",
        /// A farmer.
        Farmer => "farmer",
        /// A fisherman.
        Fisherman => "fisherman",
        /// A fletcher.
        Fletcher => "fletcher",
        /// A leatherworker.
        Leatherworker => "leatherworker",
        /// A librarian.
        Librarian => "librarian",
        /// A mason.
        Mason => "mason",
        /// A nitwit, who never takes a profession.
        Nitwit => "nitwit",
        /// A shepherd.
        Shepherd => "shepherd",
        /// A toolsmith.
        Toolsmith => "toolsmith",
        /// A weaponsmith.
        Weaponsmith => "weaponsmith",
    }
}

named! {
    /// A villager's level, as `level=` names it.
    VillagerLevel {
        /// Level 1, the default.
        Novice => "novice",
        /// Level 2.
        Apprentice => "apprentice",
        /// Level 3.
        Journeyman => "journeyman",
        /// Level 4.
        Expert => "expert",
        /// Level 5.
        Master => "master",
    }
}

impl VillagerLevel {
    /// Its number, 1 for a novice to 5 for a master.
    #[must_use]
    pub const fn number(self) -> u8 {
        self as u8 + 1
    }
}

named! {
    /// How a text display turns toward the viewer, as `billboard=` names it.
    Billboard {
        /// It keeps the way it faces, the default.
        Fixed => "fixed",
        /// It turns about the vertical axis.
        Vertical => "vertical",
        /// It turns about the horizontal axis.
        Horizontal => "horizontal",
        /// It turns about its centre to face the viewer.
        Center => "center",
    }
}

/// An item stack an entity holds or a trade names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    /// Item id, as `minecraft:diamond`.
    pub id: String,
    /// How many.
    pub count: StackCount,
    /// The enchantment it carries, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enchantment: Option<Enchantment>,
}

/// How many items one stack holds, 1 to 64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct StackCount(u8);

impl StackCount {
    /// One item.
    pub const ONE: Self = Self(1);

    /// A stack of `count` items, or `None` outside 1 to 64.
    #[must_use]
    pub fn new(count: i64) -> Option<Self> {
        u8::try_from(count)
            .ok()
            .filter(|count| (1..=64).contains(count))
            .map(Self)
    }

    /// How many items it holds.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// How many items it holds, as a signed byte.
    #[must_use]
    #[expect(clippy::cast_possible_wrap, reason = "a stack holds at most 64")]
    pub const fn as_i8(self) -> i8 {
        self.0 as i8
    }
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
