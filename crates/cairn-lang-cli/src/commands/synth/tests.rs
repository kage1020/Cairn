//! Unit coverage for the argument-surface invariants the end-to-end
//! tests under `tests/cli/` can only assert circumstantially, by
//! hard-coding both sides of a pairing.

use clap::CommandFactory;

use crate::cli::Cli;

use super::*;

/// The four Placement IR stages spell their `--stage` value, the
/// `--stage <name>` fragment `require_edition` prints, and the
/// `"stage"` key of the JSON dump the same way. `stage_cli_name`
/// already derives the second from the third, but the first is
/// clap's own kebab-casing of the variant identifier, which no
/// type ties to either — renaming `SynthStage::Route` to
/// `Routing` would silently start accepting `--stage routing`
/// while the dump kept saying `route`. Reading the name back out
/// of `ValueEnum` is the only way to pin that.
#[test]
fn placement_stage_names_match_clap() {
    for (stage, placement) in [
        (SynthStage::Placement, PlacementStage::Placement),
        (SynthStage::Route, PlacementStage::Route),
        (SynthStage::Delay, PlacementStage::Delay),
        (SynthStage::Crossing, PlacementStage::Crossing),
    ] {
        let clap_name = stage
            .to_possible_value()
            .expect("no SynthStage variant is skipped");
        assert_eq!(clap_name.get_name(), placement.as_str());
        assert_eq!(stage_cli_name(stage), placement.as_str());
    }
}

/// The edition-neutral stages have no Placement IR counterpart,
/// so their spellings stay literals in `stage_cli_name` — pinned
/// here against clap for the same reason.
#[test]
fn edition_neutral_stage_names_match_clap() {
    for stage in [SynthStage::Logic, SynthStage::Netlist, SynthStage::Edition] {
        let clap_name = stage
            .to_possible_value()
            .expect("no SynthStage variant is skipped");
        assert_eq!(clap_name.get_name(), stage_cli_name(stage));
    }
}

/// `stage_requires_edition` decides two user-visible behaviours at
/// once — whether `--edition` is refused as stray and whether its
/// absence is a usage error — so the set it names is pinned here
/// against the `--stage` spellings a caller actually types. The
/// loop walks `ValueEnum`'s full variant list rather than a
/// hand-written one, so a stage added without a decision on its
/// edition-dependence fails here instead of quietly joining the
/// edition-neutral side.
#[test]
fn edition_required_stages_match_the_documented_set() {
    for stage in SynthStage::value_variants() {
        let name = stage
            .to_possible_value()
            .expect("no SynthStage variant is skipped");
        let expected = match name.get_name() {
            "logic" | "netlist" => false,
            "edition" | "placement" | "route" | "delay" | "crossing" => true,
            other => panic!("unclassified --stage value `{other}`"),
        };
        assert_eq!(
            stage_requires_edition(*stage),
            expected,
            "--stage {} edition-dependence",
            name.get_name(),
        );
    }
}

/// The stray-`--edition` message reads its two stage lists off
/// `stage_requires_edition`, which keeps them honest but puts the
/// English between them — the commas, the conjunction, the
/// backticks — under no other check. Pinned here so the derivation
/// cannot start producing a list that is correct and unreadable.
#[test]
fn stray_edition_message_lists_read_as_english() {
    assert_eq!(
        edition_required_stage_list(),
        "`--stage edition`, `--stage placement`, `--stage route`, \
         `--stage delay`, or `--stage crossing`",
    );
    assert_eq!(edition_neutral_stage_list(), "`logic` and `netlist`");
}

/// `--help` is what a caller reads before getting `--edition` wrong,
/// and it describes the partition in doc-comment prose that nothing
/// derives from `stage_requires_edition`. Rendering the lists from
/// the function and looking for them in clap's own help text is what
/// ties the two: a stage moved across the partition fails here
/// instead of leaving `--help` naming the old set.
///
/// `synth`'s description only enumerates the required side and calls
/// the rest "the earlier stages", which holds only while every
/// edition-neutral stage precedes every edition-tagged one in
/// `--stage`'s order — so that ordering is pinned too.
#[test]
fn synth_help_names_the_edition_partition() {
    let command = Cli::command();
    let synth = command
        .find_subcommand("synth")
        .expect("synth is a subcommand");

    let edition_help = synth
        .get_arguments()
        .find(|arg| arg.get_id() == "edition")
        .and_then(|arg| arg.get_long_help().or_else(|| arg.get_help()))
        .expect("--edition has help text")
        .to_string();
    let required = format!("Required when {} is set", edition_required_stage_list());
    let refused = format!("refused for {}", edition_neutral_stage_list());
    assert!(
        edition_help.contains(&required) && edition_help.contains(&refused),
        "--edition's help should say `{required}` and `{refused}`, got: {edition_help}",
    );

    let about = synth
        .get_long_about()
        .or_else(|| synth.get_about())
        .expect("synth has a description")
        .to_string();
    let tagged = join_stages(stage_requires_edition, "and", |name| format!("`{name}`"));
    assert!(
        about.contains(&tagged),
        "synth's description should name the edition-tagged stages as `{tagged}`, \
         got: {about}",
    );
    assert!(
        about.contains("earlier stages") && about.contains("edition-neutral"),
        "synth's description should call the stages before {tagged} the earlier, \
         edition-neutral ones, got: {about}",
    );
    let first_tagged = SynthStage::value_variants()
        .iter()
        .position(|stage| stage_requires_edition(*stage))
        .expect("some stage requires --edition");
    assert!(
        SynthStage::value_variants()[first_tagged..]
            .iter()
            .all(|stage| stage_requires_edition(*stage)),
        "an edition-neutral --stage follows an edition-tagged one, so `synth`'s \
         \"earlier stages\" no longer describes the edition-neutral set",
    );
}
