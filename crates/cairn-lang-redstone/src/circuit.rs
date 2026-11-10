//! Every phase of `spec/redstone` "Connection to the IR and phases" for one edition, from the
//! Intent IR's `logic` to the circuit laid in the layers under each built struct.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockArrayIr, LaidBlock, TargetRegistry};
use cairn_lang_core::check::Severity;
use cairn_lang_core::intent::IntentModule;
use indexmap::IndexMap;

use crate::delay::compile_delay;
use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::edition_netlist::compile_edition_netlist;
use crate::legalize::compile_legalize;
use crate::netlist::compile_netlist;
use crate::placement::compile_placement;
use crate::placement_ir::{CircuitBlock, CircuitLayout};
use crate::portable::verify_portable;
use crate::routing::compile_routing;
use crate::synth::synthesize;
use crate::verify::verify_latency;

/// The block entity a Java comparator's output strength is kept in.
const COMPARATOR_ENTITY: &str = "minecraft:comparator";

/// Synthesizes, places, routes and legalizes every scope's `logic` for `edition`, its block ids
/// read from `registry`, and lays each circuit under the struct `built` holds for it.
///
/// Stops at the first phase that reports an error and then leaves `built` as it was.
pub fn build_circuits(
    module: &IntentModule,
    built: &mut BlockArrayIr,
    edition: Edition,
    registry: Option<&dyn TargetRegistry>,
) -> Vec<Diagnostic> {
    let mut found = Findings::default();
    let synth = synthesize(module);
    if found.stop(synth.diagnostics) {
        return found.all;
    }
    let netlist = compile_edition_netlist(&compile_netlist(&synth.scoped), edition);
    if found.stop(verify_portable(module, built, edition)) {
        return found.all;
    }
    let placement = compile_placement(&netlist, module, built);
    if found.stop(placement.diagnostics) {
        return found.all;
    }
    let routing = compile_routing(&placement.scoped);
    if found.stop(routing.diagnostics) {
        return found.all;
    }
    let delay = compile_delay(&routing.scoped);
    if found.stop(delay.diagnostics) {
        return found.all;
    }
    let legalized = compile_legalize(&delay.scoped, registry);
    if found.stop(legalized.diagnostics) {
        return found.all;
    }
    if found.stop(verify_latency(&legalized.scoped, module)) {
        return found.all;
    }
    let mut attached = built.clone();
    for entry in &legalized.scoped.scopes {
        let layout = &entry.ir;
        let under = layout.region.under();
        let blocks: Vec<LaidBlock> = layout
            .blocks
            .iter()
            .filter_map(|block| laid(block, under))
            .collect();
        if let Err(err) = attached.attach_circuit(
            &layout.structure,
            layout.region.void,
            &blocks,
            layout.region.span.clone(),
        ) {
            found
                .all
                .push(refused(entry.kind.label(), &entry.name, layout, &err));
            return found.all;
        }
    }
    *built = attached;
    found.all
}

/// What the phases so far reported.
#[derive(Default)]
struct Findings {
    all: Vec<Diagnostic>,
}

impl Findings {
    /// Keeps `diagnostics`, and says whether one of them is an error.
    fn stop(&mut self, diagnostics: Vec<Diagnostic>) -> bool {
        let error = diagnostics.iter().any(|d| d.severity() == Severity::Error);
        self.all.extend(diagnostics);
        error
    }
}

/// `block` in the layers under the struct's base plane, `under` of them below the circuit's, with
/// its block entity.
fn laid(block: &CircuitBlock, under: u32) -> Option<LaidBlock> {
    let at = (
        u32::try_from(block.at.x).ok()?,
        u32::try_from(block.at.y).ok()?.checked_add(under)?,
        u32::try_from(block.at.z).ok()?,
    );
    let entity = block.output_signal.map(|signal| {
        (
            COMPARATOR_ENTITY.to_owned(),
            IndexMap::from([("OutputSignal".to_owned(), i32::from(signal))]),
        )
    });
    Some(LaidBlock {
        at,
        state: block.state.clone(),
        entity,
    })
}

fn refused(
    kind: &str,
    name: &str,
    layout: &CircuitLayout,
    err: &cairn_lang_core::block_array::AttachError,
) -> Diagnostic {
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        layout.region.span.clone(),
        format!("{kind}={name}: the circuit cannot be laid under this struct: {err}"),
        "Fix: lower `void`, or split the struct's members across more than one `struct`",
    )
}

#[cfg(test)]
mod tests;
