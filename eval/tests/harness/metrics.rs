//! Tokens, the AST diff, the NBT diff, the cells a block-array IR holds, and a run's metrics.

use std::collections::BTreeMap;

use eval::compiler::Cells;
use eval::metrics::{ast_diff, nbt_diff, summarize, tokens};
use eval::model::Usage;
use eval::prompts::Topic;
use eval::record::{
    AstDiff, Attempt, Build, EditDiff, EditStability, NbtDiff, Outcome, PromptRecord, Verdict,
};

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

/// The AST diff from `before` to `after`, as `(before, after, removed, added)`.
fn diffed(before: &str, after: &str) -> (u64, u64, u64, u64) {
    let diff = ast_diff(before, after).expect("both parse");
    (diff.before, diff.after, diff.removed, diff.added)
}

#[test]
fn a_header_is_a_node() {
    assert_eq!(
        diffed(BASE, &format!("@cairn 2026.06\n{BASE}")),
        (5, 6, 0, 1)
    );
}

#[test]
fn an_added_edit_line_is_one_node_added() {
    let edited = format!("{BASE}\nedit walls[0] set height=4\n");
    assert_eq!(diffed(BASE, &edited), (5, 6, 0, 1));
}

#[test]
fn edit_lines_count_in_source_order_among_the_items() {
    let edit = "edit walls[0] set height=4\n\n";
    let before = format!("{edit}{BASE}");
    let after = format!("{BASE}\n{edit}");
    assert_eq!(diffed(&before, &after), (6, 6, 1, 1));
}

#[test]
fn the_order_of_selector_attributes_bindings_and_set_pairs_is_not_a_change() {
    let before = "\
theme t:
  slot wall -> @cobblestone
  window[class=vent][level=a] -> mat_slot=wall shape=arch

struct a size=5x5
  walls mat_slot=wall height=3

edit window[class=vent][level=a] set y=2 offset=1
";
    let after = "\
theme t:
  slot wall -> @cobblestone
  window[level=a][class=vent] -> shape=arch mat_slot=wall

struct a size=5x5
  walls mat_slot=wall height=3

edit window[level=a][class=vent] set offset=1 y=2
";
    assert_eq!(diffed(before, after), (6, 6, 0, 0));
}

#[test]
fn a_member_added_before_another_of_its_keyword_is_one_node_added() {
    let before = format!("{BASE}  window side=front offset=1 y=1 size=1x1 mat_slot=wall\n");
    let after = BASE.replace(
        "  door side=front at=center\n",
        "  window side=back offset=1 y=1 size=1x1 mat_slot=wall\n  door side=front at=center\n",
    ) + "  window side=front offset=1 y=1 size=1x1 mat_slot=wall\n";
    assert_eq!(diffed(&before, &after), (6, 7, 0, 1));
}

#[test]
fn a_truth_table_row_and_a_requires_line_are_nodes() {
    let before = "\
def gate:
  requires version>=1.20
  floor mat_slot=wall

struct d size=5x5
  floor mat_slot=wall
  pressure_plate id=plate at=inside.front offset=2 y=1 -> sig.step
  logic sig.open = sig.step
  assert truth(sig.step -> sig.open) {
    0->0
    1->1
  }
";
    let after = before
        .replace("  requires version>=1.20\n", "  requires version>=1.21\n")
        .replace("    1->1\n", "    1->0\n");
    // def, requires, floor; struct, floor, plate, logic, assert and its two rows.
    assert_eq!(diffed(before, &after), (10, 10, 2, 2));
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
fn the_nbt_diff_counts_every_cell_either_build_fills() {
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
        nbt_diff(&before, &after),
        NbtDiff {
            cells: 4,
            changed: 3,
            ratio: 0.75,
        }
    );
    assert_eq!(
        nbt_diff(&before, &before),
        NbtDiff {
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
                    {"pos": [1, 1, 0], "kind": "fields", "id": "minecraft:chest", "fields": {"b": 1, "a": 2}}
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
            "minecraft:chest{\"fields\":{\"a\":2,\"b\":1},\"id\":\"minecraft:chest\",\"kind\":\"fields\"}"
                .to_owned(),
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

#[test]
fn an_ir_whose_voxels_do_not_fill_its_dims_or_palette_is_refused() {
    let ir = |voxels: &str| {
        format!(
            r#"{{"structures": {{"struct::a": {{"dims": {{"x": 2, "y": 1, "z": 1}},
            "palette": [{{"id": "minecraft:air"}}], "voxels": {voxels}}}}}}}"#
        )
    };
    let refusal = |voxels: &str| {
        Cells::from_ir(&ir(voxels))
            .expect_err("the IR is refused")
            .to_string()
    };
    assert_eq!(
        refusal("[0]"),
        "`cairn lower` printed an IR the harness cannot read: `struct::a` holds 1 voxels for dims \
         2x1x1"
    );
    assert_eq!(
        refusal("[0, 1]"),
        "`cairn lower` printed an IR the harness cannot read: `struct::a` names palette entry 1 \
         of 1"
    );
}

/// An edit prompt that converged with these diff ratios.
fn edit(id: &str, ast: f64, nbt: f64) -> PromptRecord {
    PromptRecord {
        id: id.to_owned(),
        topic: Topic::Openings,
        base: Some("shed".to_owned()),
        outcome: Outcome::Converged {
            build: Build {
                program: String::new(),
                tokens: 1,
                blocks: 1,
            },
            edit: Some(EditDiff {
                ast: AstDiff {
                    before: 1,
                    after: 1,
                    removed: 0,
                    added: 0,
                    ratio: ast,
                },
                nbt: NbtDiff {
                    cells: 1,
                    changed: 0,
                    ratio: nbt,
                },
            }),
        },
        attempts: vec![Attempt {
            reply: String::new(),
            stop_reason: "end_turn".to_owned(),
            usage: Usage::default(),
            verdict: Verdict::Pass,
            diagnostics: Vec::new(),
            feedback: None,
        }],
    }
}

#[test]
fn edit_stability_is_the_mean_over_the_edits_measured() {
    let mut skipped = edit("c", 0.0, 0.0);
    skipped.outcome = Outcome::Skipped;
    skipped.attempts.clear();
    let prompts = [edit("a", 0.1, 0.5), edit("b", 0.3, 0.0), skipped];
    assert_eq!(
        summarize(&prompts).edit_stability,
        EditStability {
            measured: 2,
            of: 3,
            ast: Some(0.2),
            nbt: Some(0.25),
        }
    );
}
