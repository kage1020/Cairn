//! The blocks power moves and the blocks that give power, by the ids each edition writes them
//! under (`spec/redstone` "Place-and-route").

use cairn_lang_core::Edition;

/// What power does with a block of the floor row or the row over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Powered {
    /// Power moves it.
    Moves,
    /// It gives power.
    Gives,
    /// It gives power, and power moves it.
    Both,
}

/// What power does with the block `id` at `edition`, or `None` when it neither moves nor gives.
pub(super) fn powered(edition: Edition, id: &str) -> Option<Powered> {
    let name = id.strip_prefix("minecraft:")?;
    canonical(name).or_else(|| match edition {
        Edition::Java => None,
        Edition::Bedrock => bedrock(name),
    })
}

/// What power does with the canonical block `name`, the spelling both editions' palettes hold.
fn canonical(name: &str) -> Option<Powered> {
    if name.ends_with("_door")
        || name.ends_with("_trapdoor")
        || name.ends_with("_fence_gate")
        || name.ends_with("copper_bulb")
        || matches!(
            name,
            "note_block"
                | "redstone_lamp"
                | "piston"
                | "sticky_piston"
                | "dispenser"
                | "dropper"
                | "hopper"
                | "crafter"
                | "powered_rail"
                | "activator_rail"
                | "tnt"
                | "bell"
                | "command_block"
                | "chain_command_block"
                | "repeating_command_block"
        )
    {
        Some(Powered::Moves)
    } else if name.ends_with("_button")
        || name.ends_with("_pressure_plate")
        || matches!(
            name,
            "redstone_block"
                | "lever"
                | "tripwire_hook"
                | "trapped_chest"
                | "daylight_detector"
                | "observer"
                | "target"
                | "lectern"
                | "detector_rail"
                | "sculk_sensor"
                | "calibrated_sculk_sensor"
                | "lightning_rod"
        )
    {
        Some(Powered::Gives)
    } else if matches!(
        name,
        "redstone_wire" | "repeater" | "comparator" | "redstone_torch" | "redstone_wall_torch"
    ) {
        Some(Powered::Both)
    } else {
        None
    }
}

/// What power does with a block Bedrock writes under an id of its own.
fn bedrock(name: &str) -> Option<Powered> {
    match name {
        "noteblock" | "trapdoor" | "fence_gate" | "golden_rail" | "lit_redstone_lamp" => {
            Some(Powered::Moves)
        }
        "daylight_detector_inverted" => Some(Powered::Gives),
        "unpowered_repeater"
        | "powered_repeater"
        | "unpowered_comparator"
        | "powered_comparator"
        | "unlit_redstone_torch" => Some(Powered::Both),
        _ => None,
    }
}
