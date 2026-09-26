//! Throughput of the place-and-route passes over a generated circuit.
//!
//! The examples are a few dozen lines each, so a whole-pipeline run over
//! any of them is dominated by process startup. This bench generates a
//! netlist wide enough that placement, routing, delay and crossing do
//! real work, and times each pass on its own input so a change to one of
//! them — or to the release profile the bench inherits — shows up
//! against that pass rather than against the sum.
//!
//! ```sh
//! cargo bench -p cairn-lang-redstone --bench place_and_route
//! ```
//!
//! The source is generated rather than committed, and its shape is the
//! only knob: [`STRUCTS`] structs, each carrying a chain of [`GATES`]
//! two-input gates in its own `circuit` reservation.

use std::fmt::Write as _;
use std::hint::black_box;

use cairn_lang_core::check::Severity;
use cairn_lang_core::{Edition, lower, parse};
use cairn_lang_redstone::{
    DiagnosticCode, compile_crossing, compile_delay, compile_edition_netlist, compile_netlist,
    compile_placement, compile_routing, synthesize,
};
use criterion::{Criterion, criterion_group, criterion_main};

/// Scopes in the generated file. Each is placed and routed on its own,
/// so this scales the work linearly without changing what one scope asks
/// of the router.
const STRUCTS: usize = 8;

/// Gates per scope. A cell row needs `2n + 1` columns and every net is
/// measured against the 256-block attenuation cap, so a chain much longer
/// than this stops fitting one reservation.
const GATES: usize = 120;

/// A file of [`STRUCTS`] structs, each a chain of `gates` alternating
/// `and` / `or` gates fed by two pressure plates and read by two doors.
///
/// Every gate reads the two signals before it, so each net is local to
/// its neighbours in the row and fanout stays at two. A chain that fed
/// one input to every gate would be refused as congestion long before it
/// got this wide, and a refused scope is work the later passes skip.
fn circuit_source(structs: usize, gates: usize) -> String {
    let width = 2 * gates + 10;
    let mut src = String::from(
        "@cairn 2026.06\n@requires version>=1.20\n\n\
         theme bench:\n  slot wall -> @oak_planks\n  slot door -> @oak_door\n\n",
    );
    for s in 0..structs {
        let _ = write!(
            src,
            "struct chain{s} size={width}x24\n  \
             floor mat_slot=wall\n  \
             walls class=outer mat_slot=wall height=3\n  \
             door id=front side=front at=center mat_slot=door\n  \
             door id=back side=back at=center mat_slot=door\n  \
             pressure_plate id=pa at=front.outside offset=0 y=0 -> sig.a\n  \
             pressure_plate id=pb at=inside.front offset=0 y=0 -> sig.b\n",
        );
        let mut names = vec!["sig.a".to_owned(), "sig.b".to_owned()];
        for g in 0..gates {
            let op = if g % 2 == 0 { "and" } else { "or" };
            let (a, b) = (&names[names.len() - 1], &names[names.len() - 2]);
            let _ = writeln!(src, "  logic sig.g{g} = {a} {op} {b}");
            names.push(format!("sig.g{g}"));
        }
        let _ = write!(
            src,
            "  door[id=front] opened_by={}\n  \
             door[id=back] opened_by={}\n  \
             circuit region=floor void=4\n\n",
            names[names.len() - 1],
            names[names.len() - 2],
        );
    }
    src
}

fn place_and_route(c: &mut Criterion) {
    let source = circuit_source(STRUCTS, GATES);
    let intent = lower(&parse(&source).expect("the generated source parses"));
    let synth = synthesize(&intent);
    assert!(
        synth
            .diagnostics
            .iter()
            .all(|d| d.severity() != Severity::Error),
        "the generated source must synthesise cleanly: {:?}",
        synth.diagnostics,
    );
    let edition_netlist = compile_edition_netlist(&compile_netlist(&synth.scoped), Edition::Java);
    let placement = compile_placement(&edition_netlist, &intent);
    assert!(
        placement.diagnostics.is_empty(),
        "the generated source must place cleanly: {:?}",
        placement.diagnostics,
    );
    let routing = compile_routing(&placement.scoped);
    // The escape leaves strands a layer apart and says so; that advisory
    // elides nothing. Anything else means a scope dropped out, and the
    // passes after it would be timing less than the fixture claims.
    assert!(
        routing
            .diagnostics
            .iter()
            .all(|d| d.code == DiagnosticCode::RouteCrossLayerClearance),
        "the generated source must route with nothing but the cross-layer advisory: {:?}",
        routing.diagnostics,
    );
    let delay = compile_delay(&routing.scoped);
    assert!(
        delay.diagnostics.is_empty(),
        "the generated source must delay cleanly: {:?}",
        delay.diagnostics,
    );
    let crossing = compile_crossing(&delay.scoped);
    assert!(
        crossing.diagnostics.is_empty(),
        "the generated source must legalize cleanly: {:?}",
        crossing.diagnostics,
    );

    let mut group = c.benchmark_group("place_and_route");
    group.bench_function("synthesize", |b| b.iter(|| synthesize(black_box(&intent))));
    group.bench_function("placement", |b| {
        b.iter(|| compile_placement(black_box(&edition_netlist), black_box(&intent)));
    });
    group.bench_function("routing", |b| {
        b.iter(|| compile_routing(black_box(&placement.scoped)));
    });
    group.bench_function("delay", |b| {
        b.iter(|| compile_delay(black_box(&routing.scoped)));
    });
    group.bench_function("crossing", |b| {
        b.iter(|| compile_crossing(black_box(&delay.scoped)));
    });
    group.finish();
}

criterion_group!(benches, place_and_route);
criterion_main!(benches);
