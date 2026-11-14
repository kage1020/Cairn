//! A block the pinned target's constraint catalog says the game drops or lets fall, and water
//! that flows out of the build, are reported at the member that built them (`spec/lint`
//! "Constraint checks").

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::block_array::{
    BlockIdSet, BlockState, Constraint, Face, StateDomain, Support, TargetRegistry,
};

use crate::common::{bin, findings, lowered_with};

/// A target whose catalog constrains levers, torches, lanterns and sand, and whose cobblestone,
/// stone and roof faces are whole, a glass pane a post, a bottom slab whole below and a bottom
/// stair facing north whole below and behind; it cannot say what `mystery` is.
struct Catalog;

impl Catalog {
    fn whole(state: &BlockState, face: Face) -> Option<bool> {
        let value = |key: &str| state.properties.get(key).map(String::as_str);
        Some(match state.id.strip_prefix("minecraft:")? {
            "cobblestone" | "stone" => true,
            "oak_slab" => face == Face::Down && value("type") == Some("bottom"),
            "oak_stairs" => matches!(face, Face::Down | Face::North),
            "mystery" => return None,
            _ => false,
        })
    }
}

/// A target that checks states but has no constraint catalog.
struct Bare;

impl TargetRegistry for Bare {
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
}

impl TargetRegistry for Catalog {
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

    fn constraint(&self, state: &BlockState) -> Option<Constraint> {
        let value = |key: &str| state.properties.get(key).map(String::as_str);
        let centre = |host| Constraint::Rests {
            host,
            needs: Support::Center,
        };
        Some(match state.id.strip_prefix("minecraft:")? {
            "lever" => Constraint::Hangs {
                host: Face::from_name(value("facing")?)?.opposite(),
            },
            "lantern" if value("hanging") == Some("true") => centre(Face::Up),
            "torch" | "lantern" => centre(Face::Down),
            "repeater" => Constraint::Rests {
                host: Face::Down,
                needs: Support::Sturdy,
            },
            "sand" => Constraint::Falls,
            _ => return None,
        })
    }

    fn holds(&self, state: &BlockState, host: &BlockState) -> Option<bool> {
        let (toward, needs) = match self.constraint(state)? {
            Constraint::Hangs { host } => (host, Support::Sturdy),
            Constraint::Rests { host, needs } => (host, needs),
            Constraint::Falls => return None,
        };
        let face = toward.opposite();
        let post = host.id == "minecraft:glass_pane" && matches!(face, Face::Down | Face::Up);
        Some(Self::whole(host, face)? || (needs == Support::Center && post))
    }

    fn keeps_water(&self, state: &BlockState, face: Face) -> Option<bool> {
        Self::whole(state, face)
    }
}

const HANG_VALID: &str = "it hangs only on a whole face: a full block's side, or the face of a \
                          slab or stair that covers its cell; a glass pane, fence, wall or air \
                          gives none";
const HANG_FIX: &str = "put a full block where it hangs, or move the member so it hangs on one";
const CENTRE_BELOW: &str = "it needs the centre of the face of the block below it: a full block, \
                            a slab or stair whose face covers its cell, or the post of a fence, \
                            wall, pane or chain";
const CENTRE_BELOW_FIX: &str =
    "put a block that gives it that face below it, or move the member onto one";
const CENTRE_ABOVE: &str = "it needs the centre of the face of the block above it: a full block, \
                            a slab or stair whose face covers its cell, or the post of a fence, \
                            wall, pane or chain";
const CENTRE_ABOVE_FIX: &str =
    "put a block that gives it that face above it, or move the member onto one";
const WHOLE_BELOW: &str = "it needs the whole face of the block below it: a full block, or a \
                           slab or stair whose face covers its cell";
const FALLS_VALID: &str = "it stays only on a block, and falls through air or a fluid";
const FALLS_FIX: &str = "put a block under it, or build it of a block that does not fall";
const SPILL_VALID: &str = "water stays where each face it flows through, down and sideways, \
                           meets a block; a waterlogged block keeps it in only through a whole \
                           face";
const SPILL_FIX: &str = "wall the water in, fill the air it flows into, or write \
                         `waterlogged=false` on a block that should hold none";

/// [`bin`] with a stone floor under its interior, then `members`.
fn floored(members: &str) -> String {
    bin(&format!(
        "fill mat_slot=stone from=1,0,1 to=3,0,2\n  {members}"
    ))
}

#[test]
fn a_lever_on_a_glass_pane_is_dropped_and_one_on_a_wall_is_kept() {
    let source = floored(
        "raw_block mat=minecraft:glass_pane at=2,1,1\n  \
         raw_block mat=minecraft:lever[face=wall, facing=south] at=2,1,2\n  \
         raw_block mat=minecraft:lever[face=wall, facing=east] at=1,1,2",
    );
    let out = lowered_with(&source, &Catalog);
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::NoAttachmentFace,
            "raw_block mat=minecraft:lever[face=wall, facing=south] at=2,1,2",
            "`raw_block` builds a block the game drops for want of a whole face to hang on: \
             `minecraft:lever[face=wall,facing=south]` at 2,1,2 hangs on the south face of \
             `minecraft:glass_pane`",
            vec![HANG_VALID, HANG_FIX],
        )],
    );
}

#[test]
fn a_block_hung_on_air_names_the_air() {
    let source = floored("raw_block mat=minecraft:lever[face=wall, facing=north] at=2,1,1");
    let out = lowered_with(&source, &Catalog);
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::NoAttachmentFace,
            "raw_block mat=minecraft:lever[face=wall, facing=north] at=2,1,1",
            "`raw_block` builds a block the game drops for want of a whole face to hang on: \
             `minecraft:lever[face=wall,facing=north]` at 2,1,1 hangs on air",
            vec![HANG_VALID, HANG_FIX],
        )],
    );
}

#[test]
fn a_torch_needs_a_centre_below_and_a_hanging_lantern_one_above() {
    let source = floored(
        "raw_block mat=minecraft:oak_slab[type=bottom] at=1,1,1\n  \
         raw_block mat=minecraft:torch at=1,2,1\n  \
         raw_block mat=minecraft:lantern[hanging=true] at=3,0,2\n  \
         raw_block mat=minecraft:glass_pane at=3,1,1\n  \
         raw_block mat=minecraft:torch at=3,2,1\n  \
         raw_block mat=minecraft:lantern[hanging=true] at=2,2,1\n  \
         raw_block mat=minecraft:lantern at=2,1,2",
    );
    let out = lowered_with(&source, &Catalog);
    assert_eq!(
        findings(&source, &out),
        [
            (
                DiagnosticCode::UnsupportedBlock,
                "raw_block mat=minecraft:torch at=1,2,1",
                "`raw_block` builds a block the game drops for want of support: \
                 `minecraft:torch` at 1,2,1 stands on the up face of \
                 `minecraft:oak_slab[type=bottom]`",
                vec![CENTRE_BELOW, CENTRE_BELOW_FIX],
            ),
            (
                DiagnosticCode::UnsupportedBlock,
                "raw_block mat=minecraft:lantern[hanging=true] at=3,0,2",
                "`raw_block` builds a block the game drops for want of support: \
                 `minecraft:lantern[hanging=true]` at 3,0,2 hangs from air",
                vec![CENTRE_ABOVE, CENTRE_ABOVE_FIX],
            ),
        ],
        "a torch on a pane's post, a lantern under the roof and one on the floor are kept",
    );
}

#[test]
fn a_block_that_needs_a_whole_face_is_not_held_by_a_post() {
    let source = floored(
        "raw_block mat=minecraft:glass_pane at=2,1,1\n  \
         raw_block mat=minecraft:repeater at=2,2,1",
    );
    let out = lowered_with(&source, &Catalog);
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::UnsupportedBlock,
            "raw_block mat=minecraft:repeater at=2,2,1",
            "`raw_block` builds a block the game drops for want of support: \
             `minecraft:repeater` at 2,2,1 stands on the up face of `minecraft:glass_pane`",
            vec![WHOLE_BELOW, CENTRE_BELOW_FIX],
        )],
    );
}

#[test]
fn sand_over_air_or_water_falls_and_sand_on_the_floor_stays() {
    let source = bin("raw_fill mat=minecraft:sand from=1,1,1 to=3,1,1\n  \
         raw_block mat=minecraft:sand at=1,1,2\n  \
         raw_block mat=minecraft:stone at=2,0,2\n  \
         raw_block mat=minecraft:sand at=2,1,2\n  \
         raw_block mat=minecraft:sand at=3,0,2");
    let out = lowered_with(&source, &Catalog);
    assert_eq!(
        findings(&source, &out),
        [
            (
                DiagnosticCode::UnsupportedBlock,
                "raw_fill mat=minecraft:sand from=1,1,1 to=3,1,1",
                "`raw_fill` builds 3 blocks that fall: the first, `minecraft:sand` at 1,1,1, \
                 stands on air",
                vec![FALLS_VALID, FALLS_FIX],
            ),
            (
                DiagnosticCode::UnsupportedBlock,
                "raw_block mat=minecraft:sand at=1,1,2",
                "`raw_block` builds a block that falls: `minecraft:sand` at 1,1,2 stands on air",
                vec![FALLS_VALID, FALLS_FIX],
            ),
        ],
        "sand on stone, and sand in the bottom row with nothing of the build below it, stay",
    );
    let over_water =
        floored("fill fluid=water from=1,1,1 to=1,1,1\n  raw_block mat=minecraft:sand at=1,2,1");
    let out = lowered_with(&over_water, &Catalog);
    assert_eq!(
        findings(&over_water, &out)
            .into_iter()
            .filter(|(code, ..)| *code == DiagnosticCode::UnsupportedBlock)
            .map(|(_, _, primary, _)| primary)
            .collect::<Vec<_>>(),
        [
            "`raw_block` builds a block that falls: `minecraft:sand` at 1,2,1 stands on \
          `minecraft:water[level=0]`"
        ],
    );
}

#[test]
fn water_over_air_spills_and_water_on_a_floor_between_walls_stays() {
    let source = bin("fill fluid=water from=1,1,1 to=3,1,2");
    let out = lowered_with(&source, &Catalog);
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::WaterSpill,
            "fill fluid=water from=1,1,1 to=3,1,2",
            "`fill` holds water that flows into 6 cells of air: the first, at 1,0,1, from \
             `minecraft:water[level=0]` at 1,1,1",
            vec![SPILL_VALID, SPILL_FIX],
        )],
    );
    let held = floored("fill fluid=water from=1,1,1 to=3,1,2");
    assert_eq!(findings(&held, &lowered_with(&held, &Catalog)), []);
}

#[test]
fn a_waterlogged_block_spills_through_the_faces_that_are_not_whole() {
    let source = floored(
        "raw_block mat=minecraft:oak_stairs[facing=north, half=bottom, waterlogged=true] \
         at=2,1,1",
    );
    let out = lowered_with(&source, &Catalog);
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::WaterSpill,
            "raw_block mat=minecraft:oak_stairs[facing=north, half=bottom, waterlogged=true] \
             at=2,1,1",
            "`raw_block` holds water that flows into 3 cells of air: the first, at 3,1,1, from \
             `minecraft:oak_stairs[facing=north,half=bottom,waterlogged=true]` at 2,1,1",
            vec![SPILL_VALID, SPILL_FIX],
        )],
    );
}

#[test]
fn a_neighbour_the_registry_cannot_judge_or_the_build_does_not_hold_is_not_judged() {
    let source = bin("raw_block mat=minecraft:mystery at=2,1,1\n  \
         raw_block mat=minecraft:lever[face=wall, facing=south] at=2,1,2\n  \
         raw_block mat=minecraft:torch at=3,0,2");
    assert_eq!(findings(&source, &lowered_with(&source, &Catalog)), []);
}

#[test]
fn a_registry_without_a_catalog_runs_no_check() {
    let source = bin(
        "raw_block mat=minecraft:lever[face=wall, facing=north] at=2,1,1\n  \
         raw_block mat=minecraft:sand at=2,1,2\n  \
         fill fluid=water from=3,1,1 to=3,1,2",
    );
    assert_eq!(findings(&source, &lowered_with(&source, &Bare)), []);
}

/// [`Catalog`] at a target that declares only cobblestone, the lever and stone.
struct Refusing(Vec<String>);

impl TargetRegistry for Refusing {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        Catalog.lookup(token)
    }

    fn known_tokens(&self) -> Vec<String> {
        Catalog.known_tokens()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        Some(BlockIdSet::new("java 2.0", &self.0))
    }

    fn state_domain(&self, state: &BlockState) -> StateDomain {
        Catalog.state_domain(state)
    }

    fn constraint(&self, state: &BlockState) -> Option<Constraint> {
        Catalog.constraint(state)
    }

    fn holds(&self, state: &BlockState, host: &BlockState) -> Option<bool> {
        Catalog.holds(state, host)
    }

    fn keeps_water(&self, state: &BlockState, face: Face) -> Option<bool> {
        Catalog.keeps_water(state, face)
    }
}

#[test]
fn a_body_with_a_member_an_error_refuses_is_not_judged() {
    let source = floored(
        "raw_block mat=minecraft:bogus at=2,1,1\n  \
         raw_block mat=minecraft:lever[face=wall, facing=south] at=2,1,2",
    );
    let target = Refusing(
        ["cobblestone", "lever", "stone"]
            .map(|id| format!("minecraft:{id}"))
            .to_vec(),
    );
    let codes: Vec<DiagnosticCode> = findings(&source, &lowered_with(&source, &target))
        .into_iter()
        .map(|(code, ..)| code)
        .collect();
    assert_eq!(codes, [DiagnosticCode::UnknownId]);
}
