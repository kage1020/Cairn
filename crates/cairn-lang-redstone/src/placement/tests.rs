use std::fmt::Write as _;

use cairn_lang_core::{Edition, lower, parse};

use super::compile_placement;
use crate::routing_geometry::{BlockKind, BlockSite, PadColumn, block_sites};
use crate::{compile_edition_netlist, compile_netlist, synthesize};

/// Every block the placement pass puts in the one scope `source`
/// declares: cells, sensor pads and actuator pads.
///
/// Java only. A pad's coordinate is a function of the region and
/// the cell count, and a cell's of its index, so the Bedrock layout
/// is the same one.
fn placed_sites(source: &str) -> Vec<BlockSite> {
    let intent = lower(&parse(source).expect("the fixture parses"));
    let netlist = compile_netlist(&synthesize(&intent).scoped);
    let placed = compile_placement(&compile_edition_netlist(&netlist, Edition::Java), &intent);
    assert!(
        placed.diagnostics.is_empty(),
        "the fixture places: {:?}\n{source}",
        placed.diagnostics,
    );
    let ir = &placed.scoped.scopes[0].ir;
    let region = ir.region.clone().expect("a placed scope has a region");
    block_sites(ir, &region)
}

fn apart(a: &BlockSite, b: &BlockSite) -> u32 {
    a.coord.x.abs_diff(b.coord.x) + a.coord.y.abs_diff(b.coord.y) + a.coord.z.abs_diff(b.coord.z)
}

fn is_cell(site: &BlockSite) -> bool {
    matches!(site.kind, BlockKind::Cell)
}

/// Every pad that shares a face with a cell, as `(pad, cell)`.
fn pads_against_cells(sites: &[BlockSite]) -> Vec<(BlockSite, BlockSite)> {
    let mut touching = Vec::new();
    for pad in sites.iter().filter(|s| !is_cell(s)) {
        for cell in sites.iter().filter(|s| is_cell(s)) {
            if apart(pad, cell) == 1 {
                touching.push((*pad, *cell));
            }
        }
    }
    touching
}

/// The nearest any pad stands to any cell, or `None` with no cells.
fn nearest_pad_to_a_cell(sites: &[BlockSite]) -> Option<u32> {
    sites
        .iter()
        .filter(|s| !is_cell(s))
        .flat_map(|pad| {
            sites
                .iter()
                .filter(|s| is_cell(s))
                .map(|cell| apart(pad, cell))
        })
        .min()
}

/// Every two blocks on one coord, as `(first, second)`.
fn shared_coords(sites: &[BlockSite]) -> Vec<(BlockSite, BlockSite)> {
    let mut shared = Vec::new();
    for (i, first) in sites.iter().enumerate() {
        for second in &sites[i + 1..] {
            if first.coord == second.coord {
                shared.push((*first, *second));
            }
        }
    }
    shared
}

/// A chain of `cells` cells over `sensors` plates, the last cell on
/// the first door and each other door on a sensor of its own, so
/// the pads beside the row belong to nets the end cells have nothing
/// to do with.
fn chain(cells: usize, sensors: usize, doors: usize, width: usize, depth: u32) -> String {
    let mut source =
        String::from("theme t:\n  slot wall -> @oak_planks\n  slot door -> @oak_door\n\n");
    let _ = writeln!(
        source,
        "struct s size={width}x{depth}\n  floor mat_slot=wall"
    );
    for (d, side) in ["front", "back", "left", "right"]
        .iter()
        .take(doors)
        .enumerate()
    {
        let _ = writeln!(source, "  door id=d{d} side={side} at=center mat_slot=door");
    }
    for i in 0..sensors {
        let at = if i % 2 == 0 {
            "front.outside"
        } else {
            "inside.front"
        };
        let _ = writeln!(
            source,
            "  pressure_plate id=p{i} at={at} offset={i} y=0 -> sig.s{i}"
        );
    }
    let mut previous = String::from("sig.s0");
    for c in 0..cells {
        let other = (c + 1) % sensors;
        let op = if c % 2 == 0 { "or" } else { "and" };
        let _ = writeln!(source, "  logic sig.c{c} = {previous} {op} sig.s{other}");
        previous = format!("sig.c{c}");
    }
    let _ = writeln!(source, "  door[id=d0] opened_by={previous}");
    for d in 1..doors {
        let _ = writeln!(source, "  door[id=d{d}] opened_by=sig.s{}", d % sensors);
    }
    source.push_str("  circuit region=floor void=2\n");
    source
}

/// No pad stands face to face with a cell or on another block, at
/// any width the row check accepts, and at the narrowest the end
/// cells stand exactly one diagonal step from the pads.
///
/// A pad is a terminal of the one net it carries, and the router's
/// one-step rule keeps one net's dust away from another's rather
/// than a pad away from a cell, so a pad against a cell of a net it
/// has nothing to do with would take one of that cell's faces where
/// no pass looks. The pad columns stand at `x = 0` and
/// `x = width - 1`, and at the narrowest row the end cells stand in
/// the columns beside them; what keeps the two apart is that the
/// pads skip the cell row. Asserted against every cell, not only the
/// ones of other nets, because the layout gives the stronger answer.
///
/// "Not touching" alone is one-sided: pads moved far from the cells,
/// or all onto one coord, would pass it. So the sweep also holds the
/// distance to exactly 2 at the narrowest width, and every block to a
/// coord of its own.
///
/// The sweep starts at zero cells, where there is no row to step
/// over and the region is exactly as deep as the pads, so a column
/// that skipped a row it does not have would be refused or would
/// stack its last two pads. A cell-less scope starts at two columns:
/// at one, the sensor and actuator columns are the same column, which
/// this pass refuses (see [`super::pad_column_diagnostic`]).
#[test]
fn no_pad_stands_against_a_cell() {
    // Reported with this source, where `sig.b`'s sensor pad and its
    // door's pad both stood against an inverter that reads only
    // `sig.a`.
    let reported = "\
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct s size=3x5
  floor mat_slot=wall
  door id=d0 side=front at=center mat_slot=door
  door id=d1 side=back at=center mat_slot=door
  pressure_plate id=pa at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=pb at=inside.front offset=0 y=0 -> sig.b
  logic sig.x = not sig.a
  door[id=d0] opened_by=sig.x
  door[id=d1] opened_by=sig.b
  circuit region=floor void=2
";
    assert_eq!(pads_against_cells(&placed_sites(reported)), Vec::new());

    for cells in 0..=3 {
        let column = PadColumn::for_cell_count(cells);
        for sensors in 2..=4 {
            for doors in 1..=3 {
                let pads = column.rows(sensors.max(doors));
                let depth = if cells == 0 { pads } else { pads.max(3) };
                let narrowest = (2 * cells + 1).max(if cells == 0 { 2 } else { 1 });
                for width in narrowest..=narrowest + 2 {
                    let source = chain(cells, sensors, doors, width, depth);
                    let at = format!(
                        "{cells} cells, {sensors} sensors, {doors} doors, \
                         {width}x{depth}:\n{source}"
                    );
                    let sites = placed_sites(&source);
                    assert_eq!(pads_against_cells(&sites), Vec::new(), "{at}");
                    assert_eq!(shared_coords(&sites), Vec::new(), "{at}");
                    let nearest = nearest_pad_to_a_cell(&sites);
                    if cells == 0 {
                        assert_eq!(nearest, None, "{at}");
                    } else if width == narrowest {
                        assert_eq!(nearest, Some(2), "{at}");
                    } else {
                        assert!(nearest >= Some(2), "{at}");
                    }
                }
            }
        }
    }
}
