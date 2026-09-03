//! A `connect` row anchors its ports to the masonry the openings pass
//! cuts, not to the masonry the source happens to spell.
//!
//! The two used to be derived separately: the openings pass built its
//! wall column from the flattened, resolved member list, while port
//! resolution rebuilt one from the `def`'s top-level `walls` members and
//! their `height=` alone. The two derivations disagreed in both
//! directions — a strip laid to a window that was never cut when a
//! material did not resolve, and a strip refused to a window that *was*
//! cut when the walls were declared inside a `level`. Both endpoints now
//! read the column the body was lowered with, so the disagreement has
//! nowhere left to live.

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::{Diagnostic, DiagnosticCode};
use cairn_lang_core::{lower, parse, resolve};

/// Lower a source with no registry pack, merging the resolver's
/// diagnostics in front of the block-array pass's the way the CLI does.
fn lowered(src: &str) -> BlockArrayIr {
    let module = parse(src).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    let mut out = lower_to_block_array(&ir, &resolution, None);
    let mut combined = resolution.diagnostics;
    combined.append(&mut out.diagnostics);
    out.diagnostics = combined;
    out
}

/// The `W_DEFERRED_MEMBER` a `connect` row earns when one of its ports
/// cannot be placed, or `None` when every port landed.
fn port_defer(out: &BlockArrayIr) -> Option<&Diagnostic> {
    out.diagnostics
        .iter()
        .find(|d| d.primary.contains("was skipped because port"))
}

/// Every `W_DEFERRED_MEMBER` primary, the member-side ones included.
fn defers(out: &BlockArrayIr) -> Vec<&str> {
    out.diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::DeferredMember)
        .map(|d| d.primary.as_str())
        .collect()
}

fn walkway_keys(out: &BlockArrayIr) -> Vec<String> {
    out.walkways.keys().map(ToString::to_string).collect()
}

/// Two placements side by side, `home1` from `solid` and `home2` from
/// the def named by `other`, joined by one `connect` row.
///
/// `solid`'s walls paint: `wall` is a concrete token. `hollow`'s do not:
/// `ghost` lifts to an abstract token, which without a registry pack
/// defers to air — the live shape of "a wall that paints nothing" now
/// that a bare `mat_slot=` is refused outright.
fn pair(other: &str, connect: &str) -> String {
    format!(
        "@cairn 2026.06

def solid size=5x5:
  floor  id=floor mat_slot=floor
  walls  id=walls mat_slot=wall height=4
  door   id=entry side=front at=center
  window id=front side=front offset=2 y=3 size=1x1 mat_slot=glass

def hollow size=5x5:
  floor  id=floor mat_slot=floor
  walls  id=walls mat_slot=ghost height=4
  door   id=entry side=front at=center
  window id=front side=front offset=2 y=3 size=1x1 mat_slot=glass

def wall_less size=5x5:
  floor  id=floor mat_slot=floor
  door   id=entry side=front at=center
  window id=front side=front offset=2 y=3 size=1x1 mat_slot=glass

def storeyed size=5x5:
  floor  id=floor mat_slot=floor
  level  id=ground y=0
    walls id=walls mat_slot=wall height=4
  door   id=entry side=front at=center
  window id=front side=front offset=2 y=3 size=1x1 mat_slot=glass

theme medieval:
  slot floor -> @oak_planks
  slot wall  -> @cobblestone
  slot ghost -> @wall.stone.cobble
  slot glass -> @glass_pane

site pair:
  place id=home1 use=solid theme=medieval at=origin
  place id=home2 use={other} theme=medieval east_of=home1 gap=4

  connect {connect} path=@gravel
"
    )
}

#[test]
fn a_window_over_walls_that_paint_nothing_anchors_no_port() {
    // AC1. The openings pass defers the cut; the strip used to be laid
    // to the window anyway.
    let out = lowered(&pair("hollow", "home1.entry to home2.front"));
    assert!(
        walkway_keys(&out).is_empty(),
        "no strip may reach a window that was never cut, got {:?}",
        walkway_keys(&out),
    );
    let defer = port_defer(&out).expect("the connect row defers");
    assert_eq!(defer.code, DiagnosticCode::DeferredMember);
}

#[test]
fn a_door_over_walls_that_paint_nothing_anchors_no_port() {
    // AC2. The door branch consulted no column at all, so this half of
    // the disagreement predates the material axis.
    let out = lowered(&pair("hollow", "home1.front to home2.entry"));
    assert!(
        walkway_keys(&out).is_empty(),
        "no strip may reach a door that was never carved, got {:?}",
        walkway_keys(&out),
    );
    assert!(port_defer(&out).is_some(), "the connect row defers");
}

#[test]
fn a_def_with_no_walls_anchors_neither_a_door_nor_a_window() {
    // AC5. Same rule, reached without a material: nothing to cut into.
    for connect in [
        "home1.entry to home2.front",
        "home1.front to home2.entry",
        "home1.entry to home2.entry",
    ] {
        let out = lowered(&pair("wall_less", connect));
        assert!(
            walkway_keys(&out).is_empty(),
            "`{connect}` laid a strip against a def with no walls",
        );
        assert!(
            port_defer(&out).is_some(),
            "`{connect}` deferred without saying so",
        );
    }
}

#[test]
fn a_door_in_level_scoped_walls_anchors_its_port() {
    // AC3's other half. The door gate asks the same column, so a `def`
    // that keeps its walls in a `level` has to anchor a doorway port for
    // the same reason it anchors a window one — a gate that regressed to
    // "are there top-level walls" would refuse it and send the author to
    // move a door already in masonry.
    let out = lowered(&pair("storeyed", "home1.entry to home2.entry"));
    assert_eq!(
        walkway_keys(&out),
        vec!["walkway::pair::home1.entry__home2.entry".to_owned()],
        "a door in level-scoped masonry anchors its port",
    );
}

#[test]
fn a_window_cut_into_level_scoped_walls_anchors_its_port() {
    // AC3. The other direction: the openings pass cuts this window,
    // because the flattened list carries the `level`'s walls. The port
    // used to refuse it, so the author was told to move a window that
    // was already in masonry.
    let out = lowered(&pair("storeyed", "home1.entry to home2.front"));
    assert_eq!(
        walkway_keys(&out),
        vec!["walkway::pair::home1.entry__home2.front".to_owned()],
        "a window in level-scoped masonry anchors its port",
    );
    assert!(
        port_defer(&out).is_none(),
        "nothing about this row is deferred: {:?}",
        port_defer(&out).map(|d| d.primary.clone()),
    );
}

#[test]
fn the_deferral_names_only_the_endpoint_that_has_no_masonry() {
    // AC4. `home1` is `solid`; only `home2.front` is unplaceable.
    let out = lowered(&pair("hollow", "home1.entry to home2.front"));
    let defer = port_defer(&out).expect("the connect row defers");
    assert!(
        defer.primary.contains("port `home2.front`"),
        "the deferral must name the unpaintable endpoint: {}",
        defer.primary,
    );
    assert!(
        !defer.primary.contains("home1.entry`"),
        "the deferral must not name the endpoint that resolved: {}",
        defer.primary,
    );
}

#[test]
fn the_deferral_states_the_masonry_contract() {
    // AC6. Four causes now, and the masonry one is the only cause whose
    // fix is not on the `connect` row — so the note has to say where it
    // is instead.
    let out = lowered(&pair("hollow", "home1.entry to home2.front"));
    let defer = port_defer(&out).expect("the connect row defers");
    let masonry = defer
        .notes
        .iter()
        .map(|n| n.message.as_str())
        .find(|m| m.starts_with("both roles are cut into masonry"))
        .expect("the deferral states the masonry contract");
    assert!(
        masonry.contains("`walls`") && masonry.contains("`mat_slot=` that resolves"),
        "the masonry note names what a wall needs: {masonry}",
    );
    assert!(
        masonry.contains("says so on its own line"),
        "the masonry note sends the author to the member finding: {masonry}",
    );
    // …and the line it sends the author to has a finding on it. A note
    // pointing at a member that says nothing is worse than no note.
    assert!(
        defers(&out)
            .iter()
            .any(|d| d.contains("has no wall to cut into")),
        "the window that could not be cut says so itself: {:?}",
        defers(&out),
    );
}

/// Two `place` rows under one `id=`. The resolver refuses the duplicate
/// and lowering keeps the first body; the port has to be judged against
/// that one, not against the one that was dropped.
const DUPLICATE_ID: &str = "@cairn 2026.06

def solid size=5x5:
  floor  id=floor mat_slot=floor
  walls  id=walls mat_slot=wall height=4
  door   id=entry side=front at=center

def wall_less size=5x5:
  floor  id=floor mat_slot=floor
  door   id=entry side=front at=center

theme medieval:
  slot floor -> @oak_planks
  slot wall  -> @cobblestone

site pair:
  place id=home1 use=solid theme=medieval at=origin
  place id=home2 use=solid theme=medieval east_of=home1 gap=4
  place id=home2 use=wall_less theme=medieval east_of=home1 gap=4

  connect home1.entry to home2.entry path=@gravel
";

#[test]
fn a_duplicate_place_id_is_judged_by_the_body_that_was_kept() {
    // AC7. `structures` and `placements` are first-write-wins, so the
    // column map has to be too — otherwise the port is checked against
    // a body that is in no artifact.
    let out = lowered(DUPLICATE_ID);
    assert_eq!(
        walkway_keys(&out),
        vec!["walkway::pair::home1.entry__home2.entry".to_owned()],
        "the kept body paints its walls, so the port stands",
    );
}

/// A window hung in the air between two courses, and one crossing the
/// seam where two courses touch. Both questions are the column's; the
/// port has to ask it rather than approximate it.
fn two_courses(window_y: u32, upper_level: u32) -> String {
    format!(
        "@cairn 2026.06

def tower size=5x5:
  floor  id=floor mat_slot=floor
  walls  id=lower mat_slot=wall height=2
  level  id=upper y={upper_level}
    walls id=upper_walls mat_slot=wall height=2
  window id=front side=front offset=2 y={window_y} size=1x2 mat_slot=glass

def solid size=5x5:
  floor  id=floor mat_slot=floor
  walls  id=walls mat_slot=wall height=4
  door   id=entry side=front at=center

theme medieval:
  slot floor -> @oak_planks
  slot wall  -> @cobblestone
  slot glass -> @glass_pane

site pair:
  place id=home1 use=solid theme=medieval at=origin
  place id=home2 use=tower theme=medieval east_of=home1 gap=4

  connect home1.entry to home2.front path=@gravel
"
    )
}

#[test]
fn a_window_hanging_in_the_gap_between_two_courses_anchors_no_port() {
    // AC8, refusing half: `height=2` plus `level y=6 height=2` leaves
    // y=3..=6 open air, and a window at y=3 is in none of it.
    let out = lowered(&two_courses(3, 6));
    assert!(
        walkway_keys(&out).is_empty(),
        "a window in open air is not a port, got {:?}",
        walkway_keys(&out),
    );
    // Asserted alongside, so a fixture that stops resolving upstream
    // cannot pass this by leaving the walkway map empty for a reason
    // that has nothing to do with the wall.
    assert!(
        port_defer(&out).is_some(),
        "the row says why no strip was laid: {:?}",
        defers(&out),
    );
}

#[test]
fn a_window_crossing_the_seam_of_two_touching_courses_anchors_its_port() {
    // AC8, anchoring half: `height=2` plus `level y=2 height=2` is one
    // wall from y=1 to y=4, and a window at y=2..=3 is inside it.
    let out = lowered(&two_courses(2, 2));
    assert_eq!(
        walkway_keys(&out),
        vec!["walkway::pair::home1.entry__home2.front".to_owned()],
        "two touching courses are one wall",
    );
}
