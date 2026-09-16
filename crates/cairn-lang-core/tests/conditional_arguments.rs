//! The conditional-argument table against the lowering rules it describes.
//!
//! `MemberRole::conditional_arguments` is a claim about other code: that
//! `fill_roof`'s `shed` arm consults `slope_to=` and its three siblings do
//! not. Nothing in the table can check that claim, and a table that drifts
//! from the dispatch is worse than no table — it either reports a key the
//! build does read, or stays quiet about one it drops.
//!
//! So the claim is tested the way an author would notice it: write the
//! argument at a value that changes the result, build the source twice —
//! once with it and once without — and compare the voxels. An arm the table
//! says reads the key has to build something different. An arm it says does
//! not has to build the same blocks, in the same palette, at the same dims.
//!
//! Every source here is one line per member. A `\` continuation inside a
//! literal keeps the next line's indentation, and the lexer reads that as
//! the member's indent.

use std::collections::BTreeSet;

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::intent::{SelectorAxis, known_keywords, role_of};
use cairn_lang_core::{BlockArrayIr, Diagnostic, check, lower, parse, resolve};

/// Where a fixture's conditional argument goes, and what is removed with it.
const MARKER: &str = "%CONDITIONAL%";

/// One fixture per (keyword, selector value, conditional key) the table
/// names.
struct Pair {
    /// The keyword the pair sits on.
    keyword: &'static str,
    /// The selector value this fixture writes.
    selector_value: &'static str,
    /// The conditional key under test.
    key: &'static str,
    /// The struct body, with [`MARKER`] where the conditional argument goes.
    body: &'static str,
    /// The argument as the source writes it, at a value whose absence the
    /// build shows — a default written back is a fixture that proves
    /// nothing.
    argument: &'static str,
}

const ROOF_BODY: &str =
    "  walls mat_slot=wall height=3\n  roof kind=%KIND% mat_slot=roof overhang=1 %CONDITIONAL%\n";

/// A `stair` is an eave band, so it needs walls to sit beside and a roof
/// that draws an overhang for it to sit in.
const STAIR_BODY: &str = "  walls mat_slot=wall height=3\n  roof kind=gable mat_slot=roof overhang=1\n  stair kind=stairs mat_slot=roof side=front %CONDITIONAL%\n";

/// The same, without the `side=` the other stair fixtures carry.
const STAIR_BODY_NO_SIDE: &str = "  walls mat_slot=wall height=3\n  roof kind=gable mat_slot=roof overhang=1\n  stair kind=stairs mat_slot=roof %CONDITIONAL%\n";

const PAIRS: &[Pair] = &[
    Pair {
        keyword: "roof",
        selector_value: "gable",
        key: "slope_to",
        body: ROOF_BODY,
        argument: "slope_to=front",
    },
    Pair {
        keyword: "roof",
        selector_value: "shed",
        key: "slope_to",
        body: ROOF_BODY,
        argument: "slope_to=front",
    },
    Pair {
        keyword: "roof",
        selector_value: "hip",
        key: "slope_to",
        body: ROOF_BODY,
        argument: "slope_to=front",
    },
    Pair {
        keyword: "roof",
        selector_value: "flat",
        key: "slope_to",
        body: ROOF_BODY,
        argument: "slope_to=front",
    },
    Pair {
        keyword: "stair",
        selector_value: "stairs",
        key: "side",
        body: STAIR_BODY_NO_SIDE,
        argument: "side=front",
    },
    Pair {
        keyword: "stair",
        selector_value: "stairs",
        key: "half",
        body: STAIR_BODY,
        // `top` is the default, so the fixture writes the other one.
        argument: "half=bottom",
    },
    Pair {
        keyword: "stair",
        selector_value: "stairs",
        key: "facing",
        body: STAIR_BODY,
        argument: "facing=in",
    },
    Pair {
        keyword: "stair",
        selector_value: "stairs",
        key: "shape",
        body: STAIR_BODY,
        argument: "shape=outer_left",
    },
    Pair {
        keyword: "stair",
        selector_value: "stairs",
        key: "y",
        body: STAIR_BODY,
        argument: "y=2",
    },
];

const PROLOGUE: &str =
    "theme t:\n  slot wall -> @cobblestone\n  slot roof -> @spruce_stairs\n\nstruct s size=9x9\n";

impl Pair {
    /// This fixture's source, with the conditional argument or without it.
    fn source(&self, carried: bool) -> String {
        let body = self.body.replace("%KIND%", self.selector_value);
        let body = if carried {
            body.replace(MARKER, self.argument)
        } else {
            body.replace(&format!(" {MARKER}"), "")
        };
        format!("{PROLOGUE}{body}")
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
            "{} {}={} {}",
            self.keyword,
            self.axis().selector,
            self.selector_value,
            self.key,
        )
    }
}

fn built(source: &str) -> BlockArrayIr {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, None)
}

/// A rendering of the build small enough to read in a failure and specific
/// enough to say how two builds differ.
fn shape(ir: &BlockArrayIr) -> String {
    let mut out = Vec::new();
    for (key, ba) in &ir.structures {
        let painted = ba.voxels.iter().filter(|i| i.0 != 0).count();
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
            "{key}: dims={}x{}x{} painted={painted} palette={}",
            ba.dims.x,
            ba.dims.y,
            ba.dims.z,
            palette.join(" "),
        ));
    }
    out.join("\n")
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics.iter().map(|d| d.code.as_str()).collect()
}

/// The table's claim, made observable: the argument's presence moves the
/// build exactly where the table says a rule reads it.
///
/// Both directions matter and they fail differently. An arm that reads a key
/// the table omits means the pass reports a value the build uses, which is
/// advice to delete working source. An arm the table says reads a key and
/// does not means the silence the whole finding exists to end.
#[test]
fn conditional_arguments_match_the_lowering() {
    for pair in PAIRS {
        let carried = built(&pair.source(true));
        let dropped = built(&pair.source(false));
        let same = carried.structures == dropped.structures;
        if pair.table_says_read() {
            assert!(
                !same,
                "`{}` is listed as read and the build is the same without it:\n{}",
                pair.triple(),
                shape(&carried),
            );
        } else {
            assert!(
                same,
                "`{}` is listed as unread by this rule and the build moves without it:\nwith:\n{}\nwithout:\n{}",
                pair.triple(),
                shape(&carried),
                shape(&dropped),
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
                    from_table.push(format!("{keyword} {}={} {key}", axis.selector, arm.value));
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
/// derivation of the same fact: it is the join between them, and a fixture
/// whose source has some *other* problem would show up here as a finding
/// nobody asked for.
#[test]
fn the_pass_reports_exactly_the_pairs_the_build_drops() {
    for pair in PAIRS {
        let source = pair.source(true);
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
#[test]
fn the_member_that_does_not_lower_is_billed_once() {
    for kind in ["", "kind=dome "] {
        let source = format!(
            "{PROLOGUE}  walls mat_slot=wall height=3\n  roof {kind}mat_slot=roof slope_to=front\n"
        );
        let out = built(&source);
        assert_eq!(
            codes(&out.diagnostics),
            ["W_DEFERRED_MEMBER"],
            "source:\n{source}",
        );
    }
}
