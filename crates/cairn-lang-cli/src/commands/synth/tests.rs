//! Argument-surface pairings that end-to-end tests could only hard-code on both sides.

use clap::CommandFactory;

use crate::cli::Cli;

use super::*;

/// The Placement IR stage `--stage <stage>` emits, or `None` for a stage before Placement IR.
fn placement_counterpart(stage: SynthStage) -> Option<PlacementStage> {
    match stage {
        SynthStage::Logic | SynthStage::Netlist | SynthStage::Edition => None,
        SynthStage::Placement => Some(PlacementStage::Placement),
        SynthStage::Route => Some(PlacementStage::Route),
        SynthStage::Delay => Some(PlacementStage::Delay),
        SynthStage::Crossing => Some(PlacementStage::Crossing),
    }
}

#[test]
fn placement_stage_names_match_clap() {
    for stage in SynthStage::value_variants() {
        let Some(placement) = placement_counterpart(*stage) else {
            continue;
        };
        let clap_name = stage
            .to_possible_value()
            .expect("no SynthStage variant is skipped");
        assert_eq!(clap_name.get_name(), placement.as_str());
        assert_eq!(stage_cli_name(*stage), placement.as_str());
    }
}

/// `stage_cli_name`'s literal spellings, for the stages with no Placement IR counterpart.
#[test]
fn literal_stage_names_match_clap() {
    for stage in SynthStage::value_variants() {
        if placement_counterpart(*stage).is_some() {
            continue;
        }
        let clap_name = stage
            .to_possible_value()
            .expect("no SynthStage variant is skipped");
        assert_eq!(clap_name.get_name(), stage_cli_name(*stage));
    }
}

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

#[test]
fn stray_edition_message_lists_read_as_english() {
    assert_eq!(
        edition_required_stage_list(),
        "`--stage edition`, `--stage placement`, `--stage route`, \
         `--stage delay`, or `--stage crossing`",
    );
    assert_eq!(edition_neutral_stage_list(), "`logic` and `netlist`");
}

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
