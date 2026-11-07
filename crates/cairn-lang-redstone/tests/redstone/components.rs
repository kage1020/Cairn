//! Every sensor feeds an input pad and every actuator reads an output pad, on its own line or
//! through a patch (`spec/redstone` "Signal binding", "Place-and-route").

use cairn_lang_core::Edition;
use cairn_lang_redstone::{DiagnosticCode, SynthOutput};

use crate::common::{codes, legalized_from_source, only, synth_source};

/// A walled struct with one of each component, `extra` after them.
fn panel(extra: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct panel size=9x7\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         lever id=lv at=inside.front offset=1 y=1 -> sig.power\n  \
         button id=bt at=inside.front offset=2 y=1 -> sig.ring\n  \
         daylight id=dl at=inside.back offset=1 y=1 -> sig.day\n  \
         observer id=ob at=inside.back offset=2 y=1 facing=up -> sig.tick\n  \
         dispenser id=ds at=inside.right offset=1 y=1 fired_by=sig.fire\n  \
         lamp id=lp at=inside.left offset=1 y=2 lit_by=sig.day\n  \
         piston id=pi at=inside.left offset=3 y=1 powered_by=sig.tick\n  \
         logic sig.fire = sig.power and sig.ring\n{extra}  \
         circuit region=floor void=2\n"
    )
}

/// Scope `panel`'s output port names, or `None` when the scope was refused.
fn outputs(out: &SynthOutput) -> Option<Vec<String>> {
    out.scoped
        .scopes
        .iter()
        .find(|e| e.name == "panel")
        .map(|e| e.ir.outputs.iter().map(|p| p.name.to_string()).collect())
}

#[test]
fn every_sensor_feeds_an_input_pad_and_every_actuator_reads_an_output_pad() {
    for edition in [Edition::Java, Edition::Bedrock] {
        let legalized = legalized_from_source(&panel(""), edition);
        let ir = &legalized.scopes[0].ir;
        let inputs: Vec<String> = ir.inputs.iter().map(|p| p.name.to_string()).collect();
        assert_eq!(
            inputs,
            ["sig.power", "sig.ring", "sig.day", "sig.tick"],
            "{edition:?}"
        );
        let width = ir.region.as_ref().expect("the reservation survives").width;
        let outputs: Vec<(String, u32)> = ir
            .outputs
            .iter()
            .map(|p| (p.name.to_string(), p.pad.x))
            .collect();
        assert_eq!(
            outputs,
            [
                ("sig.fire".to_owned(), width - 1),
                ("sig.day".to_owned(), width - 1),
                ("sig.tick".to_owned(), width - 1),
            ],
            "{edition:?}: every actuator pad sits on the region's right edge",
        );
    }
}

#[test]
fn a_patch_binds_the_actuator_its_brackets_pick() {
    let src = panel(
        "  lamp id=spare at=inside.left offset=2 y=2\n  \
         lamp[id=spare] lit_by=sig.tick\n  \
         piston id=ram at=inside.right offset=2 y=1\n  \
         piston[id=ram] powered_by=sig.day\n  \
         dispenser id=gun at=inside.right offset=3 y=1\n  \
         dispenser[id=gun] fired_by=sig.ring\n",
    );
    let out = synth_source(&src);
    assert_eq!(codes(&out), Vec::<&str>::new(), "{:#?}", out.diagnostics);
    assert_eq!(
        outputs(&out),
        Some(
            [
                "sig.fire", "sig.day", "sig.tick", "sig.tick", "sig.day", "sig.ring"
            ]
            .map(str::to_owned)
            .to_vec()
        ),
    );
}

#[test]
fn a_patch_that_picks_no_actuator_is_refused_by_both_passes_alike() {
    for (patch, reason) in [
        (
            "lamp[id=nope] lit_by=sig.tick",
            "lamp actuator patch selects `id=nope` but no physical lamp with that id exists \
             (known lamp ids: lp)",
        ),
        (
            "piston[id=nope] powered_by=sig.tick",
            "piston actuator patch selects `id=nope` but no physical piston with that id exists \
             (known piston ids: pi)",
        ),
        (
            "dispenser[id=nope] fired_by=sig.tick",
            "dispenser actuator patch selects `id=nope` but no physical dispenser with that id \
             exists (known dispenser ids: ds)",
        ),
    ] {
        let src = panel(&format!("  {patch}\n"));
        let module = cairn_lang_core::parse(&src).expect("parse");
        let ir = cairn_lang_core::lower(&module);
        let resolution = cairn_lang_core::resolve(&ir, None);
        let lowered = cairn_lang_core::block_array::lower_to_block_array(&ir, &resolution, None);
        let refused: Vec<&str> = lowered
            .diagnostics
            .iter()
            .filter(|d| d.code.as_str() == "E_INVALID_MEMBER")
            .map(|d| d.primary.as_str())
            .collect();
        assert_eq!(refused, [reason], "{patch}");

        let out = synth_source(&src);
        let key = patch
            .split_once(' ')
            .and_then(|(_, binding)| binding.split_once('='))
            .map(|(key, _)| key)
            .expect("the patch binds a key");
        assert_eq!(
            only(&out, DiagnosticCode::LogicUnresolvedPatch).primary,
            format!("struct=panel: {reason}, so its `{key}=` binding drives nothing"),
        );
        assert_eq!(outputs(&out), None, "{patch}: an error keeps the scope out");
    }
}
