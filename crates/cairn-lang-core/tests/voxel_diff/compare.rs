use cairn_lang_core::block_array::BlockState;
use cairn_lang_core::voxel_diff::{Counts, compare};

use crate::common::{only_structure, read_example, solid_cells};
use crate::{THEME, build, hut, report, unit_of, units};

/// Counts of `blocks` cells that agree in every way, `residual` of them built by raw members.
fn agreeing(blocks: u64, residual: u64) -> Counts {
    Counts {
        structure_blocks: blocks,
        built_blocks: blocks,
        both_blocks: blocks,
        matched: blocks,
        states_matched: blocks,
        residual,
    }
}

#[test]
fn a_source_compared_with_its_own_build_agrees_at_every_cell() {
    let source = read_example("cottage.crn");
    let built = build(&source).expect("the cottage builds");
    let blocks = solid_cells(only_structure(&built)) as u64;
    let report = report(&units(&source), &source);
    assert_eq!(report.counts, agreeing(blocks, 0));
    assert!(report.converged);
    assert_eq!(report.code, None);
    assert_eq!(report.regions, []);
    assert_eq!(report.patch, None);
}

#[test]
fn every_cell_a_raw_member_builds_is_residual() {
    let structure = hut("");
    let lift = hut("  raw_fill mat=minecraft:oak_planks from=1,1,1 to=3,1,3\n");
    let units = {
        let built = build(&lift).expect("the lift builds");
        vec![unit_of("s", only_structure(&built))]
    };
    let blocks = solid_cells(only_structure(&build(&structure).expect("the hut builds"))) as u64;
    let report = report(&units, &lift);
    assert_eq!(report.counts, agreeing(blocks + 9, 9));
    assert!(!report.converged);
}

#[test]
fn a_state_the_build_leaves_out_counts_as_the_default_the_entry_gives_and_is_otherwise_unread() {
    let source = format!("{THEME}struct s size=3x3\n  raw_block mat=minecraft:stone at=1,0,1\n");
    let built = build(&source).expect("it builds");
    let mut unit = unit_of("s", only_structure(&built));
    let stone = unit
        .grid
        .palette
        .iter_mut()
        .find(|entry| entry.state.id == "minecraft:stone")
        .expect("a stone entry");
    stone.state = BlockState {
        id: "minecraft:stone".into(),
        properties: [("powered", "false"), ("lit", "true")]
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .into_iter()
            .collect(),
    };
    stone.defaults.insert("lit".into(), "false".into());
    let units = [unit];
    let report = report(&units, &source);
    assert_eq!(
        (report.counts.matched, report.counts.states_matched),
        (1, 0)
    );
    let states: Vec<_> = report.regions[0]
        .states
        .iter()
        .map(|s| (s.key.as_str(), s.structure.as_deref(), s.built.as_deref()))
        .collect();
    assert_eq!(states, [("lit", Some("true"), Some("false"))]);
}

#[test]
fn a_waterlogged_either_side_leaves_out_counts_as_dry() {
    let source = format!(
        "{THEME}struct s size=3x3\n  raw_block mat=minecraft:oak_stairs[facing=east] at=1,0,1\n"
    );
    let built = build(&source).expect("it builds");
    let with_water = |value: &str| {
        let mut unit = unit_of("s", only_structure(&built));
        for entry in &mut unit.grid.palette {
            if entry.state.id == "minecraft:oak_stairs" {
                entry
                    .state
                    .properties
                    .insert("waterlogged".into(), value.into());
            }
        }
        [unit]
    };
    let dry = report(&with_water("false"), &source);
    assert_eq!(dry.counts, agreeing(1, 1));
    let wet = report(&with_water("true"), &source);
    assert_eq!((wet.counts.matched, wet.counts.states_matched), (1, 0));
    let states: Vec<_> = wet.regions[0]
        .states
        .iter()
        .map(|s| (s.key.as_str(), s.structure.as_deref(), s.built.as_deref()))
        .collect();
    assert_eq!(states, [("waterlogged", Some("true"), Some("false"))]);

    let written_dry = source.replace("[facing=east]", "[facing=east,waterlogged=false]");
    assert_eq!(report(&units(&source), &written_dry).counts, agreeing(1, 1));
}

#[test]
fn a_state_the_build_writes_and_the_structure_does_not_differs() {
    let lift = format!(
        "{THEME}struct s size=3x3\n  raw_block mat=minecraft:oak_stairs[facing=east] at=1,0,1\n"
    );
    let structure =
        format!("{THEME}struct s size=3x3\n  raw_block mat=minecraft:oak_stairs at=1,0,1\n");
    let report = report(&units(&structure), &lift);
    let states: Vec<_> = report.regions[0]
        .states
        .iter()
        .map(|s| (s.key.as_str(), s.structure.as_deref(), s.built.as_deref()))
        .collect();
    assert_eq!(states, [("facing", None, Some("east"))]);
}

#[test]
fn air_of_any_spelling_holds_no_block() {
    let structure = format!(
        "{THEME}struct s size=3x3\n  raw_block mat=minecraft:stone at=1,0,1\n  \
         raw_block mat=minecraft:cave_air at=0,0,0\n  raw_block mat=minecraft:void_air at=2,0,2\n"
    );
    let lift = format!("{THEME}struct s size=3x3\n  raw_block mat=minecraft:stone at=1,0,1\n");
    assert_eq!(report(&units(&structure), &lift).counts, agreeing(1, 1));
}

#[test]
fn a_lone_body_on_each_side_is_compared_whatever_their_names() {
    let source = hut("");
    let mut units = units(&source);
    units[0].name = "imported".into();
    let report = report(&units, &source);
    assert!(report.converged);
    let pairs: Vec<_> = report
        .bodies
        .iter()
        .map(|b| (b.structure.as_deref(), b.source.as_deref()))
        .collect();
    assert_eq!(pairs, [(Some("imported"), Some("s"))]);
}

#[test]
fn bodies_pair_by_name_and_an_unpaired_body_counts_every_cell() {
    let two = format!(
        "{THEME}struct a size=3x3\n  floor mat_slot=floor\n\n\
         struct b size=3x3\n  floor mat_slot=wall\n"
    );
    let mut units = units(&two);
    units[1].name = "c".into();
    let report = report(&units, &two);
    let bodies: Vec<_> = report
        .bodies
        .iter()
        .map(|b| (b.structure.as_deref(), b.source.as_deref(), b.counts))
        .collect();
    let empty = Counts::default();
    assert_eq!(
        bodies,
        [
            (Some("a"), Some("a"), agreeing(9, 0)),
            (
                Some("c"),
                None,
                Counts {
                    structure_blocks: 9,
                    ..empty
                }
            ),
            (
                None,
                Some("b"),
                Counts {
                    built_blocks: 9,
                    ..empty
                }
            ),
        ]
    );
    assert_eq!(
        compare(&units, &build(&two).expect("it builds")).counts,
        Counts {
            structure_blocks: 18,
            built_blocks: 18,
            both_blocks: 9,
            matched: 9,
            states_matched: 9,
            residual: 0,
        }
    );
}
