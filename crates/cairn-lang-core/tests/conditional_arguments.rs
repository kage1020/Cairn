//! The conditional-argument table against the lowering rules it describes.
//!
//! `MemberRole::conditional_arguments` is a claim about other code: that
//! `fill_roof`'s `shed` arm consults `slope_to=` and its three siblings do
//! not. Nothing in the table can check that claim, and a table that drifts
//! from the dispatch is worse than no table — it either reports a key the
//! build does read, or stays quiet about one it drops.
//!
//! So the claim is tested the way an author would notice it: write the
//! argument two ways that a reader would tell apart, build the source once
//! for each, and compare the blocks. An arm the table says reads the key
//! has to build something different. An arm it says does not has to build
//! the same blocks, in the same palette, at the same dims, in the same
//! place. Two values rather than presence and absence, because several of
//! these keys have no default: dropping `slope_to=` from a shed defers the
//! whole member, and a build that did not happen differs from one that did
//! for a reason that has nothing to do with reading the key.
//!
//! Every build is also required to paint something and to lower without a
//! diagnostic. Two empty builds compare equal, so without that a generator
//! that drew nothing at all would satisfy every "does not read it" row.
//!
//! One caveat this file cannot close from the table side: a selector value
//! the dispatch knows and the table has no arm for takes the "names no
//! rule" path and reports nothing.
//! `block_array::roof::tests::every_roof_kind_the_dispatch_knows_has_an_arm`
//! is the guard for that direction, and it lives there because it needs
//! `RoofKind`.
//!
//! Every source here is one line per member, spelled with explicit `\n`. A
//! `\` continuation discards the newline *and* the next line's leading
//! whitespace, so a member's indentation would never reach the lexer.

use std::collections::BTreeSet;

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::intent::{SelectorAxis, known_keywords, role_of};
use cairn_lang_core::{BlockArrayIr, Diagnostic, check, lower, parse, resolve};

/// Where a fixture's conditional argument goes.
const MARKER: &str = "%CONDITIONAL%";

/// One fixture per (keyword, selector arm, conditional key) the table
/// names.
struct Pair {
    /// The keyword the pair sits on.
    keyword: &'static str,
    /// How this fixture writes the selector: the identifier it carries, or
    /// `None` for the arm that answers to the selector being absent.
    selector_value: Option<&'static str>,
    /// The conditional key under test.
    key: &'static str,
    /// What the fixture's body needs above it.
    prologue: &'static str,
    /// The body, with [`MARKER`] where the conditional argument goes.
    body: &'static str,
    /// Two spellings of the argument whose difference a reader would show
    /// in the blocks. Both are valid values, so each build is a build.
    values: (&'static str, &'static str),
}

const GEOMETRY: &str =
    "theme t:\n  slot wall -> @cobblestone\n  slot roof -> @spruce_stairs\n\nstruct s size=9x9\n";

/// `place` needs something to instantiate and a site to sit in.
const SITE: &str = "def hut size=3x3:\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n\ntheme t:\n  slot wall -> @cobblestone\n\nsite v:\n";

const ROOF_BODY: &str =
    "  walls mat_slot=wall height=3\n  roof kind=%KIND% mat_slot=roof overhang=1 %CONDITIONAL%\n";

/// A `stair` is an eave band, so it needs walls to sit beside and a roof
/// that draws an overhang for it to sit in.
const STAIR_BODY: &str = "  walls mat_slot=wall height=3\n  roof kind=gable mat_slot=roof overhang=1\n  stair kind=stairs mat_slot=roof side=front %CONDITIONAL%\n";

/// The same, without the `side=` the other stair fixtures carry.
const STAIR_BODY_NO_SIDE: &str = "  walls mat_slot=wall height=3\n  roof kind=gable mat_slot=roof overhang=1\n  stair kind=stairs mat_slot=roof %CONDITIONAL%\n";

/// The first row of a site places at the origin; `gap=` belongs to the
/// rule that places relative to it.
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

    /// The same source with the member under test deleted — the line the
    /// conditional argument is written on.
    ///
    /// The baseline for "this rule paints something". Comparing the two
    /// builds against each other cannot see a generator that stopped
    /// drawing, because the rest of the body still paints and two builds
    /// that are equally empty of *this* member compare equal; comparing
    /// against the body without the member can.
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

    /// The axis this fixture's key is conditional on. The selector is read
    /// from the table rather than repeated in the fixture, so the two cannot
    /// disagree about which argument picks the rule.
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

/// How an arm is named in a failure or a coverage listing: `kind=shed`, or
/// `no at=` for the arm that answers to an absent selector.
fn written(selector: &str, value: Option<&str>) -> String {
    value.map_or_else(
        || format!("no {selector}="),
        |value| format!("{selector}={value}"),
    )
}

fn built(source: &str) -> BlockArrayIr {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, None)
}

/// Everything about a build that an argument could move: the blocks, and
/// where the structures carrying them were placed.
///
/// Compared instead of the whole [`BlockArrayIr`] because its diagnostics
/// carry spans, and the two sources differ in length by the width of a
/// value.
fn same_build(left: &BlockArrayIr, right: &BlockArrayIr) -> bool {
    left.structures == right.structures && left.placements == right.placements
}

/// A rendering of the build small enough to read in a failure and specific
/// enough to say how two builds differ.
fn shape(ir: &BlockArrayIr) -> String {
    let mut out = Vec::new();
    for (key, ba) in &ir.structures {
        let palette: Vec<String> = ba
            .palette
            .entries
            .iter()
            .map(|e| {
                let props: Vec<String> = e
                    .properties
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect();
                format!("{}[{}]", e.id, props.join(","))
            })
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
    ir.structures
        .values()
        .map(|ba| ba.voxels.iter().filter(|i| i.0 != 0).count())
        .sum()
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics.iter().map(|d| d.code.as_str()).collect()
}

/// The table's claim, made observable: the argument moves the build exactly
/// where the table says a rule reads it.
///
/// Both directions matter and they fail differently. An arm that reads a key
/// the table omits means the pass reports a value the build uses, which is
/// advice to delete working source. An arm the table says reads a key and
/// does not means the silence the whole finding exists to end.
#[test]
fn conditional_arguments_match_the_lowering() {
    for pair in PAIRS {
        let (one, other) = pair.values;
        let first = built(&pair.source(one));
        let second = built(&pair.source(other));
        // A member that paints nothing proves nothing: the two builds would
        // be equally empty of it and compare equal, so every "does not read
        // it" row would pass on a generator that had stopped drawing. The
        // rest of the body paints either way, so the baseline is the same
        // source with this member deleted.
        let without = built(&pair.source_without_member());
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

/// Every triple the table names has a fixture, and every fixture names a
/// triple the table has.
///
/// The guard on the guard. A new arm, or a key added to one, arrives here as
/// a missing fixture rather than as untested prose.
#[test]
fn every_conditional_pair_in_the_table_has_a_fixture() {
    let mut from_table: Vec<String> = Vec::new();
    for keyword in known_keywords() {
        for axis in role_of(keyword).conditional_arguments() {
            // The axis's conditional set, not the arm's: a key one arm reads
            // is a key every other arm has to be asked about, and the arm
            // that does not read it is exactly the case under test.
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

/// What the author is told matches what the build did.
///
/// The pass and the lowering read the same table, so this is not a second
/// derivation of the same fact: it is the join between them, and the two
/// halves are checked in different places. `check` runs no block-array
/// lowering, so a source that fails to *build* cannot surface here — that
/// is what the assertions in `conditional_arguments_match_the_lowering`
/// cover. What this pins is that every dropped pair is reported and no read
/// pair is.
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

/// A selector naming no rule is one repair and gets one finding.
///
/// The member lowers to nothing, so the deferral is the whole story and the
/// argument it routed past is not a second bill. `check` staying quiet is
/// asserted in `check_arguments.rs`; what is asserted here is the other half
/// — that the deferral it defers to is actually raised, so the case is
/// reported once rather than not at all.
///
/// The spellings matter. `kind=` is written as a word no roof kind could
/// take, so the fixture cannot quietly become a *known* kind the day the
/// dispatch grows one; and the two non-identifier shapes are here because
/// `routed_past` narrows on `ValueKind::Ident` and nothing else pinned what
/// the other shapes do.
#[test]
fn the_member_that_does_not_lower_is_billed_once() {
    for kind in ["", "kind=not_a_roof_kind ", "kind=\"shed\" ", "kind=2 "] {
        let source = format!(
            "{GEOMETRY}  walls mat_slot=wall height=3\n  roof {kind}mat_slot=roof slope_to=front\n",
        );
        let out = built(&source);
        assert_eq!(
            codes(&out.diagnostics),
            ["W_DEFERRED_MEMBER"],
            "source:\n{source}",
        );
        let module = parse(&source).expect("fixture parses");
        let ir = lower(&module);
        assert_eq!(
            codes(&check(&module, &ir, None)),
            Vec::<&str>::new(),
            "the deferral carries the repair, so the check pass says nothing:\n{source}",
        );
    }
}

/// An axis with one arm is a live configuration that reports nothing.
///
/// `stair` is that shape today: `fill_stair` accepts `kind=stairs` and
/// nothing else, so every key it reads is conditional on a selector no
/// other arm answers to. `is_conditional` therefore says `true` for all
/// five while the finding can never fire — any other `kind=` names no rule
/// and its deferral carries the line. Worth pinning rather than leaving to
/// be rediscovered: the row is written for the day a second stair kind
/// lands, and until then this is what it does.
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

/// Both halves of the `place` axis, spelled out.
///
/// The arm this axis needed that a `kind=` never does: the rule that reads
/// `gap=` is the one a row reaches by *not* writing `at=`. Worth its own
/// case beside the table-driven sweep, because it is the shape the sweep
/// would look the same without.
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
