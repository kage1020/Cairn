//! Tokens, the AST diff, the block diff, and the cells a block-array IR holds.

use std::collections::BTreeMap;

use eval::compiler::Cells;
use eval::metrics::{ast_diff, block_diff, tokens};
use eval::record::{AstDiff, BlockDiff};

#[test]
fn tokens_leave_out_layout_and_comments() {
    let program = "# A shed.\nstruct shed size=5x5\n\n  # Its floor.\n  floor mat_slot=floor\n";
    // struct shed size = 5x5, floor mat_slot = floor
    assert_eq!(tokens(program), Ok(9));
}

#[test]
fn a_block_id_is_three_tokens_and_an_abstract_token_two() {
    assert_eq!(tokens("raw_block mat=minecraft:oak_planks"), Ok(6));
    assert_eq!(tokens("slot wall -> @cobblestone"), Ok(5));
}

const BASE: &str = "\
theme t:
  slot wall -> @cobblestone

struct a size=5x5
  walls mat_slot=wall height=3
  door side=front at=center
";

#[test]
fn argument_order_comments_and_spacing_are_not_a_change() {
    let rewritten = "\
theme t:
  slot   wall -> @cobblestone
# The same build.
struct a size=5x5

  walls height=3 mat_slot=wall
  door at=center side=front
";
    assert_eq!(
        ast_diff(BASE, rewritten).expect("both parse"),
        AstDiff {
            before: 5,
            after: 5,
            removed: 0,
            added: 0,
            ratio: 0.0,
        }
    );
}

#[test]
fn a_changed_line_is_one_node_removed_and_one_added() {
    let taller = BASE.replace("height=3", "height=4");
    assert_eq!(
        ast_diff(BASE, &taller).expect("both parse"),
        AstDiff {
            before: 5,
            after: 5,
            removed: 1,
            added: 1,
            ratio: 0.2,
        }
    );
}

#[test]
fn a_line_moved_under_a_level_is_a_change() {
    let levelled = "\
theme t:
  slot wall -> @cobblestone

struct a size=5x5
  walls mat_slot=wall height=3
  level y=0
    door side=front at=center
";
    assert_eq!(
        ast_diff(BASE, levelled).expect("both parse"),
        AstDiff {
            before: 5,
            after: 6,
            removed: 1,
            added: 2,
            ratio: 3.0 / 11.0,
        }
    );
}

fn cells(blocks: &[(usize, &str)]) -> Cells {
    Cells(
        blocks
            .iter()
            .map(|(x, block)| (("struct::a".to_owned(), *x, 0, 0), (*block).to_owned()))
            .collect(),
    )
}

#[test]
fn the_block_diff_counts_every_cell_either_build_fills() {
    let before = cells(&[
        (0, "minecraft:stone"),
        (1, "minecraft:stone"),
        (2, "minecraft:dirt"),
    ]);
    let after = cells(&[
        (1, "minecraft:dirt"),
        (2, "minecraft:dirt"),
        (3, "minecraft:stone"),
    ]);
    assert_eq!(
        block_diff(&before, &after),
        BlockDiff {
            cells: 4,
            changed: 3,
            ratio: 0.75,
        }
    );
    assert_eq!(
        block_diff(&before, &before),
        BlockDiff {
            cells: 3,
            changed: 0,
            ratio: 0.0,
        }
    );
}

#[test]
fn cells_leave_out_air_and_carry_properties_and_block_entities() {
    let ir = r#"{
        "structures": {
            "struct::a": {
                "dims": {"x": 2, "y": 2, "z": 1},
                "palette": [
                    {"id": "minecraft:air"},
                    {"id": "minecraft:oak_stairs", "properties": {"half": "bottom", "facing": "east"}},
                    {"id": "minecraft:chest"}
                ],
                "voxels": [1, 0, 0, 2],
                "block_entities": [
                    {"pos": [1, 1, 0], "id": "minecraft:chest", "fields": {"b": 1, "a": [2]}}
                ]
            },
            "walkway::s::a.e__b.e": {
                "dims": {"x": 1, "y": 1, "z": 2},
                "palette": [{"id": "minecraft:air"}, {"id": "minecraft:gravel"}],
                "voxels": [0, 1]
            }
        }
    }"#;
    let expected: BTreeMap<_, _> = [
        (
            ("struct::a".to_owned(), 0, 0, 0),
            "minecraft:oak_stairs[facing=east,half=bottom]".to_owned(),
        ),
        (
            ("struct::a".to_owned(), 1, 1, 0),
            "minecraft:chest{minecraft:chest:{\"a\":[2],\"b\":1}}".to_owned(),
        ),
        (
            ("walkway::s::a.e__b.e".to_owned(), 0, 0, 1),
            "minecraft:gravel".to_owned(),
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(Cells::from_ir(ir).expect("the IR reads").0, expected);
}
