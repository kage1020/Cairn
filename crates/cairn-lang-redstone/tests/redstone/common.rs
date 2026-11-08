//! Fixture builders shared by the redstone integration tests.

use std::path::PathBuf;

use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::block_array::{
    BlockArrayIr, BlockIdSet, BlockState, TargetRegistry, lower_to_block_array,
};
use cairn_lang_core::check::Severity;
use cairn_lang_core::{Edition, IntentModule, lower, parse, resolve};
use cairn_lang_redstone::{
    CellPortDriver, CircuitLayout, Diagnostic, DiagnosticCode, EditionCell, PlacementOutput,
    ScopedCircuitLayout, ScopedEditionNetlistIr, SynthOutput, TileKind, TilePort, compile_delay,
    compile_edition_netlist, compile_legalize, compile_netlist, compile_placement, compile_routing,
    synthesize,
};

/// Reads `examples/<name>` from the workspace root.
pub fn load_example(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// The examples whose circuits build, each with its struct.
pub const EXAMPLES: [(&str, &str); 3] = [
    ("redstone-door.crn", "gatehouse"),
    ("control-room.crn", "control_room"),
    ("crossbar.crn", "crossbar"),
];

pub fn synth_source(source: &str) -> SynthOutput {
    let module = parse(source).expect("parse");
    let intent = lower(&module);
    synthesize(&intent)
}

/// The 1-based line holding byte `offset`.
pub fn line_of(source: &str, offset: usize) -> usize {
    source[..offset].bytes().filter(|b| *b == b'\n').count() + 1
}

const GATE_PRELUDE: &str = concat!(
    "@cairn 2026.06\n",
    "\n",
    "theme t:\n",
    "  slot wall -> @cobblestone\n",
    "\n",
    "struct gate size=5x5\n",
    "  walls class=outer mat_slot=wall height=3\n",
);

/// A walled `struct gate` with `body` as the rest of its members.
pub fn gate_source(body: &str) -> String {
    format!("{GATE_PRELUDE}{body}")
}

pub fn codes(out: &SynthOutput) -> Vec<&'static str> {
    out.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

/// The one finding of `code` in `out`, failing on none or several.
pub fn only(out: &SynthOutput, code: DiagnosticCode) -> &Diagnostic {
    let found: Vec<_> = out.diagnostics.iter().filter(|d| d.code == code).collect();
    assert_eq!(
        found.len(),
        1,
        "expected one {code:?}: {:#?}",
        out.diagnostics
    );
    found[0]
}

/// `sig.open` as a [`DottedRef`].
pub fn sig(name: &str) -> DottedRef {
    let mut parts = name.split('.').map(str::to_owned);
    let head = parts.next().expect("dotted ref has head");
    DottedRef::new(head, parts.collect())
}

/// A door opened through an `or` from a lever and a plate at the far end of a 40-wide floor, its
/// wire long enough to need a buffer repeater.
pub const LONG_WIRE_SOURCE: &str = "\
theme t:
  slot wall -> @oak_planks

struct reach size=40x6
  floor mat_slot=wall
  walls mat_slot=wall height=3
  door id=front side=front at=center
  lever id=l at=inside.back offset=1 y=0 -> sig.a
  pressure_plate id=p at=inside.front offset=1 y=0 -> sig.b
  logic sig.c = sig.a or sig.b
  door[id=front] opened_by=sig.c
  circuit region=floor void=4
";

/// A door opened straight off a plate at the far end of a 40-wide floor: a wire and no cell.
pub const IDENTITY_WIRE_SOURCE: &str = "\
theme t:
  slot wall -> @oak_planks

struct pass size=40x6
  floor mat_slot=wall
  walls mat_slot=wall height=3
  door id=d side=front at=center
  pressure_plate id=p at=inside.front offset=1 y=0 -> sig.a
  door[id=d] opened_by=sig.a
  circuit region=floor void=3
";

/// One finding as `(code, spanned text, primary, notes)`, each note with the text its span covers.
pub type Finding<'a> = (
    &'static str,
    &'a str,
    String,
    Vec<(Option<&'a str>, String)>,
);

/// `found` as [`Finding`]s over `source`.
pub fn findings<'a>(source: &'a str, found: &[Diagnostic]) -> Vec<Finding<'a>> {
    found
        .iter()
        .map(|d| {
            let notes = d
                .notes
                .iter()
                .map(|n| (n.span.clone().map(|s| &source[s]), n.message.clone()))
                .collect();
            (
                d.code.as_str(),
                &source[d.span.clone()],
                d.primary.clone(),
                notes,
            )
        })
        .collect()
}

/// Compiles `source` to the Edition Netlist IR, failing on any front-end error.
pub fn edition_netlist_from_source(
    source: &str,
    edition: Edition,
) -> (ScopedEditionNetlistIr, IntentModule) {
    let module = parse(source).expect("parse");
    let intent = lower(&module);
    let synth = synthesize(&intent);
    assert_clean("synth", &synth.diagnostics);
    let netlist = compile_netlist(&synth.scoped);
    let edition_netlist = compile_edition_netlist(&netlist, edition);
    (edition_netlist, intent)
}

/// A target that spells every token as the block of its name, so every member stands as written.
pub struct Named;

impl TargetRegistry for Named {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        let name = token.split('.').next()?;
        Some(BlockState::bare(format!("minecraft:{name}")))
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        None
    }
}

/// `intent` built for `edition` against [`Named`].
pub fn built(intent: &IntentModule, edition: Edition) -> BlockArrayIr {
    lower_to_block_array(intent, &resolve(intent, Some(edition)), Some(&Named))
}

/// The placement pass over `source`, its findings kept.
pub fn placement_of(source: &str, edition: Edition) -> PlacementOutput {
    let (netlist, intent) = edition_netlist_from_source(source, edition);
    compile_placement(&netlist, &intent, &built(&intent, edition))
}

/// `source`'s circuits placed, failing on any finding, the lowering's included.
pub fn placed(source: &str, edition: Edition) -> ScopedCircuitLayout {
    let (netlist, intent) = edition_netlist_from_source(source, edition);
    let built = built(&intent, edition);
    assert!(
        built
            .diagnostics
            .iter()
            .all(|d| d.severity() != Severity::Error),
        "the fixture must lower: {:#?}",
        built.diagnostics,
    );
    let out = compile_placement(&netlist, &intent, &built);
    assert_clean("placement", &out.diagnostics);
    out.scoped
}

/// `source`'s circuits routed, failing on any finding.
pub fn routed(source: &str, edition: Edition) -> ScopedCircuitLayout {
    let out = compile_routing(&placed(source, edition));
    assert_clean("routing", &out.diagnostics);
    out.scoped
}

/// `source`'s circuits buffered, failing on any finding.
pub fn delayed(source: &str, edition: Edition) -> ScopedCircuitLayout {
    let out = compile_delay(&routed(source, edition));
    assert_clean("delay", &out.diagnostics);
    out.scoped
}

/// `source`'s circuits legalized with vanilla ids, failing on any finding.
pub fn legalized(source: &str, edition: Edition) -> ScopedCircuitLayout {
    let out = compile_legalize(&delayed(source, edition), None);
    assert_clean("legalize", &out.diagnostics);
    out.scoped
}

fn assert_clean(pass: &str, diagnostics: &[Diagnostic]) {
    assert!(
        diagnostics.iter().all(|d| d.severity() != Severity::Error),
        "the fixture must clear {pass}: {diagnostics:#?}",
    );
}

/// Each cell tile of `layout` with the netlist net on each of its ports.
pub fn cells(layout: &CircuitLayout) -> Vec<(EditionCell, Vec<CellPortDriver>)> {
    layout
        .tiles
        .iter()
        .filter_map(|tile| {
            let TileKind::Cell { cell } = tile.kind else {
                return None;
            };
            let drivers = tile
                .inputs
                .iter()
                .filter_map(|input| match input.port {
                    TilePort::Cell(port) => Some(CellPortDriver {
                        port,
                        net: layout.nets[input.net].source,
                    }),
                    _ => None,
                })
                .collect();
            Some((cell, drivers))
        })
        .collect()
}
