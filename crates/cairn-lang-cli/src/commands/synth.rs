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

use crate::cli::{EditionArg, SynthStage};
use crate::pipeline::{build_diagnostics, load_module, weighed_editions};
use crate::report::{FailureReport, report_diagnostics};

pub(crate) fn run_synth(
    file: &Path,
    experimental_logic_synth: bool,
    stage: SynthStage,
    edition: Option<EditionArg>,
) -> ExitCode {
    if !experimental_logic_synth {
        // Gated behind `--experimental-logic-synth` because the redstone
        // pipeline is still Internal-tier (`spec/compatibility`) — the
        // Logic IR wire form will grow the netlist / placement / route
        // layers over later changes, and every intermediate shape is fair
        // game for a breaking change. Exit 2 (usage error) so a script
        // that accidentally reaches this subcommand does not read the
        // gate as a warning it can ignore.
        eprintln!(
            "error: `cairn synth` is an internal / experimental surface; pass --experimental-logic-synth to opt in",
        );
        return ExitCode::from(2);
    }

    // Both halves of the stage-vs-edition rule are argv-only, so both are
    // decided before the file is read: a caller who got the flag wrong
    // hears about the flag, not about whatever the source has to say.
    let edition = match synth_edition(stage, edition) {
        Ok(edition) => edition,
        Err(code) => return code,
    };

    // Every refusal from here on is prose on stderr, with no failure
    // document on stdout. `run_lower` writes one under `--format json`
    // because that flag commits a command to one JSON document per input,
    // a refused one included (`spec/lint` "Machine-readable payload");
    // `synth` takes no `--format` and has no such commitment to honour.
    // Its stdout is the dump or nothing, so a redirect of a refused run
    // holds zero bytes, which no JSON reader takes for an IR.
    let (source, module) = match load_module(file, FailureReport::Text) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let ir = lower(&module);
    let lines = LineStarts::new(&source);

    // Mirror `run_check` / `run_lower` / `run_compile`: surface the check
    // diagnostics before running the redstone synth. A `.crn` whose only
    // problem is `E_UNRESOLVED_SLOT` or a typo caught by `check` would
    // otherwise exit 0 through the synth path with a partially resolved
    // IR, which is a poor CI gate. Synth does not lower to block arrays,
    // so there are no lowering diagnostics to append here.
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

/// The edition `stage` runs for, from the `--edition` the caller passed.
///
/// [`StageEdition::Neutral`] for an edition-neutral stage, which refuses a
/// stray `--edition` rather than silently ignoring it — a caller who
/// passed the flag there almost certainly expected it to shape the output,
/// and swallowing the mistake would make the CLI's stage-vs-edition axis
/// ambiguous. [`StageEdition::Tagged`] for an edition-tagged stage, which
/// requires the flag.
///
/// Must be called before `load_module`: both refusals are usage errors
/// (exit 2) decided from argv alone, and a call any later would put them
/// behind the read and the passes after it, where an error found in the
/// file ends the run before either refusal is reached and a warning is
/// printed ahead of the refusal. Being ahead of the read also means a
/// path that names nothing is reported only once `--edition` is right.
fn synth_edition(stage: SynthStage, edition: Option<EditionArg>) -> Result<StageEdition, ExitCode> {
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
    require_edition(edition, stage_cli_name(stage))
        .map(|edition| StageEdition::Tagged(edition.as_edition()))
}

/// [`synth_edition`]'s answer for a stage, which nothing else builds:
/// which side of `stage_requires_edition`'s partition the stage is on, and
/// for an edition-tagged stage the edition it runs for.
#[derive(Clone, Copy)]
enum StageEdition {
    /// An edition-neutral stage, run without `--edition`.
    Neutral,
    /// An edition-tagged stage, and the edition `--edition` named for it.
    Tagged(Edition),
}

/// Run the requested pipeline stage and return the JSON serialisation
/// plus a human-facing label.
///
/// `edition` is [`synth_edition`]'s answer for `stage`, resolved before
/// the source was read. `compile_netlist` and `compile_edition_netlist`
/// are diagnostic-free by contract, so they carry no report call.
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
    // Dispatch on the stage, with `synth_edition`'s answer checked against
    // it, rather than on the answer alone. The two agree today, but only
    // the stage makes a stage added later get an arm of its own here (a
    // bare `Neutral` arm would hand it the Netlist payload, under the
    // Netlist label, with exit 0), and only the stage stops a `Tagged`
    // answer for `Netlist` here, naming the contract it broke, rather than
    // after every later pass has run, at the `Crossing` guard below.
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

/// The spelling clap accepts for `--stage`, for the messages this binary
/// composes at runtime. The four Placement IR stages read theirs from
/// [`PlacementStage::as_str`] so the word here and the dump's `"stage"`
/// key cannot drift; `placement_stage_names_match_clap` pins the clap
/// derivation. The `--help` prose spells every stage by hand and does
/// not call this at runtime, but `synth_help_names_the_edition_partition`
/// renders its expected lists through here (via `join_stages`), so
/// renaming a stage fails that test until the help follows.
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

/// Whether `--stage <stage>` reads the target-edition cell library and
/// therefore needs `--edition <java|bedrock>` alongside it.
///
/// One partition drives both halves of the flag's contract, and
/// [`synth_edition`] applies both: it refuses a stray `--edition` on the
/// `false` stages and demands the flag on the `true` ones, and the
/// stray-`--edition` message it prints renders both lists from here. The
/// exhaustive `match` makes a new variant a compile error rather than a
/// silent default.
///
/// The `--help` prose spells the same partition by hand: `synth`'s own
/// description names the required stages and calls the rest "earlier",
/// and `--edition`'s names both sides. Those sentences stay doc comments
/// rather than `long_about` / `long_help` expressions so the source keeps
/// reading as the help it renders; `synth_help_names_the_edition_partition`
/// checks each one against this function instead, so moving a stage
/// across fails a test until the prose follows. The check is verbatim
/// containment of `join_stages`'s rendering, so the prose has to match
/// its conjunction and Oxford-comma style exactly: a third
/// edition-neutral stage makes `--edition`'s sentence read
/// "`a`, `b`, and `c`" whatever reads best there.
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

/// `` `--stage a`, `--stage b`, or `--stage c` `` over the stages that
/// require `--edition`, for the stray-flag message.
fn edition_required_stage_list() -> String {
    join_stages(stage_requires_edition, "or", |name| {
        format!("`--stage {name}`")
    })
}

/// `` `a` and `b` `` over the stages that refuse `--edition`, for the
/// same message. Renders bare stage names because that half of the
/// sentence talks about the stages themselves rather than about the
/// flag a caller would have typed.
fn edition_neutral_stage_list() -> String {
    join_stages(
        |stage| !stage_requires_edition(stage),
        "and",
        |name| format!("`{name}`"),
    )
}

/// Render the `--stage` values matching `select` as an English list,
/// in the order clap declares them, with each name passed through
/// `render` first. Walking `ValueEnum` rather than a literal list is
/// what lets the stray-`--edition` message pick up a stage the day it
/// lands instead of naming a set that has since moved on.
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

/// Enforce the `--edition <java|bedrock>` requirement for the
/// edition-tagged stages. The stage name is baked into the error
/// message so a caller sees exactly which stage tripped the gate,
/// plus a short "why" hint so a caller who doesn't know the pipeline
/// still connects the flag to the edition-specific cell library.
fn require_edition(edition: Option<EditionArg>, stage_name: &str) -> Result<EditionArg, ExitCode> {
    edition.ok_or_else(|| {
        eprintln!(
            "error: `cairn synth --stage {stage_name}` requires --edition <java|bedrock> (this stage picks the target-edition cell realisation, so the flag is not optional)",
        );
        ExitCode::from(2)
    })
}

#[cfg(test)]
mod tests;
