//! A `raw_repeat` writes the raw members under it again at each step, and `origin=imported` marks
//! a raw member that sizes the volume (`spec/materials-themes` "Raw blocks").

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::block_array::{BlockArrayIr, Dims};

use crate::common::{findings, lowered, only_structure};

/// A block id a member resolved to, and its cells.
type Built = (String, Vec<(u32, u32, u32)>);

/// The cells each member written with `role` resolved to, by block id.
fn built_by(out: &BlockArrayIr, role: &str) -> Vec<Built> {
    out.members["struct::s"]
        .iter()
        .filter(|member| member.role == role)
        .flat_map(|member| &member.resolved_state)
        .map(|block| (block.state.id.clone(), block.cells().collect()))
        .collect()
}

/// The note every refusal of a `raw_repeat`'s `count=` or `step=` carries.
const REPEAT_FORM: &str = "write `count=N step=dx,dy,dz`: the raw members indented under the \
                           `raw_repeat` are written `N` times, each copy `dx,dy,dz` past the one \
                           before";

#[test]
fn a_raw_repeat_writes_each_raw_member_under_it_once_per_copy_a_step_apart() {
    let src = "struct s size=6x4\n  raw_repeat count=2 step=3,0,2\n    \
               raw_block mat=minecraft:stone at=0,0,0\n    \
               raw_fill mat=minecraft:glass from=2,0,1 to=1,0,0\n";
    let out = lowered(src);
    assert_eq!(findings(src, &out), []);
    assert_eq!(
        built_by(&out, "raw_block"),
        [("minecraft:stone".to_owned(), vec![(0, 0, 0), (3, 0, 2)])],
    );
    assert_eq!(
        built_by(&out, "raw_fill"),
        [(
            "minecraft:glass".to_owned(),
            vec![
                (1, 0, 0),
                (2, 0, 0),
                (1, 0, 1),
                (2, 0, 1),
                (4, 0, 2),
                (5, 0, 2),
                (4, 0, 3),
                (5, 0, 3),
            ],
        )],
    );
}

#[test]
fn copies_of_one_raw_member_that_overlap_are_no_phase_conflict() {
    let src = "struct s size=5x3\n  raw_repeat count=3 step=1,0,0\n    \
               raw_fill mat=minecraft:stone from=0,0,0 to=1,0,0\n";
    let out = lowered(src);
    assert_eq!(findings(src, &out), []);
    assert_eq!(
        built_by(&out, "raw_fill"),
        [(
            "minecraft:stone".to_owned(),
            vec![(0, 0, 0), (1, 0, 0), (2, 0, 0), (3, 0, 0)],
        )],
    );
}

#[test]
fn a_raw_repeat_under_a_level_is_measured_from_the_level() {
    let src = "struct s size=5x3\n  raw_block mat=minecraft:stone at=0,3,0 origin=imported\n  \
               level y=1\n    raw_repeat count=2 step=0,1,0\n      \
               raw_block mat=minecraft:glass at=1,0,1\n";
    let out = lowered(src);
    assert_eq!(findings(src, &out), []);
    assert_eq!(
        built_by(&out, "raw_block"),
        [
            ("minecraft:glass".to_owned(), vec![(1, 1, 1), (1, 2, 1)]),
            ("minecraft:stone".to_owned(), vec![(0, 3, 0)]),
        ],
    );
}

#[test]
fn a_copy_outside_the_footprint_refuses_the_raw_member() {
    let src = "struct s size=5x3\n  raw_repeat count=4 step=2,0,0\n    \
               raw_block mat=minecraft:stone at=0,0,0\n";
    let out = lowered(src);
    assert_eq!(
        findings(src, &out),
        [(
            DiagnosticCode::InvalidMember,
            "raw_block mat=minecraft:stone at=0,0,0",
            "raw_block `at=0,0,0`, whose last of 4 `raw_repeat` copies lands at 6,0,0, lies \
             outside the struct's `size=5x3` footprint",
            vec![
                "the cell's `x` runs from 0 to 4 and its `z` from 0 to 2",
                "lower the `raw_repeat`'s `count=` or `step=`",
            ],
        )],
    );
    assert_eq!(built_by(&out, "raw_block"), []);
}

#[test]
fn a_copy_above_the_struct_refuses_the_raw_member() {
    let src = "struct s size=5x3\n  level y=1\n    raw_repeat count=2 step=0,1,0\n      \
               raw_block mat=minecraft:stone at=0,0,0\n";
    let out = lowered(src);
    assert_eq!(
        findings(src, &out),
        [(
            DiagnosticCode::InvalidMember,
            "raw_block mat=minecraft:stone at=0,0,0",
            "raw_block `at=0,0,0`, whose last of 2 `raw_repeat` copies lands at 0,1,0 under \
             `level y=1` (row 2), does not fit in the struct (dims.y=1)",
            vec![
                "the struct's top row is 0, below this raw_block's `level y=1`",
                "lower the raw_block's `level` or its `raw_repeat`'s `count=` or `step=`, or \
                 raise the `walls` to reach its rows",
            ],
        )],
    );
}

#[test]
fn a_raw_repeat_that_does_not_say_how_to_repeat_writes_nothing() {
    let cases = [
        (
            "raw_repeat step=1,0,0",
            "raw_repeat has no `count=`, so nothing says how many copies it writes",
            REPEAT_FORM,
        ),
        (
            "raw_repeat count=0 step=1,0,0",
            "raw_repeat `count=` must be a positive integer that fits in u32, not integer `0`",
            REPEAT_FORM,
        ),
        (
            "raw_repeat count=2",
            "raw_repeat has no `step=`, so nothing says where each copy goes",
            REPEAT_FORM,
        ),
        (
            "raw_repeat count=2 step=1",
            "raw_repeat `step=` must be a coordinate triple whose components fit in u32, not \
             integer `1`",
            REPEAT_FORM,
        ),
        (
            "raw_repeat count=2 step=0,0,0",
            "raw_repeat `step=0,0,0` puts each of its 2 copies in the cells of the first",
            "give `step=` the distance from one copy to the next, or write the raw members once \
             without a `raw_repeat`",
        ),
    ];
    for (line, primary, note) in cases {
        let src =
            format!("struct s size=5x3\n  {line}\n    raw_block mat=minecraft:nope at=0,0,0\n");
        let out = lowered(&src);
        assert_eq!(
            findings(&src, &out),
            [(DiagnosticCode::InvalidMember, line, primary, vec![note])],
            "the `raw_block` under a refused `raw_repeat` is not lowered at all",
        );
    }
}

#[test]
fn a_raw_repeat_with_nothing_under_it_is_refused() {
    let src = "struct s size=5x3\n  raw_repeat count=2 step=1,0,0\n";
    let out = lowered(src);
    assert_eq!(
        findings(src, &out),
        [(
            DiagnosticCode::InvalidMember,
            "raw_repeat count=2 step=1,0,0",
            "raw_repeat has no `raw_block` or `raw_fill` indented under it, so it writes nothing",
            vec!["indent the `raw_block` and `raw_fill` lines to repeat under the `raw_repeat`"],
        )],
    );
}

#[test]
fn an_imported_raw_member_sizes_the_volume_to_its_last_copy() {
    let src = "struct s size=5x3\n  raw_repeat count=3 step=0,2,0\n    \
               raw_fill mat=minecraft:stone from=0,0,0 to=1,1,0 origin=imported\n";
    let out = lowered(src);
    assert_eq!(findings(src, &out), []);
    assert_eq!(only_structure(&out).dims, Dims { x: 5, y: 6, z: 3 });
}

#[test]
fn a_raw_member_without_origin_imported_does_not_size_the_volume() {
    let imported = "struct s size=5x3\n  raw_block mat=minecraft:stone at=1,4,1 origin=imported\n";
    let out = lowered(imported);
    assert_eq!(findings(imported, &out), []);
    assert_eq!(only_structure(&out).dims, Dims { x: 5, y: 5, z: 3 });

    let authored = "struct s size=5x3\n  raw_block mat=minecraft:stone at=1,4,1\n";
    let out = lowered(authored);
    assert_eq!(
        findings(authored, &out),
        [(
            DiagnosticCode::InvalidMember,
            "raw_block mat=minecraft:stone at=1,4,1",
            "raw_block `at=1,4,1` does not fit in the struct (dims.y=1)",
            vec![
                "the cell's `y` runs from 0 to 0, the struct's top row",
                "lower the raw_block's `at=`, or raise the `walls` to reach its rows",
            ],
        )],
    );
}

#[test]
fn an_origin_other_than_imported_is_refused() {
    let src = "struct s size=5x3\n  raw_block mat=minecraft:stone at=1,0,1 origin=elsewhere\n";
    let out = lowered(src);
    assert_eq!(
        findings(src, &out),
        [(
            DiagnosticCode::InvalidMember,
            "raw_block mat=minecraft:stone at=1,0,1 origin=elsewhere",
            "raw_block `origin=` must be `imported`, not identifier `elsewhere`",
            vec![
                "`origin=imported` marks a line `cairn import` wrote; leave `origin=` out of a \
                 line written by hand",
            ],
        )],
    );
    assert_eq!(built_by(&out, "raw_block"), []);
}
