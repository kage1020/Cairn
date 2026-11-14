//! A `note_block` stands in the cell its triple names, with the `note` and `instrument` its line
//! writes and nothing else (`spec/blockstate` "Derive by default, promote on override").

use cairn_lang_core::DiagnosticCode;

use crate::common::{Finding, blocks_where, findings, lowered, only_structure};

/// A 7x5 struct with a floor and 3-row walls, `members` after them.
fn room(members: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n\n\
         struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  {members}\n"
    )
}

/// The note blocks `out` holds, each with its cell.
fn note_blocks(out: &cairn_lang_core::BlockArrayIr) -> Vec<((u32, u32, u32), String)> {
    blocks_where(only_structure(out), |id| id == "minecraft:note_block")
}

#[test]
fn a_note_block_writes_the_note_and_instrument_its_line_does_and_no_other_state() {
    let src = room("note_block at=2,1,2 instrument=bit note=12\n  note_block at=1,1,1");
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), Vec::<Finding<'_>>::new());
    assert_eq!(
        note_blocks(&out),
        [
            ((1, 1, 1), "minecraft:note_block".to_owned()),
            (
                (2, 1, 2),
                "minecraft:note_block[instrument=bit,note=12]".to_owned()
            ),
        ],
    );
}

#[test]
fn a_note_outside_two_octaves_or_an_at_that_is_no_triple_is_refused() {
    const CELL: &str = "write `at=` as the coordinate triple `x,y,z` of the cell the block goes in, as in `at=1,1,1`";
    for (line, primary, notes) in [
        (
            "note_block at=2,1,2 note=25",
            "note_block `note=25` is no note a note block plays",
            vec![
                "`note=` takes a whole number from 0 to 24, the semitones above the note block's \
                 lowest note",
            ],
        ),
        (
            "note_block at=inside.front",
            "note_block `at=` must be a coordinate triple, not reference `inside.front`",
            vec![CELL],
        ),
        (
            "note_block",
            "note_block has no `at=`, so nothing says which cell its block goes in",
            vec![CELL],
        ),
    ] {
        let src = room(line);
        let out = lowered(&src);
        assert_eq!(
            findings(&src, &out),
            [(DiagnosticCode::InvalidMember, line, primary, notes)],
            "{line}",
        );
        assert_eq!(note_blocks(&out), [], "{line}");
    }
}

#[test]
fn a_value_of_the_wrong_kind_is_ignored_and_the_note_block_built_without_it() {
    let src = room("note_block at=2,1,2 note=x instrument=3");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::IgnoredArgument,
                "x",
                "`note=` must be a whole number from 0 to 24, not identifier `x`; the value was \
                 ignored",
                vec!["the note block is built with `note=0`"],
            ),
            (
                DiagnosticCode::IgnoredArgument,
                "3",
                "`instrument=` must be the name of an instrument, such as `harp`, `bass` or \
                 `bit`, not integer `3`; the value was ignored",
                vec![
                    "the note block is built with no `instrument`, which the game sets from the \
                     block under it",
                ],
            ),
        ],
    );
    assert_eq!(
        note_blocks(&out),
        [((2, 1, 2), "minecraft:note_block".to_owned())]
    );
}
