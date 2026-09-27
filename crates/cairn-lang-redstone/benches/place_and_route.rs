//! Throughput of the place-and-route passes over a generated circuit.
//!
//! Timing the CLI over an example would mostly time process startup, so
//! the bench calls the passes in-process. An example is still too small
//! to time that way — a few dozen lines go through a pass in less than
//! the timer can tell apart from a change — so this bench generates a
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

/// Gates per scope, laid as one cell row. The row itself never stops
/// fitting, because [`circuit_source`] widens the reservation with the
/// chain, and each gate's nets stay a few blocks long however long it
/// gets. What binds is the net out to the doors' pads: it crosses the
/// spare columns, one per gate, so it grows with the chain, and past
/// about 250 gates it is over the 256-block attenuation cap and routing
/// refuses the scope. The value is sized so each pass takes
/// milliseconds, well short of that.
const GATES: usize = 120;

/// Columns one gate takes in the cell row: the cell and the clear column
/// beside it. Placement's own spacing constant is private; this mirrors
/// it so the reservation width below reads as the row it has to hold.
const ROW_COLUMNS_PER_GATE: usize = 2;

/// A file of [`STRUCTS`] structs, each a chain of `gates` alternating
/// `and` / `or` gates fed by two pressure plates and read by two doors.
///
/// Every gate reads the two signals before it, so each net is local to
/// its neighbours in the row and fanout stays at two. A chain that fed
/// one input to every gate would be refused as congestion long before it
/// got this wide, and a refused scope is work the later passes skip.
///
/// The reservation is as wide as the struct. Placement refuses a row
/// wider than that, and routing refuses the scope as congestion unless
/// two more columns are left past the row to reach the doors' pads on
/// the far edge. The width gives one spare column per gate on top of
/// those, so a wider cell spacing or an added pad column is absorbed
/// rather than turning the fixture into a refusal.
///
/// `void=4` is two layers over the floor of two. One is not enough: with
/// no layer above the cell plane, the nets that have to escape over a
/// neighbour have nowhere to go and routing refuses the scope as
/// congestion. The two layers above that floor are headroom, so a router
/// change that sends a net a layer or two higher does not do the same.
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
    // Every signal the generator writes is read, so even the one warning
    // synth can raise, an unused signal, means the gate chain came apart.
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
    // The escape leaves strands a layer apart and says so; that advisory
    // elides nothing. Anything else is a scope refused.
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
    // The checks above only see findings, and a pass drops a scope with
    // nothing to place without one: a front-end change that stopped the
    // `logic` lines reaching synth, or a synth that folded the chain,
    // would pass them all while the benches timed an empty netlist. The
    // counts pin the work itself — every scope through every pass, one
    // placed cell per gate.
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
