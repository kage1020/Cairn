//! A key `MemberRole::unread_arguments` calls unreached leaves the build as it is without the key,
//! where a key the same member reads moves it.

use cairn_lang_core::BlockArrayIr;
use cairn_lang_core::intent::{Unread, known_keywords, role_of};

use crate::common::{lowered, solid_cells};

/// Where a fixture's argument goes.
const MARKER: &str = "%ARGUMENT%";

const PROLOGUE: &str = "theme t:\n  slot wall -> @cobblestone\n  slot roof -> @spruce_stairs\n  slot glass -> @glass_pane\n\nstruct s size=9x9\n";

const WINDOW: &str = "  walls mat_slot=wall height=4\n  window side=front y=1 offset=2 size=2x2 mat_slot=glass %ARGUMENT%\n";

const ROOF: &str = "  walls mat_slot=wall height=3\n  roof kind=gable mat_slot=roof %ARGUMENT%\n";

/// An eave band, with the walls and overhanging roof a `stair` needs.
const STAIR: &str = "  walls mat_slot=wall height=3\n  roof kind=gable mat_slot=roof overhang=1\n  stair kind=stairs mat_slot=roof side=front %ARGUMENT%\n";

/// One fixture per unreached key, other than `mat_slot`, which `unread_mat_slot.rs` covers.
struct Fixture {
    keyword: &'static str,
    /// The unreached key, with the value the spec writes for it.
    unreached: &'static str,
    /// A key the member reads, which moves the build.
    read: &'static str,
    /// The body, with [`MARKER`] where the argument goes.
    body: &'static str,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        keyword: "window",
        unreached: "shape=arch",
        read: "sym=true",
        body: WINDOW,
    },
    Fixture {
        keyword: "window",
        unreached: "anchor=bottom_center",
        read: "sym=true",
        body: WINDOW,
    },
    Fixture {
        keyword: "roof",
        unreached: "footprint=struct",
        read: "overhang=1",
        body: ROOF,
    },
    Fixture {
        keyword: "roof",
        unreached: "bounds=expand",
        read: "overhang=1",
        body: ROOF,
    },
    // `false` is the state the stair is built in, so the fixture writes the other value.
    Fixture {
        keyword: "stair",
        unreached: "waterlogged=true",
        read: "half=bottom",
        body: STAIR,
    },
];

/// The fixture's body lowered with `argument` in place of the marker.
fn build(fixture: &Fixture, argument: &str) -> BlockArrayIr {
    let source = format!("{PROLOGUE}{}", fixture.body.replace(MARKER, argument));
    let ir = lowered(&source);
    assert_eq!(
        ir.diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
        Vec::<&str>::new(),
        "the fixture lowers cleanly, so the builds differ only by the argument:\n{source}",
    );
    let painted: usize = ir.structures.values().map(solid_cells).sum();
    assert!(painted > 0, "the fixture paints nothing:\n{source}");
    ir
}

/// Whether two builds match in blocks, placements and walkways; diagnostics carry spans.
fn same_build(left: &BlockArrayIr, right: &BlockArrayIr) -> bool {
    left.structures == right.structures
        && left.placements == right.placements
        && left.walkways == right.walkways
}

#[test]
fn an_unreached_key_leaves_the_build_as_it_is_without_it() {
    for fixture in FIXTURES {
        let without = build(fixture, "");
        assert!(
            same_build(&without, &build(fixture, fixture.unreached)),
            "`{} {}` is listed as unreached, and the build moves with it",
            fixture.keyword,
            fixture.unreached,
        );
        assert!(
            !same_build(&without, &build(fixture, fixture.read)),
            "`{} {}` builds what the line without it builds, so the fixture cannot tell a key \
             that is read from one that is not",
            fixture.keyword,
            fixture.read,
        );
    }
}

#[test]
fn every_unreached_key_in_the_table_is_built_here() {
    let mut from_table: Vec<(&str, &str)> = known_keywords()
        .iter()
        .flat_map(|keyword| {
            role_of(keyword)
                .unread_arguments()
                .iter()
                .filter(|entry| entry.why == Unread::Unreached && entry.key != "mat_slot")
                .map(move |entry| (*keyword, entry.key))
        })
        .collect();
    from_table.sort_unstable();
    let mut from_fixtures: Vec<(&str, &str)> = FIXTURES
        .iter()
        .map(|fixture| {
            let (key, _) = fixture
                .unreached
                .split_once('=')
                .expect("a fixture writes `key=value`");
            (fixture.keyword, key)
        })
        .collect();
    from_fixtures.sort_unstable();
    assert_eq!(from_fixtures, from_table);
}
