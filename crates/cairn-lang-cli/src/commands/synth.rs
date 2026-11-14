//! `cairn synth`: one stage of the redstone pipeline, as JSON.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::{Edition, IntentModule, lower, resolve};
use cairn_lang_formats::build::finding::Finding;
use cairn_lang_formats::build::gate::{build_diagnostics, weighed_editions};
use cairn_lang_formats::registry::builtin_pack;
use cairn_lang_redstone::{
    LayoutStage, compile_delay, compile_edition_netlist, compile_legalize, compile_netlist,
    compile_placement, compile_routing, synthesize, verify_latency, verify_portable,
    verify_truth_and_temporal,
};
use clap::ValueEnum;

use crate::cli::SynthStage;
use crate::pipeline::load_module;
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

    let reporting = Reporting {
        file,
        source: &source,
        lines: &lines,
    };
    let (json, label) = match dump_stage(stage, edition, &synth, &ir, reporting) {
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

/// Where a stage reports what its passes found.
#[derive(Clone, Copy)]
struct Reporting<'a> {
    file: &'a Path,
    source: &'a str,
    lines: &'a LineStarts,
}

impl Reporting<'_> {
    /// Report `diagnostics`, and refuse the run when one of them is an error.
    fn gate<D: Finding>(self, diagnostics: &[D]) -> Result<(), ExitCode> {
        if report_diagnostics(self.file, self.source, self.lines, diagnostics) {
            return Err(ExitCode::from(1));
        }
        Ok(())
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

/// The JSON of `stage`, the pipeline run up to it, and the label errors name it by.
fn dump_stage(
    stage: SynthStage,
    edition: StageEdition,
    synth: &cairn_lang_redstone::SynthOutput,
    ir: &IntentModule,
    reporting: Reporting<'_>,
) -> Result<(serde_json::Result<String>, &'static str), ExitCode> {
    let netlist = compile_netlist(&synth.scoped);
    let edition = match (stage, edition) {
        (SynthStage::Logic, _) => return dump("Logic IR", &synth.scoped),
        (SynthStage::Netlist, _) => return dump("Netlist IR", &netlist),
        (_, StageEdition::Tagged(edition)) => edition,
        (_, StageEdition::Neutral) => unreachable!(
            "stage_requires_edition holds past `netlist`, so synth_edition answered Tagged or refused"
        ),
    };
    let edition_netlist = compile_edition_netlist(&netlist, edition);
    let built = built_for(ir, edition);
    reporting.gate(&built.diagnostics)?;
    reporting.gate(&verify_portable(ir, &built, edition))?;
    if matches!(stage, SynthStage::Edition) {
        return dump("Edition Netlist IR", &edition_netlist);
    }

    let placement = compile_placement(&edition_netlist, ir, &built);
    reporting.gate(&placement.diagnostics)?;
    if matches!(stage, SynthStage::Placement) {
        return dump("Placement IR", &placement.scoped);
    }

    let routing = compile_routing(&placement.scoped);
    reporting.gate(&routing.diagnostics)?;
    if matches!(stage, SynthStage::Route) {
        return dump("Routed Placement IR", &routing.scoped);
    }

    let delay = compile_delay(&routing.scoped);
    reporting.gate(&delay.diagnostics)?;
    if matches!(stage, SynthStage::Delay) {
        return dump("Delayed Placement IR", &delay.scoped);
    }

    let registry = builtin_pack(edition).view(None);
    let legalized = compile_legalize(&delay.scoped, Some(&registry));
    reporting.gate(&legalized.diagnostics)?;
    reporting.gate(&verify_latency(&legalized.scoped, ir))?;
    reporting.gate(&verify_truth_and_temporal(&legalized.scoped, ir, &built))?;
    dump("Legalized Placement IR", &legalized.scoped)
}

/// A stage's IR as pretty-printed JSON, with the label errors name it by.
#[expect(
    clippy::unnecessary_wraps,
    reason = "each stage of `dump_stage` returns through it"
)]
fn dump(
    label: &'static str,
    ir: &impl serde::Serialize,
) -> Result<(serde_json::Result<String>, &'static str), ExitCode> {
    Ok((serde_json::to_string_pretty(ir), label))
}

/// `ir` lowered to the block array for `edition` on the built-in pack.
fn built_for(ir: &IntentModule, edition: Edition) -> BlockArrayIr {
    lower_to_block_array(
        ir,
        &resolve(ir, Some(edition)),
        Some(&builtin_pack(edition).view(None)),
    )
}

/// The spelling clap accepts for `--stage`, for runtime messages.
fn stage_cli_name(stage: SynthStage) -> &'static str {
    match stage {
        SynthStage::Logic => "logic",
        SynthStage::Netlist => "netlist",
        SynthStage::Edition => "edition",
        SynthStage::Placement => LayoutStage::Placed.as_str(),
        SynthStage::Route => LayoutStage::Routed.as_str(),
        SynthStage::Delay => LayoutStage::Delayed.as_str(),
        SynthStage::Legalize => LayoutStage::Legalized.as_str(),
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
        | SynthStage::Legalize => true,
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
