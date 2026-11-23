//! An `arch` cuts an opening through its box and springs it from an upside-down stair at each end
//! of its top row (`spec/blockstate` "Derive by default, promote on override").

use cairn_lang_core::DiagnosticCode;

use cairn_lang_core::BlockArrayIr;

use crate::common::{Finding, blocks_where, findings, layer, lowered, only_structure, same_build};

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n  slot step -> @oak_stairs\n\n";

/// A 7x5 struct with a floor and 3-row walls, `members` after them; its front wall is `z=4`, its
/// left `x=0`.
fn hall(members: &str) -> String {
    format!(
        "{THEME}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         {members}\n"
    )
}

/// A block as `id[properties]`, with its cell.
type Placed = ((u32, u32, u32), String);

/// The stairs `members` cuts in [`hall`], with no finding.
fn sprung(members: &str) -> Vec<Placed> {
    let src = hall(members);
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), Vec::<Finding<'_>>::new(), "{members}");
    blocks_where(only_structure(&out), |id| id.ends_with("_stairs"))
}

/// A finding as [`Finding`] holds it, owned.
type Owned = (DiagnosticCode, String, String, Vec<String>);

/// The findings `members` raises in [`hall`], which must build what the hall builds without it.
fn refused(members: &str) -> Vec<Owned> {
    let src = hall(members);
    let out = lowered(&src);
    assert!(same_build(&out, &lowered(&hall(""))), "{members}");
    owned(&src, &out)
}

/// Each finding of `out` as [`Owned`].
fn owned(src: &str, out: &BlockArrayIr) -> Vec<Owned> {
    findings(src, out)
        .into_iter()
        .map(|(code, at, primary, notes)| {
            (
                code,
                at.to_owned(),
                primary.to_owned(),
                notes.into_iter().map(str::to_owned).collect(),
            )
        })
        .collect()
}

/// An upside-down straight `id` stair facing `facing` in `cell`.
fn stair(id: &str, facing: &str, cell: (u32, u32, u32)) -> Placed {
    (
        cell,
        format!("minecraft:{id}[facing={facing},half=top,shape=straight]"),
    )
}

#[test]
fn an_arch_two_wide_is_its_two_stairs_over_an_opening() {
    let src = hall("arch from=2,1,4 to=3,2,4 mat_slot=step");
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), Vec::<Finding<'_>>::new());
    let array = only_structure(&out);
    assert_eq!(
        blocks_where(array, |id| id.ends_with("_stairs")),
        [
            stair("oak_stairs", "west", (2, 2, 4)),
            stair("oak_stairs", "east", (3, 2, 4)),
        ],
    );
    assert_eq!(layer(array, 1)[4], "##..###");
}

#[test]
fn a_longer_span_is_open_between_its_stairs() {
    let src = hall("arch from=1,1,4 to=5,3,4 mat_slot=step");
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), Vec::<Finding<'_>>::new());
    let array = only_structure(&out);
    assert_eq!(
        [1, 2, 3].map(|y| layer(array, y)[4].clone()),
        ["#.....#", "#.....#", "#s...s#"],
    );
}

#[test]
fn an_arch_without_a_material_is_built_from_the_default_arch() {
    assert_eq!(
        sprung("arch from=2,1,4 to=3,2,4"),
        [
            stair("stone_brick_stairs", "west", (2, 2, 4)),
            stair("stone_brick_stairs", "east", (3, 2, 4)),
        ],
    );
}

#[test]
fn an_arch_longer_on_z_spans_z() {
    assert_eq!(
        sprung("arch from=0,1,1 to=0,2,3 mat_slot=step"),
        [
            stair("oak_stairs", "north", (0, 2, 1)),
            stair("oak_stairs", "south", (0, 2, 3)),
        ],
    );
}

#[test]
fn a_written_axis_spans_a_box_as_long_on_x_as_on_z() {
    assert_eq!(
        sprung("arch from=2,1,1 to=3,2,2 axis=z mat_slot=step"),
        [
            stair("oak_stairs", "north", (2, 2, 1)),
            stair("oak_stairs", "south", (2, 2, 2)),
            stair("oak_stairs", "north", (3, 2, 1)),
            stair("oak_stairs", "south", (3, 2, 2)),
        ],
    );
}

#[test]
fn a_series_cuts_each_arch_a_step_past_the_one_before() {
    assert_eq!(
        sprung("arch from=1,1,4 to=2,2,4 repeat=2 step=3,0,0 mat_slot=step"),
        [
            stair("oak_stairs", "west", (1, 2, 4)),
            stair("oak_stairs", "east", (2, 2, 4)),
            stair("oak_stairs", "west", (4, 2, 4)),
            stair("oak_stairs", "east", (5, 2, 4)),
        ],
    );
}

#[test]
fn an_arch_cuts_through_a_column_written_after_it() {
    let members = "arch from=2,1,2 to=4,2,2 mat_slot=step\n  column at=3,1,2 height=3";
    let src = hall(members);
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), Vec::<Finding<'_>>::new());
    assert_eq!(
        blocks_where(only_structure(&out), |id| id == "minecraft:stone_bricks"),
        [((3, 3, 2), "minecraft:stone_bricks".to_owned())],
    );
}

#[test]
fn a_box_as_long_on_x_as_on_z_with_no_axis_is_refused() {
    let line = "arch from=2,1,1 to=3,2,2 mat_slot=step";
    assert_eq!(
        refused(line),
        [(
            DiagnosticCode::InvalidMember,
            line.to_owned(),
            "arch box is 2 long on `x` and on `z`, so nothing says which way it spans".to_owned(),
            vec!["write `axis=x` or `axis=z` for the way the arch spans".to_owned()],
        )],
    );
}

#[test]
fn a_span_of_one_cell_is_refused() {
    let line = "arch from=2,1,4 to=2,2,4 axis=x mat_slot=step";
    assert_eq!(
        refused(line),
        [(
            DiagnosticCode::InvalidMember,
            line.to_owned(),
            "arch spans 1 cell along `x`, leaving no room for its two stairs".to_owned(),
            vec![
                "an arch spans at least 2 cells: widen its box along the axis it spans".to_owned()
            ],
        )],
    );
}

#[test]
fn an_axis_that_is_neither_x_nor_z_is_refused() {
    let line = "arch from=2,1,4 to=3,2,4 axis=y mat_slot=step";
    assert_eq!(
        refused(line),
        [(
            DiagnosticCode::InvalidMember,
            line.to_owned(),
            "unknown arch `axis=y`".to_owned(),
            vec![
                "`axis=` takes x or z".to_owned(),
                "did you mean `axis=x`?".to_owned()
            ],
        )],
    );
}

#[test]
fn an_axis_that_is_no_identifier_is_ignored_and_the_longer_side_spanned() {
    let src = hall("arch from=2,1,4 to=3,2,4 axis=1 mat_slot=step");
    let out = lowered(&src);
    assert_eq!(
        owned(&src, &out),
        [(
            DiagnosticCode::IgnoredArgument,
            "1".to_owned(),
            "`axis=` must be `x` or `z`, not integer `1`; the value was ignored".to_owned(),
            vec!["the arch spans its box's longer side, as with no `axis=`".to_owned()],
        )],
    );
    assert_eq!(
        blocks_where(only_structure(&out), |id| id.ends_with("_stairs")),
        [
            stair("oak_stairs", "west", (2, 2, 4)),
            stair("oak_stairs", "east", (3, 2, 4)),
        ],
    );
}

#[test]
fn a_material_that_is_no_stair_is_refused_and_the_default_arch_built() {
    let src = hall("arch from=2,1,4 to=3,2,4 mat_slot=wall");
    let out = lowered(&src);
    assert_eq!(
        owned(&src, &out),
        [(
            DiagnosticCode::IncompatibleMaterial,
            "@cobblestone".to_owned(),
            "arch derives `facing` / `half` / `shape` from the end of its top row each stair lies \
             at and `minecraft:cobblestone` is not a stair, so those states have nowhere to go"
                .to_owned(),
            vec![
                "bind the slot to a `*_stairs` material — the registry pack's `roof.dark_wood`, \
                 `roof.light_wood`, `roof.warm_wood`, and `roof.cool_wood` all resolve to one"
                    .to_owned(),
                "reached through `mat_slot=wall`, so every member reading that slot has it too"
                    .to_owned(),
            ],
        )],
    );
    assert_eq!(
        blocks_where(only_structure(&out), |id| id.ends_with("_stairs")),
        [
            stair("stone_brick_stairs", "west", (2, 2, 4)),
            stair("stone_brick_stairs", "east", (3, 2, 4)),
        ],
    );
}
