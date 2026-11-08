//! Throughput of each place-and-route pass, timed in-process on the examples whose circuits build.

use std::hint::black_box;
use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockIdSet, BlockState, TargetRegistry, lower_to_block_array};
use cairn_lang_core::check::Severity;
use cairn_lang_core::{Edition, lower, parse, resolve};
use cairn_lang_redstone::{
    Diagnostic, build_circuits, compile_delay, compile_edition_netlist, compile_legalize,
    compile_netlist, compile_placement, compile_routing, synthesize,
};
use criterion::{Criterion, criterion_group, criterion_main};

/// The examples whose circuits build on both editions.
const EXAMPLES: [&str; 3] = ["redstone-door", "control-room", "crossbar"];

/// A target that spells every token as the block of its name, so the floor under a component
/// stands as written.
struct Named;

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

fn example(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(format!("{name}.crn"));
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn assert_clean(what: &str, diagnostics: &[Diagnostic]) {
    assert!(
        diagnostics.iter().all(|d| d.severity() != Severity::Error),
        "{what} must run clean: {diagnostics:?}",
    );
}

fn place_and_route(c: &mut Criterion) {
    let mut group = c.benchmark_group("place_and_route");
    for name in EXAMPLES {
        for edition in [Edition::Java, Edition::Bedrock] {
            let intent = lower(&parse(&example(name)).expect("the example parses"));
            let built =
                lower_to_block_array(&intent, &resolve(&intent, Some(edition)), Some(&Named));
            let synth = synthesize(&intent);
            assert_clean("synthesis", &synth.diagnostics);
            let netlist = compile_edition_netlist(&compile_netlist(&synth.scoped), edition);
            let placement = compile_placement(&netlist, &intent, &built);
            assert_clean("placement", &placement.diagnostics);
            let routing = compile_routing(&placement.scoped);
            assert_clean("routing", &routing.diagnostics);
            let delay = compile_delay(&routing.scoped);
            assert_clean("delay", &delay.diagnostics);
            assert_eq!(
                delay.scoped.scopes.len(),
                1,
                "{name}: the circuit is all there"
            );

            let label = format!("{name}/{}", edition.as_str());
            group.bench_function(format!("placement/{label}"), |b| {
                b.iter(|| compile_placement(black_box(&netlist), &intent, &built));
            });
            group.bench_function(format!("routing/{label}"), |b| {
                b.iter(|| compile_routing(black_box(&placement.scoped)));
            });
            group.bench_function(format!("delay/{label}"), |b| {
                b.iter(|| compile_delay(black_box(&routing.scoped)));
            });
            group.bench_function(format!("legalize/{label}"), |b| {
                b.iter(|| compile_legalize(black_box(&delay.scoped), None));
            });
            group.bench_function(format!("build_circuits/{label}"), |b| {
                b.iter(|| {
                    let mut laid = built.clone();
                    build_circuits(black_box(&intent), &mut laid, edition, None)
                });
            });
        }
    }
    group.finish();
}

criterion_group!(benches, place_and_route);
criterion_main!(benches);
