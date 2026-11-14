//! A `trapdoor` hangs by a wall or stands at a triple, closed in the bottom half of its cell
//! unless its line says otherwise (`spec/blockstate` "Derive by default, promote on override").

use cairn_lang_core::DiagnosticCode;

use crate::common::{Finding, blocks_where, findings, lowered, only_structure};

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n  slot hatch -> @spruce_trapdoor\n\n";

/// A 7x5 struct with a floor and 3-row walls, `members` after them; with no overhang its front
/// wall is `z=4` and its left `x=0`.
fn room(members: &str) -> String {
    format!(
        "{THEME}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         {members}\n"
    )
}

/// A block as `id[properties]`, with its cell.
type Placed = ((u32, u32, u32), String);

/// The trapdoors `out` holds.
fn trapdoors(out: &cairn_lang_core::BlockArrayIr) -> Vec<Placed> {
    blocks_where(only_structure(out), |id| id.ends_with("trapdoor"))
}

/// The trapdoors `members` sets in [`room`], which it must set without a finding.
fn hung(members: &str) -> Vec<Placed> {
    let src = room(members);
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), Vec::<Finding<'_>>::new(), "{members}");
    trapdoors(&out)
}

#[test]
fn a_trapdoor_by_a_wall_faces_away_from_it_closed_and_low() {
    assert_eq!(
        hung("trapdoor at=inside.front offset=2 y=1"),
        [(
            (2, 1, 3),
            "minecraft:oak_trapdoor[facing=north,half=bottom,open=false]".to_owned()
        )],
    );
}

#[test]
fn what_a_trapdoor_line_writes_replaces_what_is_derived() {
    // Along the left wall `offset=` grows toward the south, so `right` faces south.
    assert_eq!(
        hung("trapdoor at=inside.left offset=1 y=2 mat_slot=hatch facing=right half=top open=true"),
        [(
            (1, 2, 1),
            "minecraft:spruce_trapdoor[facing=south,half=top,open=true]".to_owned()
        )],
    );
    assert_eq!(
        hung("trapdoor at=inside.front offset=2 y=1 waterlogged=true"),
        [(
            (2, 1, 3),
            "minecraft:oak_trapdoor[facing=north,half=bottom,open=false,waterlogged=true]"
                .to_owned()
        )],
    );
}

#[test]
fn a_trapdoor_at_a_triple_takes_a_compass_facing_or_the_blocks_own() {
    assert_eq!(
        hung("trapdoor at=3,1,2 facing=east half=top"),
        [(
            (3, 1, 2),
            "minecraft:oak_trapdoor[facing=east,half=top,open=false]".to_owned()
        )],
    );
    assert_eq!(
        hung("trapdoor at=3,1,2"),
        [(
            (3, 1, 2),
            "minecraft:oak_trapdoor[half=bottom,open=false]".to_owned()
        )],
    );
}

#[test]
fn a_word_the_trapdoor_does_not_take_is_refused_and_it_is_not_built() {
    const COMPASS: &str = "`facing=` takes one of north, south, east, west: north toward the \
                           `back` wall and east toward the `right` one";
    for (line, primary, notes) in [
        (
            "trapdoor at=inside.front offset=2 y=1 facing=north",
            "unknown trapdoor `facing=north`",
            vec![
                "`facing=` takes one of in, out, left, right",
                "`facing=` names a way from the wall the trapdoor stands by, and `north` from the \
                 front wall is `facing=in`",
            ],
        ),
        (
            "trapdoor at=3,1,2 facing=in",
            "unknown trapdoor `facing=in` at a coordinate triple",
            vec![
                COMPASS,
                "`in` names a way from a wall, and a trapdoor at a coordinate triple hangs by \
                 none; set it by a wall with `at=<side>.outside` or `at=inside.<side>` to turn it \
                 that way",
            ],
        ),
        (
            "trapdoor at=3,1,2 facing=nort",
            "unknown trapdoor `facing=nort` at a coordinate triple",
            vec![COMPASS, "did you mean `facing=north`?"],
        ),
        (
            "trapdoor at=inside.front offset=2 y=1 half=middle",
            "unknown trapdoor `half=middle`",
            vec!["`half=` takes top or bottom"],
        ),
        (
            "trapdoor at=3,1,2 offset=1",
            "trapdoor `offset=` beside a coordinate triple, which names the cell whole",
            vec![
                "drop `offset=`, or set the trapdoor by a wall with `at=<side>.outside` or \
                 `at=inside.<side>`, which reads them",
            ],
        ),
        (
            "trapdoor",
            "trapdoor has no `at=`, so nothing says where it stands",
            vec![
                "write `at=` as the coordinate triple `x,y,z` of the cell the trapdoor stands in, \
                 as in `at=1,1,1`, or as a wall anchor, `at=<side>.outside` or \
                 `at=inside.<side>`",
            ],
        ),
    ] {
        let src = room(line);
        let out = lowered(&src);
        assert_eq!(
            findings(&src, &out),
            [(DiagnosticCode::InvalidMember, line, primary, notes)],
            "{line}",
        );
        assert_eq!(trapdoors(&out), [], "{line}");
    }
}

#[test]
fn every_word_a_trapdoor_line_gets_wrong_is_its_own_finding() {
    let line = "trapdoor at=inside.front offset=2 y=1 half=middle waterlogged=maybe facing=up";
    let src = room(line);
    let out = lowered(&src);
    let found: Vec<_> = findings(&src, &out)
        .into_iter()
        .map(|(code, _, primary, _)| (code, primary))
        .collect();
    assert_eq!(
        found,
        [
            (
                DiagnosticCode::InvalidMember,
                "unknown trapdoor `facing=up`"
            ),
            (
                DiagnosticCode::InvalidMember,
                "unknown trapdoor `half=middle`"
            ),
            (
                DiagnosticCode::InvalidMember,
                "unknown trapdoor `waterlogged=maybe`"
            ),
        ],
    );
}

#[test]
fn a_value_of_the_wrong_kind_is_ignored_and_the_trapdoor_built_without_it() {
    let line = "trapdoor at=inside.front offset=2 y=1 half=3 open=yes";
    let src = room(line);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::IgnoredArgument,
                "3",
                "`half=` must be `top` or `bottom`, not integer `3`; the value was ignored",
                vec!["the trapdoor is built in the bottom half of its cell, as `half=bottom`"],
            ),
            (
                DiagnosticCode::IgnoredArgument,
                "yes",
                "`open=` must be `true` or `false`, not identifier `yes`; the value was ignored",
                vec!["the trapdoor is built closed, as `open=false`"],
            ),
        ],
    );
    assert_eq!(
        trapdoors(&out),
        [(
            (2, 1, 3),
            "minecraft:oak_trapdoor[facing=north,half=bottom,open=false]".to_owned()
        )],
    );
}

#[test]
fn a_trapdoor_bound_to_no_trapdoor_is_built_from_the_default_one() {
    let src = room("trapdoor at=inside.front offset=2 y=1 mat_slot=wall");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::IncompatibleMaterial,
            "@cobblestone",
            "trapdoor derives `facing` / `half` / `open` from the wall it hangs by and its own \
             line and `minecraft:cobblestone` is not a trapdoor, so those states have nowhere to \
             go",
            vec![
                "bind the slot to a `*_trapdoor` material — the registry pack's \
                 `trapdoor.default` resolves to one",
                "reached through `mat_slot=wall`, so every member reading that slot has it too",
            ],
        )],
    );
    assert_eq!(
        trapdoors(&out),
        [(
            (2, 1, 3),
            "minecraft:oak_trapdoor[facing=north,half=bottom,open=false]".to_owned()
        )],
    );
}
