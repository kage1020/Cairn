use cairn_lang_core::Edition;

use super::*;
use crate::fixtures::placed;

/// Two plates feeding an `and` and an `or`, each driving a door at the other end, in a struct
/// too narrow for the cells to lie two columns apart.
const CROSSBAR: &str = "\
theme cross:
  slot wall -> @oak_planks

struct crossbar size=11x8
  floor mat_slot=wall
  walls class=outer mat_slot=wall height=3
  roof  kind=flat mat_slot=wall overhang=1
  door  id=front side=front at=center
  door  id=back  side=back  at=center
  pressure_plate id=plate1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=plate2 at=inside.front  offset=1 y=0 -> sig.b
  logic sig.f1 = sig.a and sig.b
  logic sig.f2 = sig.a or sig.b
  door[id=front] opened_by=sig.f1
  door[id=back]  opened_by=sig.f2
  circuit region=floor void=4
";

#[test]
fn a_circuit_is_laid_again_past_a_margin_its_cells_do_not_fit() {
    let (placed, findings) = placed(CROSSBAR, Edition::Java);
    assert_eq!(findings, []);
    let layout = &placed.scopes[0].ir;
    assert_eq!(
        layout.margin, 0,
        "placement keeps the cells' nearest layout"
    );
    assert!(route_all(layout).is_err(), "the kept layout does not route");
    assert!(
        relay("struct=crossbar:", layout, 2).is_err(),
        "the cells do not fit two columns apart",
    );
    let out = compile_routing(&placed);
    assert_eq!(out.diagnostics, []);
    assert_eq!(out.scoped.scopes[0].ir.margin, 1);
}

#[test]
fn a_layout_whose_tiles_meet_is_refused_as_a_compiler_bug_naming_where() {
    let (mut placed, findings) = placed(CROSSBAR, Edition::Java);
    assert_eq!(findings, []);
    let layout = &mut placed.scopes[0].ir;
    layout.tiles[1].origin = layout.tiles[0].origin;
    let Err(Collision { at }) = Grid::of(layout) else {
        panic!("the moved tile meets the one it was moved onto");
    };
    let span = layout.region.span.clone();
    let out = compile_routing(&placed);
    assert_eq!(
        out.diagnostics,
        [compiler_bug_at(
            span,
            &format!(
                "struct=crossbar: the circuit under this struct has a part at ({}, {}, {}) where \
                 none fits",
                at.x, at.y, at.z,
            ),
        )],
    );
    assert_eq!(out.scoped.scopes, []);
}

fn compiler_bug_at(span: cairn_lang_core::error::Span, primary: &str) -> Diagnostic {
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        span,
        primary.to_owned(),
        "Fix: report this as a compiler bug, with the struct; meanwhile widen the struct's \
         `size=WxH` so the circuit is laid differently",
    )
}
