//! Every sensor feeds an input pad and every actuator reads an output pad, on its own line or
//! through a patch, once per actuator (`spec/redstone` "Signal binding", "Place-and-route").

use cairn_lang_core::Edition;
use cairn_lang_redstone::synth::Actuator;
use cairn_lang_redstone::{DiagnosticCode, NetRef, PadKind, SynthOutput, TileKind};

use crate::common::{codes, legalized, only, synth_source};

/// A walled struct with one of each component, `extra` after them.
fn panel(extra: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct panel size=15x13\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         lever id=lv at=inside.front offset=2 y=0 -> sig.power\n  \
         button id=bt at=inside.front offset=6 y=0 -> sig.ring\n  \
         daylight id=dl at=inside.back offset=2 y=0 -> sig.day\n  \
         observer id=ob at=inside.back offset=6 y=1 facing=up -> sig.tick\n  \
         dispenser id=ds at=inside.right offset=3 y=1 fired_by=sig.fire\n  \
         lamp id=lp at=inside.left offset=2 y=1 lit_by=sig.day\n  \
         piston id=pi at=inside.left offset=6 y=1 powered_by=sig.tick\n  \
         logic sig.fire = sig.power and sig.ring\n{extra}  \
         circuit region=floor void=4\n"
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
fn every_sensor_feeds_a_pad_and_every_actuator_reads_a_riser() {
    for edition in [Edition::Java, Edition::Bedrock] {
        let legalized = legalized(&panel(""), edition);
        let layout = &legalized.scopes[0].ir;
        let source = |net: Option<usize>| net.map(|net| layout.nets[net].source);
        let pads: Vec<(PadKind, Option<NetRef>)> = layout
            .tiles
            .iter()
            .filter_map(|tile| match tile.kind {
                TileKind::Pad { pad } => Some((pad, source(tile.output))),
                _ => None,
            })
            .collect();
        assert_eq!(
            pads,
            [
                (PadKind::Dust, Some(NetRef::Input(0))),
                (PadKind::Dust, Some(NetRef::Input(1))),
                (PadKind::Repeater, Some(NetRef::Input(2))),
                (PadKind::Observer, Some(NetRef::Input(3))),
            ],
            "{edition:?}",
        );
        let risers: Vec<Option<NetRef>> = layout
            .tiles
            .iter()
            .filter(|tile| tile.kind == TileKind::Riser)
            .map(|tile| source(tile.inputs.first().map(|input| input.net)))
            .collect();
        assert_eq!(
            risers,
            [
                Some(NetRef::Cell(0)),
                Some(NetRef::Input(2)),
                Some(NetRef::Input(3)),
            ],
            "{edition:?}",
        );
    }
}

#[test]
fn a_patch_binds_the_actuator_its_brackets_pick() {
    let src = panel(
        "  lamp id=spare at=inside.left offset=4 y=1\n  \
         lamp[id=spare] lit_by=sig.tick\n  \
         piston id=ram at=inside.right offset=5 y=1\n  \
         piston[id=ram] powered_by=sig.day\n  \
         dispenser id=gun at=inside.right offset=7 y=1\n  \
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

#[test]
fn an_actuator_bound_on_its_line_and_through_a_patch_is_refused_the_second_binding() {
    for (actuator, id, first, second) in [
        (Actuator::Lamp, "lp", "sig.day", "sig.ring"),
        (Actuator::Piston, "pi", "sig.tick", "sig.day"),
        (Actuator::Dispenser, "ds", "sig.fire", "sig.tick"),
    ] {
        let (keyword, key) = (actuator.keyword(), actuator.key());
        let src = panel(&format!("  {keyword}[id={id}] {key}={second}\n"));
        let out = synth_source(&src);
        let refused = only(&out, DiagnosticCode::LogicDuplicateBinding);
        let patch = src.find(&format!("{keyword}[id={id}]")).expect("the patch");
        let declaration = src
            .find(&format!("{keyword} id={id} "))
            .expect("the declaration");
        let span_of = |line: usize, value: &str| {
            let start = line + src[line..].find(value).expect("the value");
            start..start + value.len()
        };
        let notes: Vec<(Option<std::ops::Range<usize>>, String)> = refused
            .notes
            .iter()
            .map(|note| (note.span.clone(), note.message.clone()))
            .collect();
        assert_eq!(codes(&out), ["E_LOGIC_DUPLICATE_BINDING"], "{keyword}");
        assert_eq!(refused.span, span_of(patch, second), "{keyword}");
        assert_eq!(
            refused.primary,
            format!(
                "struct=panel: `{keyword}` `{id}` is already bound by `{key}={first}`, and a \
                 second `{key}=` would drive it from two wires at once"
            ),
        );
        assert_eq!(
            notes,
            [
                (
                    Some(span_of(declaration, first)),
                    "first bound here".to_owned()
                ),
                (
                    None,
                    format!(
                        "Fix: combine the signals in the logic layer, as in `logic sig.<name> = \
                         {first} or {second}`, and bind `{key}=` once to `sig.<name>`."
                    ),
                ),
            ],
            "{keyword}",
        );
    }
}
