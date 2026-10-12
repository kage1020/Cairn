use std::collections::{BTreeSet, HashMap};

use cairn_lang_core::Edition;
use cairn_lang_core::error::Span;

use super::{
    Allowance, Blocked, DUST_ATTENUATION_LIMIT, Misfit, NoRepeaterSite, Refused, Touching,
    holds_repeater, no_repeater_site_diagnostic, repeater_sites, repeater_sites_of_scope,
    why_no_site,
};
use crate::delay::buffer_count_for_segment;
use crate::diagnostic::DiagnosticCode;
use crate::edition_netlist_ir::EditionCell;
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{CellPortDriver, NetRef, PortName};
use crate::placement_ir::{CellCoord, PlacementIr, ScopedPlacementIrEntry};
use crate::routing_geometry::NetTree;
use crate::test_fixtures::{ROUTED, placed_cell, reservation, routed_output};

/// The coords a walk from `start` visits, `start` included, taking each `(step, count)` in turn.
fn walk(start: (u32, u32, u32), moves: &[((i64, i64, i64), u32)]) -> Vec<CellCoord> {
    let mut at = start;
    let mut path = vec![CellCoord::new(at.0, at.1, at.2)];
    let axis = |value: u32, delta: i64| {
        u32::try_from(i64::from(value) + delta).expect("the walk stays on the grid")
    };
    for &((dx, dy, dz), count) in moves {
        for _ in 0..count {
            at = (axis(at.0, dx), axis(at.1, dy), axis(at.2, dz));
            path.push(CellCoord::new(at.0, at.1, at.2));
        }
    }
    path
}

const EAST: (i64, i64, i64) = (1, 0, 0);
const SOUTH: (i64, i64, i64) = (0, 0, 1);
const UP: (i64, i64, i64) = (0, 1, 0);

#[test]
fn a_straight_run_takes_a_repeater_every_sixteen_steps() {
    for length in [15u32, 16, 17, 32, 33, 48, 49] {
        let tree = NetTree::from_paths(&[&walk((0, 0, 0), &[(EAST, length)])]);
        let sites =
            repeater_sites(&tree, 0, &HashMap::new()).expect("a straight run has room everywhere");
        let expected: Vec<CellCoord> = (1..=buffer_count_for_segment(length))
            .map(|k| CellCoord::new(k * (DUST_ATTENUATION_LIMIT + 1), 0, 0))
            .collect();
        assert_eq!(sites, expected, "{length} steps");
    }
}

#[test]
fn a_repeater_steps_back_off_a_turn() {
    let tree = NetTree::from_paths(&[&walk((0, 0, 0), &[(EAST, 16), (SOUTH, 2)])]);
    assert_eq!(
        repeater_sites(&tree, 0, &HashMap::new()),
        Ok(vec![CellCoord::new(15, 0, 0)]),
        "the corner is (16,0,0); the last straight coord before it is (15,0,0)",
    );
}

#[test]
fn a_repeater_steps_back_off_a_climb() {
    let tree = NetTree::from_paths(&[&walk((0, 0, 0), &[(EAST, 14), (UP, 2), (EAST, 2)])]);
    assert_eq!(
        repeater_sites(&tree, 0, &HashMap::new()),
        Ok(vec![CellCoord::new(13, 0, 0)]),
        "neither the top of the climb, nor the vertical run, nor its foot holds a repeater",
    );
}

#[test]
fn a_repeater_steps_back_off_a_fork_onto_the_trunk() {
    let trunk = walk((0, 0, 0), &[(EAST, 16)]);
    let branch = walk((15, 0, 0), &[(SOUTH, 1), (EAST, 1)]);
    let tree = NetTree::from_paths(&[&trunk, &branch]);
    assert_eq!(
        repeater_sites(&tree, 0, &HashMap::new()),
        Ok(vec![CellCoord::new(14, 0, 0)]),
        "(15,0,0) forks; (14,0,0) is the trunk coord before it",
    );
}

#[test]
fn stepping_back_can_cost_a_repeater_the_length_does_not_imply() {
    let tree = NetTree::from_paths(&[&walk((0, 0, 0), &[(EAST, 16), (SOUTH, 16)])]);
    let sites =
        repeater_sites(&tree, 0, &HashMap::new()).expect("there is straight wire on both legs");
    assert_eq!(
        sites,
        vec![CellCoord::new(15, 0, 0), CellCoord::new(16, 0, 15)],
    );
    assert_eq!(
        buffer_count_for_segment(32),
        1,
        "the length alone asks for one"
    );
}

#[test]
fn a_staircase_has_nowhere_for_a_repeater() {
    let mut moves = Vec::new();
    for _ in 0..9 {
        moves.push((EAST, 1));
        moves.push((SOUTH, 1));
    }
    let path = walk((0, 0, 0), &moves);
    let tree = NetTree::from_paths(&[&path]);
    assert_eq!(
        repeater_sites(&tree, 0, &HashMap::new()),
        Err(NoRepeaterSite {
            from: CellCoord::new(0, 0, 0),
            spent: 0,
            unpowered: path[16],
            allowance: Allowance::Limit,
            reach: DUST_ATTENUATION_LIMIT,
            starts_on_unpowered: true,
            blocked: Blocked::NoneBetween,
            refused: Refused {
                bends: true,
                touching: None,
            },
        }),
    );
}

#[test]
fn a_straight_run_beside_its_own_net_is_refused_for_touching_it() {
    let trunk = walk((0, 0, 0), &[(EAST, 17)]);
    let branch = walk((0, 0, 0), &[(SOUTH, 1), (EAST, 16)]);
    let tree = NetTree::from_paths(&[&trunk, &branch]);
    let stretch = repeater_sites(&tree, 0, &HashMap::new());
    assert_eq!(
        stretch,
        Err(NoRepeaterSite {
            from: CellCoord::new(0, 0, 0),
            spent: 0,
            unpowered: CellCoord::new(16, 0, 0),
            allowance: Allowance::Limit,
            reach: DUST_ATTENUATION_LIMIT,
            starts_on_unpowered: true,
            blocked: Blocked::NoneBetween,
            refused: Refused {
                bends: false,
                touching: Some(Touching {
                    at: CellCoord::new(16, 0, 0),
                    touching: CellCoord::new(16, 0, 1),
                }),
            },
        }),
    );

    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(20, 3, 1));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.outputs.push(routed_output(
        NetRef::Input(0),
        *trunk.last().expect("a walk has an end"),
    ));
    let entry = ScopedPlacementIrEntry {
        kind: ScopeKind::Struct,
        name: "beside".to_owned(),
        ir: ir.clone(),
    };
    let region = ir.region.clone().expect("set above");
    let diagnostic = no_repeater_site_diagnostic(
        &ir,
        &entry,
        "routed",
        &region,
        NetRef::Input(0),
        &tree,
        stretch.expect_err("refused above"),
    );
    assert_eq!(
        diagnostic.primary,
        "routed netlist for struct `beside` routes sig.a so that the signal leaving (0,0,0) runs out before (16,0,0), past the attenuation limit of 15 blocks of dust, and never reaches output pad #0: every coord after (0,0,0) up to and including (16,0,0) runs straight but touches another block of its own net on one of its faces, as (16,0,0) touches (16,0,1) — a buffer repeater, which joins only the blocks at its back and front, would cut that block off from the dust it replaces, so none can stand there",
    );
}

#[test]
fn a_stretch_that_turns_and_touches_names_both() {
    let mut stairs = vec![(EAST, 8)];
    for _ in 0..5 {
        stairs.push((SOUTH, 1));
        stairs.push((EAST, 1));
    }
    let trunk = walk((0, 0, 1), &stairs);
    let branch = walk((0, 0, 1), &[((0, 0, -1), 1), (EAST, 7)]);
    let tree = NetTree::from_paths(&[&trunk, &branch]);
    let stretch = repeater_sites(&tree, 0, &HashMap::new());
    assert_eq!(
        stretch,
        Err(NoRepeaterSite {
            from: CellCoord::new(0, 0, 1),
            spent: 0,
            unpowered: trunk[16],
            allowance: Allowance::Limit,
            reach: DUST_ATTENUATION_LIMIT,
            starts_on_unpowered: true,
            blocked: Blocked::NoneBetween,
            refused: Refused {
                bends: true,
                touching: Some(Touching {
                    at: CellCoord::new(7, 0, 1),
                    touching: CellCoord::new(7, 0, 0),
                }),
            },
        }),
    );
}

#[test]
fn a_touching_coord_met_before_a_bend_is_kept_with_it() {
    let mut stairs = Vec::new();
    for _ in 0..4 {
        stairs.push((SOUTH, 1));
        stairs.push((EAST, 1));
    }
    stairs.push((EAST, 9));
    let trunk = walk((0, 0, 0), &stairs);
    let branch = walk((13, 0, 4), &[(UP, 1), ((-1, 0, 0), 9)]);
    let tree = NetTree::from_paths(&[&trunk, &branch]);
    assert_eq!(
        repeater_sites(&tree, 0, &HashMap::new()),
        Err(NoRepeaterSite {
            from: CellCoord::new(0, 0, 0),
            spent: 0,
            unpowered: CellCoord::new(12, 0, 4),
            allowance: Allowance::Limit,
            reach: DUST_ATTENUATION_LIMIT,
            starts_on_unpowered: true,
            blocked: Blocked::NoneBetween,
            refused: Refused {
                bends: true,
                touching: Some(Touching {
                    at: CellCoord::new(12, 0, 4),
                    touching: CellCoord::new(12, 1, 4),
                }),
            },
        }),
    );
}

#[test]
fn a_coord_touching_its_net_on_two_faces_names_the_first() {
    let trunk = walk((0, 0, 1), &[(EAST, 3)]);
    let north = walk((0, 0, 1), &[((0, 0, -1), 1), (EAST, 2)]);
    let south = walk((0, 0, 1), &[(SOUTH, 1), (EAST, 2)]);
    let tree = NetTree::from_paths(&[&trunk, &north, &south]);
    let mut children: HashMap<CellCoord, Vec<CellCoord>> = HashMap::new();
    for coord in &tree.wire_path()[1..] {
        let parent = tree.parent(*coord).expect("only the source has none");
        children.entry(parent).or_default().push(*coord);
    }
    assert_eq!(
        holds_repeater(&tree, &children, CellCoord::new(1, 0, 1)),
        Err(Misfit::Touches(Touching {
            at: CellCoord::new(1, 0, 1),
            touching: CellCoord::new(1, 0, 0),
        })),
    );
}

/// The clauses [`why_no_site`] builds its refusals from.
const STRAIGHT_CLAUSE: &str =
    "a buffer repeater carries a signal only where the wire runs straight through it at one height";
const ON_A_FACE_CLAUSE: &str = "runs straight but touches another block of its own net on one of its faces, as (7,0,1) touches (7,0,0)";
const CUTS_CLAUSE: &str = "a buffer repeater, which joins only the blocks at its back and front, would cut that block off from the dust it replaces";
const TOO_WEAK_CLAUSE: &str = "and one further back would leave too little strength to get there";

/// Every record of what a walk refused; the touching coord is `(7,0,1)`, beside `(7,0,0)`.
fn refusals() -> [Refused; 4] {
    let touching = Some(Touching {
        at: CellCoord::new(7, 0, 1),
        touching: CellCoord::new(7, 0, 0),
    });
    [
        Refused::default(),
        Refused {
            bends: true,
            touching: None,
        },
        Refused {
            bends: false,
            touching,
        },
        Refused {
            bends: true,
            touching,
        },
    ]
}

/// [`why_no_site`] for a walk from `(0,0,0)` that gave up on `(16,0,0)`.
fn why_from_origin(
    blocked: Blocked,
    starts_on_unpowered: bool,
    refused: Refused,
    (allowance, reach): (Allowance, u32),
) -> String {
    why_no_site(&NoRepeaterSite {
        from: CellCoord::new(0, 0, 0),
        spent: 0,
        unpowered: CellCoord::new(16, 0, 0),
        allowance,
        reach,
        starts_on_unpowered,
        blocked,
        refused,
    })
}

/// Includes two pairs [`repeater_sites`] never builds; the sink row's `reach` of 16, not the
/// limit's 15, tells a clause rendered from `reach` from one rendered from the limit.
#[test]
fn every_blocked_and_refused_pair_reads_as_one_clause() {
    let [nothing, bends, touches, both] = refusals();
    let (straight, on_a_face, cuts, too_weak) = (
        STRAIGHT_CLAUSE,
        ON_A_FACE_CLAUSE,
        CUTS_CLAUSE,
        TOO_WEAK_CLAUSE,
    );
    // A sink reads the dust before it, so a repeater 16 steps back still reaches it.
    let sink = (Allowance::Limit, DUST_ATTENUATION_LIMIT + 1);
    let cases = [
        (
            Blocked::NoneBetween,
            nothing,
            sink,
            "no coord stands between the two, so there is nowhere for a buffer repeater".to_owned(),
        ),
        (
            Blocked::NoneBetween,
            bends,
            sink,
            format!(
                "every coord between the two turns, climbs or branches — {straight}, so none can stand there"
            ),
        ),
        (
            Blocked::NoneBetween,
            touches,
            sink,
            format!("every coord between the two {on_a_face} — {cuts}, so none can stand there"),
        ),
        (
            Blocked::NoneBetween,
            both,
            sink,
            format!(
                "every coord between the two turns, climbs or branches, or {on_a_face} — {straight} and joins only the blocks at its back and front, so none can stand there"
            ),
        ),
        (
            Blocked::NoneCloseEnough,
            nothing,
            (Allowance::Budget(3), 3),
            format!("no coord stands within 3 blocks before it, {too_weak}"),
        ),
        (
            Blocked::NoneCloseEnough,
            bends,
            (Allowance::Budget(1), 1),
            format!(
                "every coord within 1 block before it turns, climbs or branches — {straight}, {too_weak}"
            ),
        ),
        (
            Blocked::NoneCloseEnough,
            touches,
            (Allowance::Budget(2), 2),
            format!("every coord within 2 blocks before it {on_a_face} — {cuts}, {too_weak}"),
        ),
        (
            Blocked::NoneCloseEnough,
            both,
            sink,
            format!(
                "every coord within 16 blocks before it turns, climbs or branches, or {on_a_face} — {straight} and joins only the blocks at its back and front, {too_weak}"
            ),
        ),
    ];
    for (blocked, refused, held, clause) in cases {
        assert_eq!(
            why_from_origin(blocked, false, refused, held),
            clause,
            "{blocked:?} x {refused:?}",
        );
    }
}

/// The `NoneCloseEnough` and budget rows are ones [`repeater_sites`] never builds.
#[test]
fn a_walk_that_starts_on_unpowered_names_it_among_the_coords() {
    let [_, bends, touches, both] = refusals();
    let (straight, on_a_face, cuts, too_weak) = (
        STRAIGHT_CLAUSE,
        ON_A_FACE_CLAUSE,
        CUTS_CLAUSE,
        TOO_WEAK_CLAUSE,
    );
    let through = "after (0,0,0) up to and including (16,0,0)";
    // A block of dust is stood in place of, so a repeater at most 15 steps back reaches it.
    let dust = (Allowance::Limit, DUST_ATTENUATION_LIMIT);
    let cases = [
        (
            Blocked::NoneBetween,
            bends,
            dust,
            format!(
                "every coord {through} turns, climbs or branches — {straight}, so none can stand there"
            ),
        ),
        (
            Blocked::NoneBetween,
            touches,
            dust,
            format!("every coord {through} {on_a_face} — {cuts}, so none can stand there"),
        ),
        (
            Blocked::NoneBetween,
            both,
            dust,
            format!(
                "every coord {through} turns, climbs or branches, or {on_a_face} — {straight} and joins only the blocks at its back and front, so none can stand there"
            ),
        ),
        (
            Blocked::NoneCloseEnough,
            bends,
            (Allowance::Budget(1), 1),
            format!(
                "every coord from it back to 1 block before it turns, climbs or branches — {straight}, {too_weak}"
            ),
        ),
        (
            Blocked::NoneCloseEnough,
            touches,
            (Allowance::Budget(2), 2),
            format!(
                "every coord from it back to 2 blocks before it {on_a_face} — {cuts}, {too_weak}"
            ),
        ),
        (
            Blocked::NoneCloseEnough,
            both,
            dust,
            format!(
                "every coord from it back to 15 blocks before it turns, climbs or branches, or {on_a_face} — {straight} and joins only the blocks at its back and front, {too_weak}"
            ),
        ),
    ];
    for (blocked, refused, held, clause) in cases {
        assert_eq!(
            why_from_origin(blocked, true, refused, held),
            clause,
            "{blocked:?} x {refused:?}",
        );
    }
}

#[test]
fn repeater_sites_builds_two_pairs_of_allowance_and_start() {
    let mut met = BTreeSet::new();
    for run in [0, 1, 2, 5, 15, 16, 17] {
        for stairs in 1..=10 {
            let mut moves = vec![(EAST, run)];
            for _ in 0..stairs {
                moves.push((SOUTH, 1));
                moves.push((EAST, 1));
            }
            let path = walk((0, 0, 0), &moves);
            let tree = NetTree::from_paths(&[&path]);
            let sink = *path.last().expect("a walk has an end");
            for spent in 0..=DUST_ATTENUATION_LIMIT {
                for budget in [None, Some(1), Some(2), Some(3), Some(8), Some(15)] {
                    let budgets: HashMap<CellCoord, u32> =
                        budget.map(|budget| (sink, budget)).into_iter().collect();
                    let Err(stretch) = repeater_sites(&tree, spent, &budgets) else {
                        continue;
                    };
                    let case = format!("run {run}, {stairs} stairs, {spent} spent, {budget:?}");
                    let (held, reach) = match stretch.allowance {
                        Allowance::Limit => ("limit", DUST_ATTENUATION_LIMIT),
                        Allowance::Budget(budget) => ("budget", budget),
                    };
                    assert_eq!(stretch.reach, reach, "{case}: {stretch:?}");
                    let ended = match stretch.blocked {
                        Blocked::NoneBetween => "none between",
                        Blocked::NoneCloseEnough => "none close enough",
                    };
                    met.insert((held, stretch.starts_on_unpowered, ended));
                }
            }
        }
    }
    assert_eq!(
        met,
        BTreeSet::from([
            ("budget", false, "none between"),
            ("budget", false, "none close enough"),
            ("limit", true, "none between"),
        ]),
    );
}

/// One block more spent than the limit: from the limit, a sink one step on still reads the dust.
#[test]
fn a_stretch_with_no_coord_in_it_says_so() {
    let path = walk((0, 0, 0), &[(EAST, 1)]);
    let tree = NetTree::from_paths(&[&path]);
    let stretch = repeater_sites(&tree, DUST_ATTENUATION_LIMIT + 1, &HashMap::new());
    assert_eq!(
        stretch,
        Err(NoRepeaterSite {
            from: CellCoord::new(0, 0, 0),
            spent: DUST_ATTENUATION_LIMIT + 1,
            unpowered: CellCoord::new(1, 0, 0),
            allowance: Allowance::Limit,
            reach: DUST_ATTENUATION_LIMIT + 1,
            starts_on_unpowered: false,
            blocked: Blocked::NoneBetween,
            refused: Refused::default(),
        }),
    );

    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(3, 3, 1));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.outputs
        .push(routed_output(NetRef::Input(0), CellCoord::new(1, 0, 0)));
    let entry = ScopedPlacementIrEntry {
        kind: ScopeKind::Struct,
        name: "spent".to_owned(),
        ir: ir.clone(),
    };
    let region = ir.region.clone().expect("set above");
    let diagnostic = no_repeater_site_diagnostic(
        &ir,
        &entry,
        "routed",
        &region,
        NetRef::Input(0),
        &tree,
        stretch.expect_err("refused above"),
    );
    assert!(
        diagnostic.primary.ends_with(
            ": no coord stands between the two, so there is nowhere for a buffer repeater"
        ),
        "{:?}",
        diagnostic.primary,
    );
}

/// A repeater on `unpowered` would read a block the signal no longer lights. Hand-built: the
/// budget search never hands a walk more than the limit.
#[test]
fn dust_whose_block_before_is_past_the_limit_is_walked_back_from() {
    let path = walk((0, 0, 0), &[(EAST, 6)]);
    let tree = NetTree::from_paths(&[&path]);
    assert_eq!(
        repeater_sites(&tree, DUST_ATTENUATION_LIMIT + 1, &HashMap::new()),
        Err(NoRepeaterSite {
            from: CellCoord::new(0, 0, 0),
            spent: DUST_ATTENUATION_LIMIT + 1,
            unpowered: CellCoord::new(1, 0, 0),
            allowance: Allowance::Limit,
            reach: DUST_ATTENUATION_LIMIT,
            starts_on_unpowered: false,
            blocked: Blocked::NoneBetween,
            refused: Refused::default(),
        }),
    );
}

#[test]
fn a_staircase_past_a_repeater_is_measured_from_that_repeater() {
    let mut moves = vec![(EAST, 20)];
    for _ in 0..10 {
        moves.push((SOUTH, 1));
        moves.push((EAST, 1));
    }
    let path = walk((0, 0, 0), &moves);
    let tree = NetTree::from_paths(&[&path]);
    // Repeaters at (16,0,0) and (19,0,0), the last straight coord before the staircase.
    assert_eq!(
        repeater_sites(&tree, 0, &HashMap::new()),
        Err(NoRepeaterSite {
            from: CellCoord::new(19, 0, 0),
            spent: 0,
            unpowered: path[19 + 16],
            allowance: Allowance::Limit,
            reach: DUST_ATTENUATION_LIMIT,
            starts_on_unpowered: true,
            blocked: Blocked::NoneBetween,
            refused: Refused {
                bends: true,
                touching: None,
            },
        }),
    );
}

#[test]
fn dust_spent_at_the_source_brings_the_first_repeater_forward() {
    let tree = NetTree::from_paths(&[&walk((0, 0, 0), &[(EAST, 20)])]);
    assert_eq!(
        repeater_sites(&tree, 5, &HashMap::new()),
        Ok(vec![CellCoord::new(11, 0, 0)]),
        "5 spent, so 10 more blocks of dust carry the signal to a repeater in place of the 11th",
    );
}

#[test]
fn a_sink_with_a_budget_takes_a_repeater_within_it() {
    let tree = NetTree::from_paths(&[&walk((0, 0, 0), &[(EAST, 10)])]);
    assert_eq!(repeater_sites(&tree, 0, &HashMap::new()), Ok(Vec::new()));
    let budgets = HashMap::from([(CellCoord::new(10, 0, 0), 4)]);
    assert_eq!(
        repeater_sites(&tree, 0, &budgets),
        Ok(vec![CellCoord::new(9, 0, 0)]),
        "10 blocks is under the limit and over the sink's 4; the repeater stands on the last straight coord",
    );
}

#[test]
fn a_budget_no_straight_coord_can_meet_is_refused() {
    // The coord before the sink is a corner, and the straight one before that is 2 blocks out.
    let path = walk((0, 0, 0), &[(EAST, 3), (SOUTH, 1)]);
    let tree = NetTree::from_paths(&[&path]);
    let sink = CellCoord::new(3, 0, 1);
    let budgets = HashMap::from([(sink, 1)]);
    let stretch = repeater_sites(&tree, 0, &budgets);
    assert_eq!(
        stretch,
        Err(NoRepeaterSite {
            from: CellCoord::new(0, 0, 0),
            spent: 0,
            unpowered: sink,
            allowance: Allowance::Budget(1),
            reach: 1,
            starts_on_unpowered: false,
            blocked: Blocked::NoneCloseEnough,
            refused: Refused {
                bends: true,
                touching: None,
            },
        }),
    );

    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(6, 3, 1));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.cells.push(placed_cell(
        EditionCell::JavaComparatorAnd,
        sink,
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        ROUTED,
    ));
    let entry = ScopedPlacementIrEntry {
        kind: ScopeKind::Struct,
        name: "near".to_owned(),
        ir: ir.clone(),
    };
    let region = ir.region.clone().expect("set above");
    let diagnostic = no_repeater_site_diagnostic(
        &ir,
        &entry,
        "routed",
        &region,
        NetRef::Input(0),
        &tree,
        stretch.expect_err("refused above"),
    );
    assert_eq!(
        diagnostic.primary,
        "routed netlist for struct `near` routes sig.a so that the signal leaving (0,0,0) reaches cell #0 at (3,0,1) over more than the 1 block of dust it can spare — cell #0 passes on the strength it receives rather than restoring it, and the wire past it needs the rest: every coord within 1 block before it turns, climbs or branches — a buffer repeater carries a signal only where the wire runs straight through it at one height, and one further back would leave too little strength to get there",
    );
    assert_eq!(
        diagnostic.notes[0].message,
        "Fix: leave the wire into cell #0 room to run straight within 1 block of it, or shorten the wire out of it so it can spare more — a larger `region=` is one way to do either — or drive cell #0 from a cell that restores strength, or split the logic across several scopes, each with its own `circuit` line",
    );
}

/// At 16 steps the budget would need nothing spent, which no cell ever is.
#[test]
fn a_cell_whose_own_wire_has_no_room_is_refused_on_its_own_net() {
    let cell = CellCoord::new(0, 0, 0);
    for steps in [17, 16] {
        let stairs: Vec<_> = (0..steps)
            .map(|step| (if step % 2 == 0 { EAST } else { SOUTH }, 1))
            .collect();
        let out = walk((0, 0, 0), &stairs);
        let sink = *out.last().expect("a staircase has an end");
        let into = walk((0, 0, 1), &[((0, 0, -1), 1)]);
        let trees = HashMap::from([
            (NetRef::Input(0), NetTree::from_paths(&[&into])),
            (NetRef::Cell(0), NetTree::from_paths(&[&out])),
        ]);
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(20, 20, 1));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            cell,
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
            ROUTED,
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            sink,
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Cell(0),
            }],
            ROUTED,
        ));
        let entry = ScopedPlacementIrEntry {
            kind: ScopeKind::Struct,
            name: "cramped".to_owned(),
            ir: ir.clone(),
        };
        let region = ir.region.clone().expect("set above");
        let Err(refusal) = repeater_sites_of_scope(&ir, &entry, "routed", &region, &trees) else {
            panic!("{steps}-step staircase: the comparator's own wire has no room");
        };
        assert_eq!(refusal.code, DiagnosticCode::AttenuationLimit);
        assert!(
            refusal.primary.starts_with(
                "routed netlist for struct `cramped` routes cell #0 so that the signal leaving (0,0,0), with 1 block of dust already spent, runs out before",
            ),
            "{steps}-step staircase: names the comparator's own net, got {:?}",
            refusal.primary,
        );
        assert!(
            refusal.primary.ends_with(
                ": every coord after (0,0,0) up to and including (8,0,7) turns, climbs or branches — a buffer repeater carries a signal only where the wire runs straight through it at one height, so none can stand there",
            ),
            "{steps}-step staircase: {:?}",
            refusal.primary,
        );
    }
}

/// The staircase out can take 8 blocks spent, so the repeater stands 1 block before the cell.
#[test]
fn a_cell_that_passes_strength_on_sends_its_repeater_upstream() {
    let cell = CellCoord::new(10, 0, 0);
    let pad = CellCoord::new(14, 0, 4);
    let mut stairs = Vec::new();
    for _ in 0..4 {
        stairs.push((EAST, 1));
        stairs.push((SOUTH, 1));
    }
    let into = walk((0, 0, 0), &[(EAST, 10)]);
    let out = walk((10, 0, 0), &stairs);
    assert_eq!(out.last(), Some(&pad));
    let trees = HashMap::from([
        (NetRef::Input(0), NetTree::from_paths(&[&into])),
        (NetRef::Cell(0), NetTree::from_paths(&[&out])),
    ]);
    for (kind, expected) in [
        (EditionCell::BedrockTorchOr, vec![CellCoord::new(9, 0, 0)]),
        (EditionCell::BedrockInverterTorch, Vec::new()),
    ] {
        let mut ir = PlacementIr::new(Edition::Bedrock);
        ir.region = Some(reservation(20, 6, 1));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            kind,
            cell,
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
            ROUTED,
        ));
        ir.outputs.push(routed_output(NetRef::Cell(0), pad));
        let entry = ScopedPlacementIrEntry {
            kind: ScopeKind::Struct,
            name: "upstream".to_owned(),
            ir: ir.clone(),
        };
        let region = ir.region.clone().expect("set above");
        let sites = repeater_sites_of_scope(&ir, &entry, "routed", &region, &trees)
            .unwrap_or_else(|d| panic!("{kind:?}: {d:?}"));
        assert_eq!(
            sites.along(NetRef::Input(0), cell),
            Some(expected),
            "{kind:?}: the wire into the cell",
        );
        assert_eq!(
            sites.along(NetRef::Cell(0), pad),
            Some(Vec::new()),
            "{kind:?}: the staircase out of it",
        );
    }
}

/// Hand-built: no `.crn` keeps a cell that drives nothing.
#[test]
fn a_cell_that_passes_strength_on_and_drives_nothing_is_held_to_the_limit() {
    let cell = CellCoord::new(16, 0, 0);
    let into = walk((0, 0, 0), &[(EAST, 16)]);
    assert_eq!(into.last(), Some(&cell));
    let trees = HashMap::from([(NetRef::Input(0), NetTree::from_paths(&[&into]))]);
    for (kind, expected) in [
        (EditionCell::BedrockTorchOr, vec![CellCoord::new(15, 0, 0)]),
        (EditionCell::BedrockInverterTorch, Vec::new()),
    ] {
        let mut ir = PlacementIr::new(Edition::Bedrock);
        ir.region = Some(reservation(20, 3, 1));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            kind,
            cell,
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
            ROUTED,
        ));
        let entry = ScopedPlacementIrEntry {
            kind: ScopeKind::Struct,
            name: "unused".to_owned(),
            ir: ir.clone(),
        };
        let region = ir.region.clone().expect("set above");
        let sites = repeater_sites_of_scope(&ir, &entry, "routed", &region, &trees)
            .unwrap_or_else(|d| panic!("{kind:?}: {d:?}"));
        assert_eq!(
            sites.along(NetRef::Input(0), cell),
            Some(expected),
            "{kind:?}: the wire into a cell that drives nothing",
        );
    }
}
