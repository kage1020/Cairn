//! `cairn synth`: one stage of the redstone pipeline, as JSON.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::{Edition, lower};
use cairn_lang_redstone::{
    PlacementStage, compile_crossing, compile_delay, compile_edition_netlist, compile_netlist,
    compile_placement, compile_routing, synthesize,
};
use clap::ValueEnum;

use crate::cli::SynthStage;
use crate::pipeline::{build_diagnostics, load_module, weighed_editions};
use crate::report::{FailureReport, report_diagnostics};

pub(crate) fn run_synth(
    file: &Path,
    experimental_logic_synth: bool,
    stage: SynthStage,
    edition: Option<Edition>,
) -> ExitCode {
    if !experimental_logic_synth {
        // Exit 2, a usage error, so a script cannot take the gate for a warning to ignore.
        eprintln!(
            "error: `cairn synth` is an internal / experimental surface; pass --experimental-logic-synth to opt in",
        );
        return ExitCode::from(2);
    }

    let edition = match synth_edition(stage, edition) {
        Ok(edition) => edition,
        Err(code) => return code,
    };

    // Text: `synth` takes no `--format`, so it owes no failure document.
    let (source, module) = match load_module(file, FailureReport::Text) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let ir = lower(&module);
    let lines = LineStarts::new(&source);

    let diagnostics = build_diagnostics(&module, &ir, None, &weighed_editions(None), Vec::new());
    if report_diagnostics(file, &source, &lines, &diagnostics) {
        return ExitCode::from(1);
    }

    let synth = synthesize(&ir);
    if report_diagnostics(file, &source, &lines, &synth.diagnostics) {
        return ExitCode::from(1);
    }

    let (json, label) =
        match dispatch_synth_stage(stage, edition, &synth, &ir, file, &source, &lines) {
            Ok(pair) => pair,
            Err(code) => return code,
        };
    match json {
        Ok(text) => {
            outln!("{text}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: failed to serialise {label} as JSON: {err}");
            ExitCode::from(1)
        }
    }
}

/// The edition `stage` runs for, refusing a stray or missing `--edition`.
fn synth_edition(stage: SynthStage, edition: Option<Edition>) -> Result<StageEdition, ExitCode> {
    if !stage_requires_edition(stage) {
        if edition.is_some() {
            eprintln!(
                "error: `--edition` is only meaningful with {}; the {} stages are edition-neutral",
                edition_required_stage_list(),
                edition_neutral_stage_list(),
            );
            return Err(ExitCode::from(2));
        }
        return Ok(StageEdition::Neutral);
    }
    require_edition(edition, stage_cli_name(stage)).map(StageEdition::Tagged)
}

/// [`synth_edition`]'s answer: an edition-neutral stage, or an edition-tagged one and its edition.
#[derive(Clone, Copy)]
enum StageEdition {
    Neutral,
    Tagged(Edition),
}

/// Run the pipeline up to `stage` and return its JSON and the label errors name it by.
fn dispatch_synth_stage(
    stage: SynthStage,
    edition: StageEdition,
    synth: &cairn_lang_redstone::SynthOutput,
    ir: &cairn_lang_core::IntentModule,
    file: &Path,
    source: &str,
    lines: &LineStarts,
) -> Result<(serde_json::Result<String>, &'static str), ExitCode> {
    if matches!(stage, SynthStage::Logic) {
        return Ok((serde_json::to_string_pretty(&synth.scoped), "Logic IR"));
    }

    let netlist = compile_netlist(&synth.scoped);
    // Matched on the stage too, so a new stage needs an arm of its own.
    let edition = match (stage, edition) {
        (SynthStage::Netlist, StageEdition::Neutral) => {
            return Ok((serde_json::to_string_pretty(&netlist), "Netlist IR"));
        }
        (
            SynthStage::Edition
            | SynthStage::Placement
            | SynthStage::Route
            | SynthStage::Delay
            | SynthStage::Crossing,
            StageEdition::Tagged(edition),
        ) => edition,
        (SynthStage::Logic, _) => unreachable!("the Logic guard above returns"),
        (SynthStage::Netlist, StageEdition::Tagged(_)) => unreachable!(
            "stage_requires_edition is false here, so synth_edition answered Neutral or refused"
        ),
        (
            SynthStage::Edition
            | SynthStage::Placement
            | SynthStage::Route
            | SynthStage::Delay
            | SynthStage::Crossing,
            StageEdition::Neutral,
        ) => unreachable!(
            "stage_requires_edition holds here, so synth_edition answered Tagged or refused"
        ),
    };
    let edition_netlist = compile_edition_netlist(&netlist, edition);
    if matches!(stage, SynthStage::Edition) {
        return Ok((
            serde_json::to_string_pretty(&edition_netlist),
            "Edition Netlist IR",
        ));
    }

    let placement = compile_placement(&edition_netlist, ir);
    if report_diagnostics(file, source, lines, &placement.diagnostics) {
        return Err(ExitCode::from(1));
    }
    if matches!(stage, SynthStage::Placement) {
        return Ok((
            serde_json::to_string_pretty(&placement.scoped),
            "Placement IR",
        ));
    }

    let routing = compile_routing(&placement.scoped);
    if report_diagnostics(file, source, lines, &routing.diagnostics) {
        return Err(ExitCode::from(1));
    }
    if matches!(stage, SynthStage::Route) {
        return Ok((
            serde_json::to_string_pretty(&routing.scoped),
            "Routed Placement IR",
        ));
    }

    let delay = compile_delay(&routing.scoped);
    if report_diagnostics(file, source, lines, &delay.diagnostics) {
        return Err(ExitCode::from(1));
    }
    if matches!(stage, SynthStage::Delay) {
        return Ok((
            serde_json::to_string_pretty(&delay.scoped),
            "Delayed Placement IR",
        ));
    }

    let crossing = compile_crossing(&delay.scoped);
    if report_diagnostics(file, source, lines, &crossing.diagnostics) {
        return Err(ExitCode::from(1));
    }
    match stage {
        SynthStage::Crossing => Ok((
            serde_json::to_string_pretty(&crossing.scoped),
            "Legalized Placement IR",
        )),
        SynthStage::Logic
        | SynthStage::Netlist
        | SynthStage::Edition
        | SynthStage::Placement
        | SynthStage::Route
        | SynthStage::Delay => {
            unreachable!("earlier guards return for non-Crossing stages")
        }
    }
}

/// The spelling clap accepts for `--stage`, for runtime messages.
fn stage_cli_name(stage: SynthStage) -> &'static str {
    match stage {
        SynthStage::Logic => "logic",
        SynthStage::Netlist => "netlist",
        SynthStage::Edition => "edition",
        SynthStage::Placement => PlacementStage::Placement.as_str(),
        SynthStage::Route => PlacementStage::Route.as_str(),
        SynthStage::Delay => PlacementStage::Delay.as_str(),
        SynthStage::Crossing => PlacementStage::Crossing.as_str(),
    }
}

/// Whether `--stage <stage>` reads the target-edition cell library, and so needs `--edition`.
fn stage_requires_edition(stage: SynthStage) -> bool {
    match stage {
        SynthStage::Logic | SynthStage::Netlist => false,
        SynthStage::Edition
        | SynthStage::Placement
        | SynthStage::Route
        | SynthStage::Delay
        | SynthStage::Crossing => true,
    }
}

/// `` `--stage a`, `--stage b`, or `--stage c` `` over the stages that require `--edition`.
fn edition_required_stage_list() -> String {
    join_stages(stage_requires_edition, "or", |name| {
        format!("`--stage {name}`")
    })
}

/// `` `a` and `b` `` over the stages that refuse `--edition`.
fn edition_neutral_stage_list() -> String {
    join_stages(
        |stage| !stage_requires_edition(stage),
        "and",
        |name| format!("`{name}`"),
    )
}

/// The `--stage` values matching `select`, each through `render`, as an English list.
fn join_stages(
    select: impl Fn(SynthStage) -> bool,
    conjunction: &str,
    render: impl Fn(&str) -> String,
) -> String {
    let items: Vec<String> = SynthStage::value_variants()
        .iter()
        .copied()
        .filter(|stage| select(*stage))
        .map(|stage| render(stage_cli_name(stage)))
        .collect();
    match items.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, [only])) => format!("{only} {conjunction} {last}"),
        Some((last, rest)) => format!("{}, {conjunction} {last}", rest.join(", ")),
    }
}

fn require_edition(edition: Option<Edition>, stage_name: &str) -> Result<Edition, ExitCode> {
    edition.ok_or_else(|| {
        eprintln!(
            "error: `cairn synth --stage {stage_name}` requires --edition <java|bedrock> (this stage picks the target-edition cell realisation, so the flag is not optional)",
        );
        ExitCode::from(2)
    })
}

#[cfg(test)]
mod tests;
