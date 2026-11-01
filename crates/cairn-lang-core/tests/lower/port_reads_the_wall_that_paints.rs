//! A `connect` row's port stands exactly when its door or window was cut into a wall that paints,
//! top-level or inside a `level`.

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::{Diagnostic, DiagnosticCode};

use crate::common::{defers, lowered_with_resolver_diagnostics};

/// The `W_DEFERRED_MEMBER` a `connect` row earns for a port that cannot be placed.
fn port_defer(out: &BlockArrayIr) -> Option<&Diagnostic> {
    out.diagnostics
        .iter()
        .find(|d| d.primary.contains("was skipped because port"))
}

fn walkway_keys(out: &BlockArrayIr) -> Vec<String> {
    out.walkways.keys().map(ToString::to_string).collect()
}

/// `home1` from `solid` and `home2` from `other`, joined by one `connect` row. `hollow`'s `ghost`
/// lifts to an abstract token, so with no registry pack its walls are not built.
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
    let out = lowered_with_resolver_diagnostics(&pair("hollow", "home1.entry to home2.front"));
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
    let out = lowered_with_resolver_diagnostics(&pair("hollow", "home1.front to home2.entry"));
    assert!(
        walkway_keys(&out).is_empty(),
        "no strip may reach a door that was never carved, got {:?}",
        walkway_keys(&out),
    );
    assert!(port_defer(&out).is_some(), "the connect row defers");
}

#[test]
fn a_def_with_no_walls_anchors_neither_a_door_nor_a_window() {
    for connect in [
        "home1.entry to home2.front",
        "home1.front to home2.entry",
        "home1.entry to home2.entry",
    ] {
        let out = lowered_with_resolver_diagnostics(&pair("wall_less", connect));
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
    let out = lowered_with_resolver_diagnostics(&pair("storeyed", "home1.entry to home2.entry"));
    assert_eq!(
        walkway_keys(&out),
        vec!["walkway::pair::home1.entry__home2.entry".to_owned()],
        "a door in level-scoped masonry anchors its port",
    );
}

#[test]
fn a_window_cut_into_level_scoped_walls_anchors_its_port() {
    let out = lowered_with_resolver_diagnostics(&pair("storeyed", "home1.entry to home2.front"));
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
    let out = lowered_with_resolver_diagnostics(&pair("hollow", "home1.entry to home2.front"));
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
    let out = lowered_with_resolver_diagnostics(&pair("hollow", "home1.entry to home2.front"));
    let defer = port_defer(&out).expect("the connect row defers");
    assert_eq!(
        defer.notes.len(),
        1,
        "one note for the one port: {:?}",
        defer.notes
    );
    let masonry = defer.notes[0].message.as_str();
    assert!(
        masonry.starts_with("`home2.front` is a window with no wall to cut into"),
        "the note names the masonry the window lacks: {masonry}",
    );
    assert!(
        masonry.contains("`walls`") && masonry.contains("`mat_slot=` that resolves"),
        "the masonry note names what a wall needs: {masonry}",
    );
    assert!(
        masonry.contains("says so on its own line"),
        "the masonry note sends the author to the member finding: {masonry}",
    );
    // The line the note sends the author to has a finding on it.
    assert!(
        defers(&out)
            .iter()
            .any(|d| d.contains("has no wall to cut into")),
        "the window that could not be cut says so itself: {:?}",
        defers(&out),
    );
}

/// Two `place` rows under one `id=`; lowering keeps the first body.
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
    // `structures` and `placements` are first-write-wins, so the column map has to be too.
    let out = lowered_with_resolver_diagnostics(DUPLICATE_ID);
    assert_eq!(
        walkway_keys(&out),
        vec!["walkway::pair::home1.entry__home2.entry".to_owned()],
        "the kept body paints its walls, so the port stands",
    );
}

/// Two courses and a window at `window_y`, the upper course under `level y={upper_level}`.
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
    // `height=2` and `level y=6 height=2` leave y=3..=6 open air.
    let out = lowered_with_resolver_diagnostics(&two_courses(3, 6));
    assert!(
        walkway_keys(&out).is_empty(),
        "a window in open air is not a port, got {:?}",
        walkway_keys(&out),
    );
    // Guard: the walkway map is empty because of the wall, not a fixture that stopped resolving.
    assert!(
        port_defer(&out).is_some(),
        "the row says why no strip was laid: {:?}",
        defers(&out),
    );
}

#[test]
fn a_window_crossing_the_seam_of_two_touching_courses_anchors_its_port() {
    // `height=2` and `level y=2 height=2` are one wall from y=1 to y=4.
    let out = lowered_with_resolver_diagnostics(&two_courses(2, 2));
    assert_eq!(
        walkway_keys(&out),
        vec!["walkway::pair::home1.entry__home2.front".to_owned()],
        "two touching courses are one wall",
    );
}
