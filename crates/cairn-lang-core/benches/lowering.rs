//! Throughput of each front-end stage and block-array lowering, over a generated site large
//! enough to time: `cargo bench -p cairn-lang-core --bench lowering`.

use std::fmt::Write as _;
use std::hint::black_box;

use cairn_lang_core::{check, lower, lower_to_block_array, parse, resolve};
use criterion::{Criterion, criterion_group, criterion_main};

/// Rows of `place`s in the generated site.
const ROWS: usize = 10;

/// `place`s per row.
const COLS: usize = 10;

/// A site of `rows` × `cols` placements alternating two `def`s, each row joined door to door.
/// Every id is concrete, so it lowers without a registry pack.
fn site_source(rows: usize, cols: usize) -> String {
    let mut src = String::from(
        "@cairn 2026.06\n@requires version>=1.20\n\n\
         def cottage size=9x7:\n  \
         floor  id=floor mat_slot=floor\n  \
         walls  id=walls class=outer mat_slot=wall height=4\n  \
         door   id=entry class=entry side=front at=center\n  \
         window id=front side=front y=2 offset=2 size=2x2 mat_slot=glass\n  \
         roof   id=roof  kind=gable mat_slot=roof overhang=1\n\n\
         def keep size=11x9:\n  \
         floor  id=floor mat_slot=floor\n  \
         walls  id=walls class=outer mat_slot=wall height=5\n  \
         door   id=entry class=entry side=front at=center\n  \
         window class=arrow_slit side=back repeat=3 step=2 y=2 size=1x2 mat_slot=glass\n  \
         roof   id=roof  kind=hip mat_slot=roof overhang=1\n\n\
         theme bench:\n  \
         slot floor -> @oak_planks\n  \
         slot wall  -> @cobblestone\n  \
         slot roof  -> @spruce_stairs\n  \
         slot glass -> @glass_pane\n\n\
         site town:\n",
    );
    for r in 0..rows {
        for c in 0..cols {
            let def = if (r + c) % 2 == 0 { "cottage" } else { "keep" };
            let relation = match (r, c) {
                (0, 0) => "at=origin".to_owned(),
                (_, 0) => format!("north_of=h{}_0 gap=5", r - 1),
                _ => format!("east_of=h{r}_{} gap=4", c - 1),
            };
            let _ = writeln!(src, "  place id=h{r}_{c} use={def} theme=bench {relation}");
        }
    }
    for r in 0..rows {
        for c in 1..cols {
            let _ = writeln!(
                src,
                "  connect h{r}_{}.entry to h{r}_{c}.entry path=@gravel",
                c - 1,
            );
        }
    }
    src
}

fn lowering(c: &mut Criterion) {
    let source = site_source(ROWS, COLS);
    let module = parse(&source).expect("the generated source parses");
    let intent = lower(&module);
    // Only `check` would catch a misspelt optional key, which would quietly change what is timed.
    let findings = check(&module, &intent, None);
    assert!(
        findings.is_empty(),
        "the generated source must check cleanly: {findings:?}",
    );
    let resolution = resolve(&intent, None);
    assert!(
        resolution.diagnostics.is_empty(),
        "the generated source must resolve cleanly: {:?}",
        resolution.diagnostics,
    );
    let block_ir = lower_to_block_array(&intent, &resolution, None);
    assert!(
        block_ir.diagnostics.is_empty(),
        "the generated source must lower cleanly: {:?}",
        block_ir.diagnostics,
    );
    // A placement or walkway dropping out need not raise a finding, but would time less.
    assert_eq!(block_ir.placements.len(), ROWS * COLS);
    assert_eq!(block_ir.walkways.len(), ROWS * (COLS - 1));

    let mut group = c.benchmark_group("lowering");
    group.bench_function("parse", |b| b.iter(|| parse(black_box(&source))));
    group.bench_function("intent", |b| b.iter(|| lower(black_box(&module))));
    group.bench_function("resolve", |b| {
        b.iter(|| resolve(black_box(&intent), None));
    });
    group.bench_function("block_array", |b| {
        b.iter(|| lower_to_block_array(black_box(&intent), black_box(&resolution), None));
    });
    group.finish();
}

criterion_group!(benches, lowering);
criterion_main!(benches);
