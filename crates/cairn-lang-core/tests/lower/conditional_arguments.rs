//! `MemberRole::conditional_arguments` holds against the lowering: a key an arm reads moves the
//! build, and a key it ignores does not.

use std::collections::BTreeSet;

use cairn_lang_core::intent::{SelectorAxis, known_keywords, role_of};
use cairn_lang_core::{BlockArrayIr, Diagnostic, check, lower, parse};

use crate::common::{lowered, solid_cells};

/// Where a fixture's conditional argument goes.
const MARKER: &str = "%CONDITIONAL%";

/// One fixture per (keyword, selector arm, conditional key) the table names.
struct Pair {
    keyword: &'static str,
    /// The selector's identifier, or `None` for the arm an absent selector reaches.
    selector_value: Option<&'static str>,
    key: &'static str,
    /// What the fixture's body needs above it.
    prologue: &'static str,
    /// The body, with [`MARKER`] where the conditional argument goes.
    body: &'static str,
    /// Two valid values a reader would tell apart in the blocks.
    values: (&'static str, &'static str),
}

const GEOMETRY: &str =
    "theme t:\n  slot wall -> @cobblestone\n  slot roof -> @spruce_stairs\n\nstruct s size=9x9\n";

/// `place` needs something to instantiate and a site to sit in.
const SITE: &str = "def hut size=3x3:\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n\ntheme t:\n  slot wall -> @cobblestone\n\nsite v:\n";

const ROOF_BODY: &str =
    "  walls mat_slot=wall height=3\n  roof kind=%KIND% mat_slot=roof overhang=1 %CONDITIONAL%\n";

/// An eave band, with the walls and overhanging roof a `stair` needs.
const STAIR_BODY: &str = "  walls mat_slot=wall height=3\n  roof kind=gable mat_slot=roof overhang=1\n  stair kind=stairs mat_slot=roof side=front %CONDITIONAL%\n";

/// The same, without the `side=` the other stair fixtures carry.
const STAIR_BODY_NO_SIDE: &str = "  walls mat_slot=wall height=3\n  roof kind=gable mat_slot=roof overhang=1\n  stair kind=stairs mat_slot=roof %CONDITIONAL%\n";

/// The first row of a site places at the origin, where `gap=` is not read.
const PLACE_AT_ORIGIN: &str = "  place id=a use=hut theme=t at=origin %CONDITIONAL%\n";

const PLACE_RELATIVE: &str = "  place id=a use=hut theme=t at=origin\n  place id=b use=hut theme=t east_of=a %CONDITIONAL%\n";

const PAIRS: &[Pair] = &[
    Pair {
        keyword: "roof",
        selector_value: Some("gable"),
        key: "slope_to",
        prologue: GEOMETRY,
        body: ROOF_BODY,
        values: ("slope_to=front", "slope_to=back"),
    },
    Pair {
        keyword: "roof",
        selector_value: Some("shed"),
        key: "slope_to",
        prologue: GEOMETRY,
        body: ROOF_BODY,
        values: ("slope_to=front", "slope_to=back"),
    },
    Pair {
        keyword: "roof",
        selector_value: Some("hip"),
        key: "slope_to",
        prologue: GEOMETRY,
        body: ROOF_BODY,
        values: ("slope_to=front", "slope_to=back"),
    },
    Pair {
        keyword: "roof",
        selector_value: Some("flat"),
        key: "slope_to",
        prologue: GEOMETRY,
        body: ROOF_BODY,
        values: ("slope_to=front", "slope_to=back"),
    },
    Pair {
        keyword: "stair",
        selector_value: Some("stairs"),
        key: "side",
        prologue: GEOMETRY,
        body: STAIR_BODY_NO_SIDE,
        values: ("side=front", "side=back"),
    },
    Pair {
        keyword: "stair",
        selector_value: Some("stairs"),
        key: "half",
        prologue: GEOMETRY,
        body: STAIR_BODY,
        values: ("half=top", "half=bottom"),
    },
    Pair {
        keyword: "stair",
        selector_value: Some("stairs"),
        key: "facing",
        prologue: GEOMETRY,
        body: STAIR_BODY,
        values: ("facing=out", "facing=in"),
    },
    Pair {
        keyword: "stair",
        selector_value: Some("stairs"),
        key: "shape",
        prologue: GEOMETRY,
        body: STAIR_BODY,
        values: ("shape=straight", "shape=outer_left"),
    },
    Pair {
        keyword: "stair",
        selector_value: Some("stairs"),
        key: "y",
        prologue: GEOMETRY,
        body: STAIR_BODY,
        values: ("y=0", "y=2"),
    },
    Pair {
        keyword: "place",
        selector_value: Some("origin"),
        key: "gap",
        prologue: SITE,
        body: PLACE_AT_ORIGIN,
        values: ("gap=1", "gap=5"),
    },
    Pair {
        keyword: "place",
        selector_value: None,
        key: "gap",
        prologue: SITE,
        body: PLACE_RELATIVE,
        values: ("gap=1", "gap=5"),
    },
];

impl Pair {
    /// This fixture's source, written with one of the two values.
    fn source(&self, value: &str) -> String {
        let body = self
            .body
            .replace("%KIND%", self.selector_value.unwrap_or_default())
            .replace(MARKER, value);
        format!("{}{body}", self.prologue)
    }

    /// The source without the member under test: the baseline for "this rule paints something".
    fn source_without_member(&self) -> String {
        let written = self
            .body
            .replace("%KIND%", self.selector_value.unwrap_or_default());
        let kept: Vec<&str> = written
            .lines()
            .filter(|line| !line.contains(MARKER))
            .collect();
        format!("{}{}\n", self.prologue, kept.join("\n"))
    }

    /// The axis this fixture's key is conditional on, read from the table.
    fn axis(&self) -> &'static SelectorAxis {
        role_of(self.keyword)
            .conditional_arguments()
            .iter()
            .find(|axis| axis.is_conditional(self.key))
            .unwrap_or_else(|| {
                panic!(
                    "`{}` has no conditional axis carrying `{}`",
                    self.keyword, self.key,
                )
            })
    }

    /// Whether the table says the rule this fixture selects reads the key.
    fn table_says_read(&self) -> bool {
        self.axis()
            .arm(self.selector_value)
            .is_some_and(|arm| arm.reads.contains(&self.key))
    }

    /// The triple this fixture stands for, as the coverage test spells it.
    fn triple(&self) -> String {
        format!(
            "{} {} {}",
            self.keyword,
            written(self.axis().selector, self.selector_value),
            self.key,
        )
    }
}

/// An arm as a failure names it: `kind=shed`, or `no at=` for an absent selector.
fn written(selector: &str, value: Option<&str>) -> String {
    value.map_or_else(
        || format!("no {selector}="),
        |value| format!("{selector}={value}"),
    )
}

/// Whether two builds match in blocks and placements; diagnostics are skipped, since spans move.
fn same_build(left: &BlockArrayIr, right: &BlockArrayIr) -> bool {
    left.structures == right.structures && left.placements == right.placements
}

/// A rendering of the build small enough to read in a failure.
fn shape(ir: &BlockArrayIr) -> String {
    let mut out = Vec::new();
    for (key, ba) in &ir.structures {
        let palette: Vec<String> = ba
            .palette
            .entries
            .iter()
            .map(|e| format!("{}[{}]", e.id, e.properties_text()))
            .collect();
        out.push(format!(
            "{key}: dims={}x{}x{} painted={} palette={}",
            ba.dims.x,
            ba.dims.y,
            ba.dims.z,
            painted(ir),
            palette.join(" "),
        ));
    }
    for (key, placement) in &ir.placements {
        out.push(format!("{key}: origin={:?}", placement.origin));
    }
    out.join("\n")
}

/// Voxels the build actually paints, across every structure.
fn painted(ir: &BlockArrayIr) -> usize {
    ir.structures.values().map(solid_cells).sum()
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn conditional_arguments_match_the_lowering() {
    for pair in PAIRS {
        let (one, other) = pair.values;
        let first = lowered(&pair.source(one));
        let second = lowered(&pair.source(other));
        let without = lowered(&pair.source_without_member());
        for (value, ir) in [(one, &first), (other, &second)] {
            assert!(
                painted(ir) > painted(&without),
                "`{}` with `{value}` paints no more than the same body without the member, so \
                 the comparison has no evidence in it:\n{}",
                pair.triple(),
                shape(ir),
            );
            assert_eq!(
                codes(&ir.diagnostics),
                Vec::<&str>::new(),
                "`{}` with `{value}` does not lower cleanly, so the two builds differ for a \
                 reason that is not the key",
                pair.triple(),
            );
        }
        let same = same_build(&first, &second);
        if pair.table_says_read() {
            assert!(
                !same,
                "`{}` is listed as read and `{one}` builds what `{other}` builds:\n{}",
                pair.triple(),
                shape(&first),
            );
        } else {
            assert!(
                same,
                "`{}` is listed as unread by this rule and the build moves between `{one}` and \
                 `{other}`:\nfirst:\n{}\nsecond:\n{}",
                pair.triple(),
                shape(&first),
                shape(&second),
            );
        }
    }
}

#[test]
fn every_conditional_pair_in_the_table_has_a_fixture() {
    let mut from_table: Vec<String> = Vec::new();
    for keyword in known_keywords() {
        for axis in role_of(keyword).conditional_arguments() {
            // The axis's conditional set, not the arm's: an arm not reading a key is the case here.
            let conditional: BTreeSet<&str> = axis
                .arms
                .iter()
                .flat_map(|arm| arm.reads.iter().copied())
                .collect();
            for arm in axis.arms {
                for key in &conditional {
                    from_table.push(format!(
                        "{keyword} {} {key}",
                        written(axis.selector, arm.value.ident()),
                    ));
                }
            }
        }
    }
    from_table.sort_unstable();

    let mut from_fixtures: Vec<String> = PAIRS.iter().map(Pair::triple).collect();
    from_fixtures.sort_unstable();

    assert_eq!(
        from_fixtures, from_table,
        "the fixtures and the conditional table describe different pairs",
    );
}

#[test]
fn the_pass_reports_exactly_the_pairs_the_build_drops() {
    for pair in PAIRS {
        let source = pair.source(pair.values.0);
        let module = parse(&source).expect("fixture parses");
        let ir = lower(&module);
        let found = check(&module, &ir, None);
        if pair.table_says_read() {
            assert_eq!(
                codes(&found),
                Vec::<&str>::new(),
                "`{}` is read, so the source is clean:\n{source}",
                pair.triple(),
            );
        } else {
            assert_eq!(
                codes(&found),
                ["W_IGNORED_ARGUMENT"],
                "`{}` is dropped by this rule:\n{source}",
                pair.triple(),
            );
            assert!(
                found[0].primary.contains(&format!("`{}=`", pair.key)),
                "got: {}",
                found[0].primary,
            );
        }
    }
}

/// `kind=` is a word no roof kind could take, so a new kind cannot change the fixture.
#[test]
fn the_member_that_does_not_lower_is_billed_once() {
    for kind in ["", "kind=not_a_roof_kind ", "kind=\"shed\" ", "kind=2 "] {
        let source = format!(
            "{GEOMETRY}  walls mat_slot=wall height=3\n  roof {kind}mat_slot=roof slope_to=front\n",
        );
        let out = lowered(&source);
        assert_eq!(
            codes(&out.diagnostics),
            ["E_INVALID_MEMBER"],
            "source:\n{source}",
        );
        let module = parse(&source).expect("fixture parses");
        let ir = lower(&module);
        assert_eq!(
            codes(&check(&module, &ir, None)),
            Vec::<&str>::new(),
            "the refusal carries the repair, so the check pass says nothing:\n{source}",
        );
    }
}

/// `stair` is built for `kind=stairs` alone, so its conditional keys can never fire.
#[test]
fn a_one_arm_axis_reports_nothing() {
    let conditional = role_of("stair").conditional_arguments();
    let [axis] = conditional else {
        panic!("`stair` dispatches on one selector, got {conditional:?}");
    };
    assert_eq!(axis.arms.len(), 1, "one arm is the shape under test");
    assert!(axis.is_conditional("half"), "and its keys are conditional");

    for key in axis.arms[0].reads {
        let source = format!(
            "{GEOMETRY}  walls mat_slot=wall height=3\n  roof kind=gable mat_slot=roof overhang=1\n  stair kind=spiral mat_slot=roof {key}=front\n",
        );
        let module = parse(&source).expect("fixture parses");
        let ir = lower(&module);
        let raised = check(&module, &ir, None);
        let found = codes(&raised);
        assert!(
            !found.contains(&"W_IGNORED_ARGUMENT"),
            "`stair kind=spiral` names no rule, so `{key}=` is the deferral's to carry, got \
             {found:?}:\n{source}",
        );
    }
}

#[test]
fn a_place_at_the_origin_is_told_its_gap_is_ignored() {
    let ignored = format!("{SITE}  place id=a use=hut theme=t at=origin gap=5\n");
    let module = parse(&ignored).expect("fixture parses");
    let ir = lower(&module);
    let found = check(&module, &ir, None);
    assert_eq!(codes(&found), ["W_IGNORED_ARGUMENT"], "source:\n{ignored}");
    assert!(
        found[0].primary.contains("`gap=`"),
        "got: {}",
        found[0].primary
    );
    assert!(
        found[0].primary.contains("no `at=` at all"),
        "the alternative is a way of writing the line, not a value: {}",
        found[0].primary,
    );
    assert!(
        found[0].notes[0].message.contains("drop the `at=`"),
        "got: {}",
        found[0].notes[0].message,
    );

    // And the rule that does read it stays clean.
    let read = format!(
        "{SITE}  place id=a use=hut theme=t at=origin\n  place id=b use=hut theme=t east_of=a gap=5\n",
    );
    let module = parse(&read).expect("fixture parses");
    let ir = lower(&module);
    assert_eq!(
        codes(&check(&module, &ir, None)),
        Vec::<&str>::new(),
        "source:\n{read}",
    );
}
