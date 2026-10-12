//! Throughput of each place-and-route pass, timed in-process on a generated circuit wide enough
//! for every pass to do real work.

use std::fmt::Write as _;
use std::hint::black_box;

use cairn_lang_core::{Edition, lower, parse};
use cairn_lang_redstone::{
    DiagnosticCode, compile_crossing, compile_delay, compile_edition_netlist, compile_netlist,
    compile_placement, compile_routing, synthesize,
};
use criterion::{Criterion, criterion_group, criterion_main};

/// Scopes in the generated file, each placed and routed on its own.
const STRUCTS: usize = 8;

/// Gates per scope. Past about 250, the net out to the doors' pads is over the 256-block
/// attenuation cap and routing refuses the scope.
const GATES: usize = 120;

/// Columns one gate takes in the cell row, mirroring placement's private spacing.
const ROW_COLUMNS_PER_GATE: usize = 2;

/// `structs` structs, each a chain of `gates` gates fed by two plates and read by two doors.
/// Each gate reads the two signals before it: one shared input would be refused as congestion.
/// The width past the row and `void=4` carry headroom, so a layout change does not refuse it.
fn circuit_source(structs: usize, gates: usize) -> String {
    let row = ROW_COLUMNS_PER_GATE * gates + 1;
    let width = row + 2 + gates;
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
             pressure_plate id=pb at=inside.front offset=1 y=0 -> sig.b\n",
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
        synth.diagnostics.is_empty(),
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
    // A pass drops an empty scope without a finding, so pin that the work is all there.
    assert_eq!(synth.scoped.scopes.len(), STRUCTS);
    assert_eq!(placement.scoped.scopes.len(), STRUCTS);
    assert_eq!(routing.scoped.scopes.len(), STRUCTS);
    assert_eq!(delay.scoped.scopes.len(), STRUCTS);
    assert_eq!(crossing.scoped.scopes.len(), STRUCTS);
    assert_eq!(
        crossing
            .scoped
            .scopes
            .iter()
            .map(|entry| entry.ir.cells.len())
            .sum::<usize>(),
        STRUCTS * GATES,
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
