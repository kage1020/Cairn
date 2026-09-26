//! Throughput of the front end and block-array lowering over a generated
//! site.
//!
//! The examples are a few dozen lines each, so a whole-pipeline run over
//! any of them is dominated by process startup. This bench generates a
//! site large enough that parsing, resolution and block-array lowering do
//! real work, and times each stage on its own input so a change to one of
//! them — or to the release profile the bench inherits — shows up
//! against that stage rather than against the sum.
//!
//! ```sh
//! cargo bench -p cairn-lang-core --bench lowering
//! ```
//!
//! The source is generated rather than committed, and its shape is the
//! only knob: a [`ROWS`] × [`COLS`] grid of `place`s alternating two
//! `def`s, with every neighbouring pair in a row joined by a `connect`.

use std::fmt::Write as _;
use std::hint::black_box;

use cairn_lang_core::{lower, lower_to_block_array, parse, resolve};
use criterion::{Criterion, criterion_group, criterion_main};

/// Rows of `place`s in the generated site.
const ROWS: usize = 10;

/// `place`s per row, and one fewer `connect` walkways.
const COLS: usize = 10;

/// A site of `rows` × `cols` placements. Two `def`s alternate so both a
/// gable and a hip roof, a door, windows and a repeated opening are in
/// every row, and each row's placements are joined door to door by a
/// gravel walkway — the members the lowering passes spend their time on.
///
/// Every id is concrete, so the build lowers without a registry pack and
/// the bench does not have to depend on `cairn-lang-formats`.
fn site_source(rows: usize, cols: usize) -> String {
    let mut src = String::from(
        "@cairn 2026.06\n@requires version>=1.20\n\n\
         def cottage class=house size=9x7:\n  \
         floor  id=floor mat_slot=floor\n  \
         walls  id=walls class=outer mat_slot=wall height=4\n  \
         door   id=entry class=entry side=front at=center\n  \
         window id=front side=front y=2 offset=2 size=2x2 mat_slot=glass\n  \
         roof   id=roof  kind=gable mat_slot=roof overhang=1\n\n\
         def keep class=house size=11x9:\n  \
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
    let resolution = resolve(&intent, None);
    assert!(
        resolution.diagnostics.is_empty(),
        "the generated source must resolve cleanly: {:?}",
        resolution.diagnostics,
    );
    let block_ir = lower_to_block_array(&intent, &resolution, None);
    // A finding here would mean a placement or a walkway dropped out, and
    // the bench would be timing less than the fixture claims.
    assert!(
        block_ir.diagnostics.is_empty(),
        "the generated source must lower cleanly: {:?}",
        block_ir.diagnostics,
    );
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
