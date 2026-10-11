//! Argument-surface invariants the end-to-end tests could only assert by
//! hard-coding both sides of a pairing.

use clap::CommandFactory;

use crate::cli::Cli;

use super::*;

/// clap kebab-cases the variant identifier, which no type ties to the
/// dump's `"stage"` key: renaming `SynthStage::Route` to `Routing` would
/// accept `--stage routing` while the dump kept saying `route`.
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

/// Without a Placement IR counterpart, these spellings are literals in
/// `stage_cli_name`.
#[test]
fn edition_neutral_stage_names_match_clap() {
    for stage in [SynthStage::Logic, SynthStage::Netlist, SynthStage::Edition] {
        let clap_name = stage
            .to_possible_value()
            .expect("no SynthStage variant is skipped");
        assert_eq!(clap_name.get_name(), stage_cli_name(stage));
    }
}

/// Walks `ValueEnum`'s full variant list, so a stage added without a
/// decision on its edition-dependence fails here instead of joining the
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

/// The lists are derived; this pins the English between their items.
#[test]
fn stray_edition_message_lists_read_as_english() {
    assert_eq!(
        edition_required_stage_list(),
        "`--stage edition`, `--stage placement`, `--stage route`, \
         `--stage delay`, or `--stage crossing`",
    );
    assert_eq!(edition_neutral_stage_list(), "`logic` and `netlist`");
}

/// The `--help` prose naming the partition is hand-written, so the lists
/// rendered from `stage_requires_edition` are looked for in clap's help
/// text. `synth`'s description calls the neutral side "the earlier
/// stages", so the stage order is pinned too.
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
         \"earlier stages\" does not describe the edition-neutral set",
    );
}
